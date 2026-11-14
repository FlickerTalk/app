use std::collections::{HashMap, HashSet};
use std::time::Duration;

use anyhow::{bail, ensure, Context, Result};
use ft_contacts::ContactCard;
use ft_protocol::{Body, MessageId, Packet};
use ft_push::MailboxRejected;
use ft_storage::{Contact, Message, MessageState, OutboxEntry, UpdateEntry};

use crate::{now, Core, Event, Peer};

pub const MAX_RETRY_DELAY: Duration = Duration::from_secs(300);
pub(crate) const RECEIPT_WAIT: Duration = Duration::from_secs(30);
pub(crate) const MAILBOX_WAIT: Duration = Duration::from_secs(600);
// The router keeps a blob 7 days, the daily copy covers a router that lost its table
pub(crate) const MAILBOX_COPY: Duration = Duration::from_secs(24 * 3600);
// Matches the router's mailbox TTL. Old apps never ack edits or reactions.
pub(crate) const UPDATE_LIFETIME: Duration = Duration::from_secs(7 * 24 * 3600);

fn mail_copy_due(in_mailbox: bool, mailed_at: Option<i64>, now: i64) -> bool {
    !in_mailbox || mailed_at.is_none_or(|at| now - at >= MAILBOX_COPY.as_millis() as i64)
}

pub fn retry_delay(attempts: i64) -> Duration {
    let exponent = attempts.clamp(1, 16) as u32 - 1;
    Duration::from_secs(5 * 2u64.pow(exponent)).min(MAX_RETRY_DELAY)
}

pub(crate) enum Route {
    Direct,
    Mailbox,
    Unreachable,
}

pub(crate) const EDIT_WINDOW_MS: i64 = 24 * 60 * 60 * 1000;

// Deliberately not checked against Unicode's emoji tables
pub(crate) fn is_reaction(text: &str) -> bool {
    !text.is_empty() && text.len() <= 32 && text.chars().count() <= 8 && !text.chars().any(|c| c.is_whitespace() || c.is_control())
}

impl Core {
    // Only over an already open direct connection, never the mailbox or the router
    pub async fn typing(&self, contact: &str) -> Result<bool> {
        let contact = self.contact(contact).await?;
        if contact.blocked || !contact.accepted || !contact.rules.typing {
            return Ok(false);
        }
        self.transmit_open(&contact, &Packet::new(Body::Typing)).await
    }

    pub async fn forward(&self, message_id: &str, to: &str) -> Result<String> {
        let stored = self.store.message(message_id).await?.context("that message is not here")?;
        match self.store.file(message_id).await? {
            Some(file) => {
                // `complete` is about the transfer, our own outgoing files have their bytes from the start
                ensure!(stored.outgoing || file.complete, "that file is not here whole yet");
                let path = self.file_path(&file);
                ensure!(path.exists(), "the bytes of that file are no longer here");
                self.send_file(to, &path, &file.name, &file.mime).await
            }
            None => self.send_text(to, &stored.body).await,
        }
    }

    // `send_at` in ms. An iPhone only sends it when something wakes the app.
    pub async fn schedule_text(&self, contact: &str, text: &str, send_at: i64, reply_to: Option<&str>) -> Result<String> {
        let text = text.trim();
        ensure!(!text.is_empty(), "nothing to send");
        let soonest = now() + 60_000;
        ensure!(send_at >= soonest, "that time is too soon");
        ensure!(send_at <= now() + 366 * 24 * 60 * 60 * 1000, "that time is too far away");
        self.chosen(contact).await?;
        let reply_to = match reply_to {
            Some(quoted) => {
                let quoted_message = self.store.message(quoted).await?.context("the message answered is not here")?;
                ensure!(quoted_message.contact == contact, "the message answered is from another conversation");
                Some(quoted.to_owned())
            }
            None => None,
        };
        let message_id = MessageId::new().to_string();
        self.store
            .insert_message(&Message {
                message_id: message_id.clone(),
                contact: contact.to_owned(),
                outgoing: true,
                body: text.to_owned(),
                sent_at: send_at,
                received_at: now(),
                state: MessageState::Pending,
            })
            .await?;
        if let Some(quoted) = reply_to {
            self.store.set_reply(&message_id, &quoted).await?;
        }
        self.store.enqueue_at(&message_id, contact, now(), send_at).await?;
        let _ = self.events.send(Event::MessagesChanged { contact: contact.to_owned() });
        Ok(message_id)
    }

