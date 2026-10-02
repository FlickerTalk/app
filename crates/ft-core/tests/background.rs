//! The app leaving the foreground (2026-10-01). Measured on real phones: an app in the background
//! kept its socket to the router open, iOS suspended it (Android may freeze it), the router went
//! on handing calls and mail notices to that socket and sent no push, and the phone missed them.
//! Now the app lets go of the router and of the other phones when it leaves the foreground, unless
//! a call is going on, and a push brings it back.
//!
//! Two complete phones (`online::start`, with the real router client) against a fake router with a
//! real WebSocket, which records who is connected and what it had to push. The direct connections
//! are real WebRTC on loopback.
//!
//! The same fake router also takes the app's suggestions (2026-10-02), the last test here.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use base64::engine::general_purpose::STANDARD_NO_PAD;
use base64::Engine;
use ft_core::online::{self, Online, LINGER};
use ft_core::{CallPhase, Core, Feedback};
use ft_storage::{MessageState, Store};
use ft_webrtc::SessionConfig;
use serde_json::{json, Value};
use tokio::sync::mpsc;

/// A push the router had to send, because the device was not connected.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Push {
    to: String,
    /// Marked as a call (`ft-call: 1`): PushKit on an iPhone, `t: call` on Android.
    call: bool,
}

/// What the fake router knows; one lock, so that a signal and a connection never cross.
#[derive(Default)]
struct Inner {
    /// Who is connected now: their socket's id and where its frames go.
    connected: HashMap<String, (u64, mpsc::UnboundedSender<String>)>,
    /// How many sockets each device opened.
    opened: HashMap<String, usize>,
    pushes: Vec<Push>,
    /// Router 0.4: signals for a device that is not connected, handed over after its next welcome.
    held: HashMap<String, Vec<String>>,
    /// Signals sent to each device, connected or not.
    signalled: HashMap<String, usize>,
    mail: HashMap<String, Vec<(String, Vec<u8>)>>,
    /// Each registration that went through: who, and its silent slots.
    registrations: Vec<(String, u8)>,
    register_tries: usize,
    refusing: bool,
    /// Suggestions from the app (2026-10-02): who signed each, and its body.
    feedback: Vec<(String, Value)>,
    /// What a suggestion is answered with; 204 unless a test says otherwise.
    feedback_answer: Option<StatusCode>,
}

#[derive(Default)]
struct Fake {
    inner: Mutex<Inner>,
    next: AtomicU64,
}

impl Fake {
    fn with<T>(&self, look: impl FnOnce(&mut Inner) -> T) -> T {
        look(&mut self.inner.lock().unwrap())
    }

    fn is_connected(&self, device: &str) -> bool {
        self.with(|inner| inner.connected.contains_key(device))
    }

    fn opened(&self, device: &str) -> usize {
        self.with(|inner| inner.opened.get(device).copied().unwrap_or(0))
    }

    fn pushes_to(&self, device: &str) -> Vec<Push> {
        self.with(|inner| inner.pushes.iter().filter(|push| push.to == device).cloned().collect())
    }

    fn signalled_to(&self, device: &str) -> usize {
        self.with(|inner| inner.signalled.get(device).copied().unwrap_or(0))
    }

    fn mail_for(&self, device: &str) -> usize {
        self.with(|inner| inner.mail.get(device).map_or(0, Vec::len))
    }

    /// Hands a frame to the device's socket, or records the push the router sends instead.
    fn deliver(inner: &mut Inner, to: &str, frame: String, call: bool) -> bool {
        if inner.connected.get(to).is_some_and(|(_, frames)| frames.send(frame).is_ok()) {
            return true;
        }
        inner.pushes.push(Push { to: to.to_owned(), call });
        false
    }

