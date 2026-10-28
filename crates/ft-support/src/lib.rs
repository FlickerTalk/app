//! The FlickerTalk support contact (plan of the Apple review 1.7.1, 2026-10-28): a phone with no
//! screen, on `ft_core::online`, that every install carries as a contact. It accepts whoever
//! writes, answers texts with a few canned replies, sends a sample file back for a file and a
//! voice message back for a voice message, and answers voice and video calls with a recording on
//! a loop. It keeps no history: messages and call records go after a few minutes, and nothing about
//! anyone is logged.

pub mod clips;
pub mod config;
pub mod devices;
pub mod identity;
pub mod limit;
pub mod peer;
pub mod policy;
pub mod turn;

use std::sync::Arc;

use anyhow::{Context, Result};
use ft_core::online::Online;
use ft_storage::Store;
use ft_webrtc::SessionConfig;

use crate::config::Config;
use crate::peer::{Settings, Support};

/// The name on its Contact Card.
pub const NAME: &str = "FlickerTalk";

/// The router it connects to unless told otherwise.
pub const ROUTER: &str = "https://api.flickertalk.com";

/// The support contact, connected and answering.
pub struct Running {
    pub online: Online,
    pub support: Arc<Support>,
    /// Its Contact Card link: what the app carries.
    pub card: String,
}

impl Running {
    /// Lets go of the router and closes the database.
    pub async fn stop(&self) {
        self.online.shutdown().await;
    }
}

/// Opens the identity (making it on the first run, or bringing back the bundle's), connects to the
/// router and starts answering. Writes the card link to `card_file` and the identity bundle, if
/// there is none yet. Nothing waits for the network.
pub async fn start(config: &Config) -> Result<Running> {
    let (key, store) = open(config).await?;
    // A server's interfaces do not change under it: nothing to follow.
    let base = SessionConfig { follow_interfaces: false, ..SessionConfig::default() };
    let online = ft_core::online::start(store, key, &config.router, base).await?;
    Support::prepare(&online.core, &config.files).await?;
    let support = Support::new(online.core.clone(), Settings::default(), &config.files);
    support.spawn();
    // A fresh TURN user every minute: the welcome's expires in ten (`turn`).
    let (router, network) = (online.router.clone(), online.network.clone());
    tokio::spawn(turn::keep_fresh(
        turn::REFRESH_EVERY,
        move || {
            let router = router.clone();
            async move { router.turn_credentials().await }
        },
        move |grant| network.set_turn(grant),
        online.stopped(),
    ));
    // After the name is set, and after the first card made the fallback key the bundle must carry.
    let card = write_out(&online.core, config).await?;
    Ok(Running { online, support, card })
}

/// The key and the database, with the bundle's identity brought in if the database had none.
async fn open(config: &Config) -> Result<([u8; 32], Store)> {
    let key = identity::load_or_create_key(&config.key_file)?;
    if let Some(dir) = config.db.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        tokio::fs::create_dir_all(dir).await.with_context(|| format!("cannot create {}", dir.display()))?;
    }
    owner_only(&config.db, false)?;
    let store = Store::open(&config.db).await.with_context(|| format!("cannot open {}", config.db.display()))?;
    identity::restore(&store, &key, &config.identity_file).await?;
    Ok((key, store))
}

/// The card link, the bundle (if there is none yet) and the card file. After the name is set,
/// and after the first card made the fallback key the bundle must carry.
async fn write_out(core: &ft_core::Core, config: &Config) -> Result<String> {
    let card = core.my_card().await?.to_link();
    identity::export(core.store(), &config.identity_file).await?;
    tokio::fs::write(&config.card_file, format!("{card}\n")).await.with_context(|| format!("cannot write {}", config.card_file.display()))?;
    Ok(card)
}

/// Makes `path` readable by its owner alone, creating it first (an empty file, which SQLite takes as
/// a new database, or a folder) if it is not there. SQLite gives its journal files the database's
/// permissions.
pub fn owner_only(path: &std::path::Path, folder: bool) -> Result<()> {
    if folder {
        std::fs::create_dir_all(path).with_context(|| format!("cannot create {}", path.display()))?;
    } else if !path.exists() {
        std::fs::File::create(path).with_context(|| format!("cannot create {}", path.display()))?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if folder { 0o700 } else { 0o600 };
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).with_context(|| format!("cannot protect {}", path.display()))?;
    }
    Ok(())
}

/// Makes the identity, or opens the one there is, and writes its key, identity bundle and card
/// link, without connecting to anything. Returns the card link.
pub async fn init(config: &Config) -> Result<String> {
    let (key, store) = open(config).await?;
    let core = ft_core::Core::open(store.clone(), key, Arc::new(Offline)).await?;
    core.set_name(NAME).await?;
    let card = write_out(&core, config).await?;
    store.close().await;
    Ok(card)
}

/// No network at all: what `init` opens the core with.
struct Offline;

#[async_trait::async_trait]
impl ft_core::Transport for Offline {
    async fn send_direct(&self, _to: &ft_core::Peer, _bytes: Vec<u8>) -> Result<bool> {
        Ok(false)
    }

    async fn send_mailbox(&self, _to: &ft_core::Peer, _bytes: Vec<u8>) -> Result<()> {
        anyhow::bail!("offline")
    }
}