    pub async fn edit_message(&self, message_id: &str, text: &str) -> Result<()> {
        let text = text.trim();
        ensure!(!text.is_empty(), "nothing to say");
        let message = self.store.message(message_id).await?.context("that message is not here")?;
        ensure!(message.outgoing, "only our own words can change");
        ensure!(message.state != MessageState::NotSent, "a message that was not sent is sent again, not edited");
        ensure!(self.store.file(message_id).await?.is_none(), "a file has no words to change");
        ensure!(!self.store.is_deleted(message_id).await?, "that message was taken back");
        ensure!(now() - message.sent_at <= EDIT_WINDOW_MS, "too late to change it");
        let contact = self.contact(&message.contact).await?;
        let packet = Packet::new(Body::Edit { of: MessageId::parse(message_id)?, text: text.to_owned() });
        self.store.edit_text(message_id, text, packet.sent_at as i64).await?;
        let _ = self.events.send(Event::MessagesChanged { contact: contact.device_id.clone() });
        self.tell(&contact, &packet).await
    }

    // Tell the contact even if it never left, a try may already be on its way
    pub async fn delete_for_everyone(&self, message_id: &str) -> Result<()> {
        let message = self.store.message(message_id).await?.context("that message is not here")?;
        ensure!(message.outgoing, "only our own messages can be taken back from the other side");
        let contact = self.contact(&message.contact).await?;
        if self.store.scheduled_all().await?.get(message_id).is_some_and(|send_at| *send_at > now()) {
            return self.forget_message(message_id).await;
        }
        let file = self.store.file(message_id).await?;
        self.store.mark_deleted(message_id).await?;
        if message.state == MessageState::Pending {
            self.store.advance(&[message_id.to_owned()], MessageState::Sent).await?;
        }
        self.forget_bytes(file.into_iter().collect()).await;
        let _ = self.events.send(Event::MessagesChanged { contact: contact.device_id.clone() });
        let packet = Packet::new(Body::Delete { of: MessageId::parse(message_id)? });
        self.tell(&contact, &packet).await
    }

    pub async fn updates_waiting(&self, contact: &str) -> Result<HashSet<String>> {
        let mut waiting = HashSet::new();
        for entry in self.store.update_outbox().await? {
            if entry.contact != contact {
                continue;
            }
            if let Ok(Packet { body: Body::Edit { of, .. } | Body::Delete { of }, .. }) = Packet::decode(&entry.packet) {
                waiting.insert(of.to_string());
            }
        }
        Ok(waiting)
    }

    pub async fn react(&self, contact: &str, message_id: &str, emoji: Option<&str>) -> Result<()> {
        let contact = self.contact(contact).await?;
        ensure!(contact.accepted && !contact.blocked, "not a conversation to react in");
        let message = self.store.message(message_id).await?.context("that message is not here")?;
        ensure!(message.contact == contact.device_id, "that message is from another conversation");
        let emoji = emoji.map(str::trim).filter(|emoji| !emoji.is_empty());
        if let Some(emoji) = emoji {
            ensure!(is_reaction(emoji), "a reaction is one emoji");
        }
        let packet = Packet::new(Body::Reaction { to: MessageId::parse(message_id)?, emoji: emoji.unwrap_or("").to_owned() });
        self.store.set_reaction(message_id, true, emoji, packet.sent_at as i64).await?;
        let _ = self.events.send(Event::MessagesChanged { contact: contact.device_id.clone() });
        self.tell(&contact, &packet).await
    }

    pub async fn send_text(&self, contact: &str, text: &str) -> Result<String> {
        self.send_text_replying(contact, text, None).await
    }

    pub async fn send_text_replying(&self, contact: &str, text: &str, reply_to: Option<&str>) -> Result<String> {
        let message_id = self.queue_text(contact, text, reply_to).await?;
        self.deliver_queued(&message_id).await;
        Ok(message_id)
    }

