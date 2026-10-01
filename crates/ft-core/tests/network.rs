//! Two complete devices connected by real WebRTC data channels (loopback), with a fake router in
//! memory for signalling and the mailbox (Plan §13–19, §106 M2).

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use ft_core::net::{Network, Relay};
use ft_core::{Core, Event};
use ft_push::{RouterEvent, Signalled, TurnGrant};
use ft_storage::{MessageState, Store};
use ft_webrtc::SessionConfig;
use tokio::sync::mpsc;

/// Each device's mailbox: (id, blob).
type Mailboxes = HashMap<String, Vec<(String, Vec<u8>)>>;

/// The fake router: who is connected, their capabilities and their mailboxes.
#[derive(Default)]
struct Bus {
    online: Mutex<HashMap<String, mpsc::UnboundedSender<RouterEvent>>>,
    capabilities: Mutex<HashMap<String, [u8; 32]>>,
    mail: Mutex<Mailboxes>,
    signals: AtomicUsize,
    /// Signals the caller marked as a call (2026-09-28), which ring an iPhone.
    call_signals: AtomicUsize,
    /// Call-marked signals for a device that was not connected: each one a ring (PushKit on an
    /// iPhone, `t: call` on Android). A call-marked signal to a connected device rings nothing.
    rings: AtomicUsize,
    next_id: AtomicUsize,
    /// Router 0.4.0 (2026-09-29): a signal for a device that is not connected waits for its next
    /// connection, handed over right after the welcome. Off: a router before 0.4.
    retaining: std::sync::atomic::AtomicBool,
    held: Mutex<HashMap<String, Vec<Vec<u8>>>>,
    /// The offers and answers sent to each device, connected or not.
    signalled: Mutex<HashMap<String, usize>>,
    /// While set, collecting finds the mailbox empty (the recipient has not got round to it).
    mail_held: std::sync::atomic::AtomicBool,
    /// While set, what waited for a device is not handed over when it connects (`release`).
    signals_held: std::sync::atomic::AtomicBool,
}

impl Bus {
    fn allowed(&self, to: &str, capability: &[u8; 32]) -> anyhow::Result<()> {
        match self.capabilities.lock().unwrap().get(to) {
            Some(expected) if expected == capability => Ok(()),
            _ => anyhow::bail!("wrong capability"),
        }
    }

    fn mail_for(&self, device: &str) -> usize {
        self.mail.lock().unwrap().get(device).map_or(0, Vec::len)
    }

    fn signalled_to(&self, device: &str) -> usize {
        self.signalled.lock().unwrap().get(device).copied().unwrap_or(0)
    }
}

struct FakeRelay {
    bus: Arc<Bus>,
    me: OnceLock<String>,
}

#[async_trait]
impl Relay for FakeRelay {
    async fn signal(&self, to: &str, capability: &[u8; 32], bytes: Vec<u8>) -> anyhow::Result<bool> {
        Ok(self.signal_as(to, capability, bytes, false).await? == Signalled::Delivered)
    }

    async fn signal_call(&self, to: &str, capability: &[u8; 32], bytes: Vec<u8>) -> anyhow::Result<bool> {
        Ok(self.signal_as(to, capability, bytes, true).await? == Signalled::Delivered)
    }

    async fn signal_as(&self, to: &str, capability: &[u8; 32], bytes: Vec<u8>, call: bool) -> anyhow::Result<Signalled> {
        if call {
            self.bus.call_signals.fetch_add(1, Ordering::SeqCst);
        }
        self.bus.allowed(to, capability)?;
        self.bus.signals.fetch_add(1, Ordering::SeqCst);
        *self.bus.signalled.lock().unwrap().entry(to.to_owned()).or_default() += 1;
        let online = self.bus.online.lock().unwrap().get(to).cloned();
        if let Some(device) = online {
            if device.send(RouterEvent::Signal(bytes.clone())).is_ok() {
                return Ok(Signalled::Delivered);
            }
        }
        if call {
            self.bus.rings.fetch_add(1, Ordering::SeqCst);
        }
        if self.bus.retaining.load(Ordering::SeqCst) {
            self.bus.held.lock().unwrap().entry(to.to_owned()).or_default().push(bytes);
            return Ok(Signalled::Retained);
        }
        Ok(Signalled::NotConnected)
    }

    async fn deposit(&self, to: &str, capability: &[u8; 32], blob: Vec<u8>) -> anyhow::Result<()> {
        self.bus.allowed(to, capability)?;
        let id = self.bus.next_id.fetch_add(1, Ordering::SeqCst).to_string();
        self.bus.mail.lock().unwrap().entry(to.to_owned()).or_default().push((id, blob));
        if let Some(device) = self.bus.online.lock().unwrap().get(to) {
            let _ = device.send(RouterEvent::Mail);
        }
        Ok(())
    }

    async fn collect(&self) -> anyhow::Result<Vec<(String, Vec<u8>)>> {
        if self.bus.mail_held.load(Ordering::SeqCst) {
            return Ok(Vec::new());
        }
        Ok(self.bus.mail.lock().unwrap().get(self.me.get().unwrap()).cloned().unwrap_or_default())
    }

    async fn acknowledge(&self, id: &str) -> anyhow::Result<()> {
        if let Some(mail) = self.bus.mail.lock().unwrap().get_mut(self.me.get().unwrap()) {
            mail.retain(|(stored, _)| stored != id);
        }
        Ok(())
    }
}

