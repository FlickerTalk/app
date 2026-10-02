//! Plugins on this phone (issue app#3, Plan §48–§58): a package is only installed if the
//! catalogue signed it, nothing is granted by installing, and removing leaves nothing behind.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use ft_core::{Core, Peer, Transport};
use ft_plugins::{sign_package, Permissions, Sending};
use ft_storage::Store;
use vodozemac::Ed25519SecretKey;

struct Offline;

#[async_trait]
impl Transport for Offline {
    async fn send_direct(&self, _to: &Peer, _bytes: Vec<u8>) -> anyhow::Result<bool> {
        Ok(false)
    }

    async fn send_mailbox(&self, _to: &Peer, _bytes: Vec<u8>) -> anyhow::Result<()> {
        Ok(())
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ft-plugins-{name}-{}", ft_protocol::MessageId::new()));
    std::fs::create_dir_all(&dir).expect("creates the directory");
    dir
}

async fn core() -> (Arc<Core>, PathBuf) {
    let core = Core::open(Store::open_in_memory().await.expect("store"), [4; 32], Arc::new(Offline))
        .await
        .expect("opens");
    let dir = scratch("home");
    core.set_plugins_dir(dir.clone());
    (core, dir)
}

/// What the catalogue would publish, signed with the key the app trusts.
fn signed(id: &str, version: &str, permissions: &str, catalogue: &Ed25519SecretKey) -> Vec<u8> {
    let manifest = format!(
        r#"{{"id":"{id}","name":"Code","version":"{version}","minCoreVersion":"0.1.0","components":["ft-code"],"permissions":{permissions}}}"#
    );
    sign_package(
        &[
            ("module.json".to_owned(), manifest.into_bytes()),
            ("dist/index.js".to_owned(), b"customElements.define('ft-code', class extends HTMLElement {})".to_vec()),
        ],
        catalogue,
    )
}

// §51: a capability the plugin needs and this FlickerTalk lacks is a plugin that does not install.
#[tokio::test]
async fn a_plugin_that_needs_a_newer_core_does_not_install() {
    let (core, _) = core().await;
    let catalogue = Ed25519SecretKey::new();
    let manifest = r#"{"id":"com.example.future","name":"Future","version":"1.0.0","minCoreVersion":"99.0.0","components":["ft-future"]}"#;
    let package = sign_package(&[("module.json".to_owned(), manifest.as_bytes().to_vec()), ("dist/index.js".to_owned(), b"".to_vec())], &catalogue);
    let refused = core.install_plugin(&package, &catalogue.public_key(), Permissions::default()).await;
    assert!(refused.unwrap_err().to_string().contains("99.0.0"));
    assert!(core.plugins().await.unwrap().is_empty());
    // What is new enough, and what this very version brought, installs.
    let manifest = format!(r#"{{"id":"com.example.now","name":"Now","version":"1.0.0","minCoreVersion":"{}","components":["ft-now"]}}"#, ft_core::plugins::CORE_VERSION);
    let package = sign_package(&[("module.json".to_owned(), manifest.into_bytes()), ("dist/index.js".to_owned(), b"".to_vec())], &catalogue);
    core.install_plugin(&package, &catalogue.public_key(), Permissions::default()).await.expect("installs");
}

#[tokio::test]
async fn installs_a_signed_plugin_and_grants_it_only_what_the_user_said() {
    let (core, dir) = core().await;
    let catalogue = Ed25519SecretKey::new();
    let package = signed("com.example.code", "1.0.0", "{}", &catalogue);

    let installed = core
        .install_plugin(&package, &catalogue.public_key(), Permissions::default())
        .await
        .expect("installs");
    assert_eq!(installed.id, "com.example.code");
    assert!(dir.join("com.example.code/dist/index.js").exists(), "its files are on the phone");

    let listed = core.plugins().await.expect("lists");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].manifest.version, "1.0.0");
    assert_eq!(listed[0].granted, Permissions::default(), "installing grants nothing");
}

#[tokio::test]
async fn refuses_a_package_the_catalogue_did_not_sign() {
    let (core, dir) = core().await;
    let (catalogue, stranger) = (Ed25519SecretKey::new(), Ed25519SecretKey::new());
    let package = signed("com.example.code", "1.0.0", "{}", &stranger);

    assert!(core
        .install_plugin(&package, &catalogue.public_key(), Permissions::default())
        .await
        .is_err());
    assert!(!dir.join("com.example.code").exists(), "nothing was written");
    assert!(core.plugins().await.expect("lists").is_empty());
}

#[tokio::test]
async fn never_grants_more_than_the_plugin_asked_for() {
    let (core, _dir) = core().await;
    let catalogue = Ed25519SecretKey::new();
    let asked = r#"{"messages":"given","network":["api.openai.com"]}"#;
    let package = signed("com.example.ai", "1.0.0", asked, &catalogue);
    core.install_plugin(&package, &catalogue.public_key(), Permissions::default()).await.expect("installs");

    // The user grants the network it asked for: fine.
    let granted = Permissions { network: vec!["api.openai.com".to_owned()], reads_given_messages: true, send: Sending::Nothing, ..Permissions::default() };
    core.grant_plugin("com.example.ai", granted.clone()).await.expect("grants");
    assert_eq!(core.plugins().await.unwrap()[0].granted, granted);

    // A host it never asked for, or sending on the user's behalf, cannot be granted.
    let sneaky = Permissions { network: vec!["evil.example".to_owned()], ..Permissions::default() };
    assert!(core.grant_plugin("com.example.ai", sneaky).await.is_err());
    let louder = Permissions { send: Sending::Auto, ..granted.clone() };
    assert!(core.grant_plugin("com.example.ai", louder).await.is_err());
    assert_eq!(core.plugins().await.unwrap()[0].granted, granted, "what was granted did not change");
}

#[tokio::test]
async fn removing_a_plugin_leaves_nothing_behind() {
    let (core, dir) = core().await;
    let catalogue = Ed25519SecretKey::new();
    core.install_plugin(&signed("com.example.code", "1.0.0", "{}", &catalogue), &catalogue.public_key(), Permissions::default())
        .await
        .expect("installs");

    core.remove_plugin("com.example.code").await.expect("removes");
    assert!(core.plugins().await.expect("lists").is_empty());
    assert!(!dir.join("com.example.code").exists());
}

// Issue app#4: the frame of a plugin has no origin, so the browser gives it no storage. The core
// is the one that remembers for it, apart from every other plugin and within a limit (§53).
#[tokio::test]
async fn a_plugin_remembers_through_the_core_and_only_its_own() {
    let (core, _dir) = core().await;
    let catalogue = Ed25519SecretKey::new();
    for id in ["com.example.code", "com.example.ai"] {
        let package = signed(id, "1.0.0", "{}", &catalogue);
        core.install_plugin(&package, &catalogue.public_key(), Permissions::default()).await.expect("installs");
    }

    assert_eq!(core.plugin_remembers("com.example.code", None, "pen").await.unwrap(), None);
    core.plugin_remember("com.example.code", None, "pen", "black").await.expect("remembers");
    core.plugin_remember("com.example.ai", None, "pen", "blue").await.expect("remembers");
    assert_eq!(core.plugin_remembers("com.example.code", None, "pen").await.unwrap().as_deref(), Some("black"));
    assert_eq!(core.plugin_memory_keys("com.example.code", None).await.unwrap(), ["pen"]);

    core.plugin_forget("com.example.code", None, "pen").await.expect("forgets");
    assert_eq!(core.plugin_remembers("com.example.code", None, "pen").await.unwrap(), None);
    assert_eq!(core.plugin_remembers("com.example.ai", None, "pen").await.unwrap().as_deref(), Some("blue"));
}

