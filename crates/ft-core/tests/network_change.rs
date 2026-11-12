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
use ft_core::calls::HANG_UP_REACH;
use ft_core::online::{self, Online};
use ft_core::{CallPhase, Core};
use ft_storage::{CallOutcome, Store};
use ft_webrtc::SessionConfig;
use serde_json::json;
use tokio::sync::{mpsc, Notify};

struct Socket {
    id: u64,
    frames: mpsc::UnboundedSender<String>,
    quiet: Arc<Notify>,
}

#[derive(Default)]
struct Inner {
    connected: HashMap<String, Socket>,
    opened: HashMap<String, usize>,
    held: HashMap<String, Vec<String>>,
    mail: HashMap<String, Vec<(String, Vec<u8>)>>,
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

    fn silence(&self, device: &str) {
        self.with(|inner| inner.connected.get(device).expect("connected").quiet.notify_one());
    }

    fn deliver(inner: &mut Inner, to: &str, frame: String) -> bool {
        inner.connected.get(to).is_some_and(|socket| socket.frames.send(frame).is_ok())
    }

    async fn serve(self: Arc<Self>, device: String, mut socket: WebSocket) {
        let id = self.next.fetch_add(1, SeqCst);
        let (frames, mut queued) = mpsc::unbounded_channel::<String>();
        let quiet = Arc::new(Notify::new());
        let held = self.with(|inner| {
            inner.connected.insert(device.clone(), Socket { id, frames, quiet: quiet.clone() });
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
                () = quiet.notified() => {
                    while queued.recv().await.is_some() {}
                    open = false;
                }
            }
        }
        self.with(|inner| {
            if inner.connected.get(&device).is_some_and(|socket| socket.id == id) {
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
        .route("/v1/device/register", post(|| async { StatusCode::NO_CONTENT }))
        .route("/v1/device/push", put(|| async { StatusCode::NO_CONTENT }))
        .route(
            "/v1/signal/{to}",
            post(|State(fake): State<Arc<Fake>>, Path(to): Path<String>, body: Bytes| async move {
                let frame = json!({ "kind": "signal", "signal": STANDARD_NO_PAD.encode(&body) }).to_string();
                fake.with(|inner| {
                    if Fake::deliver(inner, &to, frame.clone()) {
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
                    Fake::deliver(inner, &to, json!({ "kind": "mail" }).to_string());
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

async fn phone(base: &str, name: &str) -> Phone {
    let online = online::start(Store::open_in_memory().await.unwrap(), [4; 32], base, SessionConfig::offline()).await.unwrap();
    online.core.set_name(name).await.unwrap();
    online.core.set_call_audio(None);
    online.set_foreground(true).await;
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

async fn two_phones() -> (Arc<Fake>, Phone, Phone) {
    let (base, fake) = fake_router().await;
    let (alice, bob) = (phone(&base, "Alice").await, phone(&base, "Bob").await);
    until("both are connected", || async { fake.is_connected(&alice.id()) && fake.is_connected(&bob.id()) }).await;
    let link = bob.core().my_card().await.unwrap().to_link();
    alice.core().add_contact(&link, None).await.expect("alice adds bob");
    let id = alice.id();
    until("bob knows alice", || async { bob.core().store().contact(&id).await.unwrap().is_some() }).await;
    bob.core().accept_contact(&id).await.expect("bob accepts alice");
    (fake, alice, bob)
}

async fn call_on(alice: &Phone, bob: &Phone) -> String {
    call_on_ringing_within(alice, bob, Duration::from_secs(20)).await
}

async fn call_on_ringing_within(alice: &Phone, bob: &Phone, limit: Duration) -> String {
    let call = alice.core().place_call(&bob.id(), false).await.expect("alice calls");
    let (core, id) = (alice.core().clone(), call.clone());
    tokio::spawn(async move { core.offer_call(&id, "v=0 offer").await });
    until_within("bob's phone rings", limit, || async {
        bob.core().current_call().await.unwrap().is_some_and(|current| current.call == call && current.phase == CallPhase::Ringing)
    })
    .await;
    bob.core().answer_call(&call, "v=0 answer").await.expect("bob answers");
    until("alice hears the answer", || async {
        alice.core().store().call(&call).await.unwrap().is_some_and(|record| record.answered_at.is_some())
    })
    .await;
    call
}

async fn network_changes_under(fake: &Fake, alice: &Phone, bob: &Phone) {
    fake.silence(&bob.id());
    bob.online.network.vanish(&alice.id()).await;
    alice.online.network.vanish(&bob.id()).await;
}

fn hang_up(phone: &Phone, call: &str) -> Instant {
    let (core, id) = (phone.core().clone(), call.to_owned());
    tokio::spawn(async move { core.end_call(&id, false).await });
    Instant::now()
}

async fn ended_as_answered(phone: &Phone, call: &str) -> bool {
    phone.core().current_call().await.unwrap().is_none()
        && phone.core().store().call(call).await.unwrap().is_some_and(|record| record.outcome == Some(CallOutcome::Answered))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hang_up_gets_through_once_a_quiet_router_socket_is_opened_again() {
    let (fake, alice, bob) = two_phones().await;
    let call = call_on(&alice, &bob).await;

    network_changes_under(&fake, &alice, &bob).await;
    let hung_up = hang_up(&alice, &call);
    until_within("bob's call ends as a hang-up", HANG_UP_REACH, || ended_as_answered(&bob, &call)).await;
    eprintln!("bob heard the hang-up {} ms after it", hung_up.elapsed().as_millis());
    assert_eq!(fake.opened(&bob.id()), 2, "through a socket opened again");
}

async fn bob_sees_the_change(fake: &Fake, bob: &Phone) {
    bob.online.lifecycle().network_changed().await;
    until_within("bob's socket is open again", Duration::from_secs(3), || async { fake.opened(&bob.id()) == 2 }).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn after_a_network_change_calls_ring_and_end_at_once_both_ways() {
    let (fake, alice, bob) = two_phones().await;
    fake.silence(&bob.id());
    alice.online.network.vanish(&bob.id()).await;
    bob_sees_the_change(&fake, &bob).await;

    for bob_hangs_up in [false, true] {
        let call = call_on_ringing_within(&alice, &bob, Duration::from_secs(5)).await;
        let (by, other) = if bob_hangs_up { (&bob, &alice) } else { (&alice, &bob) };
        let hung_up = hang_up(by, &call);
        until_within("the other phone's call ends as a hang-up", Duration::from_secs(3), || ended_as_answered(other, &call)).await;
        eprintln!("hang-up heard {} ms after it", hung_up.elapsed().as_millis());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn right_after_a_network_change_a_hang_up_goes_over_a_new_connection() {
    let (fake, alice, bob) = two_phones().await;
    let call = call_on(&alice, &bob).await;
    fake.silence(&bob.id());
    alice.online.network.vanish(&bob.id()).await;
    assert!(bob.online.network.is_connected(&alice.id()).await, "bob has not noticed yet");
    bob_sees_the_change(&fake, &bob).await;

    let hung_up = hang_up(&bob, &call);
    until_within("alice's call ends as a hang-up", Duration::from_secs(3), || ended_as_answered(&alice, &call)).await;
    eprintln!("alice heard the hang-up {} ms after it", hung_up.elapsed().as_millis());
}

#[tokio::test(flavor = "multi_thread")]
async fn away_a_network_change_opens_nothing_and_the_return_opens_once() {
    let (fake, _alice, bob) = two_phones().await;
    bob.online.set_foreground(false).await;
    until("the router sees bob gone", || async { !fake.is_connected(&bob.id()) }).await;

    bob.online.lifecycle().network_changed().await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(fake.opened(&bob.id()), 1, "nothing opened while away");
    assert!(!fake.is_connected(&bob.id()));

    bob.online.set_foreground(true).await;
    until_within("bob is back", Duration::from_secs(3), || async { fake.opened(&bob.id()) == 2 }).await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(fake.opened(&bob.id()), 2, "opened once");
}