    pub async fn deliver_queued(&self, message_id: &str) {
        if let Ok(outbox) = self.store.outbox().await {
            if let Some(entry) = outbox.into_iter().find(|e| e.message_id == message_id) {
                self.attempt(&entry).await;
            }
        }
    }

    pub async fn queue_text(&self, contact: &str, text: &str, reply_to: Option<&str>) -> Result<String> {
        let text = text.trim();
        if text.is_empty() {
            bail!("nothing to send");
        }
        self.chosen(contact).await?;
        let reply_to = match reply_to {
            Some(quoted) => {
                let quoted_message = self.store.message(quoted).await?.context("the message answered is not here")?;
                ensure!(quoted_message.contact == contact, "the message answered is from another conversation");
                Some(MessageId::parse(quoted)?)
            }
            None => None,
        };
        let packet = Packet::new(Body::Message { text: text.to_owned(), reply_to });
        let message = Message {
            message_id: packet.id.to_string(),
            contact: contact.to_owned(),
            outgoing: true,
            body: text.to_owned(),
            sent_at: packet.sent_at as i64,
            received_at: packet.sent_at as i64,
            state: MessageState::Pending,
        };
        let quoted = reply_to.map(|quoted| quoted.to_string());
        self.store.queue_message(&message, quoted.as_deref(), now()).await?;
        let _ = self.events.send(Event::MessagesChanged { contact: contact.to_owned() });
        Ok(message.message_id)
    }

    pub async fn resend(&self, message_id: &str) -> Result<()> {
        let message = self.store.message(message_id).await?.context("that message is not here")?;
        ensure!(message.outgoing && message.state == MessageState::NotSent, "only a message that was not sent goes again");
        self.store.advance(std::slice::from_ref(&message.message_id), MessageState::Pending).await?;
        self.store.enqueue(message_id, &message.contact, now()).await?;
        let _ = self.events.send(Event::MessagesChanged { contact: message.contact.clone() });
        if let Some(entry) = self.store.outbox().await?.into_iter().find(|e| e.message_id == message_id) {
            self.attempt(&entry).await;
        }
        Ok(())
    }

    pub async fn send_raw_to(&self, contact: &str, packet: &Packet) -> Result<()> {
        let contact = self.contact(contact).await?;
        self.transmit(&contact, packet).await?;
        Ok(())
    }

    pub async fn retry_due(&self) -> Result<()> {
        for entry in self.store.due(now()).await? {
            self.attempt(&entry).await;
        }
        for entry in self.store.updates_due(now()).await? {
            self.attempt_update(&entry).await;
        }
        for entry in self.store.circle_due(now()).await? {
            let _ = self.deliver_circle(&entry).await;
        }
        Ok(())
    }

    pub async fn send_scheduled_due(&self) -> Result<()> {
        let later = self.store.scheduled_all().await?;
        for entry in self.store.due(now()).await? {
            if later.contains_key(&entry.message_id) {
                self.attempt(&entry).await;
            }
        }
        Ok(())
    }

    pub(crate) async fn retry_contact_now(&self, contact: &str) -> Result<()> {
        for entry in self.store.outbox().await? {
            if entry.contact == contact && entry.attempts > 0 {
                self.attempt(&entry).await;
            }
        }
        for entry in self.store.update_outbox().await? {
            if entry.contact == contact && entry.attempts > 0 {
                self.attempt_update(&entry).await;
            }
        }
        for entry in self.store.circle_outbox().await? {
            if entry.contact == contact && entry.attempts > 0 {
                let _ = self.deliver_circle(&entry).await;
            }
        }
        Ok(())
    }

    pub(crate) async fn something_waits_for(&self, contact: &str) -> Result<bool> {
        let later = self.store.scheduled_all().await?;
        let moment = now();
        let text = self.store.outbox().await?.iter().any(|entry| {
            entry.contact == contact && !later.get(&entry.message_id).is_some_and(|send_at| *send_at > moment)
        });
        Ok(text
            || self.store.update_outbox().await?.iter().any(|entry| entry.contact == contact)
            || self.store.circle_outbox().await?.iter().any(|entry| entry.contact == contact))
    }

