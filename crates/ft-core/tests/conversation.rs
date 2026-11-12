use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use async_trait::async_trait;
use ft_core::moving::{received_move, MoveUpdate};
use ft_core::{CallUpdate, Core, Event, MailboxRejected, Peer, Transport};
use ft_storage::{CallOutcome, ContactRules, FileRecord, MessageState, Store};
use tokio::sync::mpsc;

#[derive(Default)]
struct Net {
    direct: AtomicBool,
    cores: Mutex<HashMap<String, Weak<Core>>>,
    queues: Mutex<HashMap<String, mpsc::UnboundedSender<Vec<u8>>>>,
    unreachable: Mutex<HashSet<String>>,
    mailboxes: Mutex<HashMap<String, Vec<Vec<u8>>>>,
    rejecting: Mutex<HashSet<String>>,
    failing: Mutex<HashSet<String>>,
    sent: Mutex<Vec<(String, Vec<u8>)>>,
    cut_after: Mutex<Option<(String, usize)>>,
    tried: Mutex<Vec<String>>,
    hold_reach: Mutex<Option<(String, tokio::sync::oneshot::Receiver<()>)>>,
    hold_mailbox: Mutex<Option<(String, tokio::sync::oneshot::Receiver<()>)>>,
    holding: AtomicBool,
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

    fn sent_to(&self, device: &str) -> usize {
        self.sent.lock().unwrap().iter().filter(|(to, _)| to == device).count()
    }

    fn tried(&self, device: &str) -> usize {
        self.tried.lock().unwrap().iter().filter(|to| *to == device).count()
    }

    fn hold_reach(&self, device: &str) -> tokio::sync::oneshot::Sender<()> {
        let (release, held) = tokio::sync::oneshot::channel();
        *self.hold_reach.lock().unwrap() = Some((device.to_owned(), held));
        release
    }

    fn hold_mailbox(&self, device: &str) -> tokio::sync::oneshot::Sender<()> {
        let (release, held) = tokio::sync::oneshot::channel();
        *self.hold_mailbox.lock().unwrap() = Some((device.to_owned(), held));
        release
    }

    async fn held(&self, slot: &Mutex<Option<(String, tokio::sync::oneshot::Receiver<()>)>>, device: &str) {
        let held = {
            let mut slot = slot.lock().unwrap();
            if slot.as_ref().is_some_and(|(to, _)| to == device) { slot.take().map(|(_, held)| held) } else { None }
        };
        if let Some(held) = held {
            self.holding.store(true, Ordering::SeqCst);
            let _ = held.await;
            self.holding.store(false, Ordering::SeqCst);
        }
    }

    fn mailbox_len(&self, device: &str) -> usize {
        self.mailboxes.lock().unwrap().get(device).map_or(0, Vec::len)
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
    async fn reach(&self, to: &Peer, _fallback: bool) -> anyhow::Result<bool> {
        self.net.held(&self.net.hold_reach, &to.device_id).await;
        self.net.tried.lock().unwrap().push(to.device_id.clone());
        Ok(self.net.direct.load(Ordering::SeqCst) && !self.net.unreachable.lock().unwrap().contains(&to.device_id))
    }

    async fn send_direct(&self, to: &Peer, bytes: Vec<u8>) -> anyhow::Result<bool> {
        self.net.held(&self.net.hold_reach, &to.device_id).await;
        self.net.tried.lock().unwrap().push(to.device_id.clone());
        if !self.net.direct.load(Ordering::SeqCst) || self.net.unreachable.lock().unwrap().contains(&to.device_id) {
            return Ok(false);
        }
        let Some(queue) = self.net.queues.lock().unwrap().get(&to.device_id).cloned() else {
            return Ok(false);
        };
        if let Some((device, limit)) = self.net.cut_after.lock().unwrap().clone() {
            if device == to.device_id && self.net.sent_to(&device) >= limit {
                self.net.set_direct(false);
                return Ok(false);
            }
        }
        self.net.sent.lock().unwrap().push((to.device_id.clone(), bytes.clone()));
        Ok(queue.send(bytes).is_ok())
    }

    async fn send_mailbox(&self, to: &Peer, bytes: Vec<u8>) -> anyhow::Result<()> {
        if self.net.rejecting.lock().unwrap().contains(&to.device_id) {
            return Err(MailboxRejected.into());
        }
        if self.net.failing.lock().unwrap().contains(&to.device_id) {
            anyhow::bail!("the router answered 507 Insufficient Storage");
        }
        self.net.mailboxes.lock().unwrap().entry(to.device_id.clone()).or_default().push(bytes);
        self.net.held(&self.net.hold_mailbox, &to.device_id).await;
        Ok(())
    }
}

async fn device(net: &Arc<Net>, name: &str) -> Arc<Core> {
    device_with(net, name, Store::open_in_memory().await.expect("store"), [9; 32]).await
}

async fn device_with(net: &Arc<Net>, name: &str, store: Store, key: [u8; 32]) -> Arc<Core> {
    device_in(net, name, store, key, scratch(name)).await
}

async fn device_in(net: &Arc<Net>, name: &str, store: Store, key: [u8; 32], files: PathBuf) -> Arc<Core> {
    let core = Core::open(store, key, Arc::new(Link { net: net.clone() })).await.expect("opens");
    core.set_name(name).await.expect("names");
    core.set_files_dir(files);
    let id = core.device_id().as_str().to_owned();
    net.cores.lock().unwrap().insert(id.clone(), Arc::downgrade(&core));

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

async fn until<F, Fut>(what: &str, condition: F)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    until_within(what, Duration::from_secs(5), condition).await
}

async fn until_within<F, Fut>(what: &str, limit: Duration, condition: F)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..limit.as_millis() / 20 {
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
    let (alice_id, bob_id) = (alice.device_id().as_str().to_owned(), bob.device_id().as_str().to_owned());
    until("bob knows alice", || async { bob.store().contact(&alice_id).await.unwrap().is_some() }).await;
    bob.accept_contact(&alice_id).await.expect("bob accepts alice");
    until("bob's card came back", || async {
        alice.store().contact(&bob_id).await.unwrap().is_some_and(|contact| contact.introduced)
    })
    .await;
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

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ft-core-{name}-{}", ft_protocol::MessageId::new()));
    std::fs::create_dir_all(&dir).expect("creates the directory");
    dir
}

fn some_file(size: usize) -> (PathBuf, Vec<u8>) {
    let bytes: Vec<u8> = (0..size).map(|i| (i * 7 + i / 251) as u8).collect();
    let path = scratch("outgoing").join("holiday photo.jpg");
    std::fs::write(&path, &bytes).expect("writes");
    (path, bytes)
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("after 1970").as_millis() as i64
}

async fn file_of(core: &Core, message_id: &str) -> FileRecord {
    core.store().file(message_id).await.expect("reads").expect("the file exists")
}

#[tokio::test(flavor = "multi_thread")]
async fn scanning_a_card_pairs_both_devices() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;