struct Phone {
    core: Arc<Core>,
    network: Arc<Network>,
}

impl Phone {
    fn id(&self) -> String {
        self.core.device_id().as_str().to_owned()
    }

    /// Connects to the fake router, as the app does at start.
    fn go_online(&self, bus: &Arc<Bus>) {
        self.go_online_with(bus, vec![], None);
    }

    /// Online, with the STUN and TURN servers the router's welcome names.
    fn go_online_with(&self, bus: &Arc<Bus>, stun: Vec<String>, turn: Option<TurnGrant>) {
        let (events, mut incoming) = mpsc::unbounded_channel();
        bus.online.lock().unwrap().insert(self.id(), events.clone());
        let network = self.network.clone();
        tokio::spawn(async move {
            while let Some(event) = incoming.recv().await {
                network.handle(event).await;
            }
        });
        let _ = events.send(RouterEvent::Connected { stun, turn });
        // Router 0.4.0: what waited for this device comes right after the welcome.
        if !bus.signals_held.load(Ordering::SeqCst) {
            self.release(bus);
        }
    }

    /// Hands over what waited at the router for this device.
    fn release(&self, bus: &Arc<Bus>) {
        let Some(events) = bus.online.lock().unwrap().get(&self.id()).cloned() else { return };
        for bytes in bus.held.lock().unwrap().remove(&self.id()).unwrap_or_default() {
            let _ = events.send(RouterEvent::Signal(bytes));
        }
    }

    /// The router tells it mail arrived (it collects it).
    fn mail_notice(&self, bus: &Arc<Bus>) {
        if let Some(events) = bus.online.lock().unwrap().get(&self.id()) {
            let _ = events.send(RouterEvent::Mail);
        }
    }

    /// Its app closed: no longer connected to the router, and its direct connections gone.
    async fn go_offline(&self, bus: &Arc<Bus>, contact: &Phone) {
        bus.online.lock().unwrap().remove(&self.id());
        self.network.disconnect(&contact.id()).await;
        contact.network.disconnect(&self.id()).await;
    }
}

async fn phone(bus: &Arc<Bus>, name: &str) -> Phone {
    let relay = Arc::new(FakeRelay { bus: bus.clone(), me: OnceLock::new() });
    let network = Network::new(relay.clone(), SessionConfig::offline());
    let core = Core::open(Store::open_in_memory().await.unwrap(), [4; 32], network.clone()).await.expect("opens");
    core.set_name(name).await.unwrap();
    let files = std::env::temp_dir().join(format!("ft-net-{name}-{}", ft_protocol::MessageId::new()));
    std::fs::create_dir_all(&files).unwrap();
    core.set_files_dir(files);
    relay.me.set(core.device_id().as_str().to_owned()).unwrap();
    network.attach(&core);
    bus.capabilities.lock().unwrap().insert(core.device_id().as_str().to_owned(), *core.route_capability().as_bytes());
    Phone { core, network }
}

async fn until<F, Fut>(what: &str, condition: F)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..300 {
        if condition().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting until {what}");
}

async fn pair(alice: &Phone, bob: &Phone) {
    let link = bob.core.my_card().await.unwrap().to_link();
    alice.core.add_contact(&link, None).await.expect("alice adds bob");
    let id = alice.id();
    until("bob knows alice", || async { bob.core.store().contact(&id).await.unwrap().is_some() }).await;
    // Alice wrote first: Bob accepts her request (A5).
    bob.core.accept_contact(&id).await.expect("bob accepts alice");
}

async fn texts(phone: &Phone, contact: &str) -> Vec<String> {
    phone.core.store().messages(contact, 100).await.unwrap().into_iter().map(|m| m.body).collect()
}

async fn state(phone: &Phone, contact: &str, id: &str) -> MessageState {
    phone.core.store().message(id).await.unwrap().filter(|m| m.contact == contact).expect("message").state
}

#[tokio::test(flavor = "multi_thread")]
async fn messages_cross_a_real_data_channel() {
    let bus = Arc::new(Bus::default());
    let (alice, bob) = (phone(&bus, "Alice").await, phone(&bus, "Bob").await);
    alice.go_online(&bus);
    bob.go_online(&bus);
    pair(&alice, &bob).await;

    let sent = alice.core.send_text(&bob.id(), "over webrtc").await.expect("sends");
    until("bob has it", || async { texts(&bob, &alice.id()).await == ["over webrtc"] }).await;
    until("alice sees it delivered", || async { state(&alice, &bob.id(), &sent).await == MessageState::Delivered }).await;
    assert_eq!(bus.mail_for(&bob.id()) + bus.mail_for(&alice.id()), 0, "nothing went through the mailbox");
}