    pub async fn retry_now(&self) -> Result<()> {
        let later = self.store.scheduled_all().await?;
        let moment = now();
        for entry in self.store.outbox().await? {
            if later.get(&entry.message_id).is_some_and(|send_at| *send_at > moment) {
                continue;
            }
            self.attempt(&entry).await;
        }
        for entry in self.store.update_outbox().await? {
            self.attempt_update(&entry).await;
        }
        self.deliver_circle_queue().await;
        Ok(())
    }

    pub async fn mark_read(&self, contact: &str) -> Result<()> {
        let unread = self.store.unread(contact).await?;
        if unread.is_empty() {
            return Ok(());
        }
        self.store.advance(&unread, MessageState::Read).await?;
        let _ = self.events.send(Event::MessagesChanged { contact: contact.to_owned() });
        let ids = unread.iter().filter_map(|id| MessageId::parse(id).ok()).collect();
        let contact = self.contact(contact).await?;
        if contact.rules.receipts {
            let _ = self.send_control(&contact, Body::Read { ids }).await;
        }
        Ok(())
    }

    pub async fn seal_signal(&self, contact: &str, body: Body) -> Result<Vec<u8>> {
        let contact = self.contact(contact).await?;
        let plaintext = self.plaintext(&Packet::new(body)).await?;
        let _identity = self.identity.lock().await;
        let mut channel = self.channel(&contact.device_id).await?;
        let sealed = channel.encrypt(&self.device_id, &plaintext)?;
        self.store.save_channel(&contact.device_id, &channel.seal(&self.key)).await?;
        Ok(sealed.encode())
    }

    pub(crate) async fn attempt(&self, entry: &OutboxEntry) {
        let Err(error) = self.deliver(entry).await else {
            let _ = self.store.unschedule(&entry.message_id).await;
            return;
        };
        if error.downcast_ref::<MailboxRejected>().is_some() {
            if let Ok(true) = self.store.mark_not_sent(&entry.message_id).await {
                let _ = self.events.send(Event::MessagesChanged { contact: entry.contact.clone() });
            }
            return;
        }
        let attempts = entry.attempts + 1;
        let next = now() + retry_delay(attempts).as_millis() as i64;
        let _ = self.store.reschedule(&entry.message_id, attempts, next, entry.in_mailbox).await;
    }

    async fn deliver(&self, entry: &OutboxEntry) -> Result<()> {
        let Some(message) = self.store.message(&entry.message_id).await? else {
            return self.store.dequeue(&entry.message_id).await;
        };
        if self.store.is_deleted(&entry.message_id).await? {
            return self.store.dequeue(&entry.message_id).await;
        }
        let contact = self.contact(&entry.contact).await?;
        if !contact.introduced {
            self.introduce(&contact).await?;
        }
        if let Some(file) = self.store.file(&entry.message_id).await? {
            return self.deliver_file(entry, &contact, &message, &file).await;
        }
        let reply_to = match self.store.reply_to(&message.message_id).await? {
            Some(quoted) => Some(MessageId::parse(&quoted)?),
            None => None,
        };
        let packet = Packet::resend(MessageId::parse(&message.message_id)?, message.sent_at as u64, Body::Message { text: message.body, reply_to });
        let attempts = entry.attempts + 1;
        let copy = mail_copy_due(entry.in_mailbox, entry.mailed_at, now());
        let Some(route) = self.transmit_unless_taken_back(&contact, &packet, copy, &entry.message_id).await? else {
            return self.store.dequeue(&entry.message_id).await;
        };
        if copy && matches!(route, Route::Mailbox) {
            self.store.mailed(&entry.message_id, now()).await?;
        }
        let (next, in_mailbox) = match route {
            Route::Direct => (RECEIPT_WAIT, entry.in_mailbox),
            Route::Mailbox => (MAILBOX_WAIT, true),
            Route::Unreachable => (retry_delay(attempts), entry.in_mailbox),
        };
        if next != retry_delay(attempts) || in_mailbox {
            // advance only moves forward, so a receipt that came meanwhile is kept
            self.store.advance(std::slice::from_ref(&entry.message_id), MessageState::Sent).await?;
            let _ = self.events.send(Event::MessagesChanged { contact: contact.device_id.clone() });
        }
        if self.store.outbox().await?.iter().any(|e| e.message_id == entry.message_id) {
            self.store.reschedule(&entry.message_id, attempts, now() + next.as_millis() as i64, in_mailbox).await?;
        }
        Ok(())
    }

