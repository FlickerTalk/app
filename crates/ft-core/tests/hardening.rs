//! End-to-end tests of the hardening pack (informe 2026-09-24, findings A1–A5, M2, M5, M6, M9, B6):
//! two complete devices talk through an in-memory network, and each test states one promise the
//! app makes to the user. They were written before the code that makes them pass.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use async_trait::async_trait;
use ft_core::{Core, Peer, Transport};
use ft_plugins::{sign_package, Permissions, Sending};
use ft_storage::{MessageState, Store};
use tokio::sync::mpsc;
use vodozemac::Ed25519SecretKey;

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

async fn texts(core: &Core, contact: &str) -> Vec<String> {
    core.store().messages(contact, 100).await.expect("lists").into_iter().map(|m| m.body).collect()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}

fn some_file(name: &str, size: usize) -> PathBuf {
    let bytes: Vec<u8> = (0..size).map(|i| (i * 7 + i / 251) as u8).collect();
    let path = scratch("outgoing").join(name);
    std::fs::write(&path, &bytes).expect("writes");
    path
}

// ---------------------------------------------------------------------------------------------
// A1 · Sealed sender: what goes through the router names nobody.
// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn the_mailbox_blob_names_no_sender() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    net.set_direct(false);

    alice.send_text(&id(&bob), "hello from the dark").await.expect("sends");
    until("the mail is in the box", || async { !net.mailbox(&id(&bob)).is_empty() }).await;

    let alice_id = id(&alice);
    let alice_key = alice.exchange_key_bytes().await;
    let alice_key_b64 = alice.exchange_key_base64().await;
    for blob in net.mailbox(&id(&bob)) {
        assert!(!contains(&blob, alice_id.as_bytes()), "the device id of the sender is in the clear");
        assert!(!contains(&blob, &alice_key), "the Olm identity key of the sender is in the clear");
        assert!(!contains(&blob, alice_key_b64.as_bytes()), "the Olm identity key (base64) is in the clear");
        assert!(!contains(&blob, b"hello"), "the text is in the clear");
    }

    // Bob still reads it: the envelope is his to open.
    net.collect(&bob).await;
    until("bob got it", || async { texts(&bob, &id(&alice)).await == ["hello from the dark"] }).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_signal_names_no_sender_either() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;

    let sealed = alice.seal_signal(&id(&bob), ft_protocol::Body::Answer { sdp: "v=0".to_owned() }).await.expect("seals");
    let signal = ft_protocol::Signal {
        version: ft_protocol::PROTOCOL_VERSION,
        kind: ft_protocol::SignalKind::Answer,
        session: "s1".to_owned(),
        from: id(&alice),
        to: id(&bob),
        sealed,
    };
    let wrapped = alice.wrap_for(&id(&bob), signal.encode()).await.expect("wraps");
    assert!(!contains(&wrapped, id(&alice).as_bytes()), "the signal names its sender to the router");
    assert!(!contains(&wrapped, b"v=0"));

    let opened = bob.unwrap(&wrapped).await.expect("bob opens the envelope");
    assert_eq!(ft_protocol::Signal::decode(&opened).expect("a signal").from, id(&alice));
    assert!(alice.unwrap(&wrapped).await.is_err(), "nobody but the recipient opens it");
}

// A contact from an older version has no envelope key in its card: it still gets its mail.
#[tokio::test(flavor = "multi_thread")]
async fn a_contact_without_an_envelope_key_still_gets_mail() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    // Bob's card as an old app would send it: without the envelope key.
    let old_card = bob.my_card_without_envelope().await.expect("card").encode();
    alice.store().refresh_card(&id(&bob), &old_card).await.expect("stores the old card");
    net.set_direct(false);

    alice.send_text(&id(&bob), "still readable").await.expect("sends");
    until("the mail is in the box", || async { !net.mailbox(&id(&bob)).is_empty() }).await;
    net.collect(&bob).await;
    until("bob got it", || async { texts(&bob, &id(&alice)).await == ["still readable"] }).await;
}

