//! Circles (2026-09-27): a small closed group of contacts, with no server knowing it exists.
//!
//! A circle is its signed card (ft-circles). Everything said in it goes to every member through
//! the 1:1 Olm channel this phone has with each of them, directly or through their mailbox,
//! exactly like a message to one contact: the router sees n sealed envelopes and nothing more.
//! The same packet, with the same id, reaches every member, and each member's receipt clears
//! their own entry in the circle outbox.
//!
//! Membership is the admins' to change: a phone replaces the card it holds only with a later
//! revision signed by an admin of the card it holds. Whoever is put in a circle learns every
//! member's Contact Card from the circle card and opens a channel with each without scanning
//! anyone; those members are contacts of the circle only (`via_circle`), in no list and no
//! requests until they write on their own or the user chooses them.

use anyhow::{anyhow, bail, ensure, Result};
use ft_billing::Doing;
use ft_circles::{CircleCard, Member, Standing, MAX_MEMBERS};
use ft_contacts::ContactCard;
use ft_crypto::Channel;
use ft_protocol::{Body, MessageId, Packet};
use ft_storage::{CircleMember, CircleMessage, CircleOutboxEntry, CircleRecord, Contact, MessageState, NewContact};

use crate::{clean_name, now, retry_delay, Core, Event, Route, MAILBOX_WAIT, RECEIPT_WAIT};

/// What travels in the circle outbox besides texts: the card, and a goodbye.
const KIND_TEXT: &str = "text";
const KIND_CARD: &str = "card";
const KIND_LEAVE: &str = "leave";

impl Core {
    /// Makes a circle of these contacts, with this phone as its admin, and hands each of them
    /// the card. The contacts must be chosen ones of the main list, or of the open session.
    pub async fn create_circle(&self, name: &str, contacts: &[String], session: Option<&str>) -> Result<String> {
        let name = clean_name(name);
        ensure!(!name.is_empty(), "a circle needs a name");
        ensure!(!contacts.is_empty(), "a circle needs someone in it");
        ensure!(contacts.len() < MAX_MEMBERS, "a circle has at most {MAX_MEMBERS} members");
        if let Some(session) = session {
            ensure!(self.is_session_open(session), "that session is not open");
        }
        self.allowed(Doing::Start).await?;
        let mut members = vec![Member::from_card(&self.my_card_in(session).await?)];
        for contact in contacts {
            let contact = self.circle_worthy(contact, session).await?;
            members.push(Member::from_card(&ContactCard::decode(&contact.card)?));
        }
        let card = {
            let identity = self.identity.lock().await;
            CircleCard::create(&identity, &name, members, now() as u64)?
        };
        let me = self.device_id.to_string();
        self.adopt_card(&card, session, &me).await?;
        self.queue_card(card.id(), contacts).await?;
        Ok(card.id().to_owned())
    }

    /// A contact who may be put in a circle of the list or session: known, chosen, not blocked.
    async fn circle_worthy(&self, contact: &str, session: Option<&str>) -> Result<Contact> {
        let contact = self.contact(contact).await?;
        ensure!(contact.accepted && !contact.blocked, "{} is not a contact of yours", contact.name);
        ensure!(contact.session.as_deref() == session, "{} belongs to another list", contact.name);
        Ok(contact)
    }

    /// Puts a contact in the circle: only an admin, and the contact must be one of the same
    /// list. Everyone gets the new card, the newcomer included, who thus meets every member.
    pub async fn invite_to_circle(&self, circle: &str, contact: &str) -> Result<()> {
        let (record, held) = self.circle_to_change(circle).await?;
        let newcomer = self.circle_worthy(contact, record.session.as_deref()).await?;
        ensure!(!self.circle_has(circle, &newcomer.device_id).await?, "{} is in the circle already", newcomer.name);
        let member = Member::from_card(&ContactCard::decode(&newcomer.card)?);
        let revised = self.revise(&held, circle, |draft| draft.members.push(member)).await?;
        self.adopt_card(&revised, record.session.as_deref(), self.device_id.as_str()).await?;
        self.queue_card_to_members(circle).await
    }