/// `start`, then on until SIGTERM or Ctrl-C. It prints its card link and nothing about anyone.
pub async fn run(config: &Config) -> Result<()> {
    let running = start(config).await?;
    println!("FlickerTalk support contact on {}. Its Contact Card link (also in {}):", config.router, config.card_file.display());
    println!("{}", running.card);
    stopped().await?;
    println!("stopping");
    running.stop().await;
    Ok(())
}

/// Resolves on SIGTERM (`docker stop`, a Swarm update) or Ctrl-C.
async fn stopped() -> Result<()> {
    #[cfg(unix)]
    {
        let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            _ = terminate.recv() => Ok(()),
            interrupted = tokio::signal::ctrl_c() => Ok(interrupted?),
        }
    }
    #[cfg(not(unix))]
    {
        Ok(tokio::signal::ctrl_c().await?)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn offline(dir: &Path) -> Config {
        // A port nobody listens on: the router is never reached, and nothing waits for it.
        Config::from_vars(|name| match name {
            "FT_SUPPORT_DATA" => Some(dir.to_string_lossy().into_owned()),
            "FT_SUPPORT_ROUTER" => Some("http://127.0.0.1:9".to_owned()),
            _ => None,
        })
        .unwrap()
    }

    #[test]
    fn it_introduces_itself_as_flickertalk_on_the_live_router() {
        assert_eq!(NAME, "FlickerTalk");
        assert!(ROUTER.starts_with("https://"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_first_start_makes_the_key_the_identity_bundle_and_the_card_file() {
        let dir = tempfile::tempdir().unwrap();
        let config = offline(dir.path());
        let running = start(&config).await.expect("starts without the router");
        assert!(running.card.starts_with("https://flickertalk.com/add#"), "{}", running.card);
        assert_eq!(std::fs::read_to_string(&config.card_file).unwrap().trim(), running.card);
        assert!(config.key_file.exists() && config.identity_file.exists() && config.files.join("outgoing").is_dir());
        let card = ft_contacts::ContactCard::from_link(&running.card).expect("a card link");
        assert_eq!(card.name(), Some("FlickerTalk"));
        assert!(card.mailbox(), "with the mailbox on");
        running.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_card_is_the_same_on_every_start_and_after_losing_the_database() {
        let dir = tempfile::tempdir().unwrap();
        let config = offline(dir.path());
        let first = start(&config).await.unwrap();
        let card = first.card.clone();
        first.stop().await;

        let again = start(&config).await.unwrap();
        assert_eq!(again.card, card, "the same database");
        again.stop().await;

        for gone in ["support.db", "support.db-wal", "support.db-shm"] {
            let _ = std::fs::remove_file(dir.path().join(gone));
        }
        let restored = start(&config).await.unwrap();
        assert_eq!(restored.card, card, "a new database with the bundle");
        restored.stop().await;
    }

    // The database holds the contacts and, for a few minutes, what they said; the files folder
    // what they sent. Only the owner reads them.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn the_database_and_the_files_are_the_owner_s_alone() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let config = offline(dir.path());
        let running = start(&config).await.unwrap();
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&config.db), 0o600);
        assert_eq!(mode(&config.files), 0o700);
        assert_eq!(mode(&config.key_file), 0o600);
        running.stop().await;
    }

    // `ft-support init` (2026-10-28): the identity made once, without the network, so its card can
    // go into the app and its key and bundle into Swarm secrets before anything is deployed.
    #[tokio::test(flavor = "multi_thread")]
    async fn init_makes_the_identity_that_start_then_runs_with() {
        let dir = tempfile::tempdir().unwrap();
        let config = offline(dir.path());
        let card = init(&config).await.expect("makes the identity");
        assert!(card.starts_with("https://flickertalk.com/add#"));
        assert_eq!(std::fs::read_to_string(&config.card_file).unwrap().trim(), card);
        assert!(config.key_file.exists() && config.identity_file.exists());
        assert_eq!(init(&config).await.unwrap(), card, "a second init changes nothing");

        // On the cluster: the key and the bundle as secrets, a new volume for the rest.
        let cluster = tempfile::tempdir().unwrap();
        let (key, bundle) = (config.key_file.clone(), config.identity_file.clone());
        let data = cluster.path().to_string_lossy().into_owned();
        let on_the_cluster = Config::from_vars(|name| match name {
            "FT_SUPPORT_DATA" => Some(data.clone()),
            "FT_SUPPORT_KEY_FILE" => Some(key.to_string_lossy().into_owned()),
            "FT_SUPPORT_IDENTITY_FILE" => Some(bundle.to_string_lossy().into_owned()),
            "FT_SUPPORT_ROUTER" => Some("http://127.0.0.1:9".to_owned()),
            _ => None,
        })
        .unwrap();
        let running = start(&on_the_cluster).await.unwrap();
        assert_eq!(running.card, card, "the card init printed is the one that runs");
        running.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_broken_key_stops_it_before_anything_else() {
        let dir = tempfile::tempdir().unwrap();
        let config = offline(dir.path());
        std::fs::write(&config.key_file, "not a key").unwrap();
        assert!(start(&config).await.is_err());
        assert!(!config.db.exists(), "no database made");
    }

}