    async fn tell(&self, contact: &Contact, packet: &Packet) -> Result<()> {
        let at = now();
        let entry = UpdateEntry {
            packet_id: packet.id.to_string(),
            contact: contact.device_id.clone(),
            packet: packet.encode(),
            created_at: at,
            attempts: 0,
            next_attempt: at,
            in_mailbox: false,
        };
        self.store.enqueue_update(&entry.packet_id, &entry.contact, &entry.packet, at).await?;
        self.attempt_update(&entry).await;
        Ok(())
    }

    pub(crate) async fn attempt_update(&self, entry: &UpdateEntry) {
        if now() - entry.created_at > UPDATE_LIFETIME.as_millis() as i64 {
            let _ = self.store.dequeue_update(&entry.packet_id).await;
            return;
        }
        let attempts = entry.attempts + 1;
        let (next, in_mailbox) = match self.deliver_update(entry).await {
            // Old apps never ack updates, so back off on direct sends too
            Ok(Route::Direct) => (retry_delay(attempts).max(RECEIPT_WAIT), entry.in_mailbox),
            Ok(Route::Mailbox) => (MAILBOX_WAIT, true),
            Ok(Route::Unreachable) => (retry_delay(attempts), entry.in_mailbox),
            Err(error) if error.downcast_ref::<MailboxRejected>().is_some() => {
                let _ = self.store.dequeue_update(&entry.packet_id).await;
                return;
            }
            Err(_) => (retry_delay(attempts), entry.in_mailbox),
        };
        let _ = self.store.reschedule_update(&entry.packet_id, attempts, now() + next.as_millis() as i64, in_mailbox).await;
    }

    async fn deliver_update(&self, entry: &UpdateEntry) -> Result<Route> {
        let contact = self.contact(&entry.contact).await?;
        let packet = Packet::decode(&entry.packet)?;
        self.transmit_as(&contact, &packet, !entry.in_mailbox).await
    }

    pub(crate) async fn send_control(&self, contact: &Contact, body: Body) -> Result<()> {
        self.transmit(contact, &Packet::new(body)).await?;
        Ok(())
    }