    /// Takes a member out: only an admin, and never oneself (that is leaving). They get the new
    /// card too, so they know; the others stop sending them anything.
    pub async fn remove_from_circle(&self, circle: &str, contact: &str) -> Result<()> {
        let (record, held) = self.circle_to_change(circle).await?;
        ensure!(contact != self.device_id.as_str(), "leave the circle instead");
        ensure!(self.circle_has(circle, contact).await?, "they are not in the circle");
        let gone = contact.to_owned();
        let revised = self
            .revise(&held, circle, |draft| {
                draft.members.retain(|member| member.device_id != gone);
                draft.admins.retain(|admin| admin != &gone);
            })
            .await?;
        self.adopt_card(&revised, record.session.as_deref(), self.device_id.as_str()).await?;
        self.queue_card_to_members(circle).await?;
        // The one taken out learns it with the new card, and nothing else from now on.
        self.queue_card(circle, std::slice::from_ref(&gone)).await
    }

    pub async fn rename_circle(&self, circle: &str, name: &str) -> Result<()> {
        let name = clean_name(name);
        ensure!(!name.is_empty(), "a circle needs a name");
        let (record, held) = self.circle_to_change(circle).await?;
        let revised = self.revise(&held, circle, |draft| draft.name = name).await?;
        self.adopt_card(&revised, record.session.as_deref(), self.device_id.as_str()).await?;
        self.queue_card_to_members(circle).await
    }

    /// Whether only the admins write; everyone else reads.
    pub async fn set_circle_admins_only(&self, circle: &str, admins_only: bool) -> Result<()> {
        let (record, held) = self.circle_to_change(circle).await?;
        let revised = self.revise(&held, circle, |draft| draft.admins_only = admins_only).await?;
        self.adopt_card(&revised, record.session.as_deref(), self.device_id.as_str()).await?;
        self.queue_card_to_members(circle).await
    }

    /// Makes a member an admin, or an admin a plain member. The last admin cannot step down.
    pub async fn set_circle_admin(&self, circle: &str, contact: &str, admin: bool) -> Result<()> {
        let (record, held) = self.circle_to_change(circle).await?;
        ensure!(self.circle_has(circle, contact).await?, "they are not in the circle");
        let who = contact.to_owned();
        let revised = self
            .revise(&held, circle, |draft| {
                draft.admins.retain(|one| one != &who);
                if admin {
                    draft.admins.push(who.clone());
                }
            })
            .await?;
        self.adopt_card(&revised, record.session.as_deref(), self.device_id.as_str()).await?;
        self.queue_card_to_members(circle).await
    }

    /// Leaves the circle: every member is told, and what was said stays here, read only, until
    /// the circle is forgotten. An admin who leaves alone leaves the circle without one: it
    /// freezes, so hand it over first.
    pub async fn leave_circle(&self, circle: &str) -> Result<()> {
        let record = self.store.circle(circle).await?.ok_or_else(|| anyhow!("unknown circle"))?;
        ensure!(!record.left, "you are not in that circle");
        let others = self.other_members(circle).await?;
        let packet = Packet::new(Body::CircleLeave { circle: circle.to_owned() });
        self.queue_control(&record, packet, KIND_LEAVE, &others).await?;
        self.store.save_circle(&CircleRecord { left: true, ..record }).await?;
        self.note(circle, "left", &self.device_id.to_string(), &self.name().await?.unwrap_or_default()).await?;
        self.deliver_circle_queue().await;
        Ok(())
    }

    /// Erases a circle this phone is no longer in, with everything said in it. Members known
    /// through it alone go with it.
    pub async fn forget_circle(&self, circle: &str) -> Result<()> {
        let record = self.store.circle(circle).await?.ok_or_else(|| anyhow!("unknown circle"))?;
        ensure!(record.left, "leave the circle first");
        let members = self.store.circle_members(circle).await?;
        self.store.remove_circle(circle).await?;
        for member in members {
            self.prune_circle_contact(&member.device_id).await?;
        }
        let _ = self.events.send(Event::CirclesChanged);
        Ok(())
    }