    assert_eq!(alice.store().contact(&id(&bob)).await.unwrap().expect("bob").name, "Bob");
    assert_eq!(bob.store().contact(&id(&alice)).await.unwrap().expect("alice").name, "Alice");
}

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

    until("the receipt is in alice's mailbox", || async { net.mailbox_len(&id(&alice)) == 1 }).await;
    net.collect(&alice).await;
    assert_eq!(state_of(&alice, &id(&bob), &message).await, MessageState::Delivered);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rejected_mailbox_does_not_hold_back_other_contacts() {
    let net = Net::new();
    let (alice, bob, carol) = (device(&net, "Alice").await, device(&net, "Bob").await, device(&net, "Carol").await);
    carol.set_mailbox(false).await.expect("carol turns the mailbox off");
    pair(&alice, &bob).await;
    pair(&alice, &carol).await;
    net.unreachable.lock().unwrap().extend([id(&bob), id(&carol)]);
    net.rejecting.lock().unwrap().insert(id(&bob));

    let _ = alice.send_text(&id(&bob), "for bob").await;
    let for_carol = alice.send_text(&id(&carol), "for carol").await.expect("waits on the phone");
    assert_eq!(state_of(&alice, &id(&carol), &for_carol).await, MessageState::Pending);

    net.unreachable.lock().unwrap().remove(&id(&carol));
    let _ = alice.retry_now().await;
    until("carol gets it", || async { texts(&carol, &id(&alice)).await.contains(&"for carol".to_owned()) }).await;
    until("delivered to carol", || async { state_of(&alice, &id(&carol), &for_carol).await == MessageState::Delivered }).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rejected_message_leaves_the_queue_as_not_sent() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    net.unreachable.lock().unwrap().insert(id(&bob));
    net.rejecting.lock().unwrap().insert(id(&bob));

    let message = alice.send_text(&id(&bob), "for bob").await.expect("stored");
    assert_eq!(state_of(&alice, &id(&bob), &message).await, MessageState::NotSent);
    assert_eq!(outbox_len(&alice).await, 0, "out of the queue");
    alice.retry_now().await.expect("retries");
    assert_eq!(state_of(&alice, &id(&bob), &message).await, MessageState::NotSent);
    assert_eq!(net.mailbox_len(&id(&bob)), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_message_in_the_mailbox_refused_on_a_retry_stays_sent() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    net.unreachable.lock().unwrap().insert(id(&bob));

    let message = alice.send_text(&id(&bob), "for bob").await.expect("sends");
    assert_eq!(state_of(&alice, &id(&bob), &message).await, MessageState::Sent);
    net.rejecting.lock().unwrap().insert(id(&bob));
    alice.retry_now().await.expect("retries");
    assert_eq!(state_of(&alice, &id(&bob), &message).await, MessageState::Sent);
    assert_eq!(outbox_len(&alice).await, 1, "no copy within the day: the router was not asked");

    alice.store().mailed(&message, now() - DAY).await.unwrap();
    alice.retry_now().await.expect("retries");
    assert_eq!(state_of(&alice, &id(&bob), &message).await, MessageState::Sent);
    assert_eq!(outbox_len(&alice).await, 0, "out of the queue");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_message_not_sent_goes_again_when_resent() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    net.unreachable.lock().unwrap().insert(id(&bob));
    net.rejecting.lock().unwrap().insert(id(&bob));
    let message = alice.send_text(&id(&bob), "for bob").await.expect("stored");
    assert_eq!(state_of(&alice, &id(&bob), &message).await, MessageState::NotSent);

    net.rejecting.lock().unwrap().remove(&id(&bob));
    alice.resend(&message).await.expect("sends it again");
    assert_eq!(state_of(&alice, &id(&bob), &message).await, MessageState::Sent);
    net.collect(&bob).await;
    assert!(bob.store().message(&message).await.unwrap().is_some_and(|m| m.body == "for bob"), "the same message");
    until("delivered", || async { state_of(&alice, &id(&bob), &message).await == MessageState::Delivered }).await;
    assert!(alice.resend(&message).await.is_err(), "a delivered message is not sent again");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_passing_failure_keeps_the_message_queued_and_holds_back_no_one() {
    let net = Net::new();
    let (alice, bob, carol) = (device(&net, "Alice").await, device(&net, "Bob").await, device(&net, "Carol").await);
    carol.set_mailbox(false).await.expect("carol turns the mailbox off");
    pair(&alice, &bob).await;
    pair(&alice, &carol).await;
    net.unreachable.lock().unwrap().extend([id(&bob), id(&carol)]);
    net.failing.lock().unwrap().insert(id(&bob));

    let for_bob = alice.send_text(&id(&bob), "for bob").await.expect("stored");
    let for_carol = alice.send_text(&id(&carol), "for carol").await.expect("stored");
    net.unreachable.lock().unwrap().remove(&id(&carol));
    alice.retry_now().await.expect("retries");
    until("delivered to carol", || async { state_of(&alice, &id(&carol), &for_carol).await == MessageState::Delivered }).await;

    let entry = alice.store().outbox().await.unwrap().into_iter().find(|e| e.message_id == for_bob).expect("still queued");
    assert_eq!(entry.attempts, 2);
    assert!(entry.next_attempt > now(), "rescheduled, not due at once");
    assert_eq!(state_of(&alice, &id(&bob), &for_bob).await, MessageState::Pending);

    net.failing.lock().unwrap().remove(&id(&bob));
    alice.retry_now().await.expect("retries");
    assert_eq!(net.mailbox_len(&id(&bob)), 1);
    assert_eq!(state_of(&alice, &id(&bob), &for_bob).await, MessageState::Sent);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_for_a_contact_the_router_refuses_is_not_sent() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    net.unreachable.lock().unwrap().insert(id(&bob));
    let link = bob.my_card().await.expect("card").to_link();
    alice.add_contact(&link, None).await.expect("alice adds bob");
    net.rejecting.lock().unwrap().insert(id(&bob));

    let (path, _) = some_file(1000);
    let message = alice.send_file(&id(&bob), &path, "photo.jpg", "image/jpeg").await.expect("stored");
    assert_eq!(state_of(&alice, &id(&bob), &message).await, MessageState::NotSent);
    assert_eq!(outbox_len(&alice).await, 0);
}

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

const DAY: i64 = 24 * 3600 * 1000;

async fn outbox_entry(core: &Core, message_id: &str) -> ft_storage::OutboxEntry {
    core.store().outbox().await.unwrap().into_iter().find(|entry| entry.message_id == message_id).expect("in the outbox")
}

async fn in_bobs_mailbox(net: &Net, alice: &Core, bob: &Core) -> String {
    net.set_direct(false);
    let message = alice.send_text(&id(bob), "are you there?").await.expect("sends");
    assert_eq!(net.mailbox_len(&id(bob)), 1, "the first copy");
    let entry = outbox_entry(alice, &message).await;
    assert!(entry.in_mailbox);
    assert!(entry.mailed_at.is_some_and(|at| (now() - at).abs() < 5_000), "and when it went");
    message
}

#[tokio::test(flavor = "multi_thread")]
async fn a_message_in_the_mailbox_is_tried_directly_at_each_retry_and_copied_there_once_a_day() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    let message = in_bobs_mailbox(&net, &alice, &bob).await;

    for _ in 0..3 {
        let tried = net.tried(&id(&bob));
        alice.retry_now().await.expect("retries");
        assert!(net.tried(&id(&bob)) > tried, "tried directly");
        assert_eq!(net.mailbox_len(&id(&bob)), 1, "no new copy within the day");
        let entry = outbox_entry(&alice, &message).await;
        assert!(entry.in_mailbox);
        assert!(entry.next_attempt >= now() + 590_000, "next try in 10 minutes, as before");
    }

    alice.store().mailed(&message, now() - DAY).await.unwrap();
    alice.retry_now().await.expect("retries");
    assert_eq!(net.mailbox_len(&id(&bob)), 2, "a day later, a new copy");
    alice.retry_now().await.expect("retries");
    assert_eq!(net.mailbox_len(&id(&bob)), 2, "and then none for another day");

    net.collect(&bob).await;
    assert_eq!(texts(&bob, &id(&alice)).await, ["are you there?"], "two copies, shown once");
    until("both receipts wait for alice", || async { net.mailbox_len(&id(&alice)) == 2 }).await;
    net.collect(&alice).await;
    assert_eq!(state_of(&alice, &id(&bob), &message).await, MessageState::Delivered);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_direct_path_delivers_a_message_in_the_mailbox_and_ends_its_retries() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    let message = in_bobs_mailbox(&net, &alice, &bob).await;

    net.set_direct(true);
    alice.retry_now().await.expect("retries");
    until("delivered", || async { state_of(&alice, &id(&bob), &message).await == MessageState::Delivered }).await;
    assert!(alice.store().outbox().await.unwrap().is_empty());
    let tried = net.tried(&id(&bob));
    alice.store().mailed(&message, now() - DAY).await.unwrap();
    alice.retry_now().await.expect("retries");
    assert_eq!((net.tried(&id(&bob)), net.mailbox_len(&id(&bob))), (tried, 1), "nothing more");
}

#[tokio::test(flavor = "multi_thread")]
async fn with_the_mailbox_off_retries_leave_nothing_in_it() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    bob.set_mailbox(false).await.expect("bob turns the mailbox off");
    pair(&alice, &bob).await;
    net.set_direct(false);

    let message = alice.send_text(&id(&bob), "later").await.expect("sends");
    alice.retry_now().await.expect("retries");
    alice.store().mailed(&message, now() - DAY).await.unwrap();
    alice.retry_now().await.expect("retries");
    assert_eq!(net.mailbox_len(&id(&bob)), 0);
    assert!(!outbox_entry(&alice, &message).await.in_mailbox);
    assert_eq!(state_of(&alice, &id(&bob), &message).await, MessageState::Pending);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_passing_failure_on_the_daily_copy_tries_the_copy_again() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    let message = in_bobs_mailbox(&net, &alice, &bob).await;
    let day_ago = now() - DAY;
    alice.store().mailed(&message, day_ago).await.unwrap();

    net.failing.lock().unwrap().insert(id(&bob));
    alice.retry_now().await.expect("retries");
    let entry = outbox_entry(&alice, &message).await;
    assert!(entry.in_mailbox && entry.next_attempt > now(), "rescheduled, still in the mailbox");
    assert_eq!(entry.mailed_at, Some(day_ago), "the copy did not go");
    assert_eq!(state_of(&alice, &id(&bob), &message).await, MessageState::Sent);

    net.failing.lock().unwrap().remove(&id(&bob));
    alice.retry_now().await.expect("retries");
    assert_eq!(net.mailbox_len(&id(&bob)), 2, "the copy goes");
    assert!(outbox_entry(&alice, &message).await.mailed_at.is_some_and(|at| at > day_ago));
}

#[tokio::test(flavor = "multi_thread")]
async fn turning_the_mailbox_off_is_told_to_contacts() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    bob.set_mailbox(false).await.expect("bob turns it off");
    until("alice learns it", || async { !alice.store().contact(&id(&bob)).await.unwrap().unwrap().mailbox }).await;
}

async fn mailbox_of(core: &Core, contact: &Core) -> bool {
    core.store().contact(&id(contact)).await.unwrap().expect("a contact").mailbox
}

#[tokio::test(flavor = "multi_thread")]
async fn a_mailbox_turned_on_out_of_reach_is_learnt_from_the_next_packet() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    alice.set_mailbox(false).await.expect("alice starts without it");
    pair(&alice, &bob).await;
    assert!(!mailbox_of(&bob, &alice).await, "bob knows it from her card");

    net.set_direct(false);
    net.failing.lock().unwrap().insert(id(&bob));
    alice.set_mailbox(true).await.expect("alice turns it on");
    net.set_direct(true);
    net.failing.lock().unwrap().remove(&id(&bob));
    assert!(!mailbox_of(&bob, &alice).await, "the change was lost on the way");

    alice.send_text(&id(&bob), "hi").await.expect("sends");
    until("bob got it", || async { texts(&bob, &id(&alice)).await == ["hi"] }).await;
    assert!(mailbox_of(&bob, &alice).await, "her text says she uses it now");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_mailbox_turned_off_out_of_reach_is_learnt_from_the_next_packet() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    assert!(mailbox_of(&bob, &alice).await);

    net.unreachable.lock().unwrap().insert(id(&bob));
    alice.set_mailbox(false).await.expect("alice turns it off");
    net.unreachable.lock().unwrap().remove(&id(&bob));
    assert!(mailbox_of(&bob, &alice).await, "the change was lost on the way");

    let first = bob.send_text(&id(&alice), "hi").await.expect("sends");
    until("bob sees it delivered", || async { state_of(&bob, &id(&alice), &first).await == MessageState::Delivered }).await;
    assert!(!mailbox_of(&bob, &alice).await, "her receipt says she no longer uses it");

    net.set_direct(false);
    let later = bob.send_text(&id(&alice), "later").await.expect("sends");
    assert_eq!(net.mailbox_len(&id(&alice)), 0, "nothing of alice's is stored on the server");
    assert_eq!(state_of(&bob, &id(&alice), &later).await, MessageState::Pending);
}

#[tokio::test(flavor = "multi_thread")]
async fn after_one_packet_a_stale_record_no_longer_holds_a_message_back_from_the_mailbox() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    let hello = bob.send_text(&id(&alice), "hello").await.expect("sends");
    until("delivered", || async { state_of(&bob, &id(&alice), &hello).await == MessageState::Delivered }).await;
    alice.store().set_contact_mailbox(&id(&bob), false).await.expect("a stale record");

    let here = bob.send_text(&id(&alice), "here").await.expect("sends");
    until("delivered", || async { state_of(&bob, &id(&alice), &here).await == MessageState::Delivered }).await;
    assert_eq!(texts(&alice, &id(&bob)).await, ["hello", "here"]);

    net.set_direct(false);
    let message = alice.send_text(&id(&bob), "are you there?").await.expect("sends");
    assert_eq!(net.mailbox_len(&id(&bob)), 1, "the mailbox takes it");
    assert_eq!(state_of(&alice, &id(&bob), &message).await, MessageState::Sent);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_packet_that_says_nothing_of_the_mailbox_leaves_the_record_as_it_is() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    alice.set_tells_mailbox(false);
    bob.store().set_contact_mailbox(&id(&alice), false).await.expect("what bob holds");

    alice.send_text(&id(&bob), "hi").await.expect("sends");
    until("bob got it", || async { texts(&bob, &id(&alice)).await == ["hi"] }).await;
    assert!(!mailbox_of(&bob, &alice).await, "untouched");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_retried_message_is_stored_once_and_acknowledged_again() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
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