// An install from before the envelope key gets one on the first start, and every contact gets
// the new card: from then on their mail names nobody either.
#[tokio::test(flavor = "multi_thread")]
async fn an_upgraded_phone_hands_its_contacts_the_card_with_the_envelope_key() {
    let net = Net::new();
    let dir = scratch("upgrade");
    let store = Store::open(&dir.join("alice.db")).await.expect("store on disk");
    let alice = Core::open(store, [9; 32], Arc::new(Link { net: net.clone() })).await.expect("opens");
    alice.set_name("Alice").await.unwrap();
    alice.set_files_dir(dir.clone());
    let alice_id = id(&alice);
    // The link delivers to whichever instance of Alice holds the queue now.
    let listen = |core: &Arc<Core>| {
        let (queue, mut incoming) = mpsc::unbounded_channel::<Vec<u8>>();
        net.queues.lock().unwrap().insert(alice_id.clone(), queue);
        let receiver: Weak<Core> = Arc::downgrade(core);
        tokio::spawn(async move {
            while let Some(bytes) = incoming.recv().await {
                let Some(core) = receiver.upgrade() else { break };
                let _ = core.receive(&bytes).await;
            }
        });
    };
    listen(&alice);
    let bob = device(&net, "Bob").await;
    pair(&alice, &bob).await;

    // As an old app left it: no envelope key in the database, none in the card Bob holds.
    bob.store().refresh_card(&alice_id, &alice.my_card_without_envelope().await.unwrap().encode()).await.unwrap();
    alice.store().forget_envelope().await.expect("as before the upgrade");
    alice.store().close().await;
    drop(alice);

    // The upgraded app starts: a new envelope key, and the contacts are told.
    let store = Store::open(&dir.join("alice.db")).await.expect("reopens");
    let alice = Core::open(store, [9; 32], Arc::new(Link { net: net.clone() })).await.expect("opens again");
    alice.set_files_dir(dir.clone());
    listen(&alice);
    assert!(alice.card_stale().await.unwrap(), "the contacts hold an old card");
    alice.reintroduce().await.expect("tells them");
    assert!(!alice.card_stale().await.unwrap());
    until("bob has alice's envelope key", || async {
        let stored = bob.store().contact(&alice_id).await.unwrap().unwrap();
        ft_contacts::ContactCard::decode(&stored.card).unwrap().envelope_key().is_some()
    })
    .await;
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------------------------
// M9 · Padding: a short and a long message leave the phone the same size.
// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn short_and_long_texts_weigh_the_same_on_the_wire() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    net.set_direct(false);

    // Two texts that fit the same bucket: "ok" and a sentence. The router sees the same size.
    alice.send_text(&id(&bob), "ok").await.expect("sends");
    alice.send_text(&id(&bob), "te quiero mucho, de verdad").await.expect("sends");
    until("two blobs", || async { net.mailbox(&id(&bob)).len() == 2 }).await;
    let sizes: Vec<usize> = net.mailbox(&id(&bob)).iter().map(Vec::len).collect();
    assert_eq!(sizes[0], sizes[1], "the sizes tell the messages apart: {sizes:?}");
}

// ---------------------------------------------------------------------------------------------
// A2 · A plugin writes in the chat only as far as the user allowed.
// ---------------------------------------------------------------------------------------------