    /// Says something in the circle: stored once, queued once per member, and delivered to each
    /// by whichever way works, like a message to one contact.
    pub async fn send_circle_text(&self, circle: &str, text: &str) -> Result<String> {
        let text = text.trim();
        ensure!(!text.is_empty(), "nothing to send");
        let record = self.store.circle(circle).await?.ok_or_else(|| anyhow!("unknown circle"))?;
        ensure!(!record.left, "you are not in that circle");
        let card = CircleCard::decode(&record.card)?;
        ensure!(!card.admins_only() || card.is_admin(self.device_id.as_str()), "only the admins write in this circle");
        self.allowed(Doing::Reply).await?;
        let others = self.other_members(circle).await?;
        let packet = Packet::new(Body::CircleMessage { circle: circle.to_owned(), text: text.to_owned() });
        let message_id = packet.id.to_string();
        self.store
            .insert_circle_message(&CircleMessage {
                message_id: message_id.clone(),
                circle: circle.to_owned(),
                sender: self.device_id.to_string(),
                outgoing: true,
                kind: KIND_TEXT.to_owned(),
                body: text.to_owned(),
                sent_at: packet.sent_at as i64,
                received_at: packet.sent_at as i64,
                state: MessageState::Pending,
            })
            .await?;
        for member in &others {
            self.store.circle_enqueue(&message_id, member, now()).await?;
        }
        let _ = self.events.send(Event::CircleMessagesChanged { circle: circle.to_owned() });
        self.deliver_circle_queue().await;
        Ok(message_id)
    }

    /// What others said has been shown. Nothing travels: a circle tells nobody when you read.
    pub async fn mark_circle_read(&self, circle: &str) -> Result<()> {
        let unread = self.store.circle_unread(circle).await?;
        if unread.is_empty() {
            return Ok(());
        }
        self.store.advance_circle(&unread, MessageState::Read).await?;
        let _ = self.events.send(Event::CircleMessagesChanged { circle: circle.to_owned() });
        Ok(())
    }

    /// The circles of the main list, or of an open session.
    pub async fn circles(&self, session: Option<&str>) -> Result<Vec<CircleRecord>> {
        self.store.circles(session).await
    }

    pub async fn circle_members(&self, circle: &str) -> Result<Vec<CircleMember>> {
        self.store.circle_members(circle).await
    }

    /// Whether this phone may change the circle now.
    pub async fn is_circle_admin(&self, circle: &str) -> Result<bool> {
        let record = self.store.circle(circle).await?.ok_or_else(|| anyhow!("unknown circle"))?;
        Ok(!record.left && CircleCard::decode(&record.card)?.is_admin(self.device_id.as_str()))
    }

    // ---- what comes in ----

    /// A circle card from a contact (made, changed, or an invitation). Only from its signer,
    /// and only if it follows the card held; a new circle only from one of its admins, and only
    /// if this phone is in it. Always acknowledged, so the sender stops retrying.
    pub(crate) async fn circle_card_received(&self, from: &Contact, id: MessageId, bytes: &[u8]) -> Result<()> {
        let ack = |body| async move {
            let _ = self.send_control(from, body).await;
        };
        let card = match CircleCard::decode(bytes) {
            Ok(card) if card.signer().as_str() == from.device_id => card,
            // Unreadable, or signed by someone else: nothing to keep, nothing to retry.
            _ => {
                ack(Body::Delivered { ids: vec![id] }).await;
                bail!("a circle card that cannot be read, or sent by someone other than its signer");
            }
        };
        let me = self.device_id.as_str();
        match self.store.circle(card.id()).await? {
            Some(record) => {
                let held = CircleCard::decode(&record.card)?;
                let standing = held.judge(&card);
                ack(Body::Delivered { ids: vec![id] }).await;
                match standing? {
                    Standing::Newer => self.adopt_card(&card, record.session.as_deref(), &from.device_id).await,
                    Standing::Same | Standing::Older => Ok(()),
                }
            }
            None => {
                if !card.stands_alone() || !card.is_member(me) {
                    ack(Body::Delivered { ids: vec![id] }).await;
                    return Ok(());
                }
                // A new circle is only taken from someone the user chose (A5): a stranger's
                // waits, unacknowledged, and reaches this phone if they are ever accepted.
                if !from.accepted {
                    return Ok(());
                }
                ack(Body::Delivered { ids: vec![id] }).await;
                self.adopt_card(&card, from.session.as_deref(), &from.device_id).await
            }
        }
    }