#[tokio::test]
async fn a_plugin_cannot_fill_the_phone_nor_write_for_another() {
    let (core, _dir) = core().await;
    let catalogue = Ed25519SecretKey::new();
    let package = signed("com.example.code", "1.0.0", "{}", &catalogue);
    core.install_plugin(&package, &catalogue.public_key(), Permissions::default()).await.expect("installs");

    assert!(core.plugin_remember("com.example.code", None, "big", &"x".repeat(70_000)).await.is_err(), "too much");
    assert!(core.plugin_remember("com.example.code", None, &"k".repeat(200), "v").await.is_err(), "too long a key");
    assert!(core.plugin_remember("com.example.never", None, "pen", "black").await.is_err(), "it is not installed");

    for number in 0..64 {
        core.plugin_remember("com.example.code", None, &format!("key{number}"), "v").await.expect("remembers");
    }
    assert!(core.plugin_remember("com.example.code", None, "one-more", "v").await.is_err(), "too many keys");
    // What it already remembers it can still change.
    core.plugin_remember("com.example.code", None, "key0", "w").await.expect("remembers");
}

// 2026-09-27: a plugin's records are bigger than its settings and fit the room the user granted.
#[tokio::test]
async fn a_plugin_keeps_records_within_the_room_it_was_granted() {
    let (core, _dir) = core().await;
    let catalogue = Ed25519SecretKey::new();
    let package = signed("com.example.notes", "1.0.0", r#"{"storage":"large"}"#, &catalogue);
    core.install_plugin(&package, &catalogue.public_key(), Permissions::default()).await.expect("installs");

    // Installed with nothing granted: the small room, and a value beyond a setting still fits.
    let (used, quota) = core.plugin_records_usage("com.example.notes", None).await.unwrap();
    assert_eq!((used, quota), (0, ft_plugins::Storage::Small.quota()));
    core.plugin_record_set("com.example.notes", None, "note/1", &vec![7u8; 100_000]).await.expect("keeps");
    assert_eq!(core.plugin_record("com.example.notes", None, "note/1").await.unwrap().map(|v| v.len()), Some(100_000));
    assert_eq!(core.plugin_record_keys("com.example.notes", None, "note/").await.unwrap(), ["note/1"]);
    // The small room is 4 MB: one more of 4 MB does not fit; with the large room it does.
    let big = vec![1u8; 4 * 1024 * 1024];
    assert!(core.plugin_record_set("com.example.notes", None, "board", &big).await.is_err(), "no room");
    core.grant_plugin("com.example.notes", Permissions { storage: ft_plugins::Storage::Large, ..Permissions::default() }).await.unwrap();
    core.plugin_record_set("com.example.notes", None, "board", &big).await.expect("now it fits");
    let (used, quota) = core.plugin_records_usage("com.example.notes", None).await.unwrap();
    assert_eq!((used, quota), (100_000 + big.len() as u64, ft_plugins::Storage::Large.quota()));
    // Replacing a record counts the new size, not both.
    core.plugin_record_set("com.example.notes", None, "board", &big[..10]).await.expect("smaller");
    assert_eq!(core.plugin_records_usage("com.example.notes", None).await.unwrap().0, 100_010);
    assert!(core.plugin_record_set("com.example.notes", None, "huge", &vec![0u8; ft_core::plugins::RECORD_VALUE + 1]).await.is_err());
    assert!(core.plugin_record_set("com.example.never", None, "x", b"y").await.is_err(), "not installed");
    core.plugin_record_forget("com.example.notes", None, "board").await.unwrap();
    assert_eq!(core.plugin_record("com.example.notes", None, "board").await.unwrap(), None);
}

// 2026-09-27: reminders are a permission of their own; the phone's alarm clock is told.
#[tokio::test]
async fn a_plugin_sets_reminders_only_if_granted_and_the_app_hears_of_it() {
    let (core, _dir) = core().await;
    let catalogue = Ed25519SecretKey::new();
    let package = signed("com.example.notes", "1.0.0", r#"{"remind":true}"#, &catalogue);
    core.install_plugin(&package, &catalogue.public_key(), Permissions::default()).await.expect("installs");
    assert!(core.set_reminder("com.example.notes", None, "r1", 5_000, "milk").await.is_err(), "not granted yet");

    core.grant_plugin("com.example.notes", Permissions { remind: true, ..Permissions::default() }).await.unwrap();
    let mut events = core.events();
    core.set_reminder("com.example.notes", None, "r1", 5_000, "milk").await.expect("sets");
    assert_eq!(events.try_recv().ok(), Some(ft_core::Event::RemindersChanged));
    core.set_reminder("com.example.notes", None, "r2", 1_000, &"x".repeat(500)).await.expect("sets");
    let reminders = core.plugin_reminders("com.example.notes", None).await.unwrap();
    assert_eq!(reminders.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["r2", "r1"], "soonest first");
    assert_eq!(reminders[0].text.chars().count(), 200, "the text is cut");
    assert_eq!(core.due_reminders(2_000).await.unwrap().len(), 1);
    assert_eq!(core.reminders().await.unwrap().len(), 2);
    assert!(core.cancel_reminder("com.example.notes", None, "r1").await.unwrap());
    assert!(!core.cancel_reminder("com.example.notes", None, "r1").await.unwrap());
    assert!(core.set_reminder("com.example.notes", None, "", 5_000, "").await.is_err());
}

// A reminder already handed to the phone's alarm clock must not outlive the plugin, nor the
// permission: the app hears of it and tells the alarm clock again.
#[tokio::test]
async fn removing_a_plugin_or_its_remind_permission_takes_its_reminders_off_the_alarm_clock() {
    let (core, _dir) = core().await;
    let catalogue = Ed25519SecretKey::new();
    let remind = Permissions { remind: true, ..Permissions::default() };
    let package = signed("com.example.notes", "1.0.0", r#"{"remind":true}"#, &catalogue);
    core.install_plugin(&package, &catalogue.public_key(), remind.clone()).await.expect("installs");

    core.set_reminder("com.example.notes", None, "r1", 5_000, "milk").await.expect("sets");
    let mut events = core.events();
    core.grant_plugin("com.example.notes", Permissions::default()).await.unwrap();
    assert!(core.reminders().await.unwrap().is_empty(), "revoking remind drops its reminders");
    let heard: Vec<_> = std::iter::from_fn(|| events.try_recv().ok()).collect();
    assert!(heard.contains(&ft_core::Event::RemindersChanged), "the alarm clock is told: {heard:?}");

    core.grant_plugin("com.example.notes", remind).await.unwrap();
    core.set_reminder("com.example.notes", None, "r2", 5_000, "bread").await.expect("sets");
    let mut events = core.events();
    core.remove_plugin("com.example.notes").await.unwrap();
    assert!(core.reminders().await.unwrap().is_empty());
    let heard: Vec<_> = std::iter::from_fn(|| events.try_recv().ok()).collect();
    assert!(heard.contains(&ft_core::Event::RemindersChanged), "the alarm clock is told: {heard:?}");
}

/// A web that answers whatever is asked, and writes down what it was asked.
#[derive(Default)]
struct FakeWeb {
    asked: std::sync::Mutex<Vec<String>>,
}

#[async_trait]
impl ft_core::Fetch for FakeWeb {
    async fn get(&self, url: &str, _limit: u64) -> anyhow::Result<Vec<u8>> {
        self.asked.lock().unwrap().push(url.to_owned());
        Ok(b"{}".to_vec())
    }

    async fn call(&self, request: &ft_core::WebRequest, _limit: u64) -> anyhow::Result<ft_core::WebAnswer> {
        self.asked.lock().unwrap().push(request.url.clone());
        Ok(ft_core::WebAnswer { status: 200, body: b"answered".to_vec() })
    }
}

// §55: the network of a plugin goes through the core, which checks the host against what the
// user granted. The policy of the frame is the second lock, never the only one.
#[tokio::test]
async fn a_plugin_reaches_only_the_hosts_the_user_granted() {
    let (core, _dir) = core().await;
    let catalogue = Ed25519SecretKey::new();
    let package = signed("com.example.ai", "1.0.0", r#"{"network":["api.openai.com"]}"#, &catalogue);
    let asked = Permissions { network: vec!["api.openai.com".to_owned()], ..Permissions::default() };
    core.install_plugin(&package, &catalogue.public_key(), Permissions::default()).await.expect("installs");
    let web = FakeWeb::default();

    let ask = |url: &str| ft_core::WebRequest {
        url: url.to_owned(),
        method: "POST".to_owned(),
        headers: vec![("content-type".to_owned(), "application/json".to_owned())],
        body: Some(b"{}".to_vec()),
    };

    // Installed is not granted: until the user says yes, it reaches nothing (§53).
    assert!(core.plugin_fetch("com.example.ai", ask("https://api.openai.com/v1/chat"), &web).await.is_err());

    core.grant_plugin("com.example.ai", asked).await.expect("granted");
    let answer = core.plugin_fetch("com.example.ai", ask("https://api.openai.com/v1/chat"), &web).await.expect("reaches");
    assert_eq!(answer.status, 200);
    assert_eq!(answer.body, b"answered");

    for refused in [
        "https://evil.example/steal",
        "http://api.openai.com/v1/chat",
        "https://user@evil.example/x",
        "https://api.openai.com.evil.example/x",
        "file:///etc/passwd",
    ] {
        assert!(core.plugin_fetch("com.example.ai", ask(refused), &web).await.is_err(), "{refused} should be refused");
    }
    assert_eq!(web.asked.lock().unwrap().len(), 1, "nothing refused ever left the phone");
}

/// The catalogue as the site serves it: an index, its signature, and the packages next to them.
struct Shop {
    files: std::collections::HashMap<String, Vec<u8>>,
}

#[async_trait]
impl ft_core::Fetch for Shop {
    async fn get(&self, url: &str, _limit: u64) -> anyhow::Result<Vec<u8>> {
        self.files.get(url).cloned().ok_or_else(|| anyhow::anyhow!("{url} is not there"))
    }

    async fn call(&self, _request: &ft_core::WebRequest, _limit: u64) -> anyhow::Result<ft_core::WebAnswer> {
        anyhow::bail!("the catalogue is only ever read")
    }
}

fn shop(package: &[u8], catalogue: &Ed25519SecretKey) -> (Shop, String) {
    let url = format!("{}/com.example.code/1.0.0.ftplugin", ft_core::CATALOGUE_HOME);
    let index = format!(
        r#"{{"plugins":[{{"id":"com.example.code","name":"Code","version":"1.0.0","minCoreVersion":"0.1.0","size":{},"hash":"{}","url":"{url}","summary":"Shows code."}}]}}"#,
        package.len(),
        blake3::hash(package).to_hex()
    );
    let mut files = std::collections::HashMap::new();
    files.insert(format!("{}/{}", ft_core::CATALOGUE_HOME, ft_plugins::INDEX), index.clone().into_bytes());
    files.insert(
        format!("{}/{}.sig", ft_core::CATALOGUE_HOME, ft_plugins::INDEX),
        catalogue.sign(index.as_bytes()).to_base64().into_bytes(),
    );
    // What the app 1.0.0 reads lists nothing here: this core must not read it.
    let legacy = r#"{"plugins":[]}"#;
    files.insert(format!("{}/{}", ft_core::CATALOGUE_HOME, ft_plugins::LEGACY_INDEX), legacy.as_bytes().to_vec());
    files.insert(
        format!("{}/{}.sig", ft_core::CATALOGUE_HOME, ft_plugins::LEGACY_INDEX),
        catalogue.sign(legacy.as_bytes()).to_base64().into_bytes(),
    );
    files.insert(url.clone(), package.to_vec());
    (Shop { files }, url)
}

// §56: nothing travels inside the app. The phone reads a signed index, and only then downloads a
// package, which has to be exactly the bytes the index listed. Since 2026-09-28 it is the index
// for cores from 1.1.0 on, not the one the app 1.0.0 reads (that one lists only what runs there).
#[tokio::test]
async fn installs_from_the_catalogue_only_what_the_catalogue_signed() {
    let (core, dir) = core().await;
    let catalogue = Ed25519SecretKey::new();
    let package = signed("com.example.code", "1.0.0", "{}", &catalogue);
    let (shop, _url) = shop(&package, &catalogue);

    let offered = core.catalogue(&shop, &catalogue.public_key()).await.expect("reads the catalogue");
    assert_eq!(offered.len(), 1);
    assert_eq!(offered[0].id, "com.example.code");
    assert!(core.plugins().await.unwrap().is_empty(), "reading the catalogue installs nothing");

    let manifest = core.add_plugin(&offered[0], &shop, &catalogue.public_key()).await.expect("installs");
    assert_eq!(manifest.id, "com.example.code");
    assert!(dir.join("com.example.code/dist/index.js").exists());
    let installed = core.plugins().await.unwrap();
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0].granted, Permissions::default(), "installing grants nothing");
}

#[tokio::test]
async fn refuses_a_listing_that_points_anywhere_else() {
    let (core, _dir) = core().await;
    let catalogue = Ed25519SecretKey::new();
    let package = signed("com.example.code", "1.0.0", "{}", &catalogue);
    let (shop, url) = shop(&package, &catalogue);
    let listed = core.catalogue(&shop, &catalogue.public_key()).await.expect("reads the catalogue");

    let elsewhere = ft_plugins::CatalogueEntry { url: "https://evil.example/a.ftplugin".to_owned(), ..listed[0].clone() };
    assert!(core.add_plugin(&elsewhere, &shop, &catalogue.public_key()).await.is_err());

    let swapped = ft_plugins::CatalogueEntry { hash: "ab".repeat(32), url, ..listed[0].clone() };
    assert!(core.add_plugin(&swapped, &shop, &catalogue.public_key()).await.is_err(), "not the bytes listed");

    // Someone else's signature on the index is no signature at all.
    let theirs = Ed25519SecretKey::new();
    assert!(core.catalogue(&shop, &theirs.public_key()).await.is_err());
}

// 2026-10-02 (plan of the games): the catalogue says what is a game, and the app shows it apart.
// A kind a newer FlickerTalk adds is not offered here: this one would not know where to show it,
// and would not install it either.
#[tokio::test]
async fn the_catalogue_offers_tools_and_games_but_no_kind_this_core_does_not_know() {
    let (core, _dir) = core().await;
    let catalogue = Ed25519SecretKey::new();
    let entry = |id: &str, kind: &str| {
        format!(
            r#"{{"id":"{id}","name":"X","version":"1.0.0","minCoreVersion":"0.1.0","size":1,"hash":"{}","url":"{}/{id}/1.0.0.ftplugin","kind":"{kind}"}}"#,
            "ab".repeat(32),
            ft_core::CATALOGUE_HOME
        )
    };
    let index = format!(
        r#"{{"plugins":[{},{},{}]}}"#,
        entry("com.example.code", "tool"),
        entry("com.example.chess", "game"),
        entry("com.example.widget", "widget")
    );
    let mut files = std::collections::HashMap::new();
    files.insert(format!("{}/{}", ft_core::CATALOGUE_HOME, ft_plugins::INDEX), index.clone().into_bytes());
    files.insert(
        format!("{}/{}.sig", ft_core::CATALOGUE_HOME, ft_plugins::INDEX),
        catalogue.sign(index.as_bytes()).to_base64().into_bytes(),
    );

    let offered = core.catalogue(&Shop { files }, &catalogue.public_key()).await.expect("reads the catalogue");
    let kinds: Vec<(&str, ft_plugins::Kind)> = offered.iter().map(|entry| (entry.id.as_str(), entry.kind)).collect();
    assert_eq!(kinds, [("com.example.code", ft_plugins::Kind::Tool), ("com.example.chess", ft_plugins::Kind::Game)]);
}

// ---- Hidden sessions (2026-10-01, §108): what a plugin keeps inside one stays inside it ----

/// A core with a plugin that keeps records, and a hidden session open.
async fn notes_and_a_session() -> (Arc<Core>, String) {
    let (core, _dir) = core().await;
    let catalogue = Ed25519SecretKey::new();
    let package = signed("com.example.notes", "1.0.0", r#"{"storage":"large","remind":true}"#, &catalogue);
    let granted = Permissions { remind: true, ..Permissions::default() };
    core.install_plugin(&package, &catalogue.public_key(), granted).await.expect("installs");
    let session = core.open_session("123456").await.expect("opens").expect("there is room for it");
    (core, session)
}

#[tokio::test]
async fn a_record_kept_inside_a_session_is_not_seen_from_the_main_list() {
    let (core, session) = notes_and_a_session().await;
    core.plugin_record_set("com.example.notes", Some(&session), "board/1", b"the secret board").await.expect("keeps");

    assert_eq!(core.plugin_record("com.example.notes", None, "board/1").await.unwrap(), None, "not its value");
    assert!(core.plugin_record_keys("com.example.notes", None, "").await.unwrap().is_empty(), "not its key");
    assert_eq!(core.plugin_records_usage("com.example.notes", None).await.unwrap().0, 0, "not its size");
}

/// Someone to keep a hidden session from going when it is closed: one with nobody in it goes.
async fn someone_in(core: &Core, session: &str) -> String {
    let (bob, _dir) = self::core().await;
    let link = bob.my_card().await.expect("card").to_link();
    core.add_contact_in(&link, None, Some(session)).await.expect("adds inside").device_id
}

#[tokio::test]
async fn a_record_kept_inside_a_session_is_seen_inside_it_and_nowhere_else() {
    let (core, session) = notes_and_a_session().await;
    core.plugin_record_set("com.example.notes", Some(&session), "board/1", b"the secret board").await.expect("keeps");

    assert_eq!(core.plugin_record("com.example.notes", Some(&session), "board/1").await.unwrap().as_deref(), Some(&b"the secret board"[..]));
    assert_eq!(core.plugin_record_keys("com.example.notes", Some(&session), "board/").await.unwrap(), ["board/1"]);
    assert_eq!(core.plugin_records_usage("com.example.notes", Some(&session)).await.unwrap().0, 16);

    // Another session sees nothing of it either.
    let other = core.open_session("654321").await.expect("opens").expect("room");
    assert_eq!(core.plugin_record("com.example.notes", Some(&other), "board/1").await.unwrap(), None);
    assert!(core.plugin_record_keys("com.example.notes", Some(&other), "").await.unwrap().is_empty());
    assert_eq!(core.plugin_records_usage("com.example.notes", Some(&other)).await.unwrap().0, 0);
}

#[tokio::test]
async fn a_closed_session_lends_its_records_to_nobody_and_has_them_back_when_opened() {
    let (core, session) = notes_and_a_session().await;
    someone_in(&core, &session).await;
    core.plugin_record_set("com.example.notes", Some(&session), "board/1", b"the secret board").await.expect("keeps");

    assert!(!core.close_session(&session).await.expect("closes"), "it holds a contact: it stays");
    assert!(core.plugin_record("com.example.notes", Some(&session), "board/1").await.is_err(), "closed: not even with its id");
    assert!(core.plugin_record_keys("com.example.notes", Some(&session), "").await.is_err());
    assert!(core.plugin_records_usage("com.example.notes", Some(&session)).await.is_err());
    assert!(core.plugin_record_set("com.example.notes", Some(&session), "board/2", b"x").await.is_err());
    assert!(core.plugin_record_forget("com.example.notes", Some(&session), "board/1").await.is_err());
    assert!(core.plugin_record_keys("com.example.notes", None, "").await.unwrap().is_empty());

    assert_eq!(core.open_session("123456").await.unwrap().as_deref(), Some(session.as_str()));
    assert_eq!(core.plugin_record("com.example.notes", Some(&session), "board/1").await.unwrap().as_deref(), Some(&b"the secret board"[..]));
}

#[tokio::test]
async fn a_sessions_records_go_with_it() {
    let (core, session) = notes_and_a_session().await;
    someone_in(&core, &session).await;
    core.plugin_record_set("com.example.notes", Some(&session), "board/1", b"the secret board").await.expect("keeps");

    core.remove_session(&session).await.expect("removes");
    assert_eq!(core.store().plugin_records_size("com.example.notes", Some(&session)).await.unwrap(), 0, "nothing left on the phone");
    let again = core.open_session("123456").await.expect("opens").expect("room");
    assert_ne!(again, session, "a new, empty session");
    assert!(core.plugin_record_keys("com.example.notes", Some(&again), "").await.unwrap().is_empty());
    assert!(core.plugin_record_keys("com.example.notes", None, "").await.unwrap().is_empty());
}

#[tokio::test]
async fn what_the_main_list_keeps_is_seen_there_as_before_and_not_inside_a_session() {
    let (core, session) = notes_and_a_session().await;
    core.plugin_record_set("com.example.notes", None, "board/1", b"a board").await.expect("keeps");

    assert_eq!(core.plugin_record("com.example.notes", None, "board/1").await.unwrap().as_deref(), Some(&b"a board"[..]));
    assert_eq!(core.plugin_record_keys("com.example.notes", None, "").await.unwrap(), ["board/1"]);
    assert_eq!(core.plugin_records_usage("com.example.notes", None).await.unwrap().0, 7);
    // Each place has its own: the same key inside the session is another record.
    assert_eq!(core.plugin_record("com.example.notes", Some(&session), "board/1").await.unwrap(), None);
    core.plugin_record_set("com.example.notes", Some(&session), "board/1", b"another").await.expect("keeps");
    assert_eq!(core.plugin_record("com.example.notes", None, "board/1").await.unwrap().as_deref(), Some(&b"a board"[..]));
    core.plugin_record_forget("com.example.notes", Some(&session), "board/1").await.unwrap();
    assert_eq!(core.plugin_record("com.example.notes", None, "board/1").await.unwrap().as_deref(), Some(&b"a board"[..]), "forgetting inside leaves the main list's");
}

// The room a plugin has is counted for each place apart: a session that fills it must not show in
// the main list's counter, nor in a "no room left" there.
#[tokio::test]
async fn a_session_using_the_room_does_not_show_in_the_main_lists_counter() {
    let (core, session) = notes_and_a_session().await;
    let three_mb = vec![1u8; 3 * 1024 * 1024];
    core.plugin_record_set("com.example.notes", Some(&session), "board/1", &three_mb).await.expect("keeps");

    let (used, quota) = core.plugin_records_usage("com.example.notes", None).await.unwrap();
    assert_eq!((used, quota), (0, ft_plugins::Storage::Small.quota()));
    core.plugin_record_set("com.example.notes", None, "board/1", &three_mb).await.expect("the main list has all its room");
    assert!(core.plugin_record_set("com.example.notes", Some(&session), "board/2", &three_mb).await.is_err(), "the session's own room is full");
}

// The plugin's settings (`ft.store`) follow the same rule: a choice made inside a session is that
// session's.
#[tokio::test]
async fn what_a_plugin_remembers_inside_a_session_stays_inside_it() {
    let (core, session) = notes_and_a_session().await;
    someone_in(&core, &session).await;
    core.plugin_remember("com.example.notes", None, "showText", "0").await.expect("remembers");
    core.plugin_remember("com.example.notes", Some(&session), "showText", "1").await.expect("remembers");
    core.plugin_remember("com.example.notes", Some(&session), "lastBoard", "the secret board").await.expect("remembers");

    assert_eq!(core.plugin_remembers("com.example.notes", None, "showText").await.unwrap().as_deref(), Some("0"));
    assert_eq!(core.plugin_remembers("com.example.notes", None, "lastBoard").await.unwrap(), None);
    assert_eq!(core.plugin_memory_keys("com.example.notes", None).await.unwrap(), ["showText"]);
    assert_eq!(core.plugin_remembers("com.example.notes", Some(&session), "showText").await.unwrap().as_deref(), Some("1"));
    core.plugin_forget("com.example.notes", Some(&session), "showText").await.unwrap();
    assert_eq!(core.plugin_remembers("com.example.notes", None, "showText").await.unwrap().as_deref(), Some("0"), "the main list's stays");

    core.close_session(&session).await.expect("closes");
    assert!(core.plugin_remembers("com.example.notes", Some(&session), "lastBoard").await.is_err(), "closed: lent to nobody");
    assert!(core.plugin_remember("com.example.notes", Some(&session), "x", "y").await.is_err());
    assert!(core.plugin_memory_keys("com.example.notes", Some(&session)).await.is_err());

    core.open_session("123456").await.expect("opens");
    core.remove_session(&session).await.expect("removes");
    assert!(core.store().plugin_keys("com.example.notes", Some(&session)).await.unwrap().is_empty(), "gone with the session");
}

// A reminder set inside a session: its text and its ring belong to the session. While it is closed
// the phone's alarm clock does not hold it, and the app is told so it can tell the alarm clock.
#[tokio::test]
async fn a_reminder_set_inside_a_session_never_rings_nor_shows_while_it_is_closed() {
    let (core, session) = notes_and_a_session().await;
    someone_in(&core, &session).await;
    let ids = |reminders: Vec<ft_storage::Reminder>| reminders.into_iter().map(|reminder| reminder.id).collect::<Vec<_>>();
    core.set_reminder("com.example.notes", None, "r1", 5_000, "milk").await.expect("sets");
    core.set_reminder("com.example.notes", Some(&session), "r2", 4_000, "the secret").await.expect("sets");
    core.set_reminder("com.example.notes", Some(&session), "r1", 6_000, "another secret").await.expect("the same id inside is another");

    assert_eq!(ids(core.plugin_reminders("com.example.notes", None).await.unwrap()), ["r1"]);
    assert_eq!(core.plugin_reminders("com.example.notes", None).await.unwrap()[0].text, "milk");
    assert_eq!(ids(core.plugin_reminders("com.example.notes", Some(&session)).await.unwrap()), ["r2", "r1"]);
    assert_eq!(core.reminders().await.unwrap().len(), 3, "open: all of them ring");
    assert_eq!(core.ringing_reminder("com.example.notes", "r2").await.unwrap().and_then(|reminder| reminder.session), Some(session.clone()), "a tap opens it where it was set");

    let mut events = core.events();
    core.close_session(&session).await.expect("closes");
    let heard: Vec<_> = std::iter::from_fn(|| events.try_recv().ok()).collect();
    assert!(heard.contains(&ft_core::Event::RemindersChanged), "the alarm clock is told: {heard:?}");
    let told = core.reminders().await.unwrap();
    assert_eq!(ids(told.clone()), ["r1"]);
    assert!(told.iter().all(|reminder| !reminder.text.contains("secret")));
    assert_eq!(ids(core.due_reminders(10_000).await.unwrap()), ["r1"]);
    assert_eq!(core.ringing_reminder("com.example.notes", "r2").await.unwrap(), None);
    assert!(core.plugin_reminders("com.example.notes", Some(&session)).await.is_err());
    assert!(core.set_reminder("com.example.notes", Some(&session), "r3", 7_000, "x").await.is_err());
    assert!(core.cancel_reminder("com.example.notes", Some(&session), "r2").await.is_err());
    assert!(core.cancel_reminder("com.example.notes", None, "r2").await.is_ok_and(|gone| !gone), "nothing of it from outside");

    let mut events = core.events();
    core.open_session("123456").await.expect("opens");
    let heard: Vec<_> = std::iter::from_fn(|| events.try_recv().ok()).collect();
    assert!(heard.contains(&ft_core::Event::RemindersChanged), "opened: the alarm clock is told again: {heard:?}");
    assert_eq!(core.reminders().await.unwrap().len(), 3);
    assert!(core.cancel_reminder("com.example.notes", Some(&session), "r1").await.unwrap());
    assert_eq!(ids(core.plugin_reminders("com.example.notes", None).await.unwrap()), ["r1"], "cancelling inside leaves the main list's");

    let mut events = core.events();
    core.remove_session(&session).await.expect("removes");
    let heard: Vec<_> = std::iter::from_fn(|| events.try_recv().ok()).collect();
    assert!(heard.contains(&ft_core::Event::RemindersChanged), "removed: the alarm clock is told: {heard:?}");
    assert_eq!(ids(core.store().reminders(None).await.unwrap()), ["r1"], "its reminders went with it");
}

// A ref is a way back to a message: one to a message inside a session leads there only from inside
// that session while it is open, and goes with the session.
#[tokio::test]
async fn a_ref_to_a_message_inside_a_session_leads_nowhere_from_outside_it() {
    let (core, session) = notes_and_a_session().await;
    let bob = someone_in(&core, &session).await;
    let message = core.send_text(&bob, "hello").await.expect("writes");
    let reference = core.plugin_ref("com.example.notes", &message).await.expect("a ref");

    assert_eq!(core.plugin_ref_target("com.example.notes", Some(&session), &reference).await.unwrap().map(|target| target.contact), Some(bob.clone()));
    assert_eq!(core.plugin_ref_target("com.example.notes", None, &reference).await.unwrap(), None, "not from the main list");
    let other = core.open_session("654321").await.expect("opens").expect("room");
    assert_eq!(core.plugin_ref_target("com.example.notes", Some(&other), &reference).await.unwrap(), None, "not from another session");

    core.close_session(&session).await.expect("closes");
    assert!(core.plugin_ref_target("com.example.notes", Some(&session), &reference).await.is_err(), "closed: lent to nobody");
    assert_eq!(core.plugin_ref_target("com.example.notes", None, &reference).await.unwrap(), None);

    core.open_session("123456").await.expect("opens");
    core.remove_session(&session).await.expect("removes");
    assert_eq!(core.store().plugin_ref(&reference).await.unwrap(), None, "gone with the session");
}

// ---- The conversation a plugin is opened in (2026-10-02, finding 7 of the plan of the plugins) ----
//
// A plugin opened from a chat learns an opaque id of that chat, so what it keeps per conversation
// (a match, a list) stays with that conversation. The id says nothing about the contact, is its
// own for each plugin, and never matches anything on the other phone.

/// A core with two plugins and two contacts in the main list.
async fn two_plugins_and_two_contacts() -> (Arc<Core>, PathBuf, String, String) {
    let (core, dir) = core().await;
    let catalogue = Ed25519SecretKey::new();
    for id in ["com.example.chess", "com.example.list"] {
        core.install_plugin(&signed(id, "1.0.0", "{}", &catalogue), &catalogue.public_key(), Permissions::default()).await.expect("installs");
    }
    let mut contacts = Vec::new();
    for _ in 0..2 {
        let (other, _dir) = self::core().await;
        let link = other.my_card().await.expect("card").to_link();
        contacts.push(core.add_contact(&link, None).await.expect("adds").device_id);
    }
    let bob = contacts.remove(0);
    (core, dir, bob, contacts.remove(0))
}

#[tokio::test]
async fn a_plugin_gets_the_same_chat_id_for_the_same_contact_every_time() {
    let (core, _dir, bob, _carol) = two_plugins_and_two_contacts().await;
    let first = core.plugin_chat("com.example.chess", &bob).await.expect("an id");
    assert_eq!(core.plugin_chat("com.example.chess", &bob).await.unwrap(), first);
}

#[tokio::test]
async fn the_chat_id_outlives_a_restart() {
    let home = scratch("restart");
    let catalogue = Ed25519SecretKey::new();
    let (first, bob) = {
        let core = Core::open(Store::open(&home.join("db")).await.expect("store"), [9; 32], Arc::new(Offline)).await.expect("opens");
        core.set_plugins_dir(home.join("plugins"));
        core.install_plugin(&signed("com.example.chess", "1.0.0", "{}", &catalogue), &catalogue.public_key(), Permissions::default()).await.expect("installs");
        let (other, _dir) = self::core().await;
        let bob = core.add_contact(&other.my_card().await.unwrap().to_link(), None).await.expect("adds").device_id;
        (core.plugin_chat("com.example.chess", &bob).await.expect("an id"), bob)
    };
    let again = Core::open(Store::open(&home.join("db")).await.expect("store"), [9; 32], Arc::new(Offline)).await.expect("opens");
    again.set_plugins_dir(home.join("plugins"));
    assert_eq!(again.plugin_chat("com.example.chess", &bob).await.unwrap(), first);
}

#[tokio::test]
async fn the_chat_id_is_its_own_for_each_plugin_and_each_contact_and_says_nothing_of_them() {
    let (core, _dir, bob, carol) = two_plugins_and_two_contacts().await;
    let chess_bob = core.plugin_chat("com.example.chess", &bob).await.unwrap();
    let chess_carol = core.plugin_chat("com.example.chess", &carol).await.unwrap();
    let list_bob = core.plugin_chat("com.example.list", &bob).await.unwrap();
    assert_ne!(chess_bob, chess_carol, "another conversation");
    assert_ne!(chess_bob, list_bob, "two plugins cannot match their ids");
    for id in [&chess_bob, &chess_carol, &list_bob] {
        assert_eq!(id.len(), 43, "{id}");
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'), "{id}");
        for contact in [&bob, &carol] {
            assert!(!id.contains(contact.as_str()) && !id.contains(contact.trim_start_matches("ft_")), "{id} names {contact}");
        }
    }
}

#[tokio::test]
async fn there_is_no_chat_id_for_a_stranger_a_blocked_contact_or_a_plugin_not_installed() {
    let (core, _dir, bob, _carol) = two_plugins_and_two_contacts().await;
    assert!(core.plugin_chat("com.example.chess", "ft_nobody").await.is_err(), "not a contact");
    assert!(core.plugin_chat("com.example.missing", &bob).await.is_err(), "not installed");
    core.block(&bob, true).await.expect("blocks");
    assert!(core.plugin_chat("com.example.chess", &bob).await.is_err(), "blocked");
}

#[tokio::test]
async fn a_hidden_contacts_chat_id_is_there_only_while_the_session_is_open() {
    let (core, session) = notes_and_a_session().await;
    let hidden = someone_in(&core, &session).await;
    let inside = core.plugin_chat("com.example.notes", &hidden).await.expect("open: an id");
    assert!(!core.close_session(&session).await.expect("closes"));
    let closed = core.plugin_chat("com.example.notes", &hidden).await.expect_err("closed: no id");
    let stranger = core.plugin_chat("com.example.notes", "ft_nobody").await.expect_err("no id");
    assert_eq!(closed.to_string(), stranger.to_string(), "a closed session's contact looks like nobody");
    core.open_session("123456").await.unwrap();
    assert_eq!(core.plugin_chat("com.example.notes", &hidden).await.unwrap(), inside);
}

// Derived, not stored: a removed plugin has no id, and installed again it finds its own again.
// What it kept under that id went with it (`remove_plugin`), so the id leads nowhere new.
#[tokio::test]
async fn a_removed_plugin_has_no_chat_id_and_installed_again_it_finds_its_own() {
    let (core, _dir, bob, _carol) = two_plugins_and_two_contacts().await;
    let before = core.plugin_chat("com.example.chess", &bob).await.unwrap();
    core.remove_plugin("com.example.chess").await.expect("removes");
    assert!(core.plugin_chat("com.example.chess", &bob).await.is_err(), "not installed");
    let catalogue = Ed25519SecretKey::new();
    core.install_plugin(&signed("com.example.chess", "1.0.0", "{}", &catalogue), &catalogue.public_key(), Permissions::default()).await.unwrap();
    assert_eq!(core.plugin_chat("com.example.chess", &bob).await.unwrap(), before);
}

/// The phone's location service, as the tests want it to answer.
struct FakeLocator {
    answer: Option<ft_core::plugins::Fix>,
    asked: std::sync::atomic::AtomicUsize,
}

impl FakeLocator {
    fn answering(answer: Option<ft_core::plugins::Fix>) -> Self {
        Self { answer, asked: std::sync::atomic::AtomicUsize::new(0) }
    }

    fn asked(&self) -> usize {
        self.asked.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[async_trait]
impl ft_core::plugins::Locator for FakeLocator {
    async fn locate(&self) -> anyhow::Result<Option<ft_core::plugins::Fix>> {
        self.asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(self.answer)
    }
}

const MADRID: ft_core::plugins::Fix = ft_core::plugins::Fix { lat: 40.416_78, lon: -3.703_79, accuracy: 35.0, at: 1_790_000_000_000 };

// 2026-10-02: the location plugin. The core is the gate: a plugin learns where the phone is only
// if the user granted it `location`, and the phone is not even asked otherwise.
#[tokio::test]
async fn a_plugin_learns_where_the_phone_is_only_if_the_user_granted_it() {
    let (core, _dir) = core().await;
    let catalogue = Ed25519SecretKey::new();
    let package = signed("com.example.location", "1.0.0", r#"{"location":true,"send":"propose"}"#, &catalogue);
    core.install_plugin(&package, &catalogue.public_key(), Permissions::default()).await.expect("installs");
    let phone = FakeLocator::answering(Some(MADRID));

    assert!(core.plugin_location("com.example.location", &phone).await.is_err(), "installed is not granted (§53)");
    assert!(core.plugin_location("com.example.never", &phone).await.is_err(), "not installed");
    assert_eq!(phone.asked(), 0, "the phone is not even asked for a plugin that may not know");

    core.grant_plugin("com.example.location", Permissions { location: true, ..Permissions::default() }).await.expect("grants");
    assert_eq!(core.plugin_location("com.example.location", &phone).await.unwrap(), Some(MADRID));
    assert_eq!(phone.asked(), 1);

    // Taken back, it is refused again.
    core.grant_plugin("com.example.location", Permissions::default()).await.unwrap();
    assert!(core.plugin_location("com.example.location", &phone).await.is_err());
    assert_eq!(phone.asked(), 1);
}

// The user or the phone said no, location is off, or there was no fix: the plugin gets nothing,
// never an error it could tell apart, and never a position that is not one.
#[tokio::test]
async fn no_fix_or_a_fix_that_is_no_place_is_nothing() {
    let (core, _dir) = core().await;
    let catalogue = Ed25519SecretKey::new();
    let package = signed("com.example.location", "1.0.0", r#"{"location":true}"#, &catalogue);
    core.install_plugin(&package, &catalogue.public_key(), Permissions { location: true, ..Permissions::default() }).await.expect("installs");

    assert_eq!(core.plugin_location("com.example.location", &FakeLocator::answering(None)).await.unwrap(), None);
    for nowhere in [
        ft_core::plugins::Fix { lat: 91.0, ..MADRID },
        ft_core::plugins::Fix { lon: -180.5, ..MADRID },
        ft_core::plugins::Fix { lat: f64::NAN, ..MADRID },
        ft_core::plugins::Fix { accuracy: -1.0, ..MADRID },
        ft_core::plugins::Fix { accuracy: f64::INFINITY, ..MADRID },
    ] {
        let phone = FakeLocator::answering(Some(nowhere));
        assert_eq!(core.plugin_location("com.example.location", &phone).await.unwrap(), None, "{nowhere:?}");
    }

    struct Broken;
    #[async_trait]
    impl ft_core::plugins::Locator for Broken {
        async fn locate(&self) -> anyhow::Result<Option<ft_core::plugins::Fix>> {
            anyhow::bail!("the location service is gone")
        }
    }
    assert_eq!(core.plugin_location("com.example.location", &Broken).await.unwrap(), None);
}

// 2026-10-02: `ft.location` is a new capability of the Plugin API, so it came with a new core
// (§51): the location plugin asks for 1.3.0, and that is what this core is.
#[tokio::test]
async fn the_location_plugin_installs_on_the_core_that_brought_location() {
    let (core, _dir) = core().await;
    let catalogue = Ed25519SecretKey::new();
    let manifest = r#"{"id":"com.flickertalk.location","name":"Location","version":"1.0.0","minCoreVersion":"1.3.0","components":["ft-location"],"permissions":{"location":true,"send":"propose"}}"#;
    let package = sign_package(&[("module.json".to_owned(), manifest.as_bytes().to_vec()), ("dist/index.js".to_owned(), b"".to_vec())], &catalogue);
    core.install_plugin(&package, &catalogue.public_key(), Permissions::default()).await.expect("installs on 1.3.0");
    assert!(!ft_plugins::version_at_least("1.2.2", "1.3.0"), "and not on the core before it");
}

// ---- Updates of downloaded plugins (2026-10-03) ----

// A swap the phone cut short (killed between the two renames) is put right when the core starts,
// before any plugin is listed or served.
#[tokio::test]
async fn a_swap_cut_short_is_put_right_when_the_core_starts() {
    let catalogue = Ed25519SecretKey::new();
    let dir = scratch("cut-short");
    let plugin = ft_plugins::open(&signed("com.example.code", "1.0.0", "{}", &catalogue), &catalogue.public_key()).unwrap();
    ft_plugins::install(&plugin, &dir).unwrap();
    std::fs::rename(dir.join("com.example.code"), dir.join("com.example.code~old")).unwrap();

    let core = Core::open(Store::open_in_memory().await.expect("store"), [4; 32], Arc::new(Offline)).await.expect("opens");
    core.set_plugins_dir(dir.clone());
    assert!(dir.join("com.example.code/dist/index.js").exists(), "the old version is back");
    assert!(!dir.join("com.example.code~old").exists());
}

/// What the core says the plugin was granted, and its version.
async fn grant_and_version(core: &Core, id: &str) -> (Permissions, String) {
    let plugin = core.plugins().await.unwrap().into_iter().find(|one| one.manifest.id == id).expect("installed");
    (plugin.granted, plugin.manifest.version)
}

// §53: an update keeps what the user granted and never adds to it. What the new version asks for
// beyond it stays off until the user turns it on.
#[tokio::test]
async fn an_update_keeps_the_grant_and_never_widens_it() {
    let (core, _dir) = core().await;
    let catalogue = Ed25519SecretKey::new();
    let asks = r#"{"network":["api.example.com"],"send":"propose","live":true}"#;
    core.install_plugin(&signed("com.example.code", "1.0.0", asks, &catalogue), &catalogue.public_key(), Permissions::default()).await.unwrap();
    let granted = Permissions { network: vec!["api.example.com".to_owned()], send: Sending::Propose, live: true, ..Permissions::default() };
    core.grant_plugin("com.example.code", granted.clone()).await.unwrap();

    let more = r#"{"network":["api.example.com","cdn.example.com"],"send":"auto","live":true,"remind":true,"location":true}"#;
    core.install_plugin(&signed("com.example.code", "1.0.1", more, &catalogue), &catalogue.public_key(), Permissions::default()).await.expect("updates");
    assert_eq!(grant_and_version(&core, "com.example.code").await, (granted, "1.0.1".to_owned()));
}

// A version that asks for less updates too (before, the old grant no longer fitted and the update
// was refused, silently for the seeds), and the grant shrinks to what it asks for now.
#[tokio::test]
async fn an_update_that_asks_for_less_still_updates_and_the_grant_shrinks_with_it() {
    let (core, _dir) = core().await;
    let catalogue = Ed25519SecretKey::new();
    let asks = r#"{"network":["api.example.com","cdn.example.com"],"send":"auto","remind":true,"storage":"large"}"#;
    core.install_plugin(&signed("com.example.notes", "1.0.0", asks, &catalogue), &catalogue.public_key(), Permissions::default()).await.unwrap();
    let all = Permissions {
        network: vec!["api.example.com".to_owned(), "cdn.example.com".to_owned()],
        send: Sending::Auto,
        remind: true,
        storage: ft_plugins::Storage::Large,
        ..Permissions::default()
    };
    core.grant_plugin("com.example.notes", all.clone()).await.unwrap();
    core.set_reminder("com.example.notes", None, "r1", 5_000, "milk").await.unwrap();
    let mut events = core.events();

    // As the seeds are updated: with the grant the plugin had.
    let less = r#"{"network":["api.example.com"],"send":"propose"}"#;
    core.install_plugin(&signed("com.example.notes", "1.0.1", less, &catalogue), &catalogue.public_key(), all).await.expect("updates");
    let narrowed = Permissions { network: vec!["api.example.com".to_owned()], send: Sending::Propose, ..Permissions::default() };
    assert_eq!(grant_and_version(&core, "com.example.notes").await, (narrowed, "1.0.1".to_owned()));
    // Without `remind`, what it had set no longer rings, and the alarm clock is told.
    assert!(core.plugin_reminders("com.example.notes", None).await.unwrap().is_empty());
    let mut heard = Vec::new();
    while let Ok(event) = events.try_recv() {
        heard.push(event);
    }
    assert!(heard.contains(&ft_core::Event::RemindersChanged), "{heard:?}");
    assert!(heard.contains(&ft_core::Event::PluginsChanged), "{heard:?}");
}

// What the plugin keeps on this phone outlives its update: records (saved games), memory,
// reminders, the opaque id of each chat, and when it was installed.
#[tokio::test]
async fn what_a_plugin_keeps_outlives_its_update() {
    let (core, _dir, bob, _carol) = two_plugins_and_two_contacts().await;
    let catalogue = Ed25519SecretKey::new();
    let id = "com.example.list";
    // Installed with another key in the helper: this one signs the version it updates to.
    let asks = r#"{"remind":true}"#;
    core.install_plugin(&signed(id, "1.0.0", asks, &catalogue), &catalogue.public_key(), Permissions::default()).await.unwrap();
    core.grant_plugin(id, Permissions { remind: true, ..Permissions::default() }).await.unwrap();
    let chat = core.plugin_chat(id, &bob).await.unwrap();
    core.plugin_record_set(id, None, &format!("game/{chat}/1"), b"e4 e5").await.unwrap();
    core.plugin_remember(id, None, "theme", "dark").await.unwrap();
    core.set_reminder(id, None, "r1", 5_000, "milk").await.unwrap();
    let installed_at = core.plugins().await.unwrap().into_iter().find(|one| one.manifest.id == id).unwrap().installed_at;

    core.install_plugin(&signed(id, "1.0.1", asks, &catalogue), &catalogue.public_key(), Permissions::default()).await.expect("updates");
    let now = core.plugins().await.unwrap().into_iter().find(|one| one.manifest.id == id).unwrap();
    assert_eq!((now.manifest.version.as_str(), now.installed_at), ("1.0.1", installed_at));
    assert_eq!(core.plugin_chat(id, &bob).await.unwrap(), chat);
    assert_eq!(core.plugin_record(id, None, &format!("game/{chat}/1")).await.unwrap().as_deref(), Some(&b"e4 e5"[..]));
    assert_eq!(core.plugin_remembers(id, None, "theme").await.unwrap().as_deref(), Some("dark"));
    assert_eq!(core.plugin_reminders(id, None).await.unwrap().len(), 1);
}

/// A package with any manifest, signed by the catalogue.
fn package_of(manifest: &str, script: &[u8], catalogue: &Ed25519SecretKey) -> Vec<u8> {
    sign_package(&[("module.json".to_owned(), manifest.as_bytes().to_vec()), ("dist/index.js".to_owned(), script.to_vec())], catalogue)
}

fn tool(id: &str, version: &str) -> String {
    format!(r#"{{"id":"{id}","name":"Code","version":"{version}","minCoreVersion":"0.1.0","components":["ft-code"]}}"#)
}

/// The catalogue listing these packages (each with its manifest), as `ftcatalogue` writes the
/// entries.
fn listing(packages: &[(&str, &[u8])]) -> (Shop, Vec<ft_plugins::CatalogueEntry>) {
    let mut files = std::collections::HashMap::new();
    let mut entries = Vec::new();
    for (manifest, package) in packages {
        let manifest: ft_plugins::Manifest = serde_json::from_str(manifest).expect("a manifest");
        let url = format!("{}/{}/{}.ftplugin", ft_core::CATALOGUE_HOME, manifest.id, manifest.version);
        entries.push(ft_plugins::CatalogueEntry {
            id: manifest.id,
            name: manifest.name,
            version: manifest.version,
            min_core_version: manifest.min_core_version,
            size: package.len() as u64,
            hash: blake3::hash(package).to_hex().to_string(),
            url: url.clone(),
            summary: String::new(),
            kind: manifest.kind,
        });
        files.insert(url, package.to_vec());
    }
    (Shop { files }, entries)
}

async fn installed_code(core: &Core, catalogue: &Ed25519SecretKey) {
    core.install_plugin(&package_of(&tool("com.example.code", "1.0.0"), b"old()", catalogue), &catalogue.public_key(), Permissions::default())
        .await
        .expect("installs");
}

fn script(dir: &std::path::Path) -> Vec<u8> {
    std::fs::read(dir.join("com.example.code/dist/index.js")).unwrap()
}

// 2026-10-03: a plugin downloaded from the catalogue is updated when the signed catalogue lists a
// higher version of it, as the seeds are with the app. The screens hear of it.
#[tokio::test]
async fn a_downloaded_plugin_is_updated_when_the_catalogue_lists_a_higher_version() {
    let (core, dir) = core().await;
    let catalogue = Ed25519SecretKey::new();
    installed_code(&core, &catalogue).await;
    let manifest = tool("com.example.code", "1.0.1");
    let newer = package_of(&manifest, b"new()", &catalogue);
    let (shop, listed) = listing(&[(&manifest, &newer)]);
    let mut events = core.events();

    let updated = core.update_plugins(&listed, &shop, &catalogue.public_key(), &Default::default()).await;
    assert_eq!(updated, ["com.example.code"]);
    assert_eq!(script(&dir), b"new()");
    assert_eq!(grant_and_version(&core, "com.example.code").await.1, "1.0.1");
    assert_eq!(events.try_recv().ok(), Some(ft_core::Event::PluginsChanged));
}

// The version decides, not the bytes: the same or a lower one changes nothing, even repackaged.
#[tokio::test]
async fn the_same_or_a_lower_version_changes_nothing() {
    let (core, dir) = core().await;
    let catalogue = Ed25519SecretKey::new();
    installed_code(&core, &catalogue).await;
    for version in ["1.0.0", "0.9.9"] {
        let manifest = tool("com.example.code", version);
        let other = package_of(&manifest, b"other()", &catalogue);
        let (shop, listed) = listing(&[(&manifest, &other)]);
        assert!(core.update_plugins(&listed, &shop, &catalogue.public_key(), &Default::default()).await.is_empty(), "{version}");
        assert_eq!(script(&dir), b"old()");
    }
}

// §53: only what the user installed is updated; what the catalogue lists besides is only offered.
#[tokio::test]
async fn an_update_never_installs_a_plugin_the_user_did_not_install() {
    let (core, _dir) = core().await;
    let catalogue = Ed25519SecretKey::new();
    installed_code(&core, &catalogue).await;
    let manifest = tool("com.example.other", "2.0.0");
    let stranger = package_of(&manifest, b"x()", &catalogue);
    let (shop, listed) = listing(&[(&manifest, &stranger)]);
    assert!(core.update_plugins(&listed, &shop, &catalogue.public_key(), &Default::default()).await.is_empty());
    assert_eq!(core.plugins().await.unwrap().iter().map(|one| one.manifest.id.as_str()).collect::<Vec<_>>(), ["com.example.code"]);
}

// Not when the new version needs a newer FlickerTalk (§51), turns a tool into a game, or weighs
// more than a package may.
#[tokio::test]
async fn an_update_that_needs_a_newer_core_changes_kind_or_is_too_big_is_skipped() {
    let (core, dir) = core().await;
    let catalogue = Ed25519SecretKey::new();
    installed_code(&core, &catalogue).await;
    let future = tool("com.example.code", "1.0.1").replace("0.1.0", "99.0.0");
    let game = r#"{"id":"com.example.code","name":"Code","version":"1.0.2","minCoreVersion":"1.3.0","components":["ft-code"],"kind":"game"}"#.to_owned();
    let big = tool("com.example.code", "1.0.3");
    for (manifest, size) in [(&future, None), (&game, None), (&big, Some(9 * 1024 * 1024))] {
        let package = package_of(manifest, b"new()", &catalogue);
        let (shop, mut listed) = listing(&[(manifest, &package)]);
        if let Some(size) = size {
            listed[0].size = size;
        }
        let version = listed[0].version.clone();
        assert!(core.update_plugins(&listed, &shop, &catalogue.public_key(), &Default::default()).await.is_empty(), "{version}");
        assert_eq!(script(&dir), b"old()", "{version}");
    }
}

// A download that fails, or bytes that are not the ones the index listed, leave the installed
// version as it was.
#[tokio::test]
async fn a_failed_download_or_other_bytes_leave_the_installed_version() {
    let (core, dir) = core().await;
    let catalogue = Ed25519SecretKey::new();
    installed_code(&core, &catalogue).await;
    let manifest = tool("com.example.code", "1.0.1");
    let newer = package_of(&manifest, b"new()", &catalogue);
    let (mut shop, listed) = listing(&[(&manifest, &newer)]);
    shop.files.clear();
    assert!(core.update_plugins(&listed, &shop, &catalogue.public_key(), &Default::default()).await.is_empty(), "offline");
    let (mut shop, listed) = listing(&[(&manifest, &newer)]);
    let swapped = package_of(&tool("com.example.code", "1.0.1"), b"evil()", &catalogue);
    shop.files.insert(listed[0].url.clone(), swapped);
    assert!(core.update_plugins(&listed, &shop, &catalogue.public_key(), &Default::default()).await.is_empty(), "other bytes");
    assert_eq!(script(&dir), b"old()");
    assert_eq!(grant_and_version(&core, "com.example.code").await.1, "1.0.0");
}

// A plugin open on the screen (or still saying goodbye) does not change under it: it is updated
// on a later pass, once it is closed.
#[tokio::test]
async fn an_open_plugin_waits_for_the_next_pass() {
    let (core, dir) = core().await;
    let catalogue = Ed25519SecretKey::new();
    installed_code(&core, &catalogue).await;
    let manifest = tool("com.example.code", "1.0.1");
    let newer = package_of(&manifest, b"new()", &catalogue);
    let (shop, listed) = listing(&[(&manifest, &newer)]);
    let open: std::collections::HashSet<String> = ["com.example.code".to_owned()].into();
    assert!(core.update_plugins(&listed, &shop, &catalogue.public_key(), &open).await.is_empty());
    assert_eq!(script(&dir), b"old()");
    assert_eq!(core.update_plugins(&listed, &shop, &catalogue.public_key(), &Default::default()).await, ["com.example.code"]);
    assert_eq!(script(&dir), b"new()");
}
