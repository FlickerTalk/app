//! Which hidden sessions are open, kept across starts (2026-10-01, Plan §108): a session stays
//! open until the user leaves it, whatever happens to the app.
//!
//! Finding a session takes its PIN (only the PIN's hash, keyed with the storage key, is in the
//! database); reading one once found takes nothing more than its id, which the database holds in
//! the clear like everything else of it. So an open session needs no secret of its own on the
//! phone: what is kept is the list of open ids, sealed with the storage key (the one that seals
//! the identity), in the settings. A session left is no longer in it, and so is exactly as hidden
//! as before: nothing in the database tells it apart from a session that was never opened here.

use std::collections::HashMap;

use anyhow::Result;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use ft_storage::Store;
use ft_vault::cipher;

/// The settings key of the sealed list, and the label it is sealed under.
const OPEN_SESSIONS: &str = "open_sessions";
const LABEL: &str = "open sessions";
/// The random bits the unused slots show the router, one per slot (bit i for slot i).
const SLOT_NOISE: &str = "slot_noise";

/// The sessions the user left open, with their slots. Anything that cannot be read (no list, a
/// list sealed with another key, a session gone) counts as closed: never as open.
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

/// What the router is told is silent (2026-10-01): bit i for slot i. The main list (slot 0) never
/// is; a used slot is silent while its session is closed; an unused slot shows its bit of `noise`,
/// so that the router cannot count the sessions from the mask.
pub(crate) fn silent_mask(used: &[u8], open: &[u8], noise: u8) -> u8 {
    (1..8u8).fold(0, |mask, slot| {
        let silent = match used.contains(&slot) {
            true => !open.contains(&slot),
            false => noise & (1 << slot) != 0,
        };
        if silent { mask | 1 << slot } else { mask }
    })
}

/// The bits the unused slots show the router: drawn once from the OS's random generator, then
/// kept, so that the mask does not change from one registration to the next.
pub(crate) async fn noise(store: &Store) -> Result<u8> {
    if let Some(noise) = store.setting(SLOT_NOISE).await?.and_then(|kept| kept.parse().ok()) {
        return Ok(noise);
    }
    let noise = os_random();
    store.set_setting(SLOT_NOISE, &noise.to_string()).await?;
    Ok(noise)
}

/// A slot just taken by a session gets a new random bit for when it is free again: what it shows
/// then says nothing of what it showed before.
pub(crate) async fn redraw(store: &Store, slot: u8) -> Result<()> {
    let bit = 1u8 << slot;
    let noise = (noise(store).await? & !bit) | (os_random() & bit);
    store.set_setting(SLOT_NOISE, &noise.to_string()).await
}

fn os_random() -> u8 {
    use rand::TryRngCore;
    rand::rngs::OsRng.try_next_u32().map(|value| value as u8).unwrap_or_else(|_| rand::random())
}

/// Keeps the open sessions, sealed, in place of the last list.
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

    // What is kept names no session in the clear, and only the phone's key reads it back.
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