    /// A text said in a circle by a member. From someone the card does not name, or in a circle
    /// this phone does not know yet, it is dropped without a word: the sender retries, and the
    /// card that makes sense of it may arrive meanwhile.
    pub(crate) async fn circle_text_received(&self, from: &Contact, id: MessageId, sent_at: u64, circle: &str, text: String) -> Result<()> {
        let Some(record) = self.store.circle(circle).await? else { return Ok(()) };
        if record.left {
            // Not in it any more: they stop retrying and see only sent.
            let _ = self.send_control(from, Body::Received { ids: vec![id] }).await;
            return Ok(());
        }
        if !self.circle_has(circle, &from.device_id).await? {
            return Ok(());
        }
        let card = CircleCard::decode(&record.card)?;
        if card.admins_only() && !card.is_admin(&from.device_id) {
            let _ = self.send_control(from, Body::Received { ids: vec![id] }).await;
            return Ok(());
        }
        let stored = self
            .store
            .insert_circle_message(&CircleMessage {
                message_id: id.to_string(),
                circle: circle.to_owned(),
                sender: from.device_id.clone(),
                outgoing: false,
                kind: KIND_TEXT.to_owned(),
                body: text,
                sent_at: sent_at as i64,
                received_at: now(),
                state: MessageState::Delivered,
            })
            .await?;
        if stored && !self.circle_silent(&record) {
            let _ = self.events.send(Event::CircleMessagesChanged { circle: circle.to_owned() });
        }
        let _ = self.send_control(from, self.acknowledgement(from, id)).await;
        Ok(())
    }

    /// A member leaves: out of this phone's copy at once; an admin's next change consolidates it.
    pub(crate) async fn circle_leave_received(&self, from: &Contact, id: MessageId, circle: &str) -> Result<()> {
        let _ = self.send_control(from, Body::Delivered { ids: vec![id] }).await;
        let Some(record) = self.store.circle(circle).await? else { return Ok(()) };
        let members = self.store.circle_members(circle).await?;
        let Some(leaver) = members.iter().find(|member| member.device_id == from.device_id) else { return Ok(()) };
        let name = leaver.name.clone();
        let kept: Vec<CircleMember> = members.into_iter().filter(|member| member.device_id != from.device_id).collect();
        self.store.set_circle_members(circle, &kept).await?;
        self.store.circle_dequeue_member(circle, &from.device_id).await?;
        self.note(circle, "left", &from.device_id, &name).await?;
        self.prune_circle_contact(&from.device_id).await?;
        if !self.circle_silent(&record) {
            let _ = self.events.send(Event::CirclesChanged);
        }
        Ok(())
    }