async fn latest_in_list(core: &Core, contact: &str) -> Option<String> {
    let conversations = core.store().conversations().await.expect("lists");
    let conversation = conversations.into_iter().find(|c| c.contact.device_id == contact).expect("in the list");
    conversation.last.map(|message| message.body)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_message_written_while_blocked_is_dropped_after_the_unblock() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    alice.send_text(&id(&bob), "before the block").await.expect("sends");
    until("bob got it", || async { texts(&bob, &id(&alice)).await.len() == 1 }).await;
    bob.block(&id(&alice), true).await.expect("blocks");

    let blocked = alice.send_text(&id(&bob), "while blocked").await.expect("sends");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!alice.store().outbox().await.unwrap().is_empty(), "it waits in alice's outbox");
    bob.block(&id(&alice), false).await.expect("unblocks");
    alice.retry_now().await.expect("retries");

    until("alice stops retrying", || async { alice.store().outbox().await.unwrap().is_empty() }).await;
    assert_eq!(texts(&bob, &id(&alice)).await, ["before the block"]);
    assert_eq!(latest_in_list(&bob, &id(&alice)).await.as_deref(), Some("before the block"));
    assert_eq!(state_of(&alice, &id(&bob), &blocked).await, MessageState::Sent, "sent, never delivered");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_message_written_after_the_unblock_arrives() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    bob.block(&id(&alice), true).await.expect("blocks");
    bob.block(&id(&alice), false).await.expect("unblocks");

    let message = alice.send_text(&id(&bob), "friends again").await.expect("sends");
    until("bob has it", || async { texts(&bob, &id(&alice)).await == ["friends again"] }).await;
    until("alice sees it delivered", || async { state_of(&alice, &id(&bob), &message).await == MessageState::Delivered })
        .await;
    assert_eq!(latest_in_list(&bob, &id(&alice)).await.as_deref(), Some("friends again"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_message_written_while_blocked_is_dropped_after_a_restart() {
    let net = Net::new();
    let path = std::env::temp_dir().join(format!("ft-core-unblock-{}.db", ft_protocol::MessageId::new()));
    let alice = device(&net, "Alice").await;
    {
        let bob = device_with(&net, "Bob", Store::open(&path).await.unwrap(), [2; 32]).await;
        pair(&alice, &bob).await;
        bob.block(&id(&alice), true).await.expect("blocks");
        alice.send_text(&id(&bob), "while blocked").await.expect("sends");
        tokio::time::sleep(Duration::from_millis(200)).await;
        bob.block(&id(&alice), false).await.expect("unblocks");
    }

    let bob = device_with(&net, "Bob", Store::open(&path).await.unwrap(), [2; 32]).await;
    alice.retry_now().await.expect("retries");
    until("alice stops retrying", || async { alice.store().outbox().await.unwrap().is_empty() }).await;
    assert!(texts(&bob, &id(&alice)).await.is_empty());
    let _ = std::fs::remove_file(&path);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_offered_while_blocked_is_dropped_after_the_unblock() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    bob.block(&id(&alice), true).await.expect("blocks");
    let (path, _) = some_file(1_000);

    let sent = alice.send_file(&id(&bob), &path, "a.bin", "application/octet-stream").await.expect("offers");
    tokio::time::sleep(Duration::from_millis(200)).await;
    bob.block(&id(&alice), false).await.expect("unblocks");
    alice.retry_now().await.expect("retries");

    until("alice stops offering it", || async { alice.store().outbox().await.unwrap().is_empty() }).await;
    assert!(bob.store().file(&sent).await.unwrap().is_none(), "nothing of it is kept");
    assert!(texts(&bob, &id(&alice)).await.is_empty());
    assert_eq!(state_of(&alice, &id(&bob), &sent).await, MessageState::Sent, "sent, never delivered");
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

#[tokio::test(flavor = "multi_thread")]
async fn a_text_that_cannot_wait_in_the_outbox_is_not_kept() {
    let net = Net::new();
    let path = std::env::temp_dir().join(format!("ft-core-unqueued-{}.db", ft_protocol::MessageId::new()));
    let bob = device(&net, "Bob").await;
    let alice = device_with(&net, "Alice", Store::open(&path).await.unwrap(), [1; 32]).await;
    pair(&alice, &bob).await;
    let database = sqlx::SqlitePool::connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(&path)).await.expect("opens the same database");
    sqlx::query("CREATE TRIGGER forced_failure BEFORE INSERT ON pending_outbox BEGIN SELECT RAISE(ABORT, 'forced'); END")
        .execute(&database)
        .await
        .expect("creates the trigger");

    assert!(alice.queue_text(&id(&bob), "lost", None).await.is_err());
    assert!(texts(&alice, &id(&bob)).await.is_empty(), "no message left without its place in the outbox");
    assert!(alice.store().outbox().await.unwrap().is_empty());
    database.close().await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_crosses_in_chunks_and_is_verified() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    let (path, bytes) = some_file(200_000);

    let sent = alice.send_file(&id(&bob), &path, "holiday photo.jpg", "image/jpeg").await.expect("offers");
    until("bob has the whole file", || async {
        bob.store().file(&sent).await.unwrap().is_some_and(|file| file.complete)
    })
    .await;
    let received = file_of(&bob, &sent).await;
    assert_eq!(std::fs::read(bob.file_path(&received)).expect("reads"), bytes);
    assert_eq!((received.name.as_str(), received.mime.as_str(), received.size), ("holiday photo.jpg", "image/jpeg", 200_000));
    assert_eq!(texts(&bob, &id(&alice)).await, ["holiday photo.jpg"]);
    until("alice knows it arrived", || async { file_of(&alice, &sent).await.complete }).await;
    assert_eq!(state_of(&alice, &id(&bob), &sent).await, MessageState::Delivered);
}

#[tokio::test(flavor = "multi_thread")]
async fn files_never_go_through_the_mailbox() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    net.set_direct(false);
    let (path, bytes) = some_file(70_000);

    let sent = alice.send_file(&id(&bob), &path, "a.bin", "application/octet-stream").await.expect("queues");
    alice.retry_now().await.expect("retries");
    assert_eq!(net.mailbox_len(&id(&bob)), 0, "nothing of the file reaches the server");
    assert_eq!(state_of(&alice, &id(&bob), &sent).await, MessageState::Pending);

    net.set_direct(true);
    alice.retry_now().await.expect("retries");
    until("bob has it", || async { bob.store().file(&sent).await.unwrap().is_some_and(|file| file.complete) }).await;
    assert_eq!(std::fs::read(bob.file_path(&file_of(&bob, &sent).await)).unwrap(), bytes);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_interrupted_transfer_resumes_where_it_stopped() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    let chunks = 100;
    let (path, bytes) = some_file(chunks * ft_protocol::FILE_CHUNK as usize);
    let before = net.sent_to(&id(&bob));
    *net.cut_after.lock().unwrap() = Some((id(&bob), before + 60));

    let sent = alice.send_file(&id(&bob), &path, "big.bin", "application/octet-stream").await.expect("offers");
    until_within("the link went down", Duration::from_secs(20), || async { !net.direct.load(Ordering::SeqCst) }).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let got = file_of(&bob, &sent).await.chunks_done;
    assert!(got > 0 && got < chunks as i64, "bob got part of it: {got}");

    *net.cut_after.lock().unwrap() = None;
    net.set_direct(true);
    let at_resume = net.sent_to(&id(&bob));
    bob.resume_files_from(&id(&alice), Duration::ZERO).await.expect("resumes");
    until_within("bob has it all", Duration::from_secs(20), || async { file_of(&bob, &sent).await.complete }).await;
    assert_eq!(std::fs::read(bob.file_path(&file_of(&bob, &sent).await)).unwrap(), bytes);
    let resent = net.sent_to(&id(&bob)) - at_resume;
    assert!(resent < chunks - got as usize + 25, "only the missing chunks travel again: {resent}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_that_does_not_match_its_hash_is_rejected() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    net.set_direct(false);
    let (path, _) = some_file(100_000);
    let sent = alice.send_file(&id(&bob), &path, "a.bin", "application/octet-stream").await.expect("queues");
    std::fs::write(&path, vec![0u8; 100_000]).expect("the file changes after the offer");

    net.set_direct(true);
    alice.retry_now().await.expect("retries");
    until("bob gives up on it", || async { bob.store().file(&sent).await.unwrap().is_some_and(|file| file.failed) }).await;
    let received = file_of(&bob, &sent).await;
    assert!(!received.complete);
    assert!(!bob.file_path(&received).exists(), "the bad bytes are deleted");
    assert!(!file_of(&alice, &sent).await.complete);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_gone_from_the_sender_fails_on_both_sides() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    net.set_direct(false);
    let (path, _) = some_file(100_000);
    let sent = alice.send_file(&id(&bob), &path, "beach.jpg", "image/jpeg").await.expect("queues");
    std::fs::remove_file(&path).expect("the file is deleted after the offer");

    net.set_direct(true);
    alice.retry_now().await.expect("retries");
    until("bob gives up on it", || async { bob.store().file(&sent).await.unwrap().is_some_and(|file| file.failed) }).await;
    assert!(!bob.file_path(&file_of(&bob, &sent).await).exists(), "nothing half-written is left");
    until("alice knows it failed", || async { file_of(&alice, &sent).await.failed }).await;
    assert!(!file_of(&alice, &sent).await.complete);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_empty_file_arrives_at_once() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    let (path, _) = some_file(0);
    let sent = alice.send_file(&id(&bob), &path, "empty.txt", "text/plain").await.expect("offers");
    until("bob has it", || async { bob.store().file(&sent).await.unwrap().is_some_and(|file| file.complete) }).await;
    assert_eq!(std::fs::read(bob.file_path(&file_of(&bob, &sent).await)).unwrap(), Vec::<u8>::new());
}

async fn next_call(events: &mut tokio::sync::broadcast::Receiver<Event>) -> (String, String, CallUpdate) {
    let wait = async {
        loop {
            if let Ok(Event::Call { contact, call, update }) = events.recv().await {
                return (contact, call, update);
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(5), wait).await.expect("a call event in time")
}

async fn outcome_of(core: &Core, call: &str) -> Option<CallOutcome> {
    core.store().call(call).await.expect("reads").expect("logged").outcome
}

#[tokio::test(flavor = "multi_thread")]
async fn a_call_is_offered_answered_and_hung_up() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    let (mut at_alice, mut at_bob) = (alice.events(), bob.events());

    let call = alice.place_call(&id(&bob), true).await.expect("places");
    alice.offer_call(&call, "offer-sdp").await.expect("offers");
    let (from, ringing, update) = next_call(&mut at_bob).await;
    assert_eq!((from.as_str(), ringing.as_str()), (id(&alice).as_str(), call.as_str()));
    assert_eq!(update, CallUpdate::Incoming { video: true, sdp: "offer-sdp".to_owned() });

    bob.answer_call(&call, "answer-sdp").await.expect("answers");
    assert_eq!(next_call(&mut at_alice).await.2, CallUpdate::Answered { sdp: "answer-sdp".to_owned() });

    alice.end_call(&call, false).await.expect("hangs up");
    assert_eq!(next_call(&mut at_bob).await.2, CallUpdate::Ended { outcome: CallOutcome::Answered });
    for (core, outgoing) in [(&alice, true), (&bob, false)] {
        let logged = core.store().call(&call).await.unwrap().expect("logged");
        assert_eq!((logged.outgoing, logged.video, logged.outcome), (outgoing, true, Some(CallOutcome::Answered)));
        assert!(logged.answered_at.is_some() && logged.ended_at.is_some());
    }
    assert_eq!(net.mailbox_len(&id(&bob)) + net.mailbox_len(&id(&alice)), 0, "calls never use the mailbox");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_declined_call_is_logged_on_both_sides() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    let (mut at_alice, mut at_bob) = (alice.events(), bob.events());
    let call = alice.place_call(&id(&bob), false).await.unwrap();
    alice.offer_call(&call, "offer").await.unwrap();
    next_call(&mut at_bob).await;

    bob.end_call(&call, false).await.expect("declines");
    assert_eq!(next_call(&mut at_alice).await.2, CallUpdate::Ended { outcome: CallOutcome::Declined });
    assert_eq!(outcome_of(&bob, &call).await, Some(CallOutcome::Declined));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_call_given_up_by_the_caller_is_missed() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    let mut at_bob = bob.events();
    let call = alice.place_call(&id(&bob), false).await.unwrap();
    alice.offer_call(&call, "offer").await.unwrap();
    next_call(&mut at_bob).await;

    alice.end_call(&call, false).await.expect("cancels");
    assert_eq!(next_call(&mut at_bob).await.2, CallUpdate::Ended { outcome: CallOutcome::Missed });
    assert_eq!(outcome_of(&alice, &call).await, Some(CallOutcome::Cancelled));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unreachable_contact_cannot_be_called() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    net.set_direct(false);
    let mut at_alice = alice.events();
    let call = alice.place_call(&id(&bob), true).await.unwrap();
    alice.offer_call_within(&call, "offer", Duration::from_millis(300)).await.expect("tries");
    assert_eq!(next_call(&mut at_alice).await.2, CallUpdate::Ended { outcome: CallOutcome::Unreachable });
    assert_eq!(net.mailbox_len(&id(&bob)), 0);
    assert!(bob.store().call(&call).await.unwrap().is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_busy_contact_says_so() {
    let net = Net::new();
    let (alice, bob, carol) = (device(&net, "Alice").await, device(&net, "Bob").await, device(&net, "Carol").await);
    pair(&alice, &bob).await;
    pair(&carol, &bob).await;
    let (mut at_bob, mut at_carol) = (bob.events(), carol.events());
    let first = alice.place_call(&id(&bob), false).await.unwrap();
    alice.offer_call(&first, "offer").await.unwrap();
    next_call(&mut at_bob).await;

    let second = carol.place_call(&id(&bob), false).await.unwrap();
    carol.offer_call(&second, "offer").await.unwrap();
    assert_eq!(next_call(&mut at_carol).await.2, CallUpdate::Ended { outcome: CallOutcome::Busy });
    until("bob logged the missed call", || async {
        bob.store().call(&second).await.unwrap().is_some_and(|call| call.outcome == Some(CallOutcome::Missed))
    })
    .await;
    assert_eq!(bob.store().call(&first).await.unwrap().unwrap().outcome, None, "the first call goes on");
    let (_, refused, update) = next_call(&mut at_bob).await;
    assert_eq!(refused, second);
    assert_eq!(update, CallUpdate::MissedWhileBusy, "only the history changes");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_free_days_count_from_the_install() {
    let net = Net::new();
    let path = std::env::temp_dir().join(format!("ft-core-free-{}.db", ft_protocol::MessageId::new()));
    let free_days = 15 * 24 * 3600 * 1000_i64;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    let first = {
        let alice = device_with(&net, "Alice", Store::open(&path).await.unwrap(), [1; 32]).await;
        alice.free_until().await.expect("reads")
    };
    assert!((first - (now + free_days)).abs() < 60_000, "15 days from now");
    let alice = device_with(&net, "Alice", Store::open(&path).await.unwrap(), [1; 32]).await;
    assert_eq!(alice.free_until().await.unwrap(), first);
    let _ = std::fs::remove_file(&path);
}

async fn move_update(events: &mut tokio::sync::broadcast::Receiver<Event>, wanted: MoveUpdate) {
    let wait = async {
        loop {
            if let Ok(Event::Move(update)) = events.recv().await {
                if update == wanted {
                    return;
                }
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(20), wait).await.unwrap_or_else(|_| panic!("no {wanted:?} in time"));
}

#[tokio::test(flavor = "multi_thread")]
async fn moving_hands_the_whole_identity_to_the_new_phone() {
    let net = Net::new();
    let dir = scratch("move");
    let old = device_with(&net, "Alice", Store::open(&dir.join("old.db")).await.unwrap(), [1; 32]).await;
    old.set_move_dir(dir.join("old-move"));
    let bob = device(&net, "Bob").await;
    pair(&old, &bob).await;
    old.send_text(&id(&bob), "before moving").await.unwrap();
    until("bob has it", || async { texts(&bob, &id(&old)).await.len() == 1 }).await;

    let new = device_with(&net, "", Store::open(&dir.join("new.db")).await.unwrap(), [2; 32]).await;
    new.set_move_dir(dir.join("new-move"));
    let (mut at_old, mut at_new) = (old.events(), new.events());
    let invite = new.invite_move().await.expect("invites");
    old.move_to(&invite).await.expect("starts the move");
    move_update(&mut at_new, MoveUpdate::Received).await;
    move_update(&mut at_old, MoveUpdate::Sent).await;
    assert_eq!(net.mailbox_len(&id(&new)), 0, "nothing of it through the mailbox");

    let (copy, key) = received_move(&dir.join("new-move")).expect("a copy is ready");
    let moved = device_with(&net, "Alice", Store::open(&copy).await.unwrap(), key).await;
    assert_eq!(moved.device_id(), old.device_id());
    assert_eq!(moved.store().contact(&id(&bob)).await.unwrap().expect("bob came along").name, "Bob");
    assert_eq!(texts(&moved, &id(&bob)).await, ["before moving"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_move_without_the_secret_is_ignored() {
    let net = Net::new();
    let dir = scratch("move-forged");
    let old = device_with(&net, "Mallory", Store::open(&dir.join("old.db")).await.unwrap(), [1; 32]).await;
    old.set_move_dir(dir.join("old-move"));
    let new = device_with(&net, "", Store::open(&dir.join("new.db")).await.unwrap(), [2; 32]).await;
    new.set_move_dir(dir.join("new-move"));
    let invite = new.invite_move().await.expect("invites");
    let mut forged = ft_contacts::MoveInvite::from_link(&invite).unwrap();
    forged.secret = [0; 32];

    old.move_to(&forged.to_link()).await.expect("tries");
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(received_move(&dir.join("new-move")).is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn files_are_found_after_the_app_folder_moves() {
    let net = Net::new();
    let dir = scratch("folder-moves");
    let (before, after) = (dir.join("before"), dir.join("after"));
    let store_path = dir.join("alice.db");
    let bob = device(&net, "Bob").await;
    let sent = {
        let alice = device_in(&net, "Alice", Store::open(&store_path).await.unwrap(), [1; 32], before.clone()).await;
        pair(&alice, &bob).await;
        net.set_direct(false);
        let upload = before.join("outgoing").join("upload-1");
        std::fs::create_dir_all(upload.parent().unwrap()).unwrap();
        std::fs::write(&upload, vec![7u8; 60_000]).unwrap();
        alice.send_file(&id(&bob), &upload, "voice.m4a", "audio/mp4").await.expect("offers")
    };
    std::fs::rename(&before, &after).expect("the folder moves");

    let alice = device_in(&net, "Alice", Store::open(&store_path).await.unwrap(), [1; 32], after.clone()).await;
    net.set_direct(true);
    alice.retry_now().await.expect("retries");
    until_within("bob has it", Duration::from_secs(20), || async {
        bob.store().file(&sent).await.unwrap().is_some_and(|file| file.complete)
    })
    .await;
    assert_eq!(std::fs::read(alice.file_path(&file_of(&alice, &sent).await)).unwrap().len(), 60_000);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_call_waits_for_the_contact_to_wake_up() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    net.set_direct(false);
    let mut at_bob = bob.events();
    let call = alice.place_call(&id(&bob), false).await.unwrap();
    let calling = {
        let (alice, call) = (alice.clone(), call.clone());
        tokio::spawn(async move { alice.offer_call_within(&call, "offer", Duration::from_secs(10)).await })
    };
    tokio::time::sleep(Duration::from_millis(1500)).await;
    net.set_direct(true);
    let (_, ringing, update) = tokio::time::timeout(Duration::from_secs(8), next_call(&mut at_bob)).await.expect("bob rings");
    assert_eq!(ringing, call);
    assert!(matches!(update, CallUpdate::Incoming { .. }));
    calling.await.unwrap().expect("offered");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_call_given_up_while_waiting_stops_trying() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    net.set_direct(false);
    let mut at_bob = bob.events();
    let call = alice.place_call(&id(&bob), false).await.unwrap();
    let tried = net.tried(&id(&bob));
    let calling = {
        let (alice, call) = (alice.clone(), call.clone());
        tokio::spawn(async move { alice.offer_call_within(&call, "offer", Duration::from_secs(10)).await })
    };
    until("alice tried to reach bob", || async { net.tried(&id(&bob)) > tried }).await;
    alice.end_call(&call, false).await.expect("gives up");
    tokio::time::timeout(Duration::from_secs(5), calling).await.expect("stops trying").unwrap().expect("ends quietly");
    net.set_direct(true);
    until("bob heard the hang-up", || async {
        bob.store().call(&call).await.unwrap().is_some_and(|record| record.ended_at.is_some())
    })
    .await;
    while let Ok(event) = at_bob.try_recv() {
        if let Event::Call { update: CallUpdate::Incoming { .. }, .. } = event {
            panic!("bob never rang");
        }
    }
    let record = bob.store().call(&call).await.unwrap().expect("logged");
    assert_eq!((record.answered_at, record.outcome), (None, Some(CallOutcome::Missed)));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_contact_can_have_their_history_expire() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;

    let (path, _) = some_file(1_000);
    let sent = alice.send_file(&id(&bob), &path, "a.bin", "application/octet-stream").await.expect("queues");
    until("bob has the file", || async {
        bob.store().file(&sent).await.unwrap().is_some_and(|file| file.complete)
    })
    .await;
    alice.send_text(&id(&bob), "old news").await.expect("sends");
    until("bob got it", || async { texts(&bob, &id(&alice)).await.contains(&"old news".to_owned()) }).await;

    let file = file_of(&bob, &sent).await;
    let kept = bob.file_path(&file);
    assert!(kept.exists(), "the bytes are on the phone");

    bob.set_history(&id(&alice), 86_400, 0).await.expect("sets the rule");
    let contact = bob.store().contact(&id(&alice)).await.unwrap().unwrap();
    assert_eq!((contact.keep_for, contact.burn_after_read), (86_400, 0));

    bob.sweep_history_at(now() + 2 * 86_400_000).await.expect("sweeps");
    assert!(texts(&bob, &id(&alice)).await.is_empty(), "the conversation is gone");
    assert!(!kept.exists(), "and so are the bytes it kept");
    assert!(!texts(&alice, &id(&bob)).await.is_empty(), "only this phone forgets");
}

#[tokio::test(flavor = "multi_thread")]
async fn erasing_a_message_takes_its_file_with_it() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    let (path, _) = some_file(1_000);

    let sent = alice.send_file(&id(&bob), &path, "holiday.jpg", "image/jpeg").await.expect("offers");
    assert_eq!(texts(&alice, &id(&bob)).await.len(), 1);

    alice.forget_message(&sent).await.expect("erases");
    assert!(texts(&alice, &id(&bob)).await.is_empty());
    assert!(!path.exists(), "the bytes go too");
    assert!(alice.store().outbox().await.unwrap().is_empty(), "and it stops being sent");

    assert!(alice.forget_message(&sent).await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_message_can_be_sent_on_to_someone_else() {
    let net = Net::new();
    let (alice, bob, carol) = (device(&net, "Alice").await, device(&net, "Bob").await, device(&net, "Carol").await);
    pair(&alice, &bob).await;
    pair(&alice, &carol).await;

    let sent = alice.send_text(&id(&bob), "look at this").await.expect("sends");
    let again = alice.forward(&sent, &id(&carol)).await.expect("forwards");
    assert_ne!(again, sent, "it is a message of its own");
    assert_eq!(texts(&alice, &id(&carol)).await, ["look at this"]);
    until("carol has it", || async { texts(&carol, &id(&alice)).await == ["look at this"] }).await;

    let (path, bytes) = some_file(2_000);
    let file = alice.send_file(&id(&bob), &path, "note.bin", "application/octet-stream").await.expect("offers");
    let on = alice.forward(&file, &id(&carol)).await.expect("forwards the file");
    until("carol has the whole file", || async {
        carol.store().file(&on).await.unwrap().is_some_and(|one| one.complete)
    })
    .await;
    let arrived = file_of(&carol, &on).await;
    assert_eq!(std::fs::read(carol.file_path(&arrived)).expect("reads"), bytes);
    assert_eq!(arrived.name, "note.bin");

    let arriving = carol.store().file(&on).await.unwrap();
    if arriving.is_some_and(|one| !one.complete) {
        assert!(carol.forward(&on, &id(&alice)).await.is_err());
    }
    assert!(alice.forward("not a message", &id(&carol)).await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_forwarded_file_keeps_its_bytes_while_a_message_points_to_them() {
    let net = Net::new();
    let (alice, bob, carol) = (device(&net, "Alice").await, device(&net, "Bob").await, device(&net, "Carol").await);
    pair(&alice, &bob).await;
    pair(&alice, &carol).await;
    let (path, bytes) = some_file(1_000);
    let sent = alice.send_file(&id(&bob), &path, "holiday.jpg", "image/jpeg").await.expect("offers");
    let on = alice.forward(&sent, &id(&carol)).await.expect("forwards");
    assert_eq!(alice.file_path(&file_of(&alice, &on).await), alice.file_path(&file_of(&alice, &sent).await), "the same bytes");

    alice.forget_message(&on).await.expect("erases the forward");
    assert_eq!(std::fs::read(alice.file_path(&file_of(&alice, &sent).await)).expect("the first still opens"), bytes);

    alice.forget_message(&sent).await.expect("erases the first");
    assert!(!path.exists(), "nothing points to the bytes any more");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removed_session_leaves_the_bytes_a_message_outside_it_shares() {
    let net = Net::new();
    let (alice, bob, carol) = (device(&net, "Alice").await, device(&net, "Bob").await, device(&net, "Carol").await);
    pair(&alice, &bob).await;
    let session = alice.open_session("246810").await.expect("opens").expect("a session");
    let link = carol.my_card().await.expect("card").to_link();
    alice.add_contact_in(&link, None, Some(&session)).await.expect("adds carol inside");
    let (path, bytes) = some_file(1_000);
    let sent = alice.send_file(&id(&bob), &path, "holiday.jpg", "image/jpeg").await.expect("offers");
    alice.forward(&sent, &id(&carol)).await.expect("forwards inside the session");

    alice.remove_session(&session).await.expect("removes");
    assert_eq!(std::fs::read(alice.file_path(&file_of(&alice, &sent).await)).expect("still opens"), bytes);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_expired_history_leaves_the_bytes_a_forward_shares() {
    let net = Net::new();
    let (alice, bob, carol) = (device(&net, "Alice").await, device(&net, "Bob").await, device(&net, "Carol").await);
    pair(&alice, &bob).await;
    pair(&alice, &carol).await;
    let (path, bytes) = some_file(1_000);
    let sent = alice.send_file(&id(&bob), &path, "holiday.jpg", "image/jpeg").await.expect("offers");
    let on = alice.forward(&sent, &id(&carol)).await.expect("forwards");
    until("bob has the file", || async { bob.store().file(&sent).await.unwrap().is_some_and(|file| file.complete) }).await;
    until("it left alice's outbox", || async { alice.store().outbox().await.unwrap().iter().all(|entry| entry.message_id != sent) }).await;

    alice.set_history(&id(&bob), 86_400, 0).await.expect("a day of history with bob");
    alice.sweep_history_at(now() + 2 * 86_400_000).await.expect("sweeps");
    assert!(alice.store().file(&sent).await.unwrap().is_none(), "bob's conversation is gone");
    assert_eq!(std::fs::read(alice.file_path(&file_of(&alice, &on).await)).expect("the forward still opens"), bytes);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pin_opens_a_session_and_the_same_pin_opens_the_same_one() {
    let net = Net::new();
    let bob = device(&net, "Bob").await;

    let friends = bob.open_session("246810").await.expect("opens").expect("a session");
    assert_eq!(bob.open_session("246810").await.expect("opens again"), Some(friends.clone()));
    assert_eq!(bob.open_sessions(), vec![friends.clone()]);
    let work = bob.open_session("111222").await.expect("opens another").expect("a session");
    assert_ne!(work, friends);
    assert_eq!(bob.open_sessions().len(), 2);

    bob.close_session(&friends).await.expect("closes");
    assert_eq!(bob.open_sessions(), vec![work]);
    assert!(bob.open_session("12345").await.is_err(), "six digits, nothing else");
    assert!(bob.open_session("12345a").await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_contact_added_inside_a_session_stays_out_of_the_main_list() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    let friends = bob.open_session("246810").await.expect("opens").expect("a session");
    let link = alice.my_card().await.expect("card").to_link();
    bob.add_contact_in(&link, None, Some(&friends)).await.expect("adds alice in friends");
    until("alice knows bob", || async { alice.store().contact(&id(&bob)).await.unwrap().is_some() }).await;

    assert!(bob.store().conversations().await.unwrap().is_empty(), "the main list shows nothing");
    let hidden = bob.store().session_conversations(&friends).await.unwrap();
    assert_eq!(hidden.len(), 1);
    assert_eq!(hidden[0].contact.device_id, id(&alice));
    assert_eq!(bob.store().contact(&id(&alice)).await.unwrap().unwrap().session.as_deref(), Some(friends.as_str()));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_closed_session_receives_in_silence() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    let friends = bob.open_session("246810").await.expect("opens").expect("a session");
    let link = alice.my_card().await.expect("card").to_link();
    bob.add_contact_in(&link, None, Some(&friends)).await.expect("adds");
    until("alice has bob's card", || async { alice.store().contact(&id(&bob)).await.unwrap().is_some_and(|c| c.introduced) }).await;
    bob.close_session(&friends).await.expect("closes");
    let mut at_bob = bob.events();

    let message = alice.send_text(&id(&bob), "poker on friday?").await.expect("sends");
    until("bob stored it", || async { texts(&bob, &id(&alice)).await == vec!["poker on friday?"] }).await;
    until("alice got the receipt", || async { state_of(&alice, &id(&bob), &message).await == MessageState::Delivered }).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    while let Ok(event) = at_bob.try_recv() {
        assert_ne!(event, Event::MessagesChanged { contact: id(&alice) }, "a closed session makes no noise");
    }

    bob.open_session("246810").await.expect("opens again").expect("it exists");
    let hidden = bob.store().session_conversations(&friends).await.unwrap();
    assert_eq!((hidden.len(), hidden[0].unread), (1, 1));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_closed_session_answers_calls_as_busy_and_an_open_one_rings() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    let friends = bob.open_session("246810").await.expect("opens").expect("a session");
    let link = alice.my_card().await.expect("card").to_link();
    bob.add_contact_in(&link, None, Some(&friends)).await.expect("adds");
    until("alice has bob's card", || async { alice.store().contact(&id(&bob)).await.unwrap().is_some_and(|c| c.introduced) }).await;
    bob.close_session(&friends).await.expect("closes");
    let (mut at_alice, mut at_bob) = (alice.events(), bob.events());

    let call = alice.place_call(&id(&bob), false).await.expect("places");
    alice.offer_call(&call, "offer-sdp").await.expect("offers");
    assert_eq!(next_call(&mut at_alice).await.2, CallUpdate::Ended { outcome: CallOutcome::Busy });
    until("bob logged it as missed", || async { outcome_of(&bob, &call).await == Some(CallOutcome::Missed) }).await;
    let mut refused = false;
    while let Ok(event) = at_bob.try_recv() {
        assert!(!matches!(event, Event::Call { .. }), "a closed session never rings");
        refused |= event == Event::CallRefused;
    }
    assert!(refused, "the phone hears that a call was refused");

    bob.open_session("246810").await.expect("opens again").expect("it exists");
    let second = alice.place_call(&id(&bob), false).await.expect("places");
    alice.offer_call(&second, "offer-sdp").await.expect("offers");
    assert!(matches!(next_call(&mut at_bob).await.2, CallUpdate::Incoming { .. }), "an open session rings");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_closed_session_leaves_no_calls_in_the_history() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    let friends = bob.open_session("246810").await.expect("opens").expect("a session");
    let link = alice.my_card().await.expect("card").to_link();
    bob.add_contact_in(&link, None, Some(&friends)).await.expect("adds");
    until("alice has bob's card", || async { alice.store().contact(&id(&bob)).await.unwrap().is_some_and(|c| c.introduced) }).await;
    bob.close_session(&friends).await.expect("closes");
    let mut at_alice = alice.events();

    let call = alice.place_call(&id(&bob), false).await.expect("places");
    alice.offer_call(&call, "offer-sdp").await.expect("offers");
    assert_eq!(next_call(&mut at_alice).await.2, CallUpdate::Ended { outcome: CallOutcome::Busy });
    until("bob logged it", || async { outcome_of(&bob, &call).await == Some(CallOutcome::Missed) }).await;

    assert!(bob.visible_calls(100).await.expect("lists").is_empty(), "nothing shows while the session is closed");
    bob.open_session("246810").await.expect("opens again").expect("it exists");
    assert_eq!(bob.visible_calls(100).await.expect("lists").len(), 1, "the session's history is back");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_open_session_still_notifies_and_rings_after_a_restart() {
    let net = Net::new();
    let path = scratch("open-session-restart").join("flickertalk.db");
    let alice = device(&net, "Alice").await;
    {
        let bob = device_with(&net, "Bob", Store::open(&path).await.unwrap(), [2; 32]).await;
        let work = bob.open_session("246810").await.expect("opens").expect("a session");
        let link = alice.my_card().await.expect("card").to_link();
        bob.add_contact_in(&link, None, Some(&work)).await.expect("adds");
        until("alice has bob's card", || async { alice.store().contact(&id(&bob)).await.unwrap().is_some_and(|c| c.introduced) }).await;
    }

    let bob = device_with(&net, "Bob", Store::open(&path).await.unwrap(), [2; 32]).await;
    let mut at_bob = bob.events();
    alice.send_text(&id(&bob), "still at work?").await.expect("sends");
    let heard = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if at_bob.recv().await.expect("events") == (Event::MessagesChanged { contact: id(&alice) }) {
                break;
            }
        }
    })
    .await;
    assert!(heard.is_ok(), "the open session's message makes noise");

    let call = alice.place_call(&id(&bob), false).await.expect("places");
    alice.offer_call(&call, "offer-sdp").await.expect("offers");
    assert!(matches!(next_call(&mut at_bob).await.2, CallUpdate::Incoming { .. }), "its call rings");
}

async fn a_left_session_without_the_mailbox_gets_it_later_in_silence(mailbox_off_at_sender: bool) {
    let net = Net::new();
    let path = scratch(&format!("left-no-mailbox-{mailbox_off_at_sender}")).join("flickertalk.db");
    let alice = device(&net, "Alice").await;
    let bob_id = {
        let bob = device_with(&net, "Bob", Store::open(&path).await.unwrap(), [2; 32]).await;
        let work = bob.open_session("246810").await.expect("opens").expect("a session");
        bob.add_contact_in(&alice.my_card().await.expect("card").to_link(), None, Some(&work)).await.expect("adds");
        until("alice has bob's card", || async { alice.store().contact(&id(&bob)).await.unwrap().is_some_and(|c| c.introduced) }).await;
        if mailbox_off_at_sender {
            alice.set_mailbox(false).await.expect("alice turns the mailbox off");
        } else {
            bob.set_mailbox(false).await.expect("bob turns the mailbox off");
            until("alice learns it", || async { !alice.store().contact(&id(&bob)).await.unwrap().unwrap().mailbox }).await;
        }
        assert!(!bob.close_session(&work).await.expect("bob leaves work"));
        id(&bob)
    };
    net.unreachable.lock().unwrap().insert(bob_id.clone());

    let message = alice.send_text(&bob_id, "see you on monday").await.expect("sends");
    alice.retry_now().await.expect("retries");
    assert_eq!(net.mailbox_len(&bob_id), 0, "nothing goes to the server");
    assert_eq!(state_of(&alice, &bob_id, &message).await, MessageState::Pending, "waiting for the device");
    assert_eq!(outbox_len(&alice).await, 1, "kept on alice's phone");

    let bob = device_with(&net, "Bob", Store::open(&path).await.unwrap(), [2; 32]).await;
    assert!(bob.open_sessions().is_empty(), "the session is still closed");
    let mut at_bob = bob.events();
    net.unreachable.lock().unwrap().remove(&bob_id);
    alice.retry_now().await.expect("retries");
    until("delivered", || async { state_of(&alice, &bob_id, &message).await == MessageState::Delivered }).await;
    assert_eq!(texts(&bob, &id(&alice)).await, ["see you on monday"]);
    assert_eq!(outbox_len(&alice).await, 0);
    tokio::time::sleep(Duration::from_millis(100)).await;
    while let Ok(event) = at_bob.try_recv() {
        assert_ne!(event, Event::MessagesChanged { contact: id(&alice) }, "a closed session makes no noise");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_left_session_with_the_senders_mailbox_off_gets_it_later_in_silence() {
    a_left_session_without_the_mailbox_gets_it_later_in_silence(true).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_left_session_with_the_recipients_mailbox_off_gets_it_later_in_silence() {
    a_left_session_without_the_mailbox_gets_it_later_in_silence(false).await;
}

async fn outbox_len(core: &Core) -> usize {
    core.store().outbox().await.expect("reads").len()
}

#[tokio::test(flavor = "multi_thread")]
async fn with_receipts_off_the_sender_stays_at_sent_and_stops_retrying() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    bob.set_rules(&id(&alice), ContactRules { receipts: false, ..ContactRules::default() }).await.expect("sets");

    let message = alice.send_text(&id(&bob), "did you get it?").await.expect("sends");
    until("bob stored it", || async { texts(&bob, &id(&alice)).await.contains(&"did you get it?".to_owned()) }).await;
    until("alice stopped retrying", || async { outbox_len(&alice).await == 0 }).await;
    bob.mark_read(&id(&alice)).await.expect("reads");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(state_of(&alice, &id(&bob), &message).await, MessageState::Sent);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_text_for_later_waits_for_its_time() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    let in_a_while = ft_core::now() + 90_000;
    let message = alice.schedule_text(&id(&bob), "good morning!", in_a_while, None).await.expect("schedules");
    assert_eq!(alice.store().scheduled(&id(&bob)).await.unwrap().get(&message), Some(&in_a_while));
    assert_eq!(state_of(&alice, &id(&bob), &message).await, MessageState::Pending);

    alice.retry_now().await.expect("retries");
    alice.retry_due().await.expect("retries");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(texts(&bob, &id(&alice)).await.is_empty(), "nothing before its time");
    assert_eq!(net.mailbox_len(&id(&bob)), 0);

    alice.store().reschedule(&message, 0, ft_core::now() - 1, false).await.expect("the clock moves");
    alice.retry_due().await.expect("retries");
    until("bob has it", || async { texts(&bob, &id(&alice)).await.contains(&"good morning!".to_owned()) }).await;
    until("no longer for later", || async { alice.store().scheduled(&id(&bob)).await.unwrap().is_empty() }).await;
    assert_eq!(bob.store().message(&message).await.unwrap().unwrap().sent_at, in_a_while, "stamped with its time");

    let other = alice.schedule_text(&id(&bob), "never mind", ft_core::now() + 120_000, None).await.expect("schedules");
    alice.forget_message(&other).await.expect("cancels");
    assert!(alice.store().outbox().await.unwrap().iter().all(|entry| entry.message_id != other));

    assert!(alice.schedule_text(&id(&bob), "too soon", ft_core::now() + 1_000, None).await.is_err());
    assert!(alice.schedule_text(&id(&bob), "too far", ft_core::now() + 400 * 24 * 3_600_000, None).await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_conversation_is_searched_on_this_phone() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    alice.send_text(&id(&bob), "Dinner on Friday?").await.expect("sends");
    until("bob has it", || async { !texts(&bob, &id(&alice)).await.is_empty() }).await;
    let answer = bob.send_text(&id(&alice), "friday works").await.expect("sends");
    until("alice has it", || async { texts(&alice, &id(&bob)).await.len() == 2 }).await;
    until("bob sees it delivered", || async { state_of(&bob, &id(&alice), &answer).await == MessageState::Delivered }).await;
    let before = net.sent_to(&id(&bob));

    let found = alice.search(&id(&bob), "friday").await.expect("searches");
    assert_eq!(found.iter().map(|m| m.body.as_str()).collect::<Vec<_>>(), vec!["friday works", "Dinner on Friday?"]);
    assert!(alice.search(&id(&bob), "saturday").await.expect("searches").is_empty());
    assert_eq!(net.sent_to(&id(&bob)), before, "nothing left the phone");
    assert!(alice.search("nobody", "friday").await.is_err(), "an unknown contact");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_text_is_edited_or_taken_back_on_both_phones() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    let first = alice.send_text(&id(&bob), "dinner at 7").await.expect("sends");
    let second = alice.send_text(&id(&bob), "bring wine").await.expect("sends");
    until("bob has both", || async { texts(&bob, &id(&alice)).await.len() == 2 }).await;

    alice.edit_message(&first, "dinner at 8").await.expect("edits");
    assert_eq!(alice.store().message(&first).await.unwrap().unwrap().body, "dinner at 8");
    until("bob reads the new words", || async { bob.store().message(&first).await.unwrap().unwrap().body == "dinner at 8" }).await;
    assert!(bob.store().edited(&id(&alice)).await.unwrap().contains(&first), "marked as edited");
    assert!(alice.store().edited(&id(&bob)).await.unwrap().contains(&first));

    alice.delete_for_everyone(&second).await.expect("takes back");
    assert_eq!(alice.store().message(&second).await.unwrap().unwrap().body, "");
    until("bob sees it taken back", || async { bob.store().is_deleted(&second).await.unwrap() }).await;
    assert_eq!(bob.store().message(&second).await.unwrap().unwrap().body, "", "the words are gone");
    assert!(alice.edit_message(&second, "too late").await.is_err(), "a message taken back has no words to change");

    assert!(bob.edit_message(&first, "lunch").await.is_err());
    assert!(bob.delete_for_everyone(&first).await.is_err());
    let reply = bob.send_text(&id(&alice), "ok").await.expect("sends");
    until("alice has it", || async { texts(&alice, &id(&bob)).await.contains(&"ok".to_owned()) }).await;
    assert!(alice.edit_message(&reply, "no").await.is_err(), "not her words");
    assert_eq!(bob.store().message(&reply).await.unwrap().unwrap().body, "ok");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_receiver_checks_who_edits_or_takes_back_and_when() {
    use ft_protocol::{Body, MessageId, Packet};
    const HOUR: u64 = 3_600_000;
    let net = Net::new();
    let (alice, bob, carol) = (device(&net, "Alice").await, device(&net, "Bob").await, device(&net, "Carol").await);
    pair(&alice, &bob).await;
    pair(&carol, &bob).await;
    let at = ft_core::now() as u64;
    let old = Packet::resend(MessageId::new(), at - 25 * HOUR, Body::Message { text: "old words".into(), reply_to: None });
    let recent = Packet::resend(MessageId::new(), at - 23 * HOUR, Body::Message { text: "recent words".into(), reply_to: None });
    alice.send_raw_to(&id(&bob), &old).await.expect("sends");
    alice.send_raw_to(&id(&bob), &recent).await.expect("sends");
    let alices = alice.send_text(&id(&bob), "alice's words").await.expect("sends");
    until("bob has everything", || async { texts(&bob, &id(&alice)).await.len() == 3 }).await;

    alice.send_raw_to(&id(&bob), &Packet::new(Body::Edit { of: old.id, text: "too late".into() })).await.expect("sends");
    alice.send_raw_to(&id(&bob), &Packet::new(Body::Edit { of: recent.id, text: "still in time".into() })).await.expect("sends");
    until("bob takes the edit in time", || async { bob.store().message(&recent.id.to_string()).await.unwrap().unwrap().body == "still in time" }).await;
    assert_eq!(bob.store().message(&old.id.to_string()).await.unwrap().unwrap().body, "old words", "too late to change it");

    let alices_id = MessageId::parse(&alices).unwrap();
    bob.send_raw_to(&id(&alice), &Packet::new(Body::Edit { of: alices_id, text: "forged".into() })).await.expect("sends");
    bob.send_raw_to(&id(&alice), &Packet::new(Body::Delete { of: alices_id })).await.expect("sends");
    carol.send_raw_to(&id(&bob), &Packet::new(Body::Edit { of: recent.id, text: "forged".into() })).await.expect("sends");
    carol.send_raw_to(&id(&bob), &Packet::new(Body::Delete { of: recent.id })).await.expect("sends");
    bob.send_text(&id(&alice), "after").await.expect("sends");
    carol.send_text(&id(&bob), "after").await.expect("sends");
    until("alice handled bob's", || async { texts(&alice, &id(&bob)).await.contains(&"after".to_owned()) }).await;
    until("bob handled carol's", || async { texts(&bob, &id(&carol)).await == ["after"] }).await;
    assert_eq!(alice.store().message(&alices).await.unwrap().unwrap().body, "alice's words", "bob cannot change her words");
    assert!(!alice.store().is_deleted(&alices).await.unwrap(), "nor take them back");
    assert_eq!(bob.store().message(&recent.id.to_string()).await.unwrap().unwrap().body, "still in time", "carol cannot touch alice's words");
    assert!(!bob.store().is_deleted(&recent.id.to_string()).await.unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn edits_takings_back_and_reactions_wait_for_a_contact_out_of_reach() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    let first = alice.send_text(&id(&bob), "dinner at 7").await.expect("sends");
    let second = alice.send_text(&id(&bob), "bring wine").await.expect("sends");
    until("bob has both", || async { texts(&bob, &id(&alice)).await.len() == 2 }).await;
    until("both delivered", || async { outbox_len(&alice).await == 0 }).await;

    alice.set_mailbox(false).await.expect("mailbox off");
    net.set_direct(false);
    alice.edit_message(&first, "dinner at 8").await.expect("edits");
    alice.delete_for_everyone(&second).await.expect("takes back");
    alice.react(&id(&bob), &first, Some("🍷")).await.expect("reacts");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(bob.store().message(&first).await.unwrap().unwrap().body, "dinner at 7", "bob heard nothing yet");
    assert_eq!(alice.store().update_outbox().await.unwrap().len(), 3, "all three wait on alice's phone");

    net.set_direct(true);
    alice.retry_now().await.expect("retries");
    until("bob reads the new words", || async { bob.store().message(&first).await.unwrap().unwrap().body == "dinner at 8" }).await;
    until("bob sees it taken back", || async { bob.store().is_deleted(&second).await.unwrap() }).await;
    until("bob sees the reaction", || async {
        bob.store().reactions(&id(&alice)).await.unwrap().get(&first).and_then(|r| r.theirs.clone()).as_deref() == Some("🍷")
    })
    .await;
    until("their receipts empty the queue", || async { alice.store().update_outbox().await.unwrap().is_empty() }).await;

    let stale = ft_protocol::Packet::new(ft_protocol::Body::Delete { of: ft_protocol::MessageId::parse(&first).unwrap() });
    let eight_days_ago = ft_core::now() - 8 * 24 * 3_600_000;
    alice.store().enqueue_update(&stale.id.to_string(), &id(&bob), &stale.encode(), eight_days_ago).await.expect("queues");
    let before = net.sent_to(&id(&bob));
    alice.retry_now().await.expect("retries");
    assert!(alice.store().update_outbox().await.unwrap().is_empty(), "given up");
    assert_eq!(net.sent_to(&id(&bob)), before, "and not sent");
    assert!(!bob.store().is_deleted(&first).await.unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_core_says_which_messages_have_a_change_still_waiting() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    let first = alice.send_text(&id(&bob), "dinner at 7").await.expect("sends");
    let second = alice.send_text(&id(&bob), "bring wine").await.expect("sends");
    let third = alice.send_text(&id(&bob), "see you").await.expect("sends");
    until("all delivered", || async { outbox_len(&alice).await == 0 }).await;
    assert!(alice.updates_waiting(&id(&bob)).await.unwrap().is_empty());

    alice.set_mailbox(false).await.expect("mailbox off");
    net.set_direct(false);
    alice.edit_message(&first, "dinner at 8").await.expect("edits");
    alice.delete_for_everyone(&second).await.expect("takes back");
    alice.react(&id(&bob), &third, Some("👍")).await.expect("reacts");
    let waiting = alice.updates_waiting(&id(&bob)).await.unwrap();
    assert_eq!(waiting, HashSet::from([first.clone(), second.clone()]), "the edit and the taking back, not the reaction");
    assert_eq!(state_of(&alice, &id(&bob), &first).await, MessageState::Delivered, "the message itself still says what it reached");

    let mut at_alice = alice.events();
    net.set_direct(true);
    alice.retry_now().await.expect("retries");
    until("their receipts came", || async { alice.updates_waiting(&id(&bob)).await.unwrap().is_empty() }).await;
    let mut told = false;
    for _ in 0..50 {
        match at_alice.try_recv() {
            Ok(Event::MessagesChanged { contact }) if contact == id(&bob) => {
                told = true;
                break;
            }
            Ok(_) => {}
            Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
        }
    }
    assert!(told, "the UI hears of the receipts");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_older_edit_or_reaction_arriving_later_changes_nothing() {
    use ft_protocol::{Body, MessageId, Packet};
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    let message = alice.send_text(&id(&bob), "dinner at 7").await.expect("sends");
    until("bob has it", || async { !texts(&bob, &id(&alice)).await.is_empty() }).await;
    let of = MessageId::parse(&message).unwrap();
    let a_minute_ago = ft_core::now() as u64 - 60_000;

    alice.edit_message(&message, "dinner at 9").await.expect("edits");
    alice.react(&id(&bob), &message, Some("👍")).await.expect("reacts");
    until("bob has both", || async {
        bob.store().message(&message).await.unwrap().unwrap().body == "dinner at 9"
            && bob.store().reactions(&id(&alice)).await.unwrap().get(&message).and_then(|r| r.theirs.clone()).as_deref() == Some("👍")
    })
    .await;
    alice.send_raw_to(&id(&bob), &Packet::resend(MessageId::new(), a_minute_ago, Body::Edit { of, text: "dinner at 8".into() })).await.expect("sends");
    alice.send_raw_to(&id(&bob), &Packet::resend(MessageId::new(), a_minute_ago, Body::Reaction { to: of, emoji: "😢".into() })).await.expect("sends");
    alice.send_text(&id(&bob), "after").await.expect("sends");
    until("bob handled them", || async { texts(&bob, &id(&alice)).await.contains(&"after".to_owned()) }).await;
    assert_eq!(bob.store().message(&message).await.unwrap().unwrap().body, "dinner at 9", "the newer words stay");
    assert_eq!(bob.store().reactions(&id(&alice)).await.unwrap()[&message].theirs.as_deref(), Some("👍"), "the newer emoji stays");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_message_taken_back_before_it_went_out_never_goes() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    let later = alice.schedule_text(&id(&bob), "surprise!", ft_core::now() + 90_000, None).await.expect("schedules");
    alice.set_mailbox(false).await.expect("mailbox off");
    net.set_direct(false);
    let pending = alice.send_text(&id(&bob), "are you there?").await.expect("sends");
    assert_eq!(state_of(&alice, &id(&bob), &pending).await, MessageState::Pending);

    alice.delete_for_everyone(&later).await.expect("takes back");
    assert!(alice.store().message(&later).await.unwrap().is_none(), "one that never went out leaves no mark");
    assert!(!alice.store().scheduled_all().await.unwrap().contains_key(&later), "nor waits for its time");
    assert!(alice.store().update_outbox().await.unwrap().is_empty(), "and nothing is told about it");
    alice.delete_for_everyone(&pending).await.expect("takes back");
    assert!(alice.store().outbox().await.unwrap().is_empty(), "both are cancelled");
    assert!(alice.store().is_deleted(&pending).await.unwrap(), "the pending one keeps its mark");
    assert_eq!(alice.store().update_outbox().await.unwrap().len(), 1, "and its taking back waits for bob");

    net.set_direct(true);
    alice.retry_now().await.expect("retries");
    alice.retry_due().await.expect("retries");
    until("bob's receipt empties the queue", || async { alice.store().update_outbox().await.unwrap().is_empty() }).await;
    assert!(texts(&bob, &id(&alice)).await.is_empty(), "nothing arrived, blank or not");
    assert!(bob.store().message(&pending).await.unwrap().is_none(), "not even a mark");
    assert_eq!(net.mailbox_len(&id(&bob)), 0);

    alice.store().enqueue(&pending, &id(&bob), ft_core::now()).await.expect("queued again");
    alice.retry_due().await.expect("retries");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(texts(&bob, &id(&alice)).await.is_empty(), "nothing blank arrived");
    assert!(alice.store().outbox().await.unwrap().is_empty(), "and it left the queue");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pending_text_taken_back_stops_waiting_once_the_contact_hears_of_it() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    alice.set_mailbox(false).await.expect("mailbox off");
    net.set_direct(false);
    let pending = alice.send_text(&id(&bob), "are you there?").await.expect("sends");
    alice.delete_for_everyone(&pending).await.expect("takes back");
    assert_eq!(alice.updates_waiting(&id(&bob)).await.unwrap(), HashSet::from([pending.clone()]), "waits while the taking back waits");

    net.set_direct(true);
    alice.retry_now().await.expect("retries");
    until("bob's receipt empties the queue", || async { alice.store().update_outbox().await.unwrap().is_empty() }).await;
    assert!(alice.updates_waiting(&id(&bob)).await.unwrap().is_empty());
    assert_ne!(state_of(&alice, &id(&bob), &pending).await, MessageState::Pending, "the mark no longer shows a clock");
    assert!(texts(&bob, &id(&alice)).await.is_empty(), "and bob still never had the words");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_text_taken_back_while_its_delivery_waits_never_arrives() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;

    net.set_direct(false);
    let release = net.hold_reach(&id(&bob));
    let sending = tokio::spawn({
        let (alice, bob) = (alice.clone(), id(&bob));
        async move { alice.send_text(&bob, "oops, wrong chat").await.expect("sends") }
    });
    until("the delivery waits for the connection", || async { net.holding.load(Ordering::SeqCst) }).await;
    let message = alice.store().outbox().await.unwrap()[0].message_id.clone();
    alice.delete_for_everyone(&message).await.expect("takes back");
    release.send(()).expect("lets it go");
    sending.await.expect("the send ends");
    net.collect(&bob).await;
    assert!(texts(&bob, &id(&alice)).await.is_empty(), "the mailbox never got it");
    assert!(bob.store().message(&message).await.unwrap().is_none());
    net.collect(&alice).await;
    assert!(alice.store().update_outbox().await.unwrap().is_empty());

    net.set_direct(true);
    let release = net.hold_reach(&id(&bob));
    let sending = tokio::spawn({
        let (alice, bob) = (alice.clone(), id(&bob));
        async move { alice.send_text(&bob, "oops again").await.expect("sends") }
    });
    until("the delivery waits for the connection", || async { net.holding.load(Ordering::SeqCst) }).await;
    let message = alice.store().outbox().await.unwrap()[0].message_id.clone();
    alice.delete_for_everyone(&message).await.expect("takes back");
    release.send(()).expect("lets it go");
    sending.await.expect("the send ends");
    until("bob's receipt empties the queue", || async { alice.store().update_outbox().await.unwrap().is_empty() }).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(texts(&bob, &id(&alice)).await.is_empty(), "it never went directly");
    assert!(bob.store().message(&message).await.unwrap().is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_text_taken_back_right_after_reaching_the_mailbox_ends_taken_back() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    net.set_direct(false);
    let release = net.hold_mailbox(&id(&bob));
    let sending = tokio::spawn({
        let (alice, bob) = (alice.clone(), id(&bob));
        async move { alice.send_text(&bob, "oops, wrong chat").await.expect("sends") }
    });
    until("the mailbox has it", || async { net.holding.load(Ordering::SeqCst) }).await;
    let message = alice.store().outbox().await.unwrap()[0].message_id.clone();
    assert_eq!(state_of(&alice, &id(&bob), &message).await, MessageState::Pending, "still pending here");
    alice.delete_for_everyone(&message).await.expect("takes back");
    release.send(()).expect("lets it go");
    sending.await.expect("the send ends");

    net.collect(&bob).await;
    until("bob sees it taken back", || async { bob.store().is_deleted(&message).await.unwrap() }).await;
    assert_eq!(bob.store().message(&message).await.unwrap().unwrap().body, "", "the words are gone");
}

#[tokio::test(flavor = "multi_thread")]
async fn pinning_a_message_is_this_phones_alone() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    let message = alice.send_text(&id(&bob), "the address is 12 Main St").await.expect("sends");
    until("bob has it", || async { !texts(&bob, &id(&alice)).await.is_empty() }).await;
    let mut at_alice = alice.events();
    let before = net.sent_to(&id(&bob));

    alice.pin_message(&message, true).await.expect("pins");
    assert_eq!(alice.store().pinned(&id(&bob)).await.unwrap(), vec![message.clone()]);
    assert_eq!(at_alice.recv().await.unwrap(), Event::MessagesChanged { contact: id(&bob) });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(net.sent_to(&id(&bob)), before, "nothing left the phone");
    assert!(bob.store().pinned(&id(&alice)).await.unwrap().is_empty());

    alice.pin_message(&message, false).await.expect("unpins");
    assert!(alice.store().pinned(&id(&bob)).await.unwrap().is_empty());
    assert!(alice.pin_message("no-such-message", true).await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reaction_reaches_the_other_phone_and_can_be_taken_back() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    let message = alice.send_text(&id(&bob), "look at this").await.expect("sends");
    until("bob has it", || async { texts(&bob, &id(&alice)).await.contains(&"look at this".to_owned()) }).await;

    bob.react(&id(&alice), &message, Some("👍")).await.expect("reacts");
    assert_eq!(bob.store().reactions(&id(&alice)).await.unwrap()[&message].mine.as_deref(), Some("👍"));
    until("alice sees it", || async {
        alice.store().reactions(&id(&bob)).await.unwrap().get(&message).and_then(|r| r.theirs.clone()).as_deref() == Some("👍")
    })
    .await;

    bob.react(&id(&alice), &message, Some("❤️")).await.expect("changes");
    until("alice sees the new one", || async {
        alice.store().reactions(&id(&bob)).await.unwrap().get(&message).and_then(|r| r.theirs.clone()).as_deref() == Some("❤️")
    })
    .await;
    alice.react(&id(&bob), &message, Some("😂")).await.expect("reacts to her own");
    until("bob sees hers", || async {
        bob.store().reactions(&id(&alice)).await.unwrap().get(&message).and_then(|r| r.theirs.clone()).as_deref() == Some("😂")
    })
    .await;

    bob.react(&id(&alice), &message, None).await.expect("takes back");
    until("alice sees it gone", || async { alice.store().reactions(&id(&bob)).await.unwrap().get(&message).is_none_or(|r| r.theirs.is_none()) }).await;

    assert!(bob.react(&id(&alice), &message, Some("not an emoji")).await.is_err(), "a sentence is not a reaction");
    assert!(bob.react(&id(&alice), "no-such-message", Some("👍")).await.is_err());
    let carol = device(&net, "Carol").await;
    pair(&bob, &carol).await;
    assert!(bob.react(&id(&carol), &message, Some("👍")).await.is_err(), "another conversation's message");
}

#[tokio::test(flavor = "multi_thread")]
async fn edits_and_reactions_made_while_in_the_requests_arrive() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    alice.add_contact(&bob.my_card().await.expect("card").to_link(), None).await.expect("alice adds bob");
    let message = alice.send_text(&id(&bob), "dinner at 7").await.expect("sends");
    until("bob has it, in his requests", || async { texts(&bob, &id(&alice)).await == ["dinner at 7"] }).await;
    assert!(!bob.store().contact(&id(&alice)).await.unwrap().unwrap().accepted, "alice is still a request");

    alice.edit_message(&message, "dinner at 8").await.expect("edits");
    alice.react(&id(&bob), &message, Some("👍")).await.expect("reacts");
    until("their receipts empty alice's queue", || async { alice.store().update_outbox().await.unwrap().is_empty() }).await;
    assert_eq!(bob.store().message(&message).await.unwrap().unwrap().body, "dinner at 8", "the edit was kept");
    assert_eq!(bob.store().reactions(&id(&alice)).await.unwrap().get(&message).and_then(|r| r.theirs.clone()).as_deref(), Some("👍"));

    bob.accept_contact(&id(&alice)).await.expect("bob accepts alice");
    assert_eq!(bob.store().message(&message).await.unwrap().unwrap().body, "dinner at 8");
    assert!(bob.store().edited(&id(&alice)).await.unwrap().contains(&message), "marked as edited");
    assert_eq!(bob.store().reactions(&id(&alice)).await.unwrap()[&message].theirs.as_deref(), Some("👍"));

    let carol = device(&net, "Carol").await;
    carol.add_contact(&bob.my_card().await.expect("card").to_link(), None).await.expect("carol adds bob");
    let carols = carol.send_text(&id(&bob), "hi").await.expect("sends");
    until("bob has carol's request", || async { texts(&bob, &id(&carol)).await == ["hi"] }).await;
    bob.decline_contact(&id(&carol)).await.expect("declines");
    carol.edit_message(&carols, "hello").await.expect("edits");
    carol.react(&id(&bob), &carols, Some("👍")).await.expect("reacts");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(bob.store().message(&carols).await.unwrap().unwrap().body, "hi", "a declined stranger changes nothing");
    assert!(bob.store().reactions(&id(&carol)).await.unwrap().get(&carols).is_none_or(|r| r.theirs.is_none()));
    assert_eq!(carol.store().update_outbox().await.unwrap().len(), 2, "and is never told they arrived");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_answer_arrives_with_the_message_it_answers() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;

    let question = alice.send_text(&id(&bob), "dinner on friday?").await.expect("sends");
    until("bob has the question", || async { texts(&bob, &id(&alice)).await.contains(&"dinner on friday?".to_owned()) }).await;
    let answer = bob.send_text_replying(&id(&alice), "yes!", Some(&question)).await.expect("answers");
    until("alice has the answer", || async { texts(&alice, &id(&bob)).await.contains(&"yes!".to_owned()) }).await;
    assert_eq!(alice.store().reply_to(&answer).await.unwrap(), Some(question.clone()), "alice sees what it answers");
    assert_eq!(bob.store().reply_to(&answer).await.unwrap(), Some(question.clone()), "bob keeps it too");
    assert_eq!(alice.store().reply_to(&question).await.unwrap(), None, "the question answers nothing");

    let carol = device(&net, "Carol").await;
    pair(&bob, &carol).await;
    assert!(bob.send_text_replying(&id(&carol), "yes!", Some(&question)).await.is_err(), "another conversation's message");
    assert!(bob.send_text_replying(&id(&alice), "yes!", Some("not-a-message")).await.is_err(), "a message that is not here");
}

#[tokio::test(flavor = "multi_thread")]
async fn typing_is_heard_over_the_direct_connection_and_nowhere_else() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    let mut at_bob = bob.events();

    assert!(alice.typing(&id(&bob)).await.expect("tells"), "sent over the direct link");
    let heard = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if at_bob.recv().await.expect("events") == (Event::Typing { contact: id(&alice) }) {
                break;
            }
        }
    })
    .await;
    assert!(heard.is_ok(), "bob hears alice typing");
    assert!(texts(&bob, &id(&alice)).await.is_empty(), "nothing is stored");
    assert_eq!(net.mailbox_len(&id(&bob)), 0, "nothing waits in the mailbox");

    net.set_direct(false);
    assert!(!alice.typing(&id(&bob)).await.expect("tells"), "not sent");
    assert_eq!(net.mailbox_len(&id(&bob)), 0);
    assert_eq!(outbox_len(&alice).await, 0);
    net.set_direct(true);

    alice.set_rules(&id(&bob), ContactRules { typing: false, ..ContactRules::default() }).await.expect("sets");
    let before = net.sent_to(&id(&bob));
    assert!(!alice.typing(&id(&bob)).await.expect("tells"));
    assert_eq!(net.sent_to(&id(&bob)), before, "nothing left alice's phone");
    alice.set_rules(&id(&bob), ContactRules::default()).await.expect("sets");

    bob.set_rules(&id(&alice), ContactRules { accepts_chat: false, ..ContactRules::default() }).await.expect("sets");
    while at_bob.try_recv().is_ok() {}
    assert!(alice.typing(&id(&bob)).await.expect("tells"));
    tokio::time::sleep(Duration::from_millis(200)).await;
    while let Ok(event) = at_bob.try_recv() {
        assert_ne!(event, Event::Typing { contact: id(&alice) }, "chat off hears no typing");
    }

    let carol = device(&net, "Carol").await;
    let link = bob.my_card().await.expect("card").to_link();
    carol.add_contact(&link, None).await.expect("carol adds bob");
    until("bob knows carol", || async { bob.store().contact(&id(&carol)).await.unwrap().is_some() }).await;
    assert!(!bob.typing(&id(&carol)).await.expect("tells"), "nothing to a stranger in the requests");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_receipts_default_applies_to_new_contacts() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    assert!(bob.receipts_default().await.expect("reads"));
    bob.set_receipts_default(false).await.expect("sets");
    pair(&bob, &alice).await;
    assert!(!bob.store().contact(&id(&alice)).await.unwrap().unwrap().rules.receipts);
}

#[tokio::test(flavor = "multi_thread")]
async fn with_chat_off_their_messages_are_not_kept() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    bob.set_rules(&id(&alice), ContactRules { accepts_chat: false, ..ContactRules::default() }).await.expect("sets");

    let message = alice.send_text(&id(&bob), "hello?").await.expect("sends");
    until("alice stopped retrying", || async { outbox_len(&alice).await == 0 }).await;
    assert!(texts(&bob, &id(&alice)).await.is_empty(), "nothing kept");
    assert_eq!(state_of(&alice, &id(&bob), &message).await, MessageState::Sent);
}

#[tokio::test(flavor = "multi_thread")]
async fn with_calls_off_a_call_is_busy_and_leaves_no_trace() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    pair(&alice, &bob).await;
    bob.set_rules(&id(&alice), ContactRules { accepts_calls: false, ..ContactRules::default() }).await.expect("sets");
    let (mut at_alice, mut at_bob) = (alice.events(), bob.events());

    let call = alice.place_call(&id(&bob), false).await.expect("places");
    alice.offer_call(&call, "offer-sdp").await.expect("offers");
    assert_eq!(next_call(&mut at_alice).await.2, CallUpdate::Ended { outcome: CallOutcome::Busy });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(bob.store().call(&call).await.unwrap().is_none(), "no trace in the history");
    let mut refused = false;
    while let Ok(event) = at_bob.try_recv() {
        assert!(!matches!(event, Event::Call { .. }), "it never rings");
        refused |= event == Event::CallRefused;
    }
    assert!(refused, "the phone hears that a call was refused");
}

#[tokio::test(flavor = "multi_thread")]
async fn quiet_hours_are_kept_and_checked() {
    let net = Net::new();
    let bob = device(&net, "Bob").await;
    assert_eq!(bob.quiet_hours().await.expect("reads"), None, "off by default");
    let week = r#"{"days":[{"from":"18:00","to":"22:00"},{"from":"18:00","to":"22:00"},{"from":"18:00","to":"22:00"},{"from":"18:00","to":"22:00"},{"from":"15:00","to":"22:00"},"all","all"]}"#;
    bob.set_quiet_hours(Some(week)).await.expect("keeps");
    assert!(bob.quiet_hours().await.unwrap().is_some());
    assert!(bob.set_quiet_hours(Some(r#"{"days":["all"]}"#)).await.is_err(), "seven days");
    assert!(bob.set_quiet_hours(Some(r#"{"days":[{"from":"25:00","to":"22:00"},"all","all","all","all","all","all"]}"#)).await.is_err(), "real times");
    bob.set_quiet_hours(None).await.expect("turns off");
    assert_eq!(bob.quiet_hours().await.unwrap(), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_router_always_gets_eight_capabilities() {
    let net = Net::new();
    let bob = device(&net, "Bob").await;
    let hashes = bob.route_capability_hashes().await.expect("hashes");
    assert_eq!(hashes[0], bob.route_capability().hash());
    let distinct: std::collections::HashSet<_> = hashes.iter().collect();
    assert_eq!(distinct.len(), 8);
    bob.open_session("246810").await.expect("opens");
    assert_eq!(bob.route_capability_hashes().await.expect("hashes"), hashes, "sessions change nothing the router sees");
}

#[tokio::test(flavor = "multi_thread")]
async fn sessions_take_a_slot_each_up_to_seven() {
    let net = Net::new();
    let bob = device(&net, "Bob").await;
    let mut slots = Vec::new();
    for pin in 0..7 {
        let session = bob.open_session(&format!("10000{pin}")).await.expect("opens").expect("a session");
        slots.push(bob.store().session_slot(&session).await.unwrap().expect("a slot"));
    }
    slots.sort();
    assert_eq!(slots, vec![1, 2, 3, 4, 5, 6, 7]);
    assert_eq!(bob.open_slots(), vec![1, 2, 3, 4, 5, 6, 7]);
    assert_eq!(bob.open_session("999999").await.expect("asks"), None, "an eighth has no room");
    bob.close_session(&bob.open_sessions()[0]).await.expect("closes");
    assert_eq!(bob.open_slots().len(), 6);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_contact_in_a_session_gets_the_sessions_capability() {
    let net = Net::new();
    let (alice, bob, carol) = (device(&net, "Alice").await, device(&net, "Bob").await, device(&net, "Carol").await);
    pair(&carol, &bob).await;
    let friends = bob.open_session("246810").await.expect("opens").expect("a session");
    let link = alice.my_card().await.expect("card").to_link();
    bob.add_contact_in(&link, None, Some(&friends)).await.expect("adds");
    until("alice has bob's card", || async { alice.store().contact(&id(&bob)).await.unwrap().is_some_and(|c| c.introduced) }).await;

    let at_alice = alice.peer(&id(&bob)).await.expect("peer").capability;
    let at_carol = carol.peer(&id(&bob)).await.expect("peer").capability;
    assert_eq!(at_carol.as_bytes(), bob.route_capability().as_bytes(), "the main list keeps the device's own");
    assert_ne!(at_alice.as_bytes(), bob.route_capability().as_bytes());
    let slot = bob.store().session_slot(&friends).await.unwrap().expect("slot") as usize;
    assert_eq!(at_alice.hash(), bob.route_capability_hashes().await.unwrap()[slot]);
}

#[tokio::test(flavor = "multi_thread")]
async fn scanning_a_sessions_card_puts_you_in_that_session() {
    let net = Net::new();
    let (alice, bob) = (device(&net, "Alice").await, device(&net, "Bob").await);
    let friends = bob.open_session("246810").await.expect("opens").expect("a session");
    let link = bob.my_card_in(Some(&friends)).await.expect("card").to_link();
    alice.add_contact(&link, None).await.expect("alice scans");
    until("bob knows alice", || async { bob.store().contact(&id(&alice)).await.unwrap().is_some() }).await;

    assert_eq!(bob.store().contact(&id(&alice)).await.unwrap().unwrap().session.as_deref(), Some(friends.as_str()));
    assert!(bob.store().conversations().await.unwrap().is_empty(), "not in the main list");
}

struct MemoryCloud(Arc<ft_vault::Memory>);

#[async_trait]
impl ft_core::vault::Cloud for MemoryCloud {
    async fn login(&self, _: &str, _: &str, _: &dyn ft_core::vault::Authorizer) -> anyhow::Result<ft_vault::google::Tokens> {
        Ok(ft_vault::google::Tokens { access_token: "t".into(), refresh_token: None, expires_at: i64::MAX })
    }

    fn provider(&self, _: &str, _: &str, _: Arc<dyn ft_vault::google::TokenKeeper>) -> anyhow::Result<Arc<dyn ft_vault::Provider>> {
        Ok(self.0.clone())
    }
}

struct Browser;

#[async_trait]
impl ft_core::vault::Authorizer for Browser {
    async fn authorize(&self, _: &str, scheme: &str) -> anyhow::Result<String> {
        Ok(format!("{scheme}:/oauth2redirect?code=c"))
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn after_a_restore_both_sides_read_each_other_again() {
    let net = Net::new();
    let dir = scratch("restore");
    let cloud = ft_vault::Memory::new();
    let alice = device_with(&net, "Alice", Store::open(&dir.join("alice.db")).await.unwrap(), [1; 32]).await;
    alice.set_vault_dir(dir.join("alice-vault"));
    alice.set_cloud(Arc::new(MemoryCloud(cloud.clone())));
    let bob = device(&net, "Bob").await;
    let (alice_id, bob_id) = (id(&alice), id(&bob));
    pair(&alice, &bob).await;
    alice.send_text(&bob_id, "before the backup").await.unwrap();
    bob.send_text(&alice_id, "hi").await.unwrap();
    until("both have both", || async { texts(&alice, &bob_id).await.len() == 2 && texts(&bob, &alice_id).await.len() == 2 }).await;

    alice.vault_connect("memory", &Browser).await.unwrap();
    alice.vault_setup("a long phrase of mine").await.unwrap();
    alice.vault_backup().await.unwrap();

    for round in 0..2 {
        alice.send_text(&bob_id, &format!("alice after the backup {round}")).await.unwrap();
        bob.send_text(&alice_id, &format!("bob after the backup {round}")).await.unwrap();
    }
    until("the lost phone had it all", || async { texts(&alice, &bob_id).await.len() == 6 && texts(&bob, &alice_id).await.len() == 6 }).await;
    drop(alice);

    let fresh = device_with(&net, "", Store::open(&dir.join("fresh.db")).await.unwrap(), [2; 32]).await;
    fresh.set_move_dir(dir.join("fresh-move"));
    fresh.set_vault_dir(dir.join("fresh-vault"));
    fresh.set_cloud(Arc::new(MemoryCloud(cloud.clone())));
    fresh.vault_connect("memory", &Browser).await.unwrap();
    fresh.vault_unlock("a long phrase of mine").await.unwrap();
    fresh.vault_restore().await.unwrap();
    drop(fresh);
    let (copy, key) = received_move(&dir.join("fresh-move")).expect("the backup is ready to swap in");
    let restored = device_with(&net, "Alice", Store::open(&copy).await.unwrap(), key).await;
    assert_eq!(id(&restored), alice_id);
    assert_eq!(texts(&restored, &bob_id).await.len(), 2, "the history as it was backed up");

    assert!(restored.card_stale().await.unwrap(), "a restored phone renews its sessions and says so");
    restored.reintroduce().await.unwrap();

    restored.send_text(&bob_id, "I am back").await.unwrap();
    until("bob reads the restored phone", || async { texts(&bob, &alice_id).await.contains(&"I am back".to_owned()) }).await;
    bob.send_text(&alice_id, "welcome back").await.unwrap();
    until("the restored phone reads bob", || async { texts(&restored, &bob_id).await.contains(&"welcome back".to_owned()) }).await;
    for round in 0..2 {
        restored.send_text(&bob_id, &format!("again {round}")).await.unwrap();
        bob.send_text(&alice_id, &format!("still {round}")).await.unwrap();
    }
    until("both ways, in step", || async {
        let (at_bob, at_alice) = (texts(&bob, &alice_id).await, texts(&restored, &bob_id).await);
        at_bob.contains(&"again 1".to_owned()) && at_alice.contains(&"still 1".to_owned())
    })
    .await;
}