    async fn serve(self: Arc<Self>, device: String, mut socket: WebSocket) {
        let id = self.next.fetch_add(1, SeqCst);
        let (frames, mut queued) = mpsc::unbounded_channel::<String>();
        let held = self.with(|inner| {
            inner.connected.insert(device.clone(), (id, frames));
            *inner.opened.entry(device.clone()).or_default() += 1;
            inner.held.remove(&device).unwrap_or_default()
        });
        let welcome = json!({ "kind": "welcome", "stun": [], "turn": null }).to_string();
        let mut open = socket.send(Message::Text(welcome.into())).await.is_ok();
        for signal in held {
            open = open && socket.send(Message::Text(signal.into())).await.is_ok();
        }
        while open {
            tokio::select! {
                frame = queued.recv() => match frame {
                    Some(frame) => open = socket.send(Message::Text(frame.into())).await.is_ok(),
                    None => open = false,
                },
                message = socket.recv() => open = matches!(message, Some(Ok(message)) if !matches!(message, Message::Close(_))),
            }
        }
        self.with(|inner| {
            if inner.connected.get(&device).is_some_and(|(open, _)| *open == id) {
                inner.connected.remove(&device);
            }
        });
    }
}

fn device_of(headers: &HeaderMap) -> String {
    headers.get("ft-device").and_then(|value| value.to_str().ok()).unwrap_or_default().to_owned()
}