    /// A member's receipt for a circle packet: their entry goes; the message is delivered once
    /// nobody is left to reach. `Received` means they will not say more: sent, at most.
    pub(crate) async fn circle_receipt(&self, contact: &str, id: &str, state: MessageState) -> Result<bool> {
        if !self.store.circle_dequeue(id, contact).await? {
            return Ok(false);
        }
        let Some(message) = self.store.circle_message(id).await? else { return Ok(true) };
        let reached = match state {
            MessageState::Sent | MessageState::Pending => MessageState::Sent,
            MessageState::Delivered | MessageState::Read => MessageState::Delivered,
        };
        if reached == MessageState::Delivered && !self.store.circle_pending(id).await?.is_empty() {
            self.store.advance_circle(std::slice::from_ref(&message.message_id), MessageState::Sent).await?;
        } else {
            self.store.advance_circle(std::slice::from_ref(&message.message_id), reached).await?;
        }
        if message.kind == KIND_TEXT {
            let _ = self.events.send(Event::CircleMessagesChanged { circle: message.circle });
        } else if message.kind == KIND_CARD && !self.circle_has(&message.circle, contact).await? {
            // The card told them they are out; if they were known through this circle alone,
            // there is nothing left to know them by.
            self.prune_circle_contact(contact).await?;
        }
        Ok(true)
    }

    // ---- delivery ----

    /// One delivery attempt of a circle packet to one member, like `deliver` for a contact.
    pub(crate) async fn deliver_circle(&self, entry: &CircleOutboxEntry) -> Result<()> {
        let Some(message) = self.store.circle_message(&entry.message_id).await? else {
            self.store.circle_dequeue(&entry.message_id, &entry.contact).await?;
            return Ok(());
        };
        let Some(record) = self.store.circle(&message.circle).await? else {
            self.store.circle_dequeue(&entry.message_id, &entry.contact).await?;
            return Ok(());
        };
        let Some(contact) = self.store.contact(&entry.contact).await? else {
            self.store.circle_dequeue(&entry.message_id, &entry.contact).await?;
            return Ok(());
        };
        if contact.blocked || (record.left && message.kind != KIND_LEAVE) {
            self.store.circle_dequeue(&entry.message_id, &entry.contact).await?;
            return Ok(());
        }
        let body = match message.kind.as_str() {
            KIND_TEXT => Body::CircleMessage { circle: message.circle.clone(), text: message.body.clone() },
            // Always the card as it stands: a retry never hands out an old revision.
            KIND_CARD => Body::CircleCard { card: record.card.clone() },
            KIND_LEAVE => Body::CircleLeave { circle: message.circle.clone() },
            _ => {
                self.store.circle_dequeue(&entry.message_id, &entry.contact).await?;
                return Ok(());
            }
        };
        if !contact.introduced {
            self.introduce(&contact).await?;
        }
        let packet = Packet::resend(MessageId::parse(&message.message_id)?, message.sent_at as u64, body);
        let attempts = entry.attempts + 1;
        let (next, in_mailbox) = match self.transmit(&contact, &packet).await? {
            Route::Direct => (RECEIPT_WAIT, entry.in_mailbox),
            Route::Mailbox => (MAILBOX_WAIT, true),
            Route::Unreachable => (retry_delay(attempts), entry.in_mailbox),
        };
        if (next != retry_delay(attempts) || in_mailbox) && message.kind == KIND_TEXT {
            self.store.advance_circle(std::slice::from_ref(&message.message_id), MessageState::Sent).await?;
            let _ = self.events.send(Event::CircleMessagesChanged { circle: message.circle.clone() });
        }
        if !self.store.circle_pending(&entry.message_id).await?.iter().any(|e| e.contact == entry.contact) {
            return Ok(());
        }
        self.store.circle_reschedule(&entry.message_id, &entry.contact, attempts, now() + next.as_millis() as i64, in_mailbox).await
    }

    /// Tries every circle entry now.
    pub(crate) async fn deliver_circle_queue(&self) {
        if let Ok(entries) = self.store.circle_outbox().await {
            for entry in entries {
                let _ = self.deliver_circle(&entry).await;
            }
        }
    }

    // ---- the card, held and changed ----

    /// The circle and its card, for a change this phone may make.
    async fn circle_to_change(&self, circle: &str) -> Result<(CircleRecord, CircleCard)> {
        let record = self.store.circle(circle).await?.ok_or_else(|| anyhow!("unknown circle"))?;
        ensure!(!record.left, "you are not in that circle");
        let held = CircleCard::decode(&record.card)?;
        ensure!(held.is_admin(self.device_id.as_str()), "only an admin can change the circle");
        Ok((record, held))
    }

