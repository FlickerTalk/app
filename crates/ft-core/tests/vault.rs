//! The user's cloud through the core (plan-drive): a login through a fake browser, the drive, a
//! plugin's grant, the backup and a new phone that brings it down. The cloud is a memory.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use ft_core::vault::{Authorizer, Cloud, VaultState};
use ft_core::{Core, Event, Peer, Transport};
use ft_plugins::{sign_package, Permissions};
use ft_storage::Store;
use ft_vault::google::{TokenKeeper, Tokens};
use ft_vault::{cipher, Memory, Provider};
use vodozemac::Ed25519SecretKey;

struct Offline;

#[async_trait]
impl Transport for Offline {
    async fn send_direct(&self, _: &Peer, _: Vec<u8>) -> anyhow::Result<bool> {
        Ok(false)
    }
    async fn send_mailbox(&self, _: &Peer, _: Vec<u8>) -> anyhow::Result<()> {
        Ok(())
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ft-core-vault-{name}-{}", ft_protocol::MessageId::new()));
    std::fs::create_dir_all(&dir).expect("creates the directory");
    dir
}

/// A cloud in memory, shared by every phone of a test, and a login that always works.
struct MemoryCloud(Arc<Memory>);

#[async_trait]
impl Cloud for MemoryCloud {
    async fn login(&self, provider: &str, _: &str, authorizer: &dyn Authorizer) -> anyhow::Result<Tokens> {
        anyhow::ensure!(provider == "memory", "only the memory here");
        let back = authorizer.authorize("https://login.example/auth?state=s", "com.example").await?;
        anyhow::ensure!(back.contains("code="), "no code came back");
        Ok(Tokens { access_token: "t".into(), refresh_token: None, expires_at: i64::MAX })
    }

    fn provider(&self, _: &str, _: &str, keeper: Arc<dyn TokenKeeper>) -> anyhow::Result<Arc<dyn Provider>> {
        // A cloud without a login is out of reach, as Google would be.
        let tokens = futures_block(keeper.tokens());
        anyhow::ensure!(tokens.is_some(), "not logged in");
        Ok(self.0.clone())
    }
}

fn futures_block<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(future))
}

/// The browser: the user logs in and the redirect comes back with a code.
struct Browser {
    opened: std::sync::Mutex<Vec<String>>,
    refuses: bool,
}

#[async_trait]
impl Authorizer for Browser {
    async fn authorize(&self, url: &str, scheme: &str) -> anyhow::Result<String> {
        self.opened.lock().unwrap().push(url.to_owned());
        anyhow::ensure!(!self.refuses, "the user closed the browser");
        Ok(format!("{scheme}:/oauth2redirect?state=s&code=the-code"))
    }
}

/// Whether any file under `dir` holds these bytes.
fn anywhere(dir: &std::path::Path, needle: &[u8]) -> bool {
    std::fs::read_dir(dir).into_iter().flatten().flatten().any(|entry| {
        let path = entry.path();
        if path.is_dir() {
            anywhere(&path, needle)
        } else {
            std::fs::read(&path).is_ok_and(|bytes| bytes.windows(needle.len()).any(|window| window == needle))
        }
    })
}

/// The phrase the user chose (plan-recuperacion, 2026-09-28).
const PHRASE: &str = "a long phrase of mine";

async fn phone(name: &str, cloud: &Arc<Memory>) -> (Arc<Core>, PathBuf) {
    let dir = scratch(name);
    // On disk: the backup takes a snapshot of the database (`VACUUM INTO`), as the app's is.
    let core = Core::open(Store::open(&dir.join("phone.db")).await.expect("store"), [4; 32], Arc::new(Offline)).await.expect("opens");
    core.set_files_dir(dir.join("files"));
    core.set_plugins_dir(dir.join("plugins"));
    core.set_move_dir(dir.join("move"));
    core.set_vault_dir(dir.join("vault"));
    core.set_cloud(Arc::new(MemoryCloud(cloud.clone())));
    (core, dir)
}

