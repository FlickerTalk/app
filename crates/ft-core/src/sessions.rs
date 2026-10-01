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

    #[tokio::test]
    async fn nothing_kept_is_nothing_open() {
        let store = Store::open_in_memory().await.unwrap();
        assert!(restore(&store, &[9; 32]).await.unwrap().is_empty());
        store.set_setting(OPEN_SESSIONS, "garbage").await.unwrap();
        assert!(restore(&store, &[9; 32]).await.unwrap().is_empty());
    }
}