fn plugin_package(id: &str, permissions: &str, catalogue: &Ed25519SecretKey) -> Vec<u8> {
    let manifest = format!(
        r#"{{"id":"{id}","name":"Tool","version":"1.0.0","minCoreVersion":"0.1.0","components":["ft-tool"],"permissions":{permissions}}}"#
    );
    sign_package(&[("module.json".to_owned(), manifest.into_bytes()), ("dist/index.js".to_owned(), b"".to_vec())], catalogue)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plugin_sends_by_itself_only_with_the_auto_permission() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    let catalogue = Ed25519SecretKey::new();
    let package = plugin_package("com.example.tool", r#"{"send":"auto"}"#, &catalogue);
    alice.install_plugin(&package, &catalogue.public_key(), Permissions::default()).await.expect("installs");
    let made = some_file("made.txt", 100);

    // Installed is not granted (§53): it may neither send nor propose.
    assert_eq!(alice.plugin_sending("com.example.tool").await.expect("asks"), Sending::Nothing);
    assert!(!alice.plugin_may_propose("com.example.tool").await.expect("asks"));
    assert!(alice.plugin_send_file("com.example.tool", &id(&bob), &made, "made.txt", "text/plain").await.is_err());
    assert!(texts(&alice, &id(&bob)).await.is_empty(), "nothing was sent");

    // Propose: the user presses send, so the core refuses to send on its own.
    alice.grant_plugin("com.example.tool", Permissions { send: Sending::Propose, ..Permissions::default() }).await.expect("grants");
    assert!(alice.plugin_may_propose("com.example.tool").await.expect("asks"));
    assert!(alice.plugin_send_file("com.example.tool", &id(&bob), &made, "made.txt", "text/plain").await.is_err());
    assert!(texts(&alice, &id(&bob)).await.is_empty());

    // Auto: it sends.
    alice.grant_plugin("com.example.tool", Permissions { send: Sending::Auto, ..Permissions::default() }).await.expect("grants");
    alice.plugin_send_file("com.example.tool", &id(&bob), &made, "made.txt", "text/plain").await.expect("sends");
    until("bob got the file", || async { texts(&bob, &id(&alice)).await == ["made.txt"] }).await;

    // A plugin that is not installed cannot do anything at all.
    assert!(alice.plugin_send_file("com.example.ghost", &id(&bob), &made, "made.txt", "text/plain").await.is_err());
}

// ---------------------------------------------------------------------------------------------
// A3 · Hidden sessions: every PIN opens one, the same way; an empty one goes when it is closed,
// and a session that goes takes its link with it.
// ---------------------------------------------------------------------------------------------

async fn open(core: &Core, pin: &str) -> String {
    core.open_session(pin).await.expect("opens").expect("there is room for it")
}

#[tokio::test(flavor = "multi_thread")]
async fn every_pin_opens_a_session_and_the_same_pin_the_same_one() {
    let net = Net::new();
    let alice = device(&net, "Alice").await;
    let session = open(&alice, "123456").await;
    assert_eq!(alice.open_sessions(), vec![session.clone()]);
    assert_eq!(open(&alice, "123456").await, session, "the same pin is the same session");
    assert_eq!(alice.store().used_slots().await.unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_empty_session_goes_when_closed_so_pins_never_fill_the_slots() {
    let net = Net::new();
    let alice = device(&net, "Alice").await;
    let session = open(&alice, "123456").await;
    assert!(alice.close_session(&session).await.expect("closes"), "it was empty: it is gone");
    assert!(alice.open_sessions().is_empty());
    assert!(alice.store().used_slots().await.unwrap().is_empty(), "its slot is free again");
    assert_ne!(open(&alice, "123456").await, session, "nothing of it was left behind");

    // Typing pin after pin never uses up the seven slots.
    for pin in 0..20 {
        let session = open(&alice, &format!("20{pin:04}")).await;
        assert!(alice.close_session(&session).await.expect("closes"));
    }
    for pin in 1..7 {
        open(&alice, &format!("10000{pin}")).await;
    }
    assert_eq!(alice.store().used_slots().await.unwrap().len(), 7);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_with_a_contact_stays_when_closed() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    let session = open(&alice, "123456").await;
    let link = bob.my_card().await.expect("card").to_link();
    alice.add_contact_in(&link, None, Some(&session)).await.expect("adds inside");

    assert!(!alice.close_session(&session).await.expect("closes"), "it holds a contact: it stays");
    assert!(alice.open_sessions().is_empty(), "closed all the same");
    assert_eq!(open(&alice, "123456").await, session);
    assert_eq!(alice.store().session_contacts(&session).await.unwrap().len(), 1);
}

// Someone scanned the session's QR and wrote: that is something in it already.
#[tokio::test(flavor = "multi_thread")]
async fn a_session_with_only_a_request_stays_when_closed() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    let session = open(&alice, "123456").await;
    let link = alice.my_card_in(Some(&session)).await.expect("card").to_link();
    bob.add_contact(&link, None).await.expect("bob scans the session's qr");
    until("alice has bob's request", || async { alice.store().contact(&id(&bob)).await.unwrap().is_some() }).await;

    assert!(!alice.close_session(&session).await.expect("closes"));
    assert_eq!(open(&alice, "123456").await, session);
}

async fn restarted(net: &Arc<Net>, path: &std::path::Path) -> Arc<Core> {
    Core::open(Store::open(path).await.unwrap(), [9; 32], Arc::new(Link { net: net.clone() })).await.expect("opens again")
}

// 2026-10-01 (§108): a session stays open until the user leaves it, whatever happens to the app
// (closed, killed, the phone restarted, an update). After a start it is open with no PIN asked,
// empty or not: only the user closes it.
#[tokio::test(flavor = "multi_thread")]
async fn a_session_left_open_is_still_open_after_a_restart() {
    let net = Net::new();
    let path = scratch("restart-open").join("flickertalk.db");
    let (with_contact, empty, slots) = {
        let alice = restarted(&net, &path).await;
        let bob = device(&net, "Bob").await;
        let with_contact = open(&alice, "111111").await;
        alice.add_contact_in(&bob.my_card().await.expect("card").to_link(), None, Some(&with_contact)).await.expect("adds inside");
        let empty = open(&alice, "222222").await;
        (with_contact, empty, alice.open_slots())
    };

    let alice = restarted(&net, &path).await;
    let mut expected = vec![with_contact.clone(), empty.clone()];
    expected.sort();
    assert_eq!(alice.open_sessions(), expected, "both open, no PIN asked");
    assert_eq!(alice.open_slots(), slots, "on the same slots");
    assert_eq!(alice.store().used_slots().await.unwrap().len(), 2, "an open empty one is not swept away");
    assert_eq!(open(&alice, "222222").await, empty, "its PIN still opens the same one");
}

// A session the user left needs its PIN again after a start, as before.
#[tokio::test(flavor = "multi_thread")]
async fn a_session_left_closed_stays_closed_after_a_restart() {
    let net = Net::new();
    let path = scratch("restart-closed").join("flickertalk.db");
    let (kept, still_open) = {
        let alice = restarted(&net, &path).await;
        let bob = device(&net, "Bob").await;
        let kept = open(&alice, "111111").await;
        alice.add_contact_in(&bob.my_card().await.expect("card").to_link(), None, Some(&kept)).await.expect("adds inside");
        assert!(!alice.close_session(&kept).await.expect("closes"));
        let carol = device(&net, "Carol").await;
        let still_open = open(&alice, "333333").await;
        alice.add_contact_in(&carol.my_card().await.expect("card").to_link(), None, Some(&still_open)).await.expect("adds inside");
        (kept, still_open)
    };

    let alice = restarted(&net, &path).await;
    assert_eq!(alice.open_sessions(), vec![still_open], "only the one never left");
    assert_eq!(open(&alice, "111111").await, kept, "the PIN opens the closed one with what it had");
}

// A session deleted, or opened again and left, is not brought back by a start.
#[tokio::test(flavor = "multi_thread")]
async fn a_session_removed_or_left_is_not_open_after_a_restart() {
    let net = Net::new();
    let path = scratch("restart-left").join("flickertalk.db");
    let kept = {
        let alice = restarted(&net, &path).await;
        let (bob, carol) = (device(&net, "Bob").await, device(&net, "Carol").await);
        let gone = open(&alice, "111111").await;
        alice.add_contact_in(&bob.my_card().await.expect("card").to_link(), None, Some(&gone)).await.expect("adds inside");
        alice.remove_session(&gone).await.expect("removes");
        let kept = open(&alice, "222222").await;
        alice.add_contact_in(&carol.my_card().await.expect("card").to_link(), None, Some(&kept)).await.expect("adds inside");
        alice.close_session(&kept).await.expect("closes");
        open(&alice, "222222").await;
        alice.close_session(&kept).await.expect("closes again");
        kept
    };

    let alice = restarted(&net, &path).await;
    assert!(alice.open_sessions().is_empty());
    assert!(alice.open_slots().is_empty());
    assert_eq!(alice.store().used_slots().await.unwrap().len(), 1);
    assert_eq!(open(&alice, "222222").await, kept);
}

// An empty session that is not open has nothing to keep (a leftover of an app from before, which
// closed every session at start): it goes on the next start, as if closed (A3).
#[tokio::test(flavor = "multi_thread")]
async fn an_empty_session_that_is_not_open_is_gone_after_a_restart() {
    let net = Net::new();
    let path = scratch("restart-leftover").join("flickertalk.db");
    {
        let alice = restarted(&net, &path).await;
        alice.store().add_session("leftover", &[4; 32], 5).await.expect("a leftover");
    }
    let alice = restarted(&net, &path).await;
    assert!(alice.store().used_slots().await.unwrap().is_empty());
    assert!(alice.open_sessions().is_empty());
}

// 2026-10-01 (§108): the router is told which slots are silent and sends no push for them. A
// closed session's slot is silent, an open one's is not, and the main list never is.
#[tokio::test(flavor = "multi_thread")]
async fn the_router_is_told_which_sessions_are_closed() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    let before = alice.silent_slots().await.unwrap();
    assert_eq!(before & 1, 0, "the main list is never silent");

    let session = open(&alice, "123456").await;
    let bit = 1u8 << alice.store().session_slot(&session).await.unwrap().unwrap();
    assert_eq!(alice.silent_slots().await.unwrap() & bit, 0, "open: its pushes come");
    alice.add_contact_in(&bob.my_card().await.expect("card").to_link(), None, Some(&session)).await.expect("adds inside");
    assert!(!alice.close_session(&session).await.expect("closes"));
    assert_eq!(alice.silent_slots().await.unwrap() & bit, bit, "closed: silent");
    open(&alice, "123456").await;
    assert_eq!(alice.silent_slots().await.unwrap() & bit, 0, "open again");
    assert_eq!(alice.silent_slots().await.unwrap() & !bit, before & !bit, "the other slots did not move");
}

// A spare slot shows a random bit, chosen once and kept: the mask says nothing of how many
// sessions there are, and does not flicker from one registration to the next.
#[tokio::test(flavor = "multi_thread")]
async fn a_spare_slot_is_silent_or_not_at_random_and_stays_so() {
    let net = Net::new();
    let path = scratch("noise").join("flickertalk.db");
    let first = restarted(&net, &path).await.silent_slots().await.unwrap();
    let alice = restarted(&net, &path).await;
    assert_eq!(alice.silent_slots().await.unwrap(), first, "the same after a restart");
    assert_eq!(alice.silent_slots().await.unwrap(), first, "and from one asking to the next");

    let mut masks = std::collections::HashSet::new();
    for _ in 0..12 {
        masks.insert(device(&net, "Any").await.silent_slots().await.unwrap());
    }
    assert!(masks.len() > 1, "phones with no session do not all say the same");
    assert!(masks.iter().all(|mask| mask & 1 == 0));
}

// A slot's link must die with its session: otherwise whoever kept the old QR would land in the
// next session to take the slot, or in the main list.
#[tokio::test(flavor = "multi_thread")]
async fn a_session_that_goes_takes_its_link_with_it() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    let session = open(&alice, "123456").await;
    let old_link = alice.my_card_in(Some(&session)).await.expect("card").to_link();
    let before = alice.route_capability_hashes().await.unwrap();
    assert!(alice.close_session(&session).await.expect("closes"));

    let after = alice.route_capability_hashes().await.unwrap();
    assert_eq!(after[0], before[0], "the main link is untouched");
    assert_ne!(after, before, "the router gets a new hash for that slot");

    let next = open(&alice, "654321").await;
    assert_ne!(alice.my_card_in(Some(&next)).await.unwrap().to_link(), old_link);
    bob.add_contact(&old_link, None).await.expect("bob scans the old qr");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(alice.store().contact(&id(&bob)).await.unwrap().is_none(), "a retired link reaches nobody");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_is_removed_with_everything_in_it_and_its_link() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    let session = open(&alice, "123456").await;
    let link = bob.my_card().await.expect("card").to_link();
    alice.add_contact_in(&link, None, Some(&session)).await.expect("adds inside");
    until("bob knows alice", || async { bob.store().contact(&id(&alice)).await.unwrap().is_some() }).await;
    bob.send_text(&id(&alice), "secret").await.expect("sends");
    until("alice got it", || async { texts(&alice, &id(&bob)).await == ["secret"] }).await;
    let before = alice.route_capability_hashes().await.unwrap();

    alice.remove_session(&session).await.expect("removes");
    assert!(alice.open_sessions().is_empty());
    assert!(alice.store().contact(&id(&bob)).await.unwrap().is_none(), "its contacts are gone");
    assert!(alice.store().used_slots().await.unwrap().is_empty(), "its slot is free again");
    assert_ne!(alice.route_capability_hashes().await.unwrap(), before, "and its link is retired");
    assert_ne!(open(&alice, "123456").await, session, "the same pin now opens a new, empty one");
}

