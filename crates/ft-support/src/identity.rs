//! Who the support contact is, kept so that its Contact Card never changes: the card is baked into
//! every install of the app, so a new identity would leave all of them writing to nobody.
//!
//! Two files make it, both with the owner alone able to read them (0600):
//!
//! - **The key** (`FT_SUPPORT_KEY_FILE`): 32 random bytes, as 64 hex digits, that seal the
//!   identity and the Olm sessions in the database. Created on the first run.
//! - **The identity bundle** (`FT_SUPPORT_IDENTITY_FILE`, optional): the identity as the database
//!   keeps it (sealed with the key), its route capability and its envelope key (sealed too), as one
//!   line of base64url. Written once the card exists; read back into a database that has no
//!   identity (a new volume, another node), so the same card comes back. A database whose identity
//!   is another one is refused: two supports with one card would split the conversations.
//!
//! What would change the card: losing the key, or the database and the bundle together. Nothing
//! the support contact does rotates it (it never renews its link).

use std::path::Path;

use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ft_identity::Identity;
use ft_storage::{Store, StoredIdentity};
use serde::{Deserialize, Serialize};

/// The version of the bundle's format.
const BUNDLE_VERSION: u16 = 1;

/// Reads the key at `path`, or makes one there (0600) if there is none yet.
pub fn load_or_create_key(path: &Path) -> Result<[u8; 32]> {
    match std::fs::read_to_string(path) {
        Ok(text) => parse_key(&text).with_context(|| format!("{} does not hold a key (64 hex digits)", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let key: [u8; 32] = rand::random();
            write_private(path, &format!("{}\n", hex::encode(key)))?;
            Ok(key)
        }
        Err(error) => Err(error).with_context(|| format!("cannot read {}", path.display())),
    }
}

fn parse_key(text: &str) -> Result<[u8; 32]> {
    let bytes = hex::decode(text.trim())?;
    bytes.try_into().map_err(|_| anyhow::anyhow!("a key has 32 bytes"))
}

/// Creates `path` (and its folders) with `contents`, readable by the owner alone. Never replaces a
/// file that is there.
fn write_private(path: &Path, contents: &str) -> Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path).with_context(|| format!("cannot create {}", path.display()))?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()?;
    Ok(())
}

/// The identity as the database keeps it, to carry to another database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bundle {
    version: u16,
    sealed: String,
    #[serde(with = "base64url")]
    route_capability: Vec<u8>,
    envelope: String,
}

impl Bundle {
    pub fn encode(&self) -> String {
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(self).expect("a bundle always serializes"))
    }

    pub fn decode(text: &str) -> Result<Self> {
        let bytes = URL_SAFE_NO_PAD.decode(text.trim()).context("not an identity bundle")?;
        let bundle: Self = serde_json::from_slice(&bytes).context("not an identity bundle")?;
        if bundle.version != BUNDLE_VERSION {
            bail!("unsupported identity bundle version {}", bundle.version);
        }
        Ok(bundle)
    }

    fn capability(&self) -> Result<[u8; 32]> {
        self.route_capability.clone().try_into().map_err(|_| anyhow::anyhow!("a route capability has 32 bytes"))
    }
}

/// What `restore` found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Restore {
    /// No bundle: the database keeps what it has, or makes a new identity.
    NoBundle,
    /// The database had no identity: the bundle's went in.
    Restored,
    /// The database already holds the bundle's identity.
    Same,
}

/// Before the core opens: brings the bundle at `file` into a database without an identity, and
/// refuses a database whose identity is not the bundle's.
pub async fn restore(store: &Store, key: &[u8; 32], file: &Path) -> Result<Restore> {
    let text = match tokio::fs::read_to_string(file).await {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Restore::NoBundle),
        Err(error) => return Err(error).with_context(|| format!("cannot read {}", file.display())),
    };
    let bundle = Bundle::decode(&text)?;
    let theirs = Identity::unseal(&bundle.sealed, key).context("the identity bundle does not open with this key")?;
    ft_identity::EnvelopeKey::unseal_at_rest(&bundle.envelope, key).context("the bundle's envelope key does not open with this key")?;
    let capability = bundle.capability()?;
    match store.identity().await? {
        None => {
            store.save_identity(&bundle.sealed, &capability).await?;
            store.save_envelope(&bundle.envelope).await?;
            Ok(Restore::Restored)
        }
        Some(StoredIdentity { sealed, route_capability, .. }) => {
            let ours = Identity::unseal(&sealed, key).context("the database does not open with this key")?;
            if ours.device_id() != theirs.device_id() || route_capability != capability {
                bail!("the database holds another identity than the bundle at {}: refusing to run with two cards", file.display());
            }
            Ok(Restore::Same)
        }
    }
}