#[tokio::test(flavor = "multi_thread")]
async fn the_drive_is_connected_through_the_browser_set_up_and_used() {
    let cloud = Memory::new();
    let (core, dir) = phone("a", &cloud).await;
    assert_eq!(core.vault_status().await.unwrap().state, VaultState::None);
    assert!(core.vault_list(None).await.is_err(), "not open");

    let refused = Browser { opened: Default::default(), refuses: true };
    assert!(core.vault_connect("memory", &refused).await.is_err());
    assert_eq!(core.vault_status().await.unwrap().state, VaultState::None, "nothing kept of a login that failed");

    let browser = Browser { opened: Default::default(), refuses: false };
    let status = core.vault_connect("memory", &browser).await.unwrap();
    assert_eq!(status.state, VaultState::Empty, "logged in; the cloud has no drive yet");
    assert_eq!(browser.opened.lock().unwrap().len(), 1);
    let mut events = core.events();
    assert_eq!(core.vault_suggest_phrase().chars().count(), 35, "a strong phrase, for whoever wants one");
    assert!(core.vault_setup("too short").await.is_err(), "12 characters at least");
    assert_eq!(core.vault_status().await.unwrap().state, VaultState::Empty);
    core.vault_setup(PHRASE).await.unwrap();
    assert_eq!(core.vault_status().await.unwrap().state, VaultState::Ready);
    assert!(matches!(events.try_recv(), Ok(Event::VaultChanged)));
    assert!(core.vault_setup(PHRASE).await.is_err(), "once");

    // Nothing in the settings is readable: the tokens and the key are sealed with the storage key.
    for key in ["vault.tokens", "vault.key"] {
        let value = core.store().setting(key).await.unwrap().expect(key);
        assert!(!value.contains("access_token") && !value.contains('"'), "{key} is sealed");
    }
    // The phrase is kept nowhere: not on the phone, not in the cloud.
    assert!(!anywhere(&dir, PHRASE.as_bytes()), "not in the database, its journal or any file");
    assert!(!cloud.names().iter().any(|name| futures_block(cloud.read(name)).unwrap().is_some_and(|bytes| bytes.windows(PHRASE.len()).any(|w| w == PHRASE.as_bytes()))));

    std::fs::create_dir_all(dir.join("files").join("uploads")).unwrap();
    let picked = dir.join("files").join("uploads").join("photo.jpg");
    std::fs::write(&picked, b"a photo").unwrap();
    let folder = core.vault_mkdir("Photos", None).await.unwrap();
    let id = core.vault_upload(&core.vault_upload_source(&picked).unwrap(), "photo.jpg", "image/jpeg", Some(&folder)).await.unwrap().expect("up");
    // The core never deletes what it seals: a message's file ("keep in my drive") is uploaded the
    // same way and must stay; only the app's command for a fresh pick deletes the picker's copy.
    assert_eq!(std::fs::read(&picked).unwrap(), b"a photo");
    let listing = core.vault_list(Some(&folder)).await.unwrap();
    assert_eq!(listing.files[0].id, id);
    assert!(core.vault_upload_source(&dir.join("files").join("flickertalk.db")).is_err(), "not a picked file");
    let (path, file) = core.vault_download(&id).await.unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"a photo");
    assert_eq!(file.name, "photo.jpg");
    assert!(path.starts_with(dir.join("files").join("drive")));
    core.vault_rename(&id, "holiday.jpg").await.unwrap();
    core.vault_move(&id, None).await.unwrap();
    assert_eq!(core.vault_list(None).await.unwrap().files[0].name, "holiday.jpg");
    core.vault_remove(&folder).await.unwrap();
    assert_eq!(core.vault_status().await.unwrap().drive.unwrap().folders, 0);

    // Forgetting the drive forgets tokens and key here; the cloud keeps everything.
    core.vault_disconnect().await.unwrap();
    assert_eq!(core.vault_status().await.unwrap().state, VaultState::None);
    assert!(core.store().setting("vault.tokens").await.unwrap().is_none());
    assert!(cloud.names().iter().any(|name| name.starts_with("blob-")));

    // Connected again: the drive is there, and only the phrase opens it.
    let status = core.vault_connect("memory", &browser).await.unwrap();
    assert_eq!(status.state, VaultState::Locked);
    assert!(core.vault_unlock("not my phrase at all").await.is_err());
    core.vault_unlock(PHRASE).await.unwrap();
    assert_eq!(core.vault_list(None).await.unwrap().files[0].name, "holiday.jpg");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_phone_is_backed_up_and_a_new_phone_brings_it_down_ready_to_swap_in() {
    let cloud = Memory::new();
    let (old, old_dir) = phone("old", &cloud).await;
    old.set_name("Alice").await.unwrap();
    let browser = Browser { opened: Default::default(), refuses: false };
    old.vault_connect("memory", &browser).await.unwrap();
    old.vault_setup(PHRASE).await.unwrap();
    std::fs::create_dir_all(old_dir.join("files").join("m1")).unwrap();
    std::fs::write(old_dir.join("files").join("m1").join("received.pdf"), b"pdf bytes").unwrap();

    let backup = old.vault_backup().await.unwrap();
    assert_eq!(backup.files.len(), 1);
    assert!(old.vault_backup_info().await.unwrap().is_some());
    assert!(!old_dir.join("vault").join("snapshot.db").exists(), "the copy does not stay on the phone");

    let (new, new_dir) = phone("new", &cloud).await;
    new.vault_connect("memory", &browser).await.unwrap();
    new.vault_unlock(PHRASE).await.unwrap();
    let restored = new.vault_restore().await.unwrap();
    assert_eq!(restored.at, backup.at);
    // What a move leaves: the app swaps it in at the next start (§60), with the old phone's key.
    let (db, key) = ft_core::moving::received_move(&new_dir.join("move")).expect("a copy and its key");
    assert!(db.exists());
    assert_eq!(key, [4; 32]);
    let copy = Store::open(&db).await.unwrap();
    assert_eq!(copy.setting("name").await.unwrap().as_deref(), Some("Alice"));
    assert_eq!(std::fs::read(new_dir.join("files").join("m1").join("received.pdf")).unwrap(), b"pdf bytes");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plugin_uses_the_drive_only_if_granted() {
    let cloud = Memory::new();
    let (core, _) = phone("p", &cloud).await;
    let catalogue = Ed25519SecretKey::new();
    let manifest = r#"{"id":"com.example.drive","name":"Drive","version":"1.0.0","minCoreVersion":"0.1.0","components":["ft-drive"],"permissions":{"drive":true}}"#;
    let package = sign_package(&[("module.json".to_owned(), manifest.as_bytes().to_vec()), ("dist/index.js".to_owned(), b"".to_vec())], &catalogue);
    core.install_plugin(&package, &catalogue.public_key(), Permissions::default()).await.unwrap();
    assert!(!core.plugin_may_use_drive("com.example.drive").await.unwrap());
    core.grant_plugin("com.example.drive", Permissions { drive: true, ..Permissions::default() }).await.unwrap();
    assert!(core.plugin_may_use_drive("com.example.drive").await.unwrap());
    assert!(core.plugin_may_use_drive("com.example.other").await.is_err(), "not installed");
}

// Decision 2026-09-28: five wrong phrases lock the recovery on this phone for 24 hours. It only
// slows whoever tries from the app; the cloud's copy is guarded by the phrase's stretching.
#[tokio::test(flavor = "multi_thread")]
async fn five_wrong_phrases_lock_the_recovery_for_a_day() {
    let cloud = Memory::new();
    let browser = Browser { opened: Default::default(), refuses: false };
    let (made, _) = phone("made", &cloud).await;
    made.vault_connect("memory", &browser).await.unwrap();
    made.vault_setup(PHRASE).await.unwrap();

    let (core, _) = phone("tries", &cloud).await;
    let status = core.vault_connect("memory", &browser).await.unwrap();
    assert_eq!((status.state, status.tries_left, status.retry_at), (VaultState::Locked, 5, None));
    assert!(core.vault_unlock("short").await.is_err());
    assert_eq!(core.vault_status().await.unwrap().tries_left, 5, "not a phrase at all is not a try");
    for left in (1..5).rev() {
        assert!(core.vault_unlock("not my phrase at all").await.is_err());
        assert_eq!(core.vault_status().await.unwrap().tries_left, left);
    }
    let before = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    assert!(core.vault_unlock("not my phrase at all").await.is_err());
    let status = core.vault_status().await.unwrap();
    let until = status.retry_at.expect("locked");
    assert!(until >= before + 24 * 3600 * 1000 && until < before + 24 * 3600 * 1000 + 60_000, "a day");
    assert_eq!(status.tries_left, 0);
    let locked = core.vault_unlock(PHRASE).await.unwrap_err();
    assert!(locked.to_string().contains("try again"), "not even the right one, for a day: {locked}");
    assert_eq!(core.vault_status().await.unwrap().state, VaultState::Locked);

    // A day later (the phone's own record, moved back): the right phrase opens it, and the
    // count starts again.
    core.store().set_setting("vault.tries", &format!(r#"{{"failed":0,"until":{}}}"#, before - 1)).await.unwrap();
    assert_eq!(core.vault_status().await.unwrap().retry_at, None);
    core.vault_unlock(PHRASE).await.unwrap();
    assert_eq!(core.vault_status().await.unwrap().state, VaultState::Ready);
    assert_eq!(core.vault_status().await.unwrap().tries_left, 5);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_phrase_is_changed_from_the_phone_that_has_the_drive() {
    let cloud = Memory::new();
    let browser = Browser { opened: Default::default(), refuses: false };
    let (core, _) = phone("change", &cloud).await;
    assert!(core.vault_change_phrase("another phrase, a new one").await.is_err(), "no drive open");
    core.vault_connect("memory", &browser).await.unwrap();
    core.vault_setup(PHRASE).await.unwrap();
    assert!(core.vault_change_phrase("short").await.is_err());
    core.vault_change_phrase("another phrase, a new one").await.unwrap();

    let (other, _) = phone("change-other", &cloud).await;
    other.vault_connect("memory", &browser).await.unwrap();
    assert!(other.vault_unlock(PHRASE).await.is_err(), "the old one no longer opens it");
    other.vault_unlock("another phrase, a new one").await.unwrap();
}

// A drive made with the first version's code (never published): the phone says so and makes it
// again; nothing of it opens.
#[tokio::test(flavor = "multi_thread")]
async fn a_drive_of_the_first_version_asks_to_be_made_again() {
    let cloud = Memory::new();
    cloud.write("vault.json", br#"{"format":"ftvault","version":1,"id":"old"}"#.to_vec()).await.unwrap();
    cloud.write("key.ftv", cipher::seal(&[9; 32], "key", &[1; 32]).unwrap()).await.unwrap();
    let browser = Browser { opened: Default::default(), refuses: false };
    let (core, _) = phone("first", &cloud).await;
    let status = core.vault_connect("memory", &browser).await.unwrap();
    assert_eq!(status.state, VaultState::Outdated);
    assert!(core.vault_unlock(PHRASE).await.is_err());
    assert_eq!(core.vault_status().await.unwrap().tries_left, 5, "an old drive is not a wrong phrase");
    core.vault_setup(PHRASE).await.unwrap();
    assert_eq!(core.vault_status().await.unwrap().state, VaultState::Ready);
}

// The Samsung of the tests (2026-09-28): it kept the key of a drive made with the first
// version's code. At the next start the drive is not opened with it: it is to be made again.
#[tokio::test(flavor = "multi_thread")]
async fn a_phone_that_kept_the_key_of_a_first_version_drive_is_asked_to_make_it_again() {
    let cloud = Memory::new();
    let browser = Browser { opened: Default::default(), refuses: false };
    let (core, dir) = phone("kept", &cloud).await;
    core.vault_connect("memory", &browser).await.unwrap();
    core.vault_setup(PHRASE).await.unwrap();
    drop(core);
    // As the first version left it.
    cloud.write("vault.json", br#"{"format":"ftvault","version":1,"id":"old"}"#.to_vec()).await.unwrap();
    cloud.write("key.ftv", cipher::seal(&[9; 32], "key", &[1; 32]).unwrap()).await.unwrap();

    let again = Core::open(Store::open(&dir.join("phone.db")).await.unwrap(), [4; 32], Arc::new(Offline)).await.unwrap();
    again.set_vault_dir(dir.join("vault"));
    again.set_cloud(Arc::new(MemoryCloud(cloud.clone())));
    let status = again.vault_reopen().await.unwrap();
    assert_eq!((status.state, status.problem), (VaultState::Outdated, None));
    again.vault_setup(PHRASE).await.unwrap();
    assert_eq!(again.vault_status().await.unwrap().state, VaultState::Ready);
}