// The offer of a first contact carries the card: pairing works with the mailbox off.
#[tokio::test(flavor = "multi_thread")]
async fn a_first_contact_needs_no_mailbox() {
    let bus = Arc::new(Bus::default());
    let (alice, bob) = (phone(&bus, "Alice").await, phone(&bus, "Bob").await);
    bob.core.set_mailbox(false).await.unwrap();
    alice.go_online(&bus);
    bob.go_online(&bus);
    pair(&alice, &bob).await;
    assert_eq!(bus.mail_for(&bob.id()), 0);
    assert_eq!(bob.core.store().contact(&alice.id()).await.unwrap().unwrap().name, "Alice");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_offline_contact_gets_mail_and_catches_up_when_back() {
    let bus = Arc::new(Bus::default());
    let (alice, bob) = (phone(&bus, "Alice").await, phone(&bus, "Bob").await);
    alice.go_online(&bus);
    let link = bob.core.my_card().await.unwrap().to_link();
    alice.core.add_contact(&link, None).await.expect("alice adds bob while he is away");
    let sent = alice.core.send_text(&bob.id(), "while you were away").await.expect("sends");
    assert!(bus.mail_for(&bob.id()) >= 2, "the card and the text wait in bob's mailbox");

    let back = std::time::Instant::now();
    bob.go_online(&bus);
    until("bob caught up", || async { texts(&bob, &alice.id()).await == ["while you were away"] }).await;
    until("the mailbox is empty", || async { bus.mail_for(&bob.id()) == 0 }).await;
    until("alice sees it delivered", || async { state(&alice, &bob.id(), &sent).await == MessageState::Delivered }).await;
    // Answers must not wait behind the mail being processed: well under a connection timeout.
    assert!(back.elapsed() < Duration::from_secs(8), "catching up took {:?}", back.elapsed());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_open_connection_is_reused() {
    let bus = Arc::new(Bus::default());
    let (alice, bob) = (phone(&bus, "Alice").await, phone(&bus, "Bob").await);
    alice.go_online(&bus);
    bob.go_online(&bus);
    pair(&alice, &bob).await;
    alice.core.send_text(&bob.id(), "one").await.unwrap();
    until("bob has one", || async { texts(&bob, &alice.id()).await.len() == 1 }).await;

    let signals = bus.signals.load(Ordering::SeqCst);
    alice.core.send_text(&bob.id(), "two").await.unwrap();
    bob.core.send_text(&alice.id(), "three").await.unwrap();
    until("both got everything", || async {
        texts(&bob, &alice.id()).await.len() == 3 && texts(&alice, &bob.id()).await.len() == 3
    })
    .await;
    assert_eq!(bus.signals.load(Ordering::SeqCst), signals, "no new signalling");
}

// The UI shows whether a direct connection is open with each contact (§84: honest status).
#[tokio::test(flavor = "multi_thread")]
async fn the_network_reports_open_connections() {
    let bus = Arc::new(Bus::default());
    let (alice, bob) = (phone(&bus, "Alice").await, phone(&bus, "Bob").await);
    alice.go_online(&bus);
    bob.go_online(&bus);
    let mut events = alice.core.events();
    pair(&alice, &bob).await;

    alice.core.send_text(&bob.id(), "hi").await.unwrap();
    until("both see the connection", || async {
        alice.network.is_connected(&bob.id()).await && bob.network.is_connected(&alice.id()).await
    })
    .await;
    assert_eq!(alice.network.connected().await, [bob.id()]);
    let announced = async {
        loop {
            if let Ok(Event::ConnectionChanged { contact }) = events.recv().await {
                return contact;
            }
        }
    };
    let contact = tokio::time::timeout(Duration::from_secs(5), announced).await.expect("announced");
    assert_eq!(contact, bob.id());

    alice.network.disconnect(&bob.id()).await;
    assert!(!alice.network.is_connected(&bob.id()).await);
    until("bob sees it closed", || async { !bob.network.is_connected(&alice.id()).await }).await;
}

// Erasing the phone (2026-09-30): the running app lets go of every direct connection, and the
// contact sees it close.
#[tokio::test(flavor = "multi_thread")]
async fn closing_the_network_closes_every_connection() {
    let bus = Arc::new(Bus::default());
    let (alice, bob) = (phone(&bus, "Alice").await, phone(&bus, "Bob").await);
    alice.go_online(&bus);
    bob.go_online(&bus);
    pair(&alice, &bob).await;
    alice.core.send_text(&bob.id(), "hi").await.unwrap();
    until("connected", || async { alice.network.is_connected(&bob.id()).await }).await;

    alice.network.close().await;
    assert!(alice.network.connected().await.is_empty());
    until("bob sees it closed", || async { !bob.network.is_connected(&alice.id()).await }).await;
}

// §62: whole chunks, sealed with Olm, fit the real DataChannel's messages.
#[tokio::test(flavor = "multi_thread")]
async fn a_file_crosses_a_real_data_channel() {
    let bus = Arc::new(Bus::default());
    let (alice, bob) = (phone(&bus, "Alice").await, phone(&bus, "Bob").await);
    alice.go_online(&bus);
    bob.go_online(&bus);
    pair(&alice, &bob).await;
    let bytes: Vec<u8> = (0..300_000u32).map(|i| (i % 253) as u8).collect();
    let path = std::env::temp_dir().join(format!("ft-net-{}.bin", ft_protocol::MessageId::new()));
    std::fs::write(&path, &bytes).unwrap();

    let sent = alice.core.send_file(&bob.id(), &path, "data.bin", "application/octet-stream").await.expect("offers");
    until("bob has the file", || async {
        bob.core.store().file(&sent).await.unwrap().is_some_and(|file| file.complete)
    })
    .await;
    let received = bob.core.store().file(&sent).await.unwrap().unwrap();
    assert_eq!(std::fs::read(bob.core.file_path(&received)).unwrap(), bytes);
    assert_eq!(bus.mail_for(&bob.id()), 0);
}

// §35: blocking someone also closes the direct connection with them.
#[tokio::test(flavor = "multi_thread")]
async fn blocking_closes_the_connection() {
    let bus = Arc::new(Bus::default());
    let (alice, bob) = (phone(&bus, "Alice").await, phone(&bus, "Bob").await);
    alice.go_online(&bus);
    bob.go_online(&bus);
    pair(&alice, &bob).await;
    alice.core.send_text(&bob.id(), "hi").await.unwrap();
    until("connected", || async { alice.network.is_connected(&bob.id()).await }).await;

    alice.core.block(&bob.id(), true).await.expect("blocks");
    until("closed on both sides", || async {
        !alice.network.is_connected(&bob.id()).await && !bob.network.is_connected(&alice.id()).await
    })
    .await;
}

// A call to a phone that is not connected (2026-09-28): the signal that opens the way is marked
// as a call, so the router rings an iPhone through CallKit. A message never is.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_to_an_offline_phone_is_signalled_as_a_call() {
    let bus = Arc::new(Bus::default());
    let (alice, bob) = (phone(&bus, "Alice").await, phone(&bus, "Bob").await);
    alice.go_online(&bus);
    bob.go_online(&bus);
    pair(&alice, &bob).await;
    bus.online.lock().unwrap().remove(&bob.id());
    bob.network.disconnect(&alice.id()).await;
    alice.network.disconnect(&bob.id()).await;

    alice.core.send_text(&bob.id(), "are you there?").await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(bus.call_signals.load(Ordering::SeqCst), 0, "a message is not a call");

    let call = alice.core.place_call(&bob.id(), false).await.unwrap();
    alice.core.offer_call_within(&call, "offer", Duration::from_millis(300)).await.expect("tries");
    assert!(bus.call_signals.load(Ordering::SeqCst) > 0, "the call rings");
}

/// Two paired phones behind a router that retains signals (0.4.0); Bob's app is closed.
async fn bob_asleep() -> (Arc<Bus>, Phone, Phone) {
    let bus = Arc::new(Bus::default());
    bus.retaining.store(true, Ordering::SeqCst);
    let (alice, bob) = (phone(&bus, "Alice").await, phone(&bus, "Bob").await);
    alice.go_online(&bus);
    bob.go_online(&bus);
    pair(&alice, &bob).await;
    bob.go_offline(&bus, &alice).await;
    (bus, alice, bob)
}

/// Sends a text to Bob, whose app is closed; returns its id once `send_text` is done, and how long
/// that took.
async fn text_to_bob(alice: &Phone, bob: &Phone, text: &str) -> (String, Duration) {
    let started = std::time::Instant::now();
    let sent = alice.core.send_text(&bob.id(), text).await.expect("sends");
    (sent, started.elapsed())
}

// The message gets delivered first (§17, §19; 2026-09-29): a text to a phone that is not
// connected goes to its mailbox at once, as it always did, whether the router keeps our
// connection offer (0.4.0) or not. It never waits for the direct connection first: a sender who
// closes the app right after writing leaves it in the mailbox, not in the outbox.
#[tokio::test(flavor = "multi_thread")]
async fn a_text_to_a_closed_app_goes_to_the_mailbox_at_once() {
    for retaining in [true, false] {
        let (bus, alice, bob) = bob_asleep().await;
        bus.retaining.store(retaining, Ordering::SeqCst);
        let (sent, took) = text_to_bob(&alice, &bob, "are you there?").await;
        eprintln!("text to a closed app, retaining {retaining}: in the mailbox after {} ms", took.as_millis());
        assert!(took < Duration::from_secs(1), "retaining {retaining}: it waited {took:?}");
        assert_eq!(state(&alice, &bob.id(), &sent).await, MessageState::Sent, "in the mailbox");
        assert_eq!(bus.mail_for(&bob.id()), 1, "retaining {retaining}");
    }
}

// The sender closes its app right after writing: the message is already in the mailbox, and Bob
// gets it when he opens his, once.
#[tokio::test(flavor = "multi_thread")]
async fn a_sender_that_closes_right_after_writing_leaves_the_text_in_the_mailbox() {
    let (bus, alice, bob) = bob_asleep().await;
    let (core, to) = (alice.core.clone(), bob.id());
    let sending = tokio::spawn(async move { core.send_text(&to, "bye").await });
    // The app is closed a second later: whatever the send was still doing stops there.
    tokio::time::sleep(Duration::from_secs(1)).await;
    sending.abort();
    assert_eq!(bus.mail_for(&bob.id()), 1, "already in the mailbox");
    bus.online.lock().unwrap().remove(&alice.id());
    alice.network.disconnect(&bob.id()).await;
    drop(alice.core.clone());
    bob.go_online(&bus);
    until("bob has it", || async { texts(&bob, &alice.id()).await == ["bye"] }).await;
    until("the mailbox is empty", || async { bus.mail_for(&bob.id()) == 0 }).await;
}

// Bob wakes and answers our kept offer before he gets round to his mailbox: the text goes
// directly too, is shown once, and becomes delivered with his receipt. The copy in the mailbox is
// still collected and removed from the router.
#[tokio::test(flavor = "multi_thread")]
async fn a_text_in_the_mailbox_also_goes_directly_when_the_connection_opens_first() {
    let (bus, alice, bob) = bob_asleep().await;
    let before = bus.signalled_to(&bob.id());
    let (sent, _) = text_to_bob(&alice, &bob, "wake up").await;
    bus.mail_held.store(true, Ordering::SeqCst);
    let back = std::time::Instant::now();
    bob.go_online(&bus);
    until("bob has it", || async { texts(&bob, &alice.id()).await == ["wake up"] }).await;
    eprintln!("the kept offer: the text arrived directly {} ms after bob connected", back.elapsed().as_millis());
    until("alice sees it delivered", || async { state(&alice, &bob.id(), &sent).await == MessageState::Delivered }).await;
    assert_eq!(bus.signalled_to(&bob.id()) - before, 1, "one offer, kept open");
    assert_eq!(bus.mail_for(&bob.id()), 1, "the copy still waits in the mailbox");

    bus.mail_held.store(false, Ordering::SeqCst);
    bob.mail_notice(&bus);
    until("the mailbox is empty", || async { bus.mail_for(&bob.id()) == 0 }).await;
    assert_eq!(texts(&bob, &alice.id()).await, ["wake up"], "shown once");
    assert_eq!(state(&alice, &bob.id(), &sent).await, MessageState::Delivered);
}

// Bob gets his mailbox before our kept offer reaches him: the text is shown once, delivered with
// his receipt, and the connection that opens later changes nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_text_in_the_mailbox_that_arrives_first_is_not_shown_twice() {
    let (bus, alice, bob) = bob_asleep().await;
    let (sent, _) = text_to_bob(&alice, &bob, "hello").await;
    bus.signals_held.store(true, Ordering::SeqCst);
    bob.go_online(&bus);
    until("bob has it", || async { texts(&bob, &alice.id()).await == ["hello"] }).await;
    until("the mailbox is empty", || async { bus.mail_for(&bob.id()) == 0 }).await;
    until("alice sees it delivered", || async { state(&alice, &bob.id(), &sent).await == MessageState::Delivered }).await;

    bus.signals_held.store(false, Ordering::SeqCst);
    bob.release(&bus);
    until("connected", || async { alice.network.is_connected(&bob.id()).await }).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(texts(&bob, &alice.id()).await, ["hello"], "shown once");
    assert_eq!(state(&alice, &bob.id(), &sent).await, MessageState::Delivered);
    assert_eq!(bus.mail_for(&bob.id()), 0);
}

