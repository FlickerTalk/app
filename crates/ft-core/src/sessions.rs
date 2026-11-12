//! The list of open sessions is sealed with the storage key, so a session left looks like one never opened.

use std::collections::HashMap;

use anyhow::{anyhow, bail, ensure, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use ft_contacts::RouteCapability;
use ft_protocol::MessageId;
use ft_storage::{Contact, Store};
use ft_vault::cipher;

use crate::{Core, Event, NEEDS_SUBSCRIPTION, SPARE_SLOTS};

const OPEN_SESSIONS: &str = "open_sessions";
const LABEL: &str = "open sessions";
const SLOT_NOISE: &str = "slot_noise";
const SESSION_PIN_CONTEXT: &str = "flickertalk 2026-09-23 hidden session pin";
const SESSION_PIN_LENGTH: usize = 6;

impl Core {
    fn pin_hash(&self, pin: &str) -> Result<[u8; 32]> {
        ensure!(pin.len() == SESSION_PIN_LENGTH && pin.bytes().all(|byte| byte.is_ascii_digit()), "a PIN is six digits");
        Ok(*blake3::keyed_hash(&blake3::derive_key(SESSION_PIN_CONTEXT, &self.key), pin.as_bytes()).as_bytes())
    }

    // Any PIN opens a session, a new empty one if no session has it, and the two must look alike.
    // Without the subscription every PIN is refused the same way.
    pub async fn open_session(&self, pin: &str) -> Result<Option<String>> {
        let hash = self.pin_hash(pin)?;
        ensure!(self.access().await?.may(ft_billing::Doing::UseSession), NEEDS_SUBSCRIPTION);
        if let Some(id) = self.open_session_with(&hash).await? {
            self.keep_open_sessions().await?;
            return Ok(Some(id));
        }
        let Some(slot) = self.free_slot().await? else { return Ok(None) };
        let id = MessageId::new().to_string();
        self.store.add_session(&id, &hash, slot).await?;
        redraw(&self.store, slot).await?;
        self.open_sessions.lock().expect("sessions poisoned").insert(id.clone(), slot);
        self.keep_open_sessions().await?;
        Ok(Some(id))
    }

    async fn keep_open_sessions(&self) -> Result<()> {
        {
            let _one_at_a_time = self.keeping_sessions.lock().await;
            keep(&self.store, &self.key, &self.open_sessions()).await?;
        }
        self.registration_changed();
        let _ = self.events.send(Event::RemindersChanged);
        Ok(())
    }

    async fn open_session_with(&self, hash: &[u8; 32]) -> Result<Option<String>> {
        let Some(id) = self.store.session_by_pin(hash).await? else { return Ok(None) };
        let slot = match self.store.session_slot(&id).await? {
            Some(slot) => slot,
            None => {
                let slot = self.free_slot().await?.ok_or_else(|| anyhow!("no room for another session"))?;
                self.store.set_session_slot(&id, slot).await?;
                redraw(&self.store, slot).await?;
                self.open_sessions.lock().expect("sessions poisoned").insert(id.clone(), slot);
                for contact in self.store.session_contacts(&id).await? {
                    let _ = self.introduce(&contact).await;
                }
                slot
            }
        };
        self.open_sessions.lock().expect("sessions poisoned").insert(id.clone(), slot);
        Ok(Some(id))
    }

    pub async fn remove_session(&self, session: &str) -> Result<()> {
        ensure!(self.open_sessions.lock().expect("sessions poisoned").contains_key(session), "that session is not open");
        let mut files = Vec::new();
        for contact in self.store.session_contacts(session).await? {
            files.extend(self.store.files(&contact.device_id).await?);
        }
        self.cut_off(session).await?;
        forget_session(&self.store, session).await?;
        self.forget_bytes(files).await;
        self.open_sessions.lock().expect("sessions poisoned").remove(session);
        self.keep_open_sessions().await?;
        let _ = self.events.send(Event::ContactsChanged);
        Ok(())
    }

    async fn free_slot(&self) -> Result<Option<u8>> {
        let used = self.store.used_slots().await?;
        Ok((1..=SPARE_SLOTS).find(|slot| !used.contains(slot)))
    }

    pub async fn close_session(&self, session: &str) -> Result<bool> {
        self.open_sessions.lock().expect("sessions poisoned").remove(session);
        self.keep_open_sessions().await?;
        self.cut_off(session).await?;
        if !self.store.empty_sessions().await?.iter().any(|empty| empty == session) {
            return Ok(false);
        }
        forget_session(&self.store, session).await?;
        self.registration_changed();
        Ok(true)
    }

    // The router can't close a connection that's already open, so close them here.
    // Hang up a call first so its end still goes out on that connection.
    async fn cut_off(&self, session: &str) -> Result<()> {
        let all = self.store.all_contacts().await?;
        let contacts: Vec<String> = all.into_iter().filter(|contact| contact.session.as_deref() == Some(session)).map(|contact| contact.device_id).collect();
        self.hang_up_with(&contacts).await?;
        for contact in &contacts {
            self.transport.disconnect(contact).await;
        }
        Ok(())
    }

    pub fn open_sessions(&self) -> Vec<String> {
        let mut open: Vec<String> = self.open_sessions.lock().expect("sessions poisoned").keys().cloned().collect();
        open.sort();
        open
    }

    pub fn open_slots(&self) -> Vec<u8> {
        let mut slots: Vec<u8> = self.open_sessions.lock().expect("sessions poisoned").values().copied().collect();
        slots.sort();
        slots
    }

    pub async fn silent_slots(&self) -> Result<u8> {
        let (used, noise) = (self.store.used_slots().await?, noise(&self.store).await?);
        Ok(silent_mask(&used, &self.open_slots(), noise))
    }

    pub async fn route_capability_hashes(&self) -> Result<[[u8; 32]; 8]> {
        let mut hashes = [self.route_capability().hash(); 8];
        for (slot, capability) in self.store.spare_capabilities().await? {
            hashes[slot as usize] = RouteCapability::from_bytes(capability).hash();
        }
        Ok(hashes)
    }

    pub(crate) async fn capability_of(&self, session: Option<&str>) -> Result<RouteCapability> {
        let Some(session) = session else { return Ok(self.route_capability()) };
        let slot = self.store.session_slot(session).await?.ok_or_else(|| anyhow!("the session has no slot"))?;
        let spare = self.store.spare_capabilities().await?.into_iter().find(|(spare, _)| *spare == slot);
        spare.map(|(_, capability)| RouteCapability::from_bytes(capability)).ok_or_else(|| anyhow!("no capability for the slot"))
    }

    // An unknown or retired link reaches nobody, not the slot's next session and not the main list
    pub(crate) async fn session_via(&self, via: Option<&[u8]>) -> Result<Option<String>> {
        let Some(via) = via else { return Ok(None) };
        let hashes = self.route_capability_hashes().await?;
        if hashes[0].as_slice() == via {
            return Ok(None);
        }
        let Some(slot) = (1..8).find(|slot| hashes[*slot].as_slice() == via) else { bail!("that link is retired") };
        match self.store.session_with_slot(slot as u8).await? {
            Some(session) => Ok(Some(session)),
            None => bail!("that link is retired"),
        }
    }

    pub(crate) fn silent(&self, contact: &Contact) -> bool {
        !contact.accepted
            || contact.session.as_deref().is_some_and(|session| !self.open_sessions.lock().expect("sessions poisoned").contains_key(session))
    }
}

// anything unreadable counts as closed
pub(crate) async fn restore(store: &Store, key: &[u8; 32]) -> Result<HashMap<String, u8>> {
    let mut open = HashMap::new();
    let Some(kept) = store.setting(OPEN_SESSIONS).await? else { return Ok(open) };
    let Some(ids) = unseal(key, &kept) else { return Ok(open) };
    for id in ids {
        if let Some(slot) = store.session_slot(&id).await? {
            open.insert(id, slot);
        }
    }
    Ok(open)
}

// Unused slots show random bits so the router can't count sessions
pub(crate) fn silent_mask(used: &[u8], open: &[u8], noise: u8) -> u8 {
    (1..8u8).fold(0, |mask, slot| {
        let silent = match used.contains(&slot) {
            true => !open.contains(&slot),
            false => noise & (1 << slot) != 0,
        };
        if silent { mask | 1 << slot } else { mask }
    })
}

// Kept so the mask doesn't change between registrations
pub(crate) async fn noise(store: &Store) -> Result<u8> {
    if let Some(noise) = store.setting(SLOT_NOISE).await?.and_then(|kept| kept.parse().ok()) {
        return Ok(noise);
    }
    let noise = os_random();
    store.set_setting(SLOT_NOISE, &noise.to_string()).await?;
    Ok(noise)
}

// A new bit so a freed slot shows nothing of what it showed before
pub(crate) async fn redraw(store: &Store, slot: u8) -> Result<()> {
    let bit = 1u8 << slot;
    let noise = (noise(store).await? & !bit) | (os_random() & bit);
    store.set_setting(SLOT_NOISE, &noise.to_string()).await
}

fn os_random() -> u8 {
    use rand::TryRngCore;
    rand::rngs::OsRng.try_next_u32().map(|value| value as u8).unwrap_or_else(|_| rand::random())
}

pub(crate) async fn keep(store: &Store, key: &[u8; 32], open: &[String]) -> Result<()> {
    store.set_setting(OPEN_SESSIONS, &seal(key, open)?).await
}

fn seal(key: &[u8; 32], open: &[String]) -> Result<String> {
    Ok(STANDARD.encode(cipher::seal(key, LABEL, &serde_json::to_vec(open)?)?))
}

fn unseal(key: &[u8; 32], kept: &str) -> Option<Vec<String>> {
    let sealed = STANDARD.decode(kept).ok()?;
    serde_json::from_slice(&cipher::open(key, LABEL, &sealed).ok()?).ok()
}

// New capability so the old QR reaches nobody, not even the slot's next session
pub(crate) async fn forget_session(store: &Store, session: &str) -> Result<()> {
    if let Some(slot) = store.session_slot(session).await? {
        store.set_spare_capability(slot, RouteCapability::generate().as_bytes()).await?;
    }
    store.remove_session(session).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_open_list_is_sealed_with_the_phones_key() {
        let open = vec!["0190a-session".to_owned()];
        let kept = seal(&[9; 32], &open).unwrap();
        assert!(!kept.contains("0190a-session"));
        assert!(!String::from_utf8_lossy(&STANDARD.decode(&kept).unwrap()).contains("0190a-session"));
        assert_eq!(unseal(&[9; 32], &kept), Some(open));
        assert_eq!(unseal(&[8; 32], &kept), None, "another key reads nothing");
        assert_eq!(unseal(&[9; 32], "not sealed"), None);
    }

    #[test]
    fn a_closed_session_is_silent_an_open_one_is_not_and_a_spare_slot_is_noise() {
        assert_eq!(silent_mask(&[], &[], 0), 0);
        assert_eq!(silent_mask(&[], &[], 0b1111_1111), 0b1111_1110, "the main list is never silent");
        assert_eq!(silent_mask(&[2], &[], 0), 0b0000_0100, "closed: silent");
        assert_eq!(silent_mask(&[2], &[2], 0b1111_1111), 0b1111_1010, "open: heard, whatever the noise");
        assert_eq!(silent_mask(&[1, 3, 7], &[3], 0b0101_0000), 0b1101_0010);
    }

    #[tokio::test]
    async fn nothing_kept_is_nothing_open() {
        let store = Store::open_in_memory().await.unwrap();
        assert!(restore(&store, &[9; 32]).await.unwrap().is_empty());
        store.set_setting(OPEN_SESSIONS, "garbage").await.unwrap();
        assert!(restore(&store, &[9; 32]).await.unwrap().is_empty());
    }
}