// Seven sessions with something in them fill the phone: an eighth pin opens nothing, and the
// app shows an empty screen as for any other pin.
#[tokio::test(flavor = "multi_thread")]
async fn with_every_slot_taken_a_new_pin_opens_nothing() {
    let net = Net::new();
    let alice = device(&net, "Alice").await;
    for pin in 1..=7 {
        open(&alice, &format!("10000{pin}")).await;
    }
    let hashes = alice.route_capability_hashes().await.unwrap();
    assert_eq!(alice.open_session("999999").await.expect("asks"), None);
    assert_eq!(alice.open_sessions().len(), 7);
    assert_eq!(alice.route_capability_hashes().await.unwrap(), hashes);
}

// ---------------------------------------------------------------------------------------------
// A4 · Files: small ones come alone, big ones wait for the user, absurd ones are refused.
// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_big_file_waits_for_the_user_and_a_small_one_does_not() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    bob.set_auto_download_limit(10_000).await.expect("sets");
    assert_eq!(bob.auto_download_limit().await.expect("reads"), 10_000);

    let small = alice.send_file(&id(&bob), &some_file("small.bin", 5_000), "small.bin", "application/octet-stream").await.expect("offers");
    until("the small file arrives whole", || async { bob.store().file(&small).await.unwrap().is_some_and(|f| f.complete) }).await;

    let big = alice.send_file(&id(&bob), &some_file("big.bin", 50_000), "big.bin", "application/octet-stream").await.expect("offers");
    until("the big offer is known", || async { bob.store().file(&big).await.unwrap().is_some() }).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let record = bob.store().file(&big).await.unwrap().unwrap();
    assert!(record.waiting, "it waits for the user");
    assert_eq!(record.chunks_done, 0, "not a byte was asked for");
    assert!(!record.complete);

    bob.accept_file(&big).await.expect("the user asks for it");
    until("the big file arrives whole", || async { bob.store().file(&big).await.unwrap().is_some_and(|f| f.complete && !f.waiting) }).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_waiting_file_is_left_alone_by_the_resume_loop() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    bob.set_auto_download_limit(1_000).await.expect("sets");
    let big = alice.send_file(&id(&bob), &some_file("big.bin", 20_000), "big.bin", "application/octet-stream").await.expect("offers");
    until("the offer is known", || async { bob.store().file(&big).await.unwrap().is_some() }).await;
    bob.resume_files().await.expect("resumes");
    bob.resume_files_from(&id(&alice), Duration::ZERO).await.expect("resumes");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(bob.store().file(&big).await.unwrap().unwrap().chunks_done, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_beyond_the_hard_limit_is_refused_on_both_sides() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    // The offer says it is enormous; no bytes exist, and none should ever be asked for.
    let huge = ft_protocol::Packet::new(ft_protocol::Body::File {
        name: "planet.iso".to_owned(),
        size: ft_core::MAX_FILE_SIZE + 1,
        mime: "application/octet-stream".to_owned(),
        hash: [1; 32],
        chunk: ft_protocol::FILE_CHUNK,
    });
    alice.send_raw_to(&id(&bob), &huge).await.expect("offers");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(bob.store().message(&huge.id.to_string()).await.unwrap().is_none(), "the offer was not even stored");
}

