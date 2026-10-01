//! End-to-end tests of what a plugin does across two phones (2026-09-27): the live channel
//! (`ft.live`) and the refs a plugin is handed. Two complete devices talk through an in-memory
//! network, as in `hardening.rs`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use async_trait::async_trait;
use ft_core::{Core, Event, Peer, Transport};
use ft_plugins::{sign_package, Permissions};
use vodozemac::Ed25519SecretKey;
use ft_storage::Store;
use tokio::sync::mpsc;

/// A direct link that can be cut, and one mailbox per device, as the router keeps it.
#[derive(Default)]
struct Net {
    direct: AtomicBool,
    queues: Mutex<HashMap<String, mpsc::UnboundedSender<Vec<u8>>>>,
    mailboxes: Mutex<HashMap<String, Vec<Vec<u8>>>>,
}

impl Net {
    fn new() -> Arc<Self> {
        let net = Arc::new(Self::default());
        net.direct.store(true, Ordering::SeqCst);
        net
    }

    fn set_direct(&self, up: bool) {
        self.direct.store(up, Ordering::SeqCst);
    }

    /// What the router would hold for a device right now.
    fn mailbox(&self, device: &str) -> Vec<Vec<u8>> {
        self.mailboxes.lock().unwrap().get(device).cloned().unwrap_or_default()
    }

}

struct Link {
    net: Arc<Net>,
}

#[async_trait]
impl Transport for Link {
    async fn send_direct(&self, to: &Peer, bytes: Vec<u8>) -> anyhow::Result<bool> {
        if !self.net.direct.load(Ordering::SeqCst) {
            return Ok(false);
        }
        let Some(queue) = self.net.queues.lock().unwrap().get(&to.device_id).cloned() else { return Ok(false) };
        Ok(queue.send(bytes).is_ok())
    }

