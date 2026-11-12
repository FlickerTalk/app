//! The list of open sessions is sealed with the storage key, so a session left looks like one never opened.

use std::collections::HashMap;

use anyhow::Result;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use ft_storage::Store;
use ft_vault::cipher;

const OPEN_SESSIONS: &str = "open_sessions";
const LABEL: &str = "open sessions";
const SLOT_NOISE: &str = "slot_noise";

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
