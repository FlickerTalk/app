//! Two complete devices talk through an in-memory network (Plan §18–19, §26–27, §106 M1): pairing
//! by Contact Card, end-to-end encrypted text, receipts, the mailbox when there is no direct
//! connection, and the outbox when the mailbox is off.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use async_trait::async_trait;
use ft_core::{Core, Peer, Transport};
use ft_storage::{MessageState, Store};
use tokio::sync::mpsc;

/// The network between the test devices: a direct link that can be cut, and mailboxes. Like a
/// DataChannel, the direct link delivers in order, through one queue per device.
#[derive(Default)]
struct Net {
    direct: AtomicBool,
    cores: Mutex<HashMap<String, Weak<Core>>>,
    queues: Mutex<HashMap<String, mpsc::UnboundedSender<Vec<u8>>>>,
    /// Devices the direct link cannot reach, one way.
    unreachable: Mutex<HashSet<String>>,
    mailboxes: Mutex<HashMap<String, Vec<Vec<u8>>>>,
    /// Every packet handed to the direct link, to replay duplicates.
    sent: Mutex<Vec<(String, Vec<u8>)>>,
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

    fn mailbox_len(&self, device: &str) -> usize {
        self.mailboxes.lock().unwrap().get(device).map_or(0, Vec::len)
    }

    /// The device collects its mailbox (what a push would trigger).
    async fn collect(&self, core: &Core) {
        let blobs = self.mailboxes.lock().unwrap().remove(core.device_id().as_str()).unwrap_or_default();
        for blob in blobs {
            let _ = core.receive(&blob).await;
        }
    }
}

struct Link {
    net: Arc<Net>,
}

#[async_trait]
impl Transport for Link {
    async fn send_direct(&self, to: &Peer, bytes: Vec<u8>) -> anyhow::Result<bool> {
        if !self.net.direct.load(Ordering::SeqCst) || self.net.unreachable.lock().unwrap().contains(&to.device_id) {
            return Ok(false);
        }
        let Some(queue) = self.net.queues.lock().unwrap().get(&to.device_id).cloned() else {
            return Ok(false);
        };
        self.net.sent.lock().unwrap().push((to.device_id.clone(), bytes.clone()));
        Ok(queue.send(bytes).is_ok())
    }

    async fn send_mailbox(&self, to: &Peer, bytes: Vec<u8>) -> anyhow::Result<()> {
        self.net.mailboxes.lock().unwrap().entry(to.device_id.clone()).or_default().push(bytes);
        Ok(())
    }
}

async fn device(net: &Arc<Net>, name: &str) -> Arc<Core> {
    device_with(net, name, Store::open_in_memory().await.expect("store"), [9; 32]).await
}

async fn device_with(net: &Arc<Net>, name: &str, store: Store, key: [u8; 32]) -> Arc<Core> {
    let core = Core::open(store, key, Arc::new(Link { net: net.clone() })).await.expect("opens");
    core.set_name(name).await.expect("names");
    let id = core.device_id().as_str().to_owned();
    net.cores.lock().unwrap().insert(id.clone(), Arc::downgrade(&core));

    // Delivered asynchronously, like a real network: no re-entrant calls into the sender.
    let (queue, mut incoming) = mpsc::unbounded_channel::<Vec<u8>>();
    net.queues.lock().unwrap().insert(id, queue);
    let receiver = Arc::downgrade(&core);
    tokio::spawn(async move {
        while let Some(bytes) = incoming.recv().await {
            let Some(core) = receiver.upgrade() else { break };
            let _ = core.receive(&bytes).await;
        }
    });
    core
}

/// Polls until the condition holds, for at most five seconds.
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

async fn pair(alice: &Core, bob: &Core) {
    let link = bob.my_card().await.expect("card").to_link();
    alice.add_contact(&link, None).await.expect("alice adds bob");
    let alice_id = alice.device_id().as_str().to_owned();
    until("bob knows alice", || async { bob.store().contact(&alice_id).await.unwrap().is_some() }).await;
}