async fn fake_router() -> (String, Arc<Fake>) {
    let fake = Arc::new(Fake::default());
    let router = Router::new()
        .route(
            "/v1/connect",
            get(|State(fake): State<Arc<Fake>>, Query(query): Query<HashMap<String, String>>, upgrade: WebSocketUpgrade| async move {
                let device = query.get("ft-device").cloned().unwrap_or_default();
                upgrade.on_upgrade(move |socket| fake.serve(device, socket))
            }),
        )
        .route(
            "/v1/device/register",
            post(|State(fake): State<Arc<Fake>>, headers: HeaderMap, body: Bytes| async move {
                let registration: Value = serde_json::from_slice(&body).unwrap();
                fake.with(|inner| {
                    inner.register_tries += 1;
                    if inner.refusing {
                        return StatusCode::SERVICE_UNAVAILABLE;
                    }
                    inner.registrations.push((device_of(&headers), registration["silent_slots"].as_u64().unwrap() as u8));
                    StatusCode::NO_CONTENT
                })
            }),
        )
        .route("/v1/device/push", put(|| async { StatusCode::NO_CONTENT }))
        .route(
            "/v1/signal/{to}",
            post(|State(fake): State<Arc<Fake>>, Path(to): Path<String>, headers: HeaderMap, body: Bytes| async move {
                let call = headers.get("ft-call").is_some_and(|value| value == "1");
                let frame = json!({ "kind": "signal", "signal": STANDARD_NO_PAD.encode(&body) }).to_string();
                fake.with(|inner| {
                    *inner.signalled.entry(to.clone()).or_default() += 1;
                    if Fake::deliver(inner, &to, frame.clone(), call) {
                        return StatusCode::ACCEPTED.into_response();
                    }
                    inner.held.entry(to).or_default().push(frame);
                    (StatusCode::NOT_FOUND, [("ft-retained", "1")]).into_response()
                })
            }),
        )
        .route(
            "/v1/mailbox",
            get(|State(fake): State<Arc<Fake>>, headers: HeaderMap| async move {
                let mail = fake.with(|inner| inner.mail.get(&device_of(&headers)).cloned().unwrap_or_default());
                Json(mail.into_iter().map(|(id, blob)| json!({ "id": id, "blob": STANDARD_NO_PAD.encode(blob) })).collect::<Vec<_>>())
            }),
        )
        .route(
            "/v1/mailbox/{key}",
            post(|State(fake): State<Arc<Fake>>, Path(to): Path<String>, body: Bytes| async move {
                let id = fake.next.fetch_add(1, SeqCst).to_string();
                fake.with(|inner| {
                    inner.mail.entry(to.clone()).or_default().push((id, body.to_vec()));
                    Fake::deliver(inner, &to, json!({ "kind": "mail" }).to_string(), false);
                });
                StatusCode::CREATED
            })
            .delete(|State(fake): State<Arc<Fake>>, Path(id): Path<String>, headers: HeaderMap| async move {
                fake.with(|inner| {
                    if let Some(mail) = inner.mail.get_mut(&device_of(&headers)) {
                        mail.retain(|(stored, _)| *stored != id);
                    }
                });
                StatusCode::NO_CONTENT
            }),
        )
        .route(
            "/v1/feedback",
            post(|State(fake): State<Arc<Fake>>, headers: HeaderMap, body: Bytes| async move {
                fake.with(|inner| {
                    inner.feedback.push((device_of(&headers), serde_json::from_slice(&body).unwrap()));
                    inner.feedback_answer.unwrap_or(StatusCode::NO_CONTENT)
                })
            }),
        )
        .with_state(fake.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (format!("http://{address}"), fake)
}

struct Phone {
    online: Online,
}

impl Phone {
    fn id(&self) -> String {
        self.online.core.device_id().as_str().to_owned()
    }

    fn core(&self) -> &Arc<Core> {
        &self.online.core
    }
}

/// A phone started as the app starts it: in front, unless a push started it (`in_front` false),
/// and then the platform says at once that it is not.
async fn phone(base: &str, name: &str, in_front: bool) -> Phone {
    let online = online::start(Store::open_in_memory().await.unwrap(), [4; 32], base, SessionConfig::offline()).await.unwrap();
    online.core.set_name(name).await.unwrap();
    // The WebView's calls: no audio device in a test.
    online.core.set_call_audio(None);
    let files = std::env::temp_dir().join(format!("ft-bg-{name}-{}", ft_protocol::MessageId::new()));
    std::fs::create_dir_all(&files).unwrap();
    online.core.set_files_dir(files);
    online.set_foreground(in_front).await;
    Phone { online }
}

async fn until_within<F, Fut>(what: &str, limit: Duration, condition: F)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let started = Instant::now();
    while !condition().await {
        assert!(started.elapsed() < limit, "timed out waiting until {what}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn until<F, Fut>(what: &str, condition: F)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    until_within(what, Duration::from_secs(20), condition).await;
}

/// Two phones in front, connected to the router and paired.
async fn two_phones() -> (Arc<Fake>, Phone, Phone) {
    let (base, fake) = fake_router().await;
    let (alice, bob) = (phone(&base, "Alice", true).await, phone(&base, "Bob", true).await);
    pair(&fake, &alice, &bob).await;
    (fake, alice, bob)
}

async fn pair(fake: &Fake, alice: &Phone, bob: &Phone) {
    until("both are connected", || async { fake.is_connected(&alice.id()) && fake.is_connected(&bob.id()) }).await;
    let link = bob.core().my_card().await.unwrap().to_link();
    alice.core().add_contact(&link, None).await.expect("alice adds bob");
    let id = alice.id();
    until("bob knows alice", || async { bob.core().store().contact(&id).await.unwrap().is_some() }).await;
    bob.core().accept_contact(&id).await.expect("bob accepts alice");
}

async fn texts(phone: &Phone, contact: &str) -> Vec<String> {
    phone.core().store().messages(contact, 100).await.unwrap().into_iter().map(|message| message.body).collect()
}

async fn state(phone: &Phone, id: &str) -> MessageState {
    phone.core().store().message(id).await.unwrap().expect("the message").state
}

/// Alice calls Bob (the WebView's way: the offer is the WebView's), trying in the background.
async fn call(alice: &Phone, bob: &Phone) -> String {
    let call = alice.core().place_call(&bob.id(), false).await.expect("alice calls");
    let (core, id) = (alice.core().clone(), call.clone());
    tokio::spawn(async move { core.offer_call(&id, "v=0 offer").await });
    call
}

async fn rings(bob: &Phone, call: &str) -> bool {
    bob.core().current_call().await.unwrap().is_some_and(|current| current.call == call && current.phase == CallPhase::Ringing)
}

// Leaving the foreground lets go of the router at once: it sees the phone gone, and what comes for
// it, a message or a call, is pushed instead of handed to a socket nobody reads.
#[tokio::test(flavor = "multi_thread")]
async fn in_the_background_the_router_sees_the_phone_gone_and_pushes_what_comes() {
    let (fake, alice, bob) = two_phones().await;

    bob.online.set_foreground(false).await;
    until_within("the router sees bob gone", Duration::from_secs(1), || async { !fake.is_connected(&bob.id()) }).await;

    alice.core().send_text(&bob.id(), "are you there?").await.unwrap();
    until("bob's phone is woken for it", || async { fake.pushes_to(&bob.id()).contains(&Push { to: bob.id(), call: false }) }).await;
    let call = call(&alice, &bob).await;
    until("bob's phone rings through the push", || async { fake.pushes_to(&bob.id()).contains(&Push { to: bob.id(), call: true }) }).await;
    alice.core().end_call(&call, false).await.unwrap();
    assert!(!fake.is_connected(&bob.id()), "nothing brought bob back but a push");
}

// Back in front, the phone connects, takes what waited for it at the router and collects its mail,
// as a start does; the sender's message is delivered.
#[tokio::test(flavor = "multi_thread")]
async fn coming_back_reconnects_takes_what_waited_and_collects_the_mail() {
    let (fake, alice, bob) = two_phones().await;
    bob.online.set_foreground(false).await;
    until("the router sees bob gone", || async { !fake.is_connected(&bob.id()) }).await;
    let sent = alice.core().send_text(&bob.id(), "while you were away").await.unwrap();
    until("the text waits in bob's mailbox", || async { fake.mail_for(&bob.id()) > 0 }).await;
    assert!(texts(&bob, &alice.id()).await.is_empty());

    assert!(bob.online.set_foreground(true).await, "bob came back");
    until_within("bob is connected again", Duration::from_secs(2), || async { fake.is_connected(&bob.id()) }).await;
    until("bob has the text", || async { texts(&bob, &alice.id()).await == ["while you were away"] }).await;
    until("alice sees it delivered", || async { state(&alice, &sent).await == MessageState::Delivered }).await;
    until("bob's mailbox is empty", || async { fake.mail_for(&bob.id()) == 0 }).await;
}

// A call is never cut by leaving the foreground: the connection stays while it rings, is answered
// and goes on, and goes when it ends (a moment later), since the app is still not in front.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_keeps_the_connection_in_the_background_until_it_ends() {
    let (fake, alice, bob) = two_phones().await;
    let call = call(&alice, &bob).await;
    until("bob's phone rings", || rings(&bob, &call)).await;

    bob.online.set_foreground(false).await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(fake.is_connected(&bob.id()), "the ringing call keeps the router");
    assert!(alice.online.network.is_connected(&bob.id()).await, "and the direct connection");
    bob.core().answer_call(&call, "v=0 answer").await.expect("bob answers");
    until("alice hears the answer", || async {
        alice.core().store().call(&call).await.unwrap().is_some_and(|record| record.answered_at.is_some())
    })
    .await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(fake.is_connected(&bob.id()) && alice.online.network.is_connected(&bob.id()).await, "the call goes on");

    alice.core().end_call(&call, false).await.unwrap();
    until("bob's call ends", || async { bob.core().current_call().await.unwrap().is_none() }).await;
    let ended = Instant::now();
    until_within("bob lets go once the call is over", LINGER + Duration::from_secs(3), || async { !fake.is_connected(&bob.id()) }).await;
    assert!(ended.elapsed() >= LINGER - Duration::from_millis(500), "a moment for the hang-up to go out first");
    until_within("the direct connection goes too", Duration::from_secs(3), || async { !alice.online.network.is_connected(&bob.id()).await }).await;
}

// The other phone no longer believes the direct connection is open ("Direct"): it closes too.
#[tokio::test(flavor = "multi_thread")]
async fn the_other_phone_sees_the_direct_connection_close() {
    let (_fake, alice, bob) = two_phones().await;
    alice.core().send_text(&bob.id(), "hi").await.unwrap();
    until("both see the direct connection", || async {
        alice.online.network.is_connected(&bob.id()).await && bob.online.network.is_connected(&alice.id()).await
    })
    .await;

    bob.online.set_foreground(false).await;
    until_within("alice sees it closed", Duration::from_secs(3), || async { !alice.online.network.is_connected(&bob.id()).await }).await;
    assert!(!bob.online.network.is_connected(&alice.id()).await);
}

// A registration still waiting when the app leaves (the router could not be reached) is not lost:
// it goes as soon as the app is back, with what the phone says then.
#[tokio::test(flavor = "multi_thread")]
async fn a_registration_waiting_when_the_app_leaves_goes_out_when_it_is_back() {
    let (fake, _alice, bob) = two_phones().await;
    until("bob registered", || async { fake.with(|inner| inner.registrations.iter().any(|(who, _)| *who == bob.id())) }).await;
    let tries = fake.with(|inner| {
        inner.refusing = true;
        inner.register_tries
    });
    bob.core().open_session("482915").await.unwrap().expect("a session");
    // Three tries failed: the next waits several seconds more (the phone backs off).
    until("three tries failed", || async { fake.with(|inner| inner.register_tries) >= tries + 3 }).await;
    bob.online.set_foreground(false).await;
    fake.with(|inner| inner.refusing = false);

    let back = Instant::now();
    bob.online.set_foreground(true).await;
    let wanted = bob.core().silent_slots().await.unwrap();
    until_within("the waiting registration went out", Duration::from_millis(1_500), || async {
        fake.with(|inner| inner.registrations.last().is_some_and(|(who, silent)| *who == bob.id() && *silent == wanted & !1))
    })
    .await;
    eprintln!("registered {:?} after coming back", back.elapsed());
}

// Leaving twice or coming back twice changes nothing more than once: one socket when back.
#[tokio::test(flavor = "multi_thread")]
async fn leaving_or_coming_back_twice_is_harmless() {
    let (fake, _alice, bob) = two_phones().await;
    assert!(!bob.online.set_foreground(false).await, "leaving brings nothing back");
    until_within("the router sees bob gone", Duration::from_secs(1), || async { !fake.is_connected(&bob.id()) }).await;
    let opened = fake.opened(&bob.id());
    assert!(!bob.online.set_foreground(false).await);
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(!fake.is_connected(&bob.id()));

    assert!(bob.online.set_foreground(true).await, "back");
    assert!(!bob.online.set_foreground(true).await, "back already");
    until_within("bob is connected again", Duration::from_secs(2), || async { fake.is_connected(&bob.id()) }).await;
    tokio::time::sleep(Duration::from_millis(1_500)).await;
    assert_eq!(fake.opened(&bob.id()), opened + 1, "one socket");
}

// A process a call push started out of the foreground (iOS PushKit with no scene) connects for the
// call and lets go a moment after it is over, so the router pushes again: a redial rings, through
// the push that brings the connection back.
#[tokio::test(flavor = "multi_thread")]
async fn a_core_a_push_started_lets_go_after_its_call_and_a_redial_rings_again() {
    let (base, fake) = fake_router().await;
    let alice = phone(&base, "Alice", true).await;
    let bob = phone(&base, "Bob", false).await;
    pair(&fake, &alice, &bob).await;

    let first = call(&alice, &bob).await;
    until("bob's phone rings", || rings(&bob, &first)).await;
    alice.core().end_call(&first, false).await.unwrap();
    until("bob's call ends", || async { bob.core().current_call().await.unwrap().is_none() }).await;
    until_within("bob lets go once the call is over", LINGER + Duration::from_secs(3), || async { !fake.is_connected(&bob.id()) }).await;

    let rings_before = fake.pushes_to(&bob.id()).iter().filter(|push| push.call).count();
    let second = call(&alice, &bob).await;
    until("the redial is pushed as a call", || async { fake.pushes_to(&bob.id()).iter().filter(|push| push.call).count() > rings_before }).await;
    // The push: PushKit (or FCM) hands the call to the app, which connects for it.
    assert!(bob.online.woken().await, "the push brought the connection back");
    until("bob's phone rings again", || rings(&bob, &second)).await;
    alice.core().end_call(&second, false).await.unwrap();
}

// Out of the foreground nothing is retried: a message waiting in the outbox does not open a
// connection (the other phone would see this one as there) nor have the router wake this phone
// for the answer. It goes when the app is back.
#[tokio::test(flavor = "multi_thread")]
async fn nothing_is_retried_in_the_background() {
    let (fake, alice, bob) = two_phones().await;
    // Strictly direct: Bob's message waits in his outbox until Alice can be reached.
    bob.core().set_mailbox(false).await.unwrap();
    alice.online.set_foreground(false).await;
    until("the router sees alice gone", || async { !fake.is_connected(&alice.id()) }).await;
    let sent = bob.core().send_text(&alice.id(), "later").await.unwrap();
    bob.online.set_foreground(false).await;
    until("the router sees bob gone", || async { !fake.is_connected(&bob.id()) }).await;

    alice.online.set_foreground(true).await;
    until("alice is back", || async { fake.is_connected(&alice.id()) }).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let signalled = fake.signalled_to(&alice.id());
    tokio::time::sleep(Duration::from_secs(12)).await;
    assert_eq!(fake.signalled_to(&alice.id()), signalled, "bob tried nothing while away");
    assert!(texts(&alice, &bob.id()).await.is_empty());

    bob.online.set_foreground(true).await;
    until("alice has it", || async { texts(&alice, &bob.id()).await == ["later"] }).await;
    until("bob sees it delivered", || async { state(&bob, &sent).await == MessageState::Delivered }).await;
}

// Leaving a session (2026-10-01). The router (0.6.0) stops what would come for a session the phone
// declared silent, but it cannot close a direct connection already open: the phone closes those
// with the session's contacts itself, at once.

const LEFT_PIN: &str = "482915";

/// Bob scans the other phone's card into his session; they say yes.
async fn pair_in(fake: &Fake, bob: &Phone, other: &Phone, session: &str) {
    until("they are connected", || async { fake.is_connected(&other.id()) }).await;
    let link = other.core().my_card().await.unwrap().to_link();
    bob.core().add_contact_in(&link, None, Some(session)).await.expect("bob adds them in the session");
    let id = bob.id();
    until("they know bob", || async { other.core().store().contact(&id).await.unwrap().is_some() }).await;
    other.core().accept_contact(&id).await.expect("they accept bob");
}

/// A text from Bob, delivered over a direct connection both sides see open.
async fn linked(bob: &Phone, other: &Phone) {
    let sent = bob.core().send_text(&other.id(), "hi").await.unwrap();
    until("delivered", || async { state(bob, &sent).await == MessageState::Delivered }).await;
    until("both see the direct connection", || async { both_connected(bob, other).await }).await;
}

async fn both_connected(bob: &Phone, other: &Phone) -> bool {
    bob.online.network.is_connected(&other.id()).await && other.online.network.is_connected(&bob.id()).await
}

/// Bob with Alice in his main list, Carol in the session he will leave and Dave in another open
/// one, a direct connection open with each. Returns (Bob, Alice, Carol, Dave, the session to leave).
async fn bob_with_sessions() -> (Phone, Phone, Phone, Phone, String) {
    let (base, fake) = fake_router().await;
    let bob = phone(&base, "Bob", true).await;
    let (alice, carol, dave) = (phone(&base, "Alice", true).await, phone(&base, "Carol", true).await, phone(&base, "Dave", true).await);
    pair(&fake, &alice, &bob).await;
    let left = bob.core().open_session(LEFT_PIN).await.unwrap().expect("a session");
    let kept = bob.core().open_session("135790").await.unwrap().expect("another");
    pair_in(&fake, &bob, &carol, &left).await;
    pair_in(&fake, &bob, &dave, &kept).await;
    for other in [&alice, &carol, &dave] {
        linked(&bob, other).await;
    }
    (bob, alice, carol, dave, left)
}

#[tokio::test(flavor = "multi_thread")]
async fn leaving_a_session_closes_its_direct_connections_and_no_other() {
    let (bob, alice, carol, dave, left) = bob_with_sessions().await;

    assert!(!bob.core().close_session(&left).await.unwrap(), "carol is in it: it stays");
    until_within("carol sees it closed", Duration::from_secs(3), || async { !carol.online.network.is_connected(&bob.id()).await }).await;
    assert!(!bob.online.network.is_connected(&carol.id()).await);

    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(!both_connected(&bob, &carol).await, "it stays closed");
    assert!(both_connected(&bob, &alice).await, "the main list's connection stays");
    assert!(both_connected(&bob, &dave).await, "another open session's connection stays");
}

#[tokio::test(flavor = "multi_thread")]
async fn deleting_a_session_closes_its_direct_connections_and_no_other() {
    let (bob, alice, carol, dave, left) = bob_with_sessions().await;

    bob.core().remove_session(&left).await.unwrap();
    until_within("carol sees it closed", Duration::from_secs(3), || async { !carol.online.network.is_connected(&bob.id()).await }).await;
    assert!(!bob.online.network.is_connected(&carol.id()).await);

    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(both_connected(&bob, &alice).await, "the main list's connection stays");
    assert!(both_connected(&bob, &dave).await, "another open session's connection stays");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_opened_again_opens_a_connection_when_it_needs_one() {
    let (bob, _alice, carol, _dave, left) = bob_with_sessions().await;
    bob.core().close_session(&left).await.unwrap();
    until("carol sees it closed", || async { !carol.online.network.is_connected(&bob.id()).await }).await;

    assert_eq!(bob.core().open_session(LEFT_PIN).await.unwrap(), Some(left));
    let sent = bob.core().send_text(&carol.id(), "back").await.unwrap();
    until("carol has it", || async { texts(&carol, &bob.id()).await.last().map(String::as_str) == Some("back") }).await;
    until("delivered", || async { state(&bob, &sent).await == MessageState::Delivered }).await;
    until("both see the direct connection again", || async { both_connected(&bob, &carol).await }).await;
}

// A call with one of the session's contacts when it is left: leaving means nothing more from them
// reaches the phone, and the router would hold back their hang-up from now on, so the call is hung
// up first, as the red button would, and its end goes out before the connection closes.
#[tokio::test(flavor = "multi_thread")]
async fn leaving_a_session_hangs_up_a_call_with_one_of_its_contacts() {
    let (bob, _alice, carol, _dave, left) = bob_with_sessions().await;
    let call = call(&carol, &bob).await;
    until("bob's phone rings", || rings(&bob, &call)).await;
    bob.core().answer_call(&call, "v=0 answer").await.expect("bob answers");
    until("carol hears the answer", || async {
        carol.core().store().call(&call).await.unwrap().is_some_and(|record| record.answered_at.is_some())
    })
    .await;

    bob.core().close_session(&left).await.unwrap();
    assert!(bob.core().current_call().await.unwrap().is_none(), "bob's call is over");
    until_within("carol's call is over", Duration::from_secs(3), || async { carol.core().current_call().await.unwrap().is_none() }).await;
    let ended = carol.core().store().call(&call).await.unwrap().expect("the call");
    assert_eq!(ended.outcome, Some(ft_storage::CallOutcome::Answered), "hung up, not failed");
    until_within("carol sees the connection closed", Duration::from_secs(3), || async { !carol.online.network.is_connected(&bob.id()).await }).await;
}

// A suggestion from the app (2026-10-02): it goes to the router signed by this phone, as the app
// writes it without the spaces around it, with the app's version and the platform it runs on.
// Only the router's 204 is "sent"; its 429 is the day's limit and anything else a failure.
#[tokio::test(flavor = "multi_thread")]
async fn a_suggestion_reaches_the_router_with_the_app_version_and_the_platform() {
    let (base, fake) = fake_router().await;
    let phone = phone(&base, "Alice", true).await;
    assert_eq!(phone.online.send_feedback("  Stickers, please\n", "1.3.0").await, Feedback::Sent);
    let sent = fake.with(|inner| inner.feedback.clone());
    assert_eq!(sent, [(phone.id(), json!({ "text": "Stickers, please", "app": "1.3.0", "platform": std::env::consts::OS }))]);

    fake.with(|inner| inner.feedback_answer = Some(StatusCode::TOO_MANY_REQUESTS));
    assert_eq!(phone.online.send_feedback("one more", "1.3.0").await, Feedback::TooMany);
    fake.with(|inner| inner.feedback_answer = Some(StatusCode::SERVICE_UNAVAILABLE));
    assert_eq!(phone.online.send_feedback("one more", "1.3.0").await, Feedback::Failed);
}