/// After the card exists: writes the bundle to `file` (0600), unless one is there already.
/// Returns whether it wrote one.
pub async fn export(store: &Store, file: &Path) -> Result<bool> {
    if tokio::fs::try_exists(file).await? {
        return Ok(false);
    }
    let saved = store.identity().await?.context("there is no identity to export yet")?;
    let bundle = Bundle {
        version: BUNDLE_VERSION,
        sealed: saved.sealed,
        route_capability: saved.route_capability.to_vec(),
        envelope: saved.envelope.context("the identity has no envelope key yet")?,
    };
    write_private(file, &format!("{}\n", bundle.encode()))?;
    Ok(true)
}

mod base64url {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&URL_SAFE_NO_PAD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        URL_SAFE_NO_PAD.decode(text).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::Arc;

    use super::*;

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn the_first_run_makes_a_key_only_the_owner_reads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deep/support.key");
        let key = load_or_create_key(&path).expect("makes one");
        assert_eq!(mode(&path), 0o600);
        assert_eq!(std::fs::read_to_string(&path).unwrap().trim(), hex::encode(key));
        assert_ne!(key, [0; 32]);
    }

    #[test]
    fn the_same_key_comes_back_on_every_run() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("support.key");
        let key = load_or_create_key(&path).unwrap();
        assert_eq!(load_or_create_key(&path).unwrap(), key);
    }

    #[test]
    fn a_key_written_by_hand_reads_with_a_trailing_newline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("support.key");
        std::fs::write(&path, format!("{}\n", "ab".repeat(32))).unwrap();
        assert_eq!(load_or_create_key(&path).unwrap(), [0xab; 32]);
    }

    #[test]
    fn a_broken_key_is_an_error_and_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("support.key");
        std::fs::write(&path, "not a key").unwrap();
        assert!(load_or_create_key(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "not a key", "never replaced: that would change the card");
    }

    #[test]
    fn a_bundle_reads_back_as_written() {
        let bundle = Bundle { version: BUNDLE_VERSION, sealed: "sealed".to_owned(), route_capability: vec![7; 32], envelope: "env".to_owned() };
        let text = bundle.encode();
        assert!(!text.contains(['\n', '=', '"', ' ']), "one token, safe in an env file: {text}");
        assert_eq!(Bundle::decode(&format!("{text}\n")).unwrap(), bundle);
        assert!(Bundle::decode("garbage").is_err());
    }

    /// A database with an identity and a card, as a support contact's first run leaves it.
    async fn first_run(key: [u8; 32]) -> (Store, String) {
        let store = Store::open_in_memory().await.unwrap();
        let core = ft_core::Core::open(store.clone(), key, Arc::new(NoNetwork)).await.unwrap();
        let card = core.my_card().await.unwrap().to_link();
        (store, card)
    }

    #[tokio::test]
    async fn a_bundle_brings_the_same_card_back_into_an_empty_database() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("support.identity");
        let key = [3; 32];
        let (store, card) = first_run(key).await;
        assert!(export(&store, &file).await.unwrap(), "written the first time");
        assert_eq!(mode(&file), 0o600);
        assert!(!export(&store, &file).await.unwrap(), "never written over");

        let fresh = Store::open_in_memory().await.unwrap();
        assert_eq!(restore(&fresh, &key, &file).await.unwrap(), Restore::Restored);
        let core = ft_core::Core::open(fresh, key, Arc::new(NoNetwork)).await.unwrap();
        assert_eq!(core.my_card().await.unwrap().to_link(), card, "the very same link");
    }

    #[tokio::test]
    async fn a_database_with_the_bundle_s_identity_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("support.identity");
        let (store, _) = first_run([3; 32]).await;
        export(&store, &file).await.unwrap();
        assert_eq!(restore(&store, &[3; 32], &file).await.unwrap(), Restore::Same);
    }

    #[tokio::test]
    async fn a_database_with_another_identity_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("support.identity");
        let (theirs, _) = first_run([3; 32]).await;
        export(&theirs, &file).await.unwrap();
        let (ours, _) = first_run([3; 32]).await;
        let refused = restore(&ours, &[3; 32], &file).await.expect_err("two identities");
        assert!(format!("{refused:#}").contains("another identity"), "{refused:#}");
    }

    #[tokio::test]
    async fn without_a_bundle_nothing_changes() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_in_memory().await.unwrap();
        assert_eq!(restore(&store, &[3; 32], &dir.path().join("none")).await.unwrap(), Restore::NoBundle);
        assert!(store.identity().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_bundle_under_another_key_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("support.identity");
        let (store, _) = first_run([3; 32]).await;
        export(&store, &file).await.unwrap();
        let fresh = Store::open_in_memory().await.unwrap();
        assert!(restore(&fresh, &[4; 32], &file).await.is_err());
        assert!(fresh.identity().await.unwrap().is_none(), "nothing went in");
    }

    /// The core with no network at all: the identity tests send nothing.
    struct NoNetwork;

    #[async_trait::async_trait]
    impl ft_core::Transport for NoNetwork {
        async fn send_direct(&self, _to: &ft_core::Peer, _bytes: Vec<u8>) -> Result<bool> {
            Ok(false)
        }
        async fn send_mailbox(&self, _to: &ft_core::Peer, _bytes: Vec<u8>) -> Result<()> {
            bail!("no network")
        }
    }
}