async fn state_of(core: &Core, contact: &str, message_id: &str) -> MessageState {
    let messages = core.store().messages(contact, 100).await.expect("lists");
    messages.into_iter().find(|m| m.message_id == message_id).expect("message exists").state
}

async fn texts(core: &Core, contact: &str) -> Vec<String> {
    core.store().messages(contact, 100).await.expect("lists").into_iter().map(|m| m.body).collect()
}

fn id(core: &Core) -> String {
    core.device_id().as_str().to_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn scanning_a_card_pairs_both_devices() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;

    assert_eq!(alice.store().contact(&id(&bob)).await.unwrap().expect("bob").name, "Bob");
    assert_eq!(bob.store().contact(&id(&alice)).await.unwrap().expect("alice").name, "Alice");
}

// §29: the safety number both users compare in person.
#[tokio::test(flavor = "multi_thread")]
async fn both_sides_show_the_same_fingerprint() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    let at_alice = alice.fingerprint(&id(&bob)).await.expect("fingerprint");
    assert_eq!(at_alice, bob.fingerprint(&id(&alice)).await.expect("fingerprint"));
    assert_eq!(at_alice.split(' ').count(), 12);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_text_is_delivered_and_acknowledged() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;

    let message = alice.send_text(&id(&bob), "hello bob").await.expect("sends");

    until("bob has the text", || async { texts(&bob, &id(&alice)).await == ["hello bob"] }).await;
    until("alice sees it delivered", || async { state_of(&alice, &id(&bob), &message).await == MessageState::Delivered })
        .await;
    assert!(alice.store().outbox().await.unwrap().is_empty(), "delivered messages leave the outbox");
}

#[tokio::test(flavor = "multi_thread")]
async fn both_sides_can_write() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;

    alice.send_text(&id(&bob), "ping").await.expect("sends");
    until("bob got ping", || async { texts(&bob, &id(&alice)).await.len() == 1 }).await;
    bob.send_text(&id(&alice), "pong").await.expect("sends");
    until("alice got pong", || async { texts(&alice, &id(&bob)).await == ["ping", "pong"] }).await;
}

// §19: no direct connection, both use the mailbox → it goes encrypted to Bob's mailbox.
#[tokio::test(flavor = "multi_thread")]
async fn without_a_direct_connection_the_mailbox_carries_it() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    net.set_direct(false);

    let message = alice.send_text(&id(&bob), "are you there?").await.expect("sends");
    assert_eq!(net.mailbox_len(&id(&bob)), 1);
    assert_eq!(state_of(&alice, &id(&bob), &message).await, MessageState::Sent);

    net.collect(&bob).await;
    assert_eq!(texts(&bob, &id(&alice)).await, ["are you there?"]);

    // The receipt comes back through Alice's mailbox too.
    until("the receipt is in alice's mailbox", || async { net.mailbox_len(&id(&alice)) == 1 }).await;
    net.collect(&alice).await;
    assert_eq!(state_of(&alice, &id(&bob), &message).await, MessageState::Delivered);
}

