use std::time::Duration;

use anyhow::{anyhow, bail, ensure, Result};
use ft_contacts::{ContactCard, RouteCapability};
use ft_crypto::Channel;
use ft_identity::DeviceId;
use ft_protocol::{Body, Packet};
use ft_push::{caller_tag, CallRule, CallerRule};
use ft_storage::{Contact, ContactRules, Conversation, NewContact};

use crate::{Core, Event, Peer, BUNDLED_CONTACT, BUNDLED_DUE, CARD_STALE};

impl Core {
    pub async fn card_stale(&self) -> Result<bool> {
        Ok(self.store.setting(CARD_STALE).await?.as_deref() == Some("1"))
    }

    pub async fn reintroduce(&self) -> Result<()> {
        for contact in self.store.all_contacts().await? {
            if contact.blocked || self.quiet_until_written(&contact).await {
                continue;
            }
            let _ = self.introduce(&contact).await;
        }
        self.store.set_setting(CARD_STALE, "0").await
    }

    pub async fn renew_link(&self, session: Option<&str>) -> Result<[[u8; 32]; 8]> {
        let fresh = RouteCapability::generate();
        match session {
            None => {
                self.store.set_route_capability(fresh.as_bytes()).await?;
                *self.capability.write().expect("capability poisoned") = fresh;
            }
            Some(session) => {
                let slot = self.store.session_slot(session).await?.ok_or_else(|| anyhow!("the session has no slot"))?;
                self.store.set_spare_capability(slot, fresh.as_bytes()).await?;
            }
        }
        let mut contacts = match session {
            None => self.store.contacts().await?,
            Some(session) => self.store.session_contacts(session).await?,
        };
        let mut known: std::collections::HashSet<String> = contacts.iter().map(|contact| contact.device_id.clone()).collect();
        for circle in self.store.circles(session).await?.into_iter().filter(|circle| !circle.left) {
            for member in self.store.circle_members(&circle.id).await? {
                if member.device_id == self.device_id.as_str() || !known.insert(member.device_id.clone()) {
                    continue;
                }
                if let Some(contact) = self.store.contact(&member.device_id).await?.filter(|contact| !contact.blocked) {
                    contacts.push(contact);
                }
            }
        }
        for contact in contacts {
            if self.quiet_until_written(&contact).await {
                continue;
            }
            let _ = self.introduce(&contact).await;
        }
        let _ = self.events.send(Event::ContactsChanged);
        self.registration_changed();
        self.route_capability_hashes().await
    }

    pub async fn my_card(&self) -> Result<ContactCard> {
        self.my_card_in(None).await
    }

    pub async fn my_card_in(&self, session: Option<&str>) -> Result<ContactCard> {
        self.card_in(session, true).await
    }

    pub async fn my_card_without_envelope(&self) -> Result<ContactCard> {
        self.card_in(None, false).await
    }

    async fn card_in(&self, session: Option<&str>, with_envelope: bool) -> Result<ContactCard> {
        let capability = self.capability_of(session).await?;
        let (name, mailbox) = (self.name().await?, self.mailbox().await?);
        let envelope = with_envelope.then(|| self.envelope.public_key());
        let mut identity = self.identity.lock().await;
        let card = ContactCard::create(&mut identity, name, capability, mailbox, envelope);
        // Creating the first card adds the Olm fallback key to the account
        self.store.save_identity(&identity.seal(&self.key), self.route_capability().as_bytes()).await?;
        Ok(card)
    }

    pub async fn add_contact(&self, link: &str, name: Option<String>) -> Result<Contact> {
        self.add_contact_in(link, name, None).await
    }

    pub async fn add_contact_in(&self, link: &str, name: Option<String>, session: Option<&str>) -> Result<Contact> {
        if let Some(session) = session {
            ensure!(self.open_sessions.lock().expect("sessions poisoned").contains_key(session), "that session is not open");
        }
        let card = ContactCard::from_link(link)?;
        if card.device_id() == self.device_id {
            bail!("that is your own contact card");
        }
        let contact = self.keep_contact(&card, name, session).await?;
        self.introduce(&contact).await?;
        let _ = self.events.send(Event::ContactsChanged);
        Ok(contact)
    }

