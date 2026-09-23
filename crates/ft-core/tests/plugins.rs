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
    let granted = Permissions { network: vec!["api.openai.com".to_owned()], reads_given_messages: true, send: Sending::Nothing, print: false };
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

    assert_eq!(core.plugin_remembers("com.example.code", "pen").await.unwrap(), None);
    core.plugin_remember("com.example.code", "pen", "black").await.expect("remembers");
    core.plugin_remember("com.example.ai", "pen", "blue").await.expect("remembers");
    assert_eq!(core.plugin_remembers("com.example.code", "pen").await.unwrap().as_deref(), Some("black"));
    assert_eq!(core.plugin_memory_keys("com.example.code").await.unwrap(), ["pen"]);

    core.plugin_forget("com.example.code", "pen").await.expect("forgets");
    assert_eq!(core.plugin_remembers("com.example.code", "pen").await.unwrap(), None);
    assert_eq!(core.plugin_remembers("com.example.ai", "pen").await.unwrap().as_deref(), Some("blue"));
}

#[tokio::test]
async fn a_plugin_cannot_fill_the_phone_nor_write_for_another() {
    let (core, _dir) = core().await;
    let catalogue = Ed25519SecretKey::new();
    let package = signed("com.example.code", "1.0.0", "{}", &catalogue);
    core.install_plugin(&package, &catalogue.public_key(), Permissions::default()).await.expect("installs");

    assert!(core.plugin_remember("com.example.code", "big", &"x".repeat(70_000)).await.is_err(), "too much");
    assert!(core.plugin_remember("com.example.code", &"k".repeat(200), "v").await.is_err(), "too long a key");
    assert!(core.plugin_remember("com.example.never", "pen", "black").await.is_err(), "it is not installed");

    for number in 0..64 {
        core.plugin_remember("com.example.code", &format!("key{number}"), "v").await.expect("remembers");
    }
    assert!(core.plugin_remember("com.example.code", "one-more", "v").await.is_err(), "too many keys");
    // What it already remembers it can still change.
    core.plugin_remember("com.example.code", "key0", "w").await.expect("remembers");
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
    files.insert(format!("{}/index.json", ft_core::CATALOGUE_HOME), index.clone().into_bytes());
    files.insert(
        format!("{}/index.json.sig", ft_core::CATALOGUE_HOME),
        catalogue.sign(index.as_bytes()).to_base64().into_bytes(),
    );
    files.insert(url.clone(), package.to_vec());
    (Shop { files }, url)
}

// §56: nothing travels inside the app. The phone reads a signed index, and only then downloads a
// package, which has to be exactly the bytes the index listed.
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