// Both at once, as it happens when Bob opens his app: the mailbox and our kept offer come right
// after the welcome. Shown once, delivered, nothing left at the router.
#[tokio::test(flavor = "multi_thread")]
async fn a_text_that_comes_both_ways_is_shown_once() {
    let (bus, alice, bob) = bob_asleep().await;
    let (sent, _) = text_to_bob(&alice, &bob, "twice?").await;
    bob.go_online(&bus);
    until("alice sees it delivered", || async { state(&alice, &bob.id(), &sent).await == MessageState::Delivered }).await;
    until("connected", || async { alice.network.is_connected(&bob.id()).await }).await;
    until("the mailbox is empty", || async { bus.mail_for(&bob.id()) == 0 }).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(texts(&bob, &alice.id()).await, ["twice?"], "shown once");
}

// With the mailbox off (strict P2P) nothing goes to the router: the text waits in the outbox and
// the kept offer delivers it directly as soon as Bob answers it, even after the connection window.
#[tokio::test(flavor = "multi_thread")]
async fn with_the_mailbox_off_the_kept_offer_delivers_the_waiting_text() {
    let (bus, alice, bob) = bob_asleep().await;
    // Telling Bob the mailbox is off already makes the offer (it waits at the router too).
    let before = bus.signalled_to(&bob.id());
    let deposits = bus.next_id.load(Ordering::SeqCst);
    alice.core.set_mailbox(false).await.unwrap();
    let (sent, _) = text_to_bob(&alice, &bob, "only directly").await;
    assert_eq!(state(&alice, &bob.id(), &sent).await, MessageState::Pending, "waits in the outbox");

    let back = std::time::Instant::now();
    bob.go_online(&bus);
    until("bob has it", || async { texts(&bob, &alice.id()).await == ["only directly"] }).await;
    let arrived = back.elapsed();
    eprintln!("mailbox off: the waiting text arrived {} ms after bob connected", arrived.as_millis());
    until("alice sees it delivered", || async { state(&alice, &bob.id(), &sent).await == MessageState::Delivered }).await;
    assert_eq!(bus.next_id.load(Ordering::SeqCst), deposits, "nothing went through the mailbox");
    assert_eq!(bus.signalled_to(&bob.id()) - before, 1, "one offer, kept open");
    assert!(arrived < Duration::from_millis(1_500), "{} ms", arrived.as_millis());
}

