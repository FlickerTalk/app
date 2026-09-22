//! Two complete devices connected by real WebRTC data channels (loopback), with a fake router in
//! memory for signalling and the mailbox (Plan §13–19, §106 M2).

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use ft_core::net::{Network, Relay};
use ft_core::{Core, Event};
use ft_push::RouterEvent;
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
    next_id: AtomicUsize,
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
}

struct FakeRelay {
    bus: Arc<Bus>,
    me: OnceLock<String>,
}

#[async_trait]
impl Relay for FakeRelay {
    async fn signal(&self, to: &str, capability: &[u8; 32], bytes: Vec<u8>) -> anyhow::Result<bool> {
        self.bus.allowed(to, capability)?;
        self.bus.signals.fetch_add(1, Ordering::SeqCst);
        let online = self.bus.online.lock().unwrap().get(to).cloned();
        Ok(online.is_some_and(|device| device.send(RouterEvent::Signal(bytes)).is_ok()))
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
        let (events, mut incoming) = mpsc::unbounded_channel();
        bus.online.lock().unwrap().insert(self.id(), events.clone());
        let network = self.network.clone();
        tokio::spawn(async move {
            while let Some(event) = incoming.recv().await {
                network.handle(event).await;
            }
        });
        let _ = events.send(RouterEvent::Connected { stun: vec![], turn: None });
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
    assert_eq!(std::fs::read(received.path).unwrap(), bytes);
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