    async fn send_mailbox(&self, to: &Peer, bytes: Vec<u8>) -> anyhow::Result<()> {
        self.net.mailboxes.lock().unwrap().entry(to.device_id.clone()).or_default().push(bytes);
        Ok(())
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ft-hardening-{name}-{}", ft_protocol::MessageId::new()));
    std::fs::create_dir_all(&dir).expect("creates the directory");
    dir
}

async fn device(net: &Arc<Net>, name: &str) -> Arc<Core> {
    let core = Core::open(Store::open_in_memory().await.expect("store"), [9; 32], Arc::new(Link { net: net.clone() }))
        .await
        .expect("opens");
    core.set_name(name).await.expect("names");
    core.set_files_dir(scratch(name));
    core.set_plugins_dir(scratch(&format!("{name}-plugins")));
    let id = core.device_id().as_str().to_owned();
    let (queue, mut incoming) = mpsc::unbounded_channel::<Vec<u8>>();
    net.queues.lock().unwrap().insert(id, queue);
    let receiver: Weak<Core> = Arc::downgrade(&core);
    tokio::spawn(async move {
        while let Some(bytes) = incoming.recv().await {
            let Some(core) = receiver.upgrade() else { break };
            let _ = core.receive(&bytes).await;
        }
    });
    core
}

async fn until<F, Fut>(what: &str, condition: F)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..250 {
        if condition().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out waiting until {what}");
}

fn id(core: &Core) -> String {
    core.device_id().as_str().to_owned()
}

/// Alice scans Bob; pairing is over when Bob's card is back.
async fn pair(alice: &Core, bob: &Core) {
    let link = bob.my_card().await.expect("card").to_link();
    alice.add_contact(&link, None).await.expect("alice adds bob");
    let (alice_id, bob_id) = (id(alice), id(bob));
    until("bob knows alice", || async { bob.store().contact(&alice_id).await.unwrap().is_some() }).await;
    // Alice wrote first: she waits in Bob's requests until he says yes (A5). He does.
    bob.accept_contact(&alice_id).await.expect("bob accepts alice");
    until("bob's card came back", || async {
        alice.store().contact(&bob_id).await.unwrap().is_some_and(|contact| contact.introduced)
    })
    .await;
}


/// Bob's phone, with a plugin installed and granted what `permissions` says.
async fn with_plugin(core: &Core, permissions: &str, granted: Permissions) -> Ed25519SecretKey {
    let catalogue = Ed25519SecretKey::new();
    let manifest = format!(
        r#"{{"id":"com.example.board","name":"Board","version":"1.0.0","minCoreVersion":"0.1.0","components":["ft-board"],"permissions":{permissions}}}"#
    );
    let package = sign_package(
        &[("module.json".to_owned(), manifest.into_bytes()), ("dist/index.js".to_owned(), b"".to_vec())],
        &catalogue,
    );
    core.install_plugin(&package, &catalogue.public_key(), granted).await.expect("installs");
    catalogue
}

async fn heard(events: &mut tokio::sync::broadcast::Receiver<Event>, plugin: &str) -> Option<(String, Vec<u8>)> {
    for _ in 0..100 {
        while let Ok(event) = events.try_recv() {
            if let Event::PluginEvent { plugin: id, contact, data } = event {
                if id == plugin {
                    return Some((contact, data));
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    None
}

// ---------------------------------------------------------------------------------------------
// ft.live: what a plugin says reaches its twin, only directly, only if both were granted it.
// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_plugin_talks_to_its_twin_on_the_other_phone() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    let granted = Permissions { live: true, ..Permissions::default() };
    with_plugin(&alice, r#"{"live":true}"#, granted.clone()).await;
    with_plugin(&bob, r#"{"live":true}"#, granted).await;
    let mut at_bob = bob.events();

    assert!(alice.plugin_live_send("com.example.board", &id(&bob), b"stroke 1".to_vec()).await.expect("sends"));
    let (from, data) = heard(&mut at_bob, "com.example.board").await.expect("bob's plugin hears it");
    assert_eq!(from, id(&alice));
    assert_eq!(data, b"stroke 1");
    // Nothing of it is kept: it is not a message.
    assert!(bob.store().messages(&id(&alice), 10).await.unwrap().is_empty());

    // Without a direct connection it does not go, and it never goes through the mailbox.
    net.set_direct(false);
    assert!(!alice.plugin_live_send("com.example.board", &id(&bob), b"stroke 2".to_vec()).await.expect("tries"));
    assert!(net.mailbox(&id(&bob)).is_empty(), "the mailbox is never used for this");
    assert!(alice.plugin_live_send("com.example.board", &id(&bob), vec![0; ft_core::plugins::LIVE_LIMIT + 1]).await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_channel_needs_the_grant_on_both_sides() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    with_plugin(&alice, r#"{"live":true}"#, Permissions::default()).await;
    assert!(alice.plugin_live_send("com.example.board", &id(&bob), b"x".to_vec()).await.is_err(), "alice did not grant it");

    alice.grant_plugin("com.example.board", Permissions { live: true, ..Permissions::default() }).await.unwrap();
    with_plugin(&bob, r#"{"live":true}"#, Permissions::default()).await;
    let mut at_bob = bob.events();
    assert!(alice.plugin_live_send("com.example.board", &id(&bob), b"x".to_vec()).await.expect("sends"));
    assert!(heard(&mut at_bob, "com.example.board").await.is_none(), "bob did not grant it: dropped");

    bob.grant_plugin("com.example.board", Permissions { live: true, ..Permissions::default() }).await.unwrap();
    assert!(alice.plugin_live_send("com.example.board", &id(&bob), b"y".to_vec()).await.expect("sends"));
    assert_eq!(heard(&mut at_bob, "com.example.board").await.map(|(_, data)| data), Some(b"y".to_vec()));

    // A stranger's plugin says nothing to ours.
    let carol = device(&net, "Carol").await;
    with_plugin(&carol, r#"{"live":true}"#, Permissions { live: true, ..Permissions::default() }).await;
    let link = bob.my_card().await.expect("card").to_link();
    carol.add_contact(&link, None).await.expect("carol adds bob");
    until("bob has carol's request", || async { bob.store().contact(&id(&carol)).await.unwrap().is_some() }).await;
    assert!(carol.plugin_live_send("com.example.board", &id(&bob), b"z".to_vec()).await.expect("carol sends"));
    assert!(heard(&mut at_bob, "com.example.board").await.is_none(), "not accepted: dropped");
}

// ---------------------------------------------------------------------------------------------
// Refs: a way back to the message, and nothing about the contact.
// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_ref_leads_back_to_the_message_and_only_for_its_plugin() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    with_plugin(&bob, "{}", Permissions::default()).await;
    alice.send_text(&id(&bob), "buy milk").await.expect("sends");
    until("bob has it", || async { !bob.store().messages(&id(&alice), 10).await.unwrap().is_empty() }).await;
    let message = bob.store().messages(&id(&alice), 10).await.unwrap()[0].message_id.clone();

    let reference = bob.plugin_ref("com.example.board", &message).await.expect("a ref");
    assert!(reference.starts_with("ref_") && !reference.contains(&id(&alice)), "nothing of the contact in it");
    assert_eq!(bob.plugin_ref("com.example.board", &message).await.unwrap(), reference, "the same message, the same ref");
    let target = bob.plugin_ref_target("com.example.board", None, &reference).await.unwrap().expect("leads somewhere");
    assert_eq!((target.contact.as_str(), target.message_id.as_str()), (id(&alice).as_str(), message.as_str()));
    assert!(bob.plugin_ref_target("com.example.other", None, &reference).await.unwrap().is_none(), "another plugin's ref");
    assert!(bob.plugin_ref_target("com.example.board", None, "ref_nothing").await.unwrap().is_none());
    assert!(bob.plugin_ref("com.example.board", "no-such-message").await.is_err());

    bob.forget_message(&message).await.expect("erases");
    assert!(bob.plugin_ref_target("com.example.board", None, &reference).await.unwrap().is_none(), "the message went");
}