// A call to a phone whose app is closed (2026-09-29): its connection's offer waits at the router,
// and the call rings as soon as the phone connects, with no new offer and no retry round.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_to_a_phone_that_wakes_rings_as_soon_as_it_connects() {
    let (bus, alice, bob) = bob_asleep().await;
    let before = bus.signalled_to(&bob.id());
    let mut bob_events = bob.core.events();
    let call = alice.core.place_call(&bob.id(), false).await.unwrap();
    let (core, id) = (alice.core.clone(), call.clone());
    tokio::spawn(async move { core.offer_call(&id, "offer").await });
    tokio::time::sleep(Duration::from_millis(1_000)).await;

    let back = std::time::Instant::now();
    bob.go_online(&bus);
    let rang = async {
        loop {
            if let Ok(Event::Call { update: ft_core::CallUpdate::Incoming { .. }, .. }) = bob_events.recv().await {
                return;
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(10), rang).await.expect("bob's phone rings");
    let ringing = back.elapsed();
    eprintln!("retained call offer: rang {} ms after bob connected", ringing.as_millis());
    assert_eq!(bus.signalled_to(&bob.id()) - before, 1, "one offer, kept open");
    assert!(ringing < Duration::from_millis(1_500), "{} ms", ringing.as_millis());
    let _ = alice.core.end_call(&call, false).await;
}

/// Waits for the phone to ring with a call.
async fn rings(events: &mut tokio::sync::broadcast::Receiver<Event>, within: Duration) {
    let rang = async {
        loop {
            if let Ok(Event::Call { update: ft_core::CallUpdate::Incoming { .. }, .. }) = events.recv().await {
                return;
            }
        }
    };
    tokio::time::timeout(within, rang).await.expect("the phone rings");
}

/// Waits until the router has been asked to ring someone, and says how long that took.
async fn until_rung(bus: &Bus, since: std::time::Instant, within: Duration) -> Duration {
    while bus.rings.load(Ordering::SeqCst) == 0 {
        assert!(since.elapsed() < within, "the router was never asked to ring the callee");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    since.elapsed()
}

// A call placed while an offer made for an undelivered message still waits at the router
// (2026-10-01, seen on an iPhone whose app was closed): that offer never rang, so the call must
// make the router ring, and not just wait for the message's offer to be answered. When the phone
// wakes, the call rings and a single connection opens.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_rings_although_an_offer_for_a_message_waits_at_the_router() {
    let (bus, alice, bob) = bob_asleep().await;
    let (sent, _) = text_to_bob(&alice, &bob, "still waiting for you").await;
    assert_eq!(bus.rings.load(Ordering::SeqCst), 0, "a message never rings");
    let mut bob_events = bob.core.events();

    let placed = std::time::Instant::now();
    let call = alice.core.place_call(&bob.id(), false).await.unwrap();
    let (core, id) = (alice.core.clone(), call.clone());
    tokio::spawn(async move { core.offer_call(&id, "offer").await });
    let rung = until_rung(&bus, placed, Duration::from_secs(10)).await;
    eprintln!("call behind a message's kept offer: the router was asked to ring after {} ms", rung.as_millis());
    assert!(rung < Duration::from_secs(2), "{} ms", rung.as_millis());
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(bus.rings.load(Ordering::SeqCst), 1, "one ring for one call");

    bob.go_online(&bus);
    rings(&mut bob_events, Duration::from_secs(10)).await;
    until("alice sees the text delivered", || async { state(&alice, &bob.id(), &sent).await == MessageState::Delivered }).await;
    assert_eq!(alice.network.connected().await, [bob.id()]);
    let _ = alice.core.end_call(&call, false).await;
    // The message's offer, dropped by Alice, never becomes a second connection on Bob's side.
    tokio::time::sleep(ft_core::net::CONNECT_WAIT + Duration::from_secs(1)).await;
    assert_eq!(bob.network.connected().await, [alice.id()], "one connection");
    assert_eq!(bus.rings.load(Ordering::SeqCst), 1, "still one ring");
}

// The same with the mailbox off (strict P2P): the message waits in the outbox for its offer to be
// answered, holding the way to the contact, and the call does not wait behind it to ring.
#[tokio::test(flavor = "multi_thread")]
async fn with_the_mailbox_off_a_call_rings_although_a_message_waits_for_its_offer() {
    let (bus, alice, bob) = bob_asleep().await;
    // Telling Bob the mailbox is off makes the offer, and waits for its connection for a while.
    let before = bus.signalled_to(&bob.id());
    let (core, to) = (alice.core.clone(), bob.id());
    tokio::spawn(async move {
        core.set_mailbox(false).await.unwrap();
        core.send_text(&to, "only directly").await
    });
    until("the offer waits at the router", || async { bus.signalled_to(&bob.id()) > before }).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(bus.rings.load(Ordering::SeqCst), 0, "a message never rings");
    let mut bob_events = bob.core.events();

    let placed = std::time::Instant::now();
    let call = alice.core.place_call(&bob.id(), false).await.unwrap();
    let (core, id) = (alice.core.clone(), call.clone());
    tokio::spawn(async move { core.offer_call(&id, "offer").await });
    let rung = until_rung(&bus, placed, Duration::from_secs(15)).await;
    eprintln!("mailbox off, call behind a waiting message: the router was asked to ring after {} ms", rung.as_millis());
    assert!(rung < Duration::from_secs(2), "{} ms", rung.as_millis());

    bob.go_online(&bus);
    rings(&mut bob_events, Duration::from_secs(10)).await;
    until("bob has the text", || async { texts(&bob, &alice.id()).await == ["only directly"] }).await;
    assert_eq!(bus.rings.load(Ordering::SeqCst), 1, "one ring for one call");
    let _ = alice.core.end_call(&call, false).await;
}

// The opposite order: a message sent while a call's offer waits at the router goes to the
// mailbox at once, makes no offer of its own, and the call still rings when the phone wakes.
#[tokio::test(flavor = "multi_thread")]
async fn a_message_sent_while_a_call_waits_at_the_router_does_not_hold_the_call_back() {
    let (bus, alice, bob) = bob_asleep().await;
    let mut bob_events = bob.core.events();
    let call = alice.core.place_call(&bob.id(), false).await.unwrap();
    let (core, id) = (alice.core.clone(), call.clone());
    tokio::spawn(async move { core.offer_call(&id, "offer").await });
    until_rung(&bus, std::time::Instant::now(), Duration::from_secs(5)).await;
    let signalled = bus.signalled_to(&bob.id());

    let (sent, took) = text_to_bob(&alice, &bob, "call me back").await;
    assert!(took < Duration::from_secs(1), "it waited {took:?}");
    assert_eq!(state(&alice, &bob.id(), &sent).await, MessageState::Sent, "in the mailbox");
    assert_eq!(bus.signalled_to(&bob.id()), signalled, "no offer of its own");

    bob.go_online(&bus);
    rings(&mut bob_events, Duration::from_secs(10)).await;
    until("alice sees the text delivered", || async { state(&alice, &bob.id(), &sent).await == MessageState::Delivered }).await;
    assert_eq!(bus.rings.load(Ordering::SeqCst), 1, "one ring for one call");
    let _ = alice.core.end_call(&call, false).await;
}

// A connection that looks open but is dead (2026-10-01, seen on an iPhone after a recent chat: its
// app was suspended or killed, and the caller's side has not noticed yet): a call's offer sent
// into it reaches nobody. The caller notices within a few seconds that the offer is not
// acknowledged, drops that connection and goes through the router, marked as a call, so the
// phone rings; once it wakes, a single connection opens.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_into_a_dead_connection_goes_through_the_router_within_seconds() {
    let bus = Arc::new(Bus::default());
    bus.retaining.store(true, Ordering::SeqCst);
    let (alice, bob) = (phone(&bus, "Alice").await, phone(&bus, "Bob").await);
    alice.go_online(&bus);
    bob.go_online(&bus);
    pair(&alice, &bob).await;
    alice.core.send_text(&bob.id(), "talk soon").await.unwrap();
    until("connected", || async { alice.network.is_connected(&bob.id()).await && bob.network.is_connected(&alice.id()).await }).await;

    // Bob's app is killed: its socket to the router and its connection stop without a word.
    bus.online.lock().unwrap().remove(&bob.id());
    bob.network.vanish(&alice.id()).await;
    assert!(alice.network.is_connected(&bob.id()).await, "alice's side still looks open");
    let mut bob_events = bob.core.events();

    let placed = std::time::Instant::now();
    let call = alice.core.place_call(&bob.id(), false).await.unwrap();
    let (core, id) = (alice.core.clone(), call.clone());
    tokio::spawn(async move { core.offer_call(&id, "offer").await });
    let rung = until_rung(&bus, placed, Duration::from_secs(15)).await;
    eprintln!("call into a dead connection: the router was asked to ring after {} ms", rung.as_millis());
    assert!(rung < Duration::from_secs(5), "{} ms", rung.as_millis());

    bob.go_online(&bus);
    rings(&mut bob_events, Duration::from_secs(10)).await;
    until("one connection each", || async {
        alice.network.connected().await == [bob.id()] && bob.network.connected().await == [alice.id()]
    })
    .await;
    assert_eq!(bus.rings.load(Ordering::SeqCst), 1, "one ring for one call");
    let _ = alice.core.end_call(&call, false).await;
}