    // Stays local, not even our card goes out until the user writes to it
    pub async fn add_bundled_contact(&self, link: &str, name: Option<String>) -> Result<Option<Contact>> {
        if self.store.setting(BUNDLED_CONTACT).await?.as_deref() != Some(BUNDLED_DUE) {
            return Ok(None);
        }
        let Ok(card) = ContactCard::from_link(link) else { return Ok(None) };
        if card.device_id() == self.device_id {
            return Ok(None);
        }
        let contact = self.keep_contact(&card, name, None).await?;
        self.store.set_setting(BUNDLED_CONTACT, &contact.device_id).await?;
        let _ = self.events.send(Event::ContactsChanged);
        Ok(Some(contact))
    }

    pub(crate) async fn quiet_until_written(&self, contact: &Contact) -> bool {
        !contact.introduced && matches!(self.store.setting(BUNDLED_CONTACT).await, Ok(Some(id)) if id == contact.device_id)
    }

    async fn keep_contact(&self, card: &ContactCard, name: Option<String>, session: Option<&str>) -> Result<Contact> {
        let device_id = card.device_id();
        let name = name
            .map(|name| clean_name(&name))
            .filter(|name| !name.is_empty())
            .or_else(|| card.name().map(clean_name).filter(|name| !name.is_empty()))
            .unwrap_or_else(|| short_name(&device_id));
        self.store
            .add_contact(&NewContact {
                device_id: device_id.to_string(),
                name,
                card: card.encode(),
                mailbox: card.mailbox(),
                session: session.map(str::to_owned),
                receipts: self.receipts_default().await?,
                accepted: true,
            })
            .await?;
        {
            let identity = self.identity.lock().await;
            if self.store.channel(device_id.as_str()).await?.is_none() {
                let channel = Channel::open(&identity, &card.contact_keys())?;
                self.store.save_channel(device_id.as_str(), &channel.seal(&self.key)).await?;
            }
        }
        self.contact(device_id.as_str()).await
    }

    pub async fn requests(&self) -> Result<Vec<Conversation>> {
        self.store.request_conversations(None).await
    }

    pub async fn session_requests(&self, session: &str) -> Result<Vec<Conversation>> {
        self.store.request_conversations(Some(session)).await
    }

    pub async fn accept_contact(&self, contact: &str) -> Result<()> {
        let stored = self.contact(contact).await?;
        if stored.accepted {
            return Ok(());
        }
        self.store.set_accepted(contact, true).await?;
        let _ = self.events.send(Event::ContactsChanged);
        let _ = self.events.send(Event::MessagesChanged { contact: contact.to_owned() });
        let accepted = self.contact(contact).await?;
        let _ = self.introduce(&accepted).await;
        let _ = self.resume_files_from(contact, Duration::ZERO).await;
        Ok(())
    }

    pub async fn decline_contact(&self, contact: &str) -> Result<()> {
        self.contact(contact).await?;
        self.block(contact, true).await
    }

    pub(crate) async fn chosen(&self, contact: &str) -> Result<Contact> {
        let stored = self.contact(contact).await?;
        if !stored.accepted {
            self.accept_contact(contact).await?;
            return self.contact(contact).await;
        }
        Ok(stored)
    }

    pub async fn set_rules(&self, contact: &str, rules: ContactRules) -> Result<()> {
        self.contact(contact).await?;
        self.store.set_rules(contact, &rules).await?;
        let _ = self.events.send(Event::ContactsChanged);
        self.registration_changed();
        Ok(())
    }