// ---------------------------------------------------------------------------------------------
// M5 · M6 · Retention counts from arrival, and never eats an undelivered message.
// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn retention_counts_from_arrival_not_from_the_senders_clock() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    bob.set_history(&id(&alice), 86_400, 0).await.expect("keeps a day");
    // Alice's clock says the message is a week old.
    let stale = ft_protocol::Packet {
        version: ft_protocol::PROTOCOL_VERSION,
        id: ft_protocol::MessageId::new(),
        sent_at: 1_600_000_000_000,
        body: ft_protocol::Body::Message { text: "from the past".to_owned() },
    };
    alice.send_raw_to(&id(&bob), &stale).await.expect("sends");
    until("bob got it", || async { texts(&bob, &id(&alice)).await == ["from the past"] }).await;
    bob.sweep_history().await.expect("sweeps");
    assert_eq!(texts(&bob, &id(&alice)).await, ["from the past"], "arrived today: it stays a day");
    let stored = bob.store().message(&stale.id.to_string()).await.unwrap().unwrap();
    assert!(stored.received_at > stale.sent_at as i64, "the phone remembers when it really arrived");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_undelivered_message_survives_the_sweep() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    alice.set_history(&id(&bob), 1, 0).await.expect("keeps a second");
    bob.set_mailbox(false).await.expect("no mailbox");
    until("alice knows", || async { alice.store().contact(&id(&bob)).await.unwrap().is_some_and(|c| !c.mailbox) }).await;
    net.set_direct(false);
    let pending = alice.send_text(&id(&bob), "waiting").await.expect("queues");
    tokio::time::sleep(Duration::from_millis(1_200)).await;
    alice.sweep_history().await.expect("sweeps");
    assert_eq!(alice.store().message(&pending).await.unwrap().map(|m| m.state), Some(MessageState::Pending), "still waiting to go");
    assert_eq!(alice.store().outbox().await.unwrap().len(), 1);
}