// A live connection acknowledges the call's offer: it is kept, nothing goes through the router and
// nothing rings twice.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_over_a_live_connection_keeps_it() {
    let bus = Arc::new(Bus::default());
    bus.retaining.store(true, Ordering::SeqCst);
    let (alice, bob) = (phone(&bus, "Alice").await, phone(&bus, "Bob").await);
    alice.go_online(&bus);
    bob.go_online(&bus);
    pair(&alice, &bob).await;
    alice.core.send_text(&bob.id(), "about to call").await.unwrap();
    until("connected", || async { alice.network.is_connected(&bob.id()).await && bob.network.is_connected(&alice.id()).await }).await;
    let (signals, mut bob_events) = (bus.signals.load(Ordering::SeqCst), bob.core.events());

    let call = alice.core.place_call(&bob.id(), false).await.unwrap();
    let offered = std::time::Instant::now();
    alice.core.offer_call(&call, "offer").await.expect("offers");
    assert!(offered.elapsed() < ft_core::net::CONNECT_WAIT / 4, "acknowledged in {:?}", offered.elapsed());
    rings(&mut bob_events, Duration::from_secs(5)).await;
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert_eq!(bus.signals.load(Ordering::SeqCst), signals, "nothing through the router");
    assert_eq!(bus.rings.load(Ordering::SeqCst), 0);
    assert!(alice.network.is_connected(&bob.id()).await, "the connection is kept");
    let _ = alice.core.end_call(&call, false).await;
}