    pub async fn call_rules(&self) -> Result<Vec<CallerRule>> {
        let mut listed = Vec::new();
        for contact in self.store.all_contacts().await? {
            let Some(rule) = call_rule(&contact) else { continue };
            let Ok(capability) = self.capability_of(contact.session.as_deref()).await else { continue };
            listed.push(CallerRule { caller: caller_tag(capability.as_bytes(), &contact.device_id), rule });
        }
        Ok(listed)
    }

    pub async fn fingerprint(&self, contact: &str) -> Result<String> {
        let card = ContactCard::decode(&self.contact(contact).await?.card)?;
        let mine = self.identity.lock().await.signing_key();
        Ok(ft_contacts::fingerprint(&mine, &card.signing_key()))
    }

    pub async fn rename_contact(&self, contact: &str, name: &str) -> Result<()> {
        self.store.rename_contact(contact, name.trim()).await?;
        let _ = self.events.send(Event::ContactsChanged);
        Ok(())
    }

    pub async fn remove_contact(&self, contact: &str) -> Result<()> {
        self.contact(contact).await?;
        self.store.remove_contact(contact).await?;
        let _ = self.events.send(Event::ContactsChanged);
        self.registration_changed();
        Ok(())
    }

    // seconds, 0 = forever / never
    pub async fn set_history(&self, contact: &str, keep_for: i64, burn_after_read: i64) -> Result<()> {
        self.store.set_history(contact, keep_for.max(0), burn_after_read.max(0)).await?;
        let _ = self.events.send(Event::ContactsChanged);
        self.sweep_history().await
    }

    pub async fn block(&self, contact: &str, blocked: bool) -> Result<()> {
        self.store.set_blocked(contact, blocked).await?;
        if blocked {
            self.transport.disconnect(contact).await;
        }
        let _ = self.events.send(Event::ContactsChanged);
        self.registration_changed();
        Ok(())
    }

    pub async fn peer(&self, contact: &str) -> Result<Peer> {
        let contact = self.contact(contact).await?;
        let card = ContactCard::decode(&contact.card)?;
        Ok(Peer { device_id: contact.device_id, capability: card.route_capability() })
    }

    pub(crate) async fn introduce(&self, contact: &Contact) -> Result<()> {
        let card = self.my_card_in(contact.session.as_deref()).await?;
        let via = self.via(contact)?;
        self.transmit(contact, &Packet::new(Body::ContactCard { card: card.encode(), via })).await?;
        Ok(())
    }

    pub(crate) fn via(&self, contact: &Contact) -> Result<Option<Vec<u8>>> {
        Ok(Some(ContactCard::decode(&contact.card)?.route_capability().hash().to_vec()))
    }

    pub(crate) async fn contact(&self, device_id: &str) -> Result<Contact> {
        self.store.contact(device_id).await?.ok_or_else(|| anyhow!("unknown contact"))
    }
}

fn call_rule(contact: &Contact) -> Option<CallRule> {
    if contact.blocked || !contact.rules.accepts_calls {
        Some(CallRule::None)
    } else if contact.rules.muted {
        Some(CallRule::Quiet)
    } else {
        None
    }
}

pub(crate) fn short_name(device_id: &DeviceId) -> String {
    device_id.as_str().chars().take(9).collect()
}

pub const NAME_LIMIT: usize = 40;

// Drops control chars and bidi overrides, which can make a name display as something else
pub fn clean_name(name: &str) -> String {
    let readable = name
        .chars()
        .filter(|c| !c.is_control())
        .filter(|c| !matches!(*c, '\u{200e}' | '\u{200f}' | '\u{061c}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}'))
        .collect::<String>();
    readable.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(NAME_LIMIT).collect::<String>().trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_tamed() {
        assert_eq!(clean_name("  Bob   Smith "), "Bob Smith");
        assert_eq!(clean_name("Mam\u{202e}á\u{0000}"), "Mamá");
        assert_eq!(clean_name(&"x".repeat(100)).chars().count(), NAME_LIMIT);
        assert_eq!(clean_name("\t\n"), "");
        assert_eq!(clean_name("Zoë 😀"), "Zoë 😀");
    }
}