// ---------------------------------------------------------------------------------------------
// A5 · Strangers wait in requests; a link can be renewed; a card's name is tamed (B6).
// ---------------------------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn a_stranger_waits_in_requests_until_accepted() {
    let net = Net::new();
    let (alice, mallory) = (device(&net, "Alice").await, device(&net, "Mallory").await);
    // Mallory got Alice's link somewhere and adds herself.
    let link = alice.my_card().await.expect("card").to_link();
    mallory.add_contact(&link, None).await.expect("adds");
    mallory.send_text(&id(&alice), "hey").await.expect("writes");
    until("alice has the request, with its text", || async {
        alice.requests().await.unwrap().first().is_some_and(|r| r.last.as_ref().is_some_and(|m| m.body == "hey"))
    })
    .await;

    let request = &alice.requests().await.unwrap()[0];
    assert_eq!(request.contact.device_id, id(&mallory));
    assert!(!request.contact.accepted, "the text is there to judge by, but nothing more happens");
    assert!(alice.store().contacts().await.unwrap().is_empty(), "not in the main list");
    assert!(alice.store().conversations().await.unwrap().is_empty());
    // The phone stored it, so the sender is told so: honest, and the retries stop (§27, §84).
    until("mallory sees it delivered", || async {
        mallory.store().messages(&id(&alice), 10).await.unwrap()[0].state == MessageState::Delivered
    })
    .await;

    alice.accept_contact(&id(&mallory)).await.expect("accepts");
    assert!(alice.requests().await.unwrap().is_empty());
    assert_eq!(alice.store().contacts().await.unwrap().len(), 1);
    until("mallory gets alice's card", || async {
        mallory.store().contact(&id(&alice)).await.unwrap().is_some_and(|c| c.introduced)
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_declined_stranger_is_blocked_and_gone_from_requests() {
    let net = Net::new();
    let (alice, mallory) = (device(&net, "Alice").await, device(&net, "Mallory").await);
    let link = alice.my_card().await.expect("card").to_link();
    mallory.add_contact(&link, None).await.expect("adds");
    until("alice has the request", || async { alice.requests().await.unwrap().len() == 1 }).await;
    alice.decline_contact(&id(&mallory)).await.expect("declines");
    assert!(alice.requests().await.unwrap().is_empty());
    assert!(alice.store().contact(&id(&mallory)).await.unwrap().is_some_and(|c| c.blocked));
}

// Scanning someone means you want them: they are accepted at once, even if they wrote first.
#[tokio::test(flavor = "multi_thread")]
async fn scanning_a_pending_stranger_accepts_them() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    let link = alice.my_card().await.expect("card").to_link();
    bob.add_contact(&link, None).await.expect("adds");
    until("alice has the request", || async { alice.requests().await.unwrap().len() == 1 }).await;
    alice.add_contact(&bob.my_card().await.unwrap().to_link(), None).await.expect("alice scans bob too");
    assert!(alice.requests().await.unwrap().is_empty());
    assert_eq!(alice.store().contacts().await.unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pending_stranger_gets_no_files_and_no_calls() {
    let net = Net::new();
    let (alice, mallory) = (device(&net, "Alice").await, device(&net, "Mallory").await);
    let link = alice.my_card().await.expect("card").to_link();
    mallory.add_contact(&link, None).await.expect("adds");
    until("alice has the request", || async { alice.requests().await.unwrap().len() == 1 }).await;

    let file = mallory.send_file(&id(&alice), &some_file("bait.bin", 3_000), "bait.bin", "application/octet-stream").await.expect("offers");
    until("the offer is known", || async { alice.store().file(&file).await.unwrap().is_some() }).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(alice.store().file(&file).await.unwrap().unwrap().chunks_done, 0, "not a byte comes in");

    let mut at_alice = alice.events();
    let call = mallory.place_call(&id(&alice), false).await.expect("dials");
    mallory.offer_call_within(&call, "v=0", Duration::from_millis(500)).await.expect("offers");
    until("mallory hears busy", || async {
        mallory.store().call(&call).await.unwrap().is_some_and(|c| c.outcome == Some(ft_storage::CallOutcome::Busy))
    })
    .await;
    assert!(alice.visible_calls(10).await.unwrap().is_empty(), "and alice's phone never rang");
    let mut refused = false;
    while let Ok(event) = at_alice.try_recv() {
        assert!(!matches!(event, ft_core::Event::Call { .. }), "it never rings");
        refused |= event == ft_core::Event::CallRefused;
    }
    assert!(refused, "a ring the push started stops at once");
}

#[tokio::test(flavor = "multi_thread")]
async fn renewing_the_link_retires_the_old_one_and_tells_the_contacts() {
    let net = Net::new();
    let (alice, bob, mallory) = (device(&net, "Alice").await, device(&net, "Bob").await, device(&net, "Mallory").await);
    pair(&alice, &bob).await;
    let old_link = alice.my_card().await.expect("card").to_link();
    let old_capability = alice.route_capability();

    let hashes = alice.renew_link(None).await.expect("renews");
    assert_ne!(alice.route_capability(), old_capability);
    assert_eq!(hashes[0], alice.route_capability().hash(), "the router gets the new hash first");
    assert_eq!(alice.my_card().await.unwrap().route_capability(), alice.route_capability());

    // Bob gets the new card without doing anything.
    let bob_id = id(&bob);
    until("bob has the new card", || async {
        let stored = bob.store().contact(&id(&alice)).await.unwrap().unwrap();
        ft_contacts::ContactCard::decode(&stored.card).unwrap().route_capability() == alice.route_capability()
    })
    .await;
    let _ = bob_id;

    // Mallory holds the old link: whoever scans it now is talking to a retired address.
    let stale = ft_contacts::ContactCard::from_link(&old_link).expect("still a valid card");
    assert_ne!(stale.route_capability(), alice.route_capability());
    let _ = mallory;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cards_name_is_tamed_before_it_is_shown() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    bob.set_name(&format!("Mam\u{202e}á\u{0000} {}", "x".repeat(200))).await.expect("names");
    pair(&alice, &bob).await;
    let shown = alice.store().contact(&id(&bob)).await.unwrap().unwrap().name;
    assert!(!shown.contains('\u{202e}'), "no bidi override");
    assert!(!shown.contains('\u{0000}'), "no control characters");
    assert!(shown.chars().count() <= 40, "no endless names: {shown:?}");
    assert!(shown.starts_with("Mamá"));
    assert_eq!(ft_core::clean_name("  \t "), "", "nothing is nothing");
}