    async fn plaintext(&self, packet: &Packet) -> Result<Vec<u8>> {
        if !self.tells_mailbox.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(packet.encode());
        }
        Ok(Packet { mailbox: Some(self.mailbox().await?), ..packet.clone() }.encode())
    }

    async fn seal_for(&self, contact: &Contact, packet: &Packet) -> Result<Vec<u8>> {
        let plaintext = self.plaintext(packet).await?;
        let _identity = self.identity.lock().await;
        let mut channel = self.channel(&contact.device_id).await?;
        let sealed = channel.encrypt(&self.device_id, &plaintext)?;
        self.store.save_channel(&contact.device_id, &channel.seal(&self.key)).await?;
        Ok(sealed.encode())
    }

    pub(crate) async fn transmit_direct(&self, contact: &Contact, packet: &Packet) -> Result<bool> {
        let bytes = self.seal_for(contact, packet).await?;
        let card = ContactCard::decode(&contact.card)?;
        let peer = Peer { device_id: contact.device_id.clone(), capability: card.route_capability() };
        Ok(self.transport.send_direct(&peer, bytes).await.unwrap_or(false))
    }

    pub(crate) async fn transmit_direct_call(&self, contact: &Contact, packet: &Packet) -> Result<bool> {
        let bytes = self.seal_for(contact, packet).await?;
        let card = ContactCard::decode(&contact.card)?;
        let peer = Peer { device_id: contact.device_id.clone(), capability: card.route_capability() };
        Ok(self.transport.send_direct_call(&peer, bytes).await.unwrap_or(false))
    }

    pub(crate) async fn transmit_open(&self, contact: &Contact, packet: &Packet) -> Result<bool> {
        let bytes = self.seal_for(contact, packet).await?;
        let card = ContactCard::decode(&contact.card)?;
        let peer = Peer { device_id: contact.device_id.clone(), capability: card.route_capability() };
        Ok(self.transport.send_open(&peer, bytes).await.unwrap_or(false))
    }

    pub(crate) fn pongs_from(&self, contact: &str) -> (u64, tokio::sync::watch::Receiver<HashMap<String, u64>>) {
        let pongs = self.pongs.subscribe();
        let heard = pongs.borrow().get(contact).copied().unwrap_or(0);
        (heard, pongs)
    }

    pub(crate) async fn drop_connection(&self, contact: &str) {
        self.transport.disconnect(contact).await;
    }

    pub(crate) async fn open_direct_call(&self, contact: &Contact) -> Result<bool> {
        let card = ContactCard::decode(&contact.card)?;
        let peer = Peer { device_id: contact.device_id.clone(), capability: card.route_capability() };
        Ok(self.transport.open_direct_call(&peer).await.unwrap_or(false))
    }

    pub(crate) async fn transmit(&self, contact: &Contact, packet: &Packet) -> Result<Route> {
        self.transmit_as(contact, packet, true).await
    }

    async fn transmit_as(&self, contact: &Contact, packet: &Packet, copy: bool) -> Result<Route> {
        Ok(self.transmit_checked(contact, packet, copy, None).await?.unwrap_or(Route::Unreachable))
    }

    async fn transmit_unless_taken_back(&self, contact: &Contact, packet: &Packet, copy: bool, message_id: &str) -> Result<Option<Route>> {
        self.transmit_checked(contact, packet, copy, Some(message_id)).await
    }

    async fn still_to_send(&self, message_id: &str) -> Result<bool> {
        Ok(self.store.message(message_id).await?.is_some() && !self.store.is_deleted(message_id).await?)
    }

    async fn transmit_checked(&self, contact: &Contact, packet: &Packet, copy: bool, message: Option<&str>) -> Result<Option<Route>> {
        let bytes = self.seal_for(contact, packet).await?;
        let card = ContactCard::decode(&contact.card)?;
        let peer = Peer { device_id: contact.device_id.clone(), capability: card.route_capability() };

        let mailbox = contact.mailbox && self.mailbox().await?;
        let direct = match message {
            Some(message_id) => {
                let open = self.transport.reach(&peer, mailbox).await.unwrap_or(false);
                if !self.still_to_send(message_id).await? {
                    return Ok(None);
                }
                if open { self.transport.send_open(&peer, bytes.clone()).await } else { Ok(false) }
            }
            None if mailbox => self.transport.try_direct(&peer, bytes.clone()).await,
            None => self.transport.send_direct(&peer, bytes.clone()).await,
        };
        if direct.unwrap_or(false) {
            return Ok(Some(Route::Direct));
        }
        if mailbox && !copy {
            return Ok(Some(Route::Mailbox));
        }
        if mailbox {
            if let Some(message_id) = message {
                if !self.still_to_send(message_id).await? {
                    return Ok(None);
                }
            }
            // sealed so the router only sees who it's for
            let mail = self.wrap_for(&contact.device_id, bytes).await?;
            self.transport.send_mailbox(&peer, mail).await.context("the mailbox is not reachable")?;
            return Ok(Some(Route::Mailbox));
        }
        Ok(Some(Route::Unreachable))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_in_the_mailbox_gets_a_new_copy_there_once_a_day() {
        let day = MAILBOX_COPY.as_millis() as i64;
        assert!(mail_copy_due(false, None, 1_000), "the first copy goes as always");
        assert!(mail_copy_due(true, None, 1_000), "queued before the time was kept: a copy now");
        assert!(!mail_copy_due(true, Some(1_000), 1_000 + MAILBOX_WAIT.as_millis() as i64));
        assert!(!mail_copy_due(true, Some(1_000), 1_000 + day - 1));
        assert!(mail_copy_due(true, Some(1_000), 1_000 + day));
    }

    #[test]
    fn unreachable_contacts_are_retried_ever_more_slowly_up_to_a_limit() {
        assert!(retry_delay(1) < retry_delay(2));
        assert!(retry_delay(2) < retry_delay(4));
        assert_eq!(retry_delay(50), MAX_RETRY_DELAY);
    }
}