// §19: a message only goes to the mailbox if both use it; otherwise it waits on the phone.
#[tokio::test(flavor = "multi_thread")]
async fn without_the_mailbox_the_message_waits_on_the_phone() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    bob.set_mailbox(false).await.expect("bob turns the mailbox off");
    pair(&alice, &bob).await;
    net.set_direct(false);

    let message = alice.send_text(&id(&bob), "later").await.expect("sends");
    assert_eq!(net.mailbox_len(&id(&bob)), 0, "nothing of bob's is stored on the server");
    assert_eq!(state_of(&alice, &id(&bob), &message).await, MessageState::Pending);
    assert_eq!(alice.store().outbox().await.unwrap().len(), 1);

    net.set_direct(true);
    alice.retry_now().await.expect("retries");
    until("delivered once bob is reachable", || async {
        state_of(&alice, &id(&bob), &message).await == MessageState::Delivered
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn turning_the_mailbox_off_is_told_to_contacts() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    bob.set_mailbox(false).await.expect("bob turns it off");
    until("alice learns it", || async { !alice.store().contact(&id(&bob)).await.unwrap().unwrap().mailbox }).await;
}

// §27: when the receipt is lost, the sender retries; the receiver stores the message once and
// acknowledges it again.
#[tokio::test(flavor = "multi_thread")]
async fn a_retried_message_is_stored_once_and_acknowledged_again() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    // Bob's receipts cannot reach Alice directly: they wait in her mailbox.
    net.unreachable.lock().unwrap().insert(id(&alice));

    let message = alice.send_text(&id(&bob), "once").await.expect("sends");
    until("bob got it", || async { texts(&bob, &id(&alice)).await.len() == 1 }).await;
    until("the first receipt waits", || async { net.mailbox_len(&id(&alice)) == 1 }).await;

    alice.retry_now().await.expect("retries");
    until("the second receipt waits", || async { net.mailbox_len(&id(&alice)) == 2 }).await;
    assert_eq!(texts(&bob, &id(&alice)).await, ["once"], "stored once");

    net.collect(&alice).await;
    assert_eq!(state_of(&alice, &id(&bob), &message).await, MessageState::Delivered);
}

// Replaying the very same bytes is not a retry: Olm refuses to decrypt a message twice.
#[tokio::test(flavor = "multi_thread")]
async fn a_replayed_packet_is_rejected() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    alice.send_text(&id(&bob), "once").await.expect("sends");
    until("bob got it", || async { texts(&bob, &id(&alice)).await.len() == 1 }).await;

    let last_to_bob = net.sent.lock().unwrap().iter().rev().find(|(to, _)| *to == id(&bob)).map(|(_, b)| b.clone());
    assert!(bob.receive(&last_to_bob.expect("a packet reached bob")).await.is_err());
    assert_eq!(texts(&bob, &id(&alice)).await, ["once"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn reading_a_conversation_sends_read_receipts() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    let message = alice.send_text(&id(&bob), "read me").await.expect("sends");
    until("bob got it", || async { texts(&bob, &id(&alice)).await.len() == 1 }).await;

    bob.mark_read(&id(&alice)).await.expect("reads");
    assert!(bob.store().unread(&id(&alice)).await.unwrap().is_empty());
    until("alice sees it read", || async { state_of(&alice, &id(&bob), &message).await == MessageState::Read }).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn blocked_contacts_are_ignored() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    bob.block(&id(&alice), true).await.expect("blocks");

    let message = alice.send_text(&id(&bob), "let me in").await.expect("sends");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(texts(&bob, &id(&alice)).await.is_empty());
    assert_ne!(state_of(&alice, &id(&bob), &message).await, MessageState::Delivered);
}

#[tokio::test(flavor = "multi_thread")]
async fn garbage_from_strangers_is_rejected() {
    let net = Net::new();
    let bob = device(&net, "Bob").await;
    assert!(bob.receive(b"not a packet").await.is_err());
    assert!(bob.store().contacts().await.unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_conversation_survives_a_restart() {
    let net = Net::new();
    let path = std::env::temp_dir().join(format!("ft-core-restart-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let bob = device(&net, "Bob").await;
    {
        let alice = device_with(&net, "Alice", Store::open(&path).await.unwrap(), [1; 32]).await;
        pair(&alice, &bob).await;
        alice.send_text(&id(&bob), "before").await.expect("sends");
        until("bob got it", || async { texts(&bob, &id(&alice)).await.len() == 1 }).await;
    }

    let alice = device_with(&net, "Alice", Store::open(&path).await.unwrap(), [1; 32]).await;
    alice.send_text(&id(&bob), "after").await.expect("sends after restarting");
    until("bob got both", || async { texts(&bob, &id(&alice)).await == ["before", "after"] }).await;
    let _ = std::fs::remove_file(&path);
}
