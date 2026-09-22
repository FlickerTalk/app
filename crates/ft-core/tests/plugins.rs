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
    let granted = Permissions { network: vec!["api.openai.com".to_owned()], reads_given_messages: true, send: Sending::Nothing };
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