// A blocked contact's call (§35, §109): its offer is dropped unread, and the phone hears that a
// call may have been refused. With the app closed, a push has set the iPhone's call screen
// ringing before the core could know who called: it stops at once, and nothing shows.
#[tokio::test(flavor = "multi_thread")]
async fn a_blocked_contacts_call_is_refused_without_ringing() {
    let (bus, alice, bob) = bob_asleep().await;
    bob.core.block(&alice.id(), true).await.expect("blocks");
    let mut bob_events = bob.core.events();
    let call = alice.core.place_call(&bob.id(), false).await.unwrap();
    let (core, id) = (alice.core.clone(), call.clone());
    tokio::spawn(async move { core.offer_call_within(&id, "offer", Duration::from_secs(3)).await });
    tokio::time::sleep(Duration::from_millis(500)).await;

    bob.go_online(&bus);
    let refused = async {
        loop {
            match bob_events.recv().await {
                Ok(Event::CallRefused) => return,
                Ok(Event::Call { .. }) => panic!("a blocked contact's call never rings"),
                _ => {}
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(10), refused).await.expect("the phone hears it was refused");
    assert!(bob.core.current_call().await.unwrap().is_none());
    assert!(bob.core.visible_calls(10).await.unwrap().is_empty(), "no trace in the history");
}

/// STUN and TURN servers that never answer: local UDP sockets nobody reads (a server behind a
/// network that drops the packets). Gathering takes its whole second. Nothing leaves the machine.
struct Blackhole {
    _stun: std::net::UdpSocket,
    _turn: std::net::UdpSocket,
    stun: Vec<String>,
    turn: TurnGrant,
}

fn blackhole() -> Blackhole {
    let (stun, turn) = (std::net::UdpSocket::bind("127.0.0.1:0").unwrap(), std::net::UdpSocket::bind("127.0.0.1:0").unwrap());
    let (stun_port, turn_port) = (stun.local_addr().unwrap().port(), turn.local_addr().unwrap().port());
    Blackhole {
        stun: vec![format!("stun:127.0.0.1:{stun_port}")],
        turn: TurnGrant { urls: vec![format!("turn:127.0.0.1:{turn_port}?transport=udp")], username: "u".to_owned(), credential: "c".to_owned() },
        _stun: stun,
        _turn: turn,
    }
}

// A router before 0.4 loses the offer for a phone that is not connected, and every message or
// call retry makes another. The candidates already gathered are not thrown away: the next attempt
// goes with the same offer, at once, instead of gathering again.
#[tokio::test(flavor = "multi_thread")]
async fn without_retention_a_retry_reuses_the_offer_already_gathered() {
    let hole = blackhole();
    let bus = Arc::new(Bus::default());
    let (alice, bob) = (phone(&bus, "Alice").await, phone(&bus, "Bob").await);
    alice.go_online_with(&bus, hole.stun.clone(), Some(hole.turn.clone()));
    bob.go_online_with(&bus, hole.stun.clone(), Some(hole.turn.clone()));
    pair(&alice, &bob).await;
    bob.go_offline(&bus, &alice).await;

    let started = std::time::Instant::now();
    alice.core.send_text(&bob.id(), "one").await.expect("sends");
    let first = started.elapsed();
    let started = std::time::Instant::now();
    alice.core.send_text(&bob.id(), "two").await.expect("sends");
    let second = started.elapsed();
    eprintln!("no retention: first attempt {} ms, second {} ms", first.as_millis(), second.as_millis());
    assert!(first >= Duration::from_millis(900), "the first gathers: {first:?}");
    assert!(second < Duration::from_millis(400), "the second gathered again: {second:?}");
    assert_eq!(bus.mail_for(&bob.id()), 2, "both in the mailbox");

    // Bob is back: the reused offer connects as any other.
    bob.go_online_with(&bus, hole.stun.clone(), Some(hole.turn.clone()));
    until("bob caught up", || async { texts(&bob, &alice.id()).await.len() == 2 }).await;
    alice.core.send_text(&bob.id(), "three").await.expect("sends");
    until("bob has the third", || async { texts(&bob, &alice.id()).await.len() == 3 }).await;
}

// With servers that never answer, the offer's gathering is all a text to a closed app waits
// before the mailbox: a second at most (3 s before 2026-09-29), with a router that keeps the
// offer or not; a retry does not gather again.
#[tokio::test(flavor = "multi_thread")]
async fn behind_servers_that_never_answer_a_text_reaches_the_mailbox_within_a_second() {
    for retaining in [true, false] {
        let hole = blackhole();
        let bus = Arc::new(Bus::default());
        bus.retaining.store(retaining, Ordering::SeqCst);
        let (alice, bob) = (phone(&bus, "Alice").await, phone(&bus, "Bob").await);
        alice.go_online_with(&bus, hole.stun.clone(), Some(hole.turn.clone()));
        bob.go_online_with(&bus, hole.stun.clone(), Some(hole.turn.clone()));
        pair(&alice, &bob).await;
        bob.go_offline(&bus, &alice).await;
        let (_, first) = text_to_bob(&alice, &bob, "one").await;
        let (_, second) = text_to_bob(&alice, &bob, "two").await;
        eprintln!("unreachable servers, retaining {retaining}: in the mailbox after {} ms, the next after {} ms", first.as_millis(), second.as_millis());
        assert!(first < Duration::from_millis(1_500), "retaining {retaining}: {first:?}");
        assert!(second < Duration::from_millis(400), "retaining {retaining}: {second:?}");
        assert_eq!(bus.mail_for(&bob.id()), 2);
    }
}