    /// Signs the next revision. Members who left since the card was signed are consolidated
    /// out of it, and out of the admins, first.
    async fn revise(&self, held: &CircleCard, circle: &str, change: impl FnOnce(&mut ft_circles::Draft)) -> Result<CircleCard> {
        let present: Vec<String> = self.store.circle_members(circle).await?.into_iter().map(|member| member.device_id).collect();
        let identity = self.identity.lock().await;
        held.revise(&identity, |draft| {
            draft.members.retain(|member| present.contains(&member.device_id));
            draft.admins.retain(|admin| present.contains(admin));
            change(draft);
        })
    }

    /// Keeps a card as the circle's truth and makes this phone match it: the members, the
    /// contacts it needs to reach them, and a line in the conversation for what changed.
    async fn adopt_card(&self, card: &CircleCard, session: Option<&str>, by: &str) -> Result<()> {
        let me = self.device_id.as_str();
        let before = self.store.circle(card.id()).await?;
        let old_members = match &before {
            Some(_) => self.store.circle_members(card.id()).await?,
            None => Vec::new(),
        };
        let left = !card.is_member(me);
        let name = clean_name(card.name());
        self.store
            .save_circle(&CircleRecord {
                id: card.id().to_owned(),
                name: name.clone(),
                card: card.encode(),
                revision: card.revision() as i64,
                admins_only: card.admins_only(),
                session: session.map(str::to_owned),
                left,
                created_at: card.created_at() as i64,
            })
            .await?;
        let members: Vec<CircleMember> = card
            .members()
            .iter()
            .map(|member| CircleMember {
                device_id: member.device_id.clone(),
                name: member_name(member),
                admin: card.is_admin(&member.device_id),
            })
            .collect();
        self.store.set_circle_members(card.id(), &members).await?;

        // Everyone in it is someone this phone can reach from now on.
        for member in card.members() {
            if member.device_id != me {
                self.meet(member, session).await?;
            }
        }

        // What changed, told in the conversation.
        match &before {
            None => self.note(card.id(), "created", by, &name).await?,
            Some(before) => {
                if before.name != name {
                    self.note(card.id(), "renamed", by, &name).await?;
                }
                for member in &members {
                    if !old_members.iter().any(|old| old.device_id == member.device_id) {
                        self.note(card.id(), "joined", by, &member.name).await?;
                    }
                }
                for old in &old_members {
                    if !members.iter().any(|member| member.device_id == old.device_id) {
                        self.note(card.id(), "removed", by, &old.name).await?;
                        self.store.circle_dequeue_member(card.id(), &old.device_id).await?;
                        if by != me {
                            self.prune_circle_contact(&old.device_id).await?;
                        }
                    }
                }
            }
        }
        if left {
            for member in &members {
                self.store.circle_dequeue_member(card.id(), &member.device_id).await?;
            }
        }
        let _ = self.events.send(Event::CirclesChanged);
        let _ = self.events.send(Event::CircleMessagesChanged { circle: card.id().to_owned() });
        Ok(())
    }

    /// A member of a circle becomes someone this phone can write to: a contact of the circle
    /// only, unless they were known already, with an Olm channel opened from their card.
    async fn meet(&self, member: &Member, session: Option<&str>) -> Result<()> {
        if self.store.contact(&member.device_id).await?.is_some() {
            return Ok(());
        }
        let card = ContactCard::decode(&member.card)?;
        self.store
            .add_contact(&NewContact {
                device_id: member.device_id.clone(),
                name: member_name(member),
                card: member.card.clone(),
                mailbox: card.mailbox(),
                session: session.map(str::to_owned),
                receipts: self.receipts_default().await?,
                accepted: false,
            })
            .await?;
        self.store.set_via_circle(&member.device_id, true).await?;
        let identity = self.identity.lock().await;
        if self.store.channel(&member.device_id).await?.is_none() {
            let channel = Channel::open(&identity, &card.contact_keys())?;
            self.store.save_channel(&member.device_id, &channel.seal(&self.key)).await?;
        }
        Ok(())
    }

    /// Someone known through a circle alone, and in none now, is forgotten, unless they wrote.
    async fn prune_circle_contact(&self, device_id: &str) -> Result<()> {
        let Some(contact) = self.store.contact(device_id).await? else { return Ok(()) };
        if !contact.via_circle || contact.accepted || contact.blocked {
            return Ok(());
        }
        if !self.store.circles_with(device_id).await?.is_empty() || !self.store.messages(device_id, 1).await?.is_empty() {
            return Ok(());
        }
        self.store.remove_contact(device_id).await
    }

    /// A line in the conversation for something that happened: who did it, and to whom or what.
    async fn note(&self, circle: &str, kind: &str, by: &str, body: &str) -> Result<()> {
        let at = now();
        self.store
            .insert_circle_message(&CircleMessage {
                message_id: MessageId::new().to_string(),
                circle: circle.to_owned(),
                sender: by.to_owned(),
                outgoing: by == self.device_id.as_str(),
                kind: kind.to_owned(),
                body: body.to_owned(),
                sent_at: at,
                received_at: at,
                state: MessageState::Read,
            })
            .await?;
        Ok(())
    }

    /// Queues the card, as it stands, for these members, and tries at once.
    async fn queue_card(&self, circle: &str, to: &[String]) -> Result<()> {
        let record = self.store.circle(circle).await?.ok_or_else(|| anyhow!("unknown circle"))?;
        let packet = Packet::new(Body::CircleCard { card: record.card.clone() });
        self.queue_control(&record, packet, KIND_CARD, to).await?;
        self.deliver_circle_queue().await;
        Ok(())
    }

    async fn queue_card_to_members(&self, circle: &str) -> Result<()> {
        let others = self.other_members(circle).await?;
        self.queue_card(circle, &others).await
    }

    async fn queue_control(&self, record: &CircleRecord, packet: Packet, kind: &str, to: &[String]) -> Result<()> {
        if to.is_empty() {
            return Ok(());
        }
        let message_id = packet.id.to_string();
        self.store
            .insert_circle_message(&CircleMessage {
                message_id: message_id.clone(),
                circle: record.id.clone(),
                sender: self.device_id.to_string(),
                outgoing: true,
                kind: kind.to_owned(),
                body: String::new(),
                sent_at: packet.sent_at as i64,
                received_at: packet.sent_at as i64,
                state: MessageState::Pending,
            })
            .await?;
        for member in to {
            self.store.circle_enqueue(&message_id, member, now()).await?;
        }
        Ok(())
    }

    /// Every member but this phone.
    async fn other_members(&self, circle: &str) -> Result<Vec<String>> {
        Ok(self
            .store
            .circle_members(circle)
            .await?
            .into_iter()
            .map(|member| member.device_id)
            .filter(|member| member != self.device_id.as_str())
            .collect())
    }

    async fn circle_has(&self, circle: &str, device_id: &str) -> Result<bool> {
        Ok(self.store.circle_members(circle).await?.iter().any(|member| member.device_id == device_id))
    }

    /// Whether what happens in the circle must make no noise: it belongs to a closed session.
    fn circle_silent(&self, record: &CircleRecord) -> bool {
        record.session.as_deref().is_some_and(|session| !self.is_session_open(session))
    }

    pub(crate) fn is_session_open(&self, session: &str) -> bool {
        self.open_sessions.lock().expect("sessions poisoned").contains_key(session)
    }
}

/// How a member is called from their card, tamed (B6), or a short id if the card says nothing.
fn member_name(member: &Member) -> String {
    member
        .name
        .as_deref()
        .map(clean_name)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| member.device_id.chars().take(9).collect())
}
