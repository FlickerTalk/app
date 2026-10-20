//! A phone that changes networks with the app open (app#200, 2026-10-20). Seen on an iPhone moved
//! from mobile data to Wi-Fi: texts went on, but a call's end reached the other phone late or not
//! at all, and a call rang 40 s late, until both apps were restarted.
//!
//! What a network change leaves behind: the socket to the router stays open on a path that is
//! gone. Nothing closes it; it goes quiet, and the router, which still takes the phone for
//! connected, hands it what comes for the phone (the offers that open a direct connection, so the
//! hang-up that needs one). The direct connections on the old path die the same way.
//!
//! Two complete phones (`online::start`, with the real router client) against a fake router with a
//! real WebSocket that can make one phone's socket go quiet as a network change does. The direct
//! connections are real WebRTC on loopback.

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
use ft_core::calls::CALL_REACH;
use ft_core::online::{self, Online};
use ft_core::{CallPhase, Core};
use ft_storage::{CallOutcome, Store};
use ft_webrtc::SessionConfig;
use serde_json::json;
use tokio::sync::{mpsc, Notify};

/// A phone's socket as the fake router holds it.
struct Socket {
    id: u64,
    frames: mpsc::UnboundedSender<String>,
    /// Makes this socket go quiet, as a network change does.
    quiet: Arc<Notify>,
}

#[derive(Default)]
struct Inner {
    /// Who the router takes for connected, and how to reach their socket.
    connected: HashMap<String, Socket>,
    /// How many sockets each device opened.
    opened: HashMap<String, usize>,
    /// Signals for a device that is not connected, handed over after its next welcome (0.4).
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

    /// The device's socket goes quiet: from now on nothing is read from it or written to it, and
    /// what the router hands it is lost. The router still takes the device for connected, until
    /// the device opens another socket.
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
                // Reading is also what answers the client's pings.
                message = socket.recv() => open = matches!(message, Some(Ok(message)) if !matches!(message, Message::Close(_))),
                () = quiet.notified() => {
                    // The path is gone: nothing more is read or written and what comes is lost,
                    // until the device opens another socket and this one is forgotten.
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

/// A phone started as the app starts it, in front.
async fn phone(base: &str, name: &str) -> Phone {
    let online = online::start(Store::open_in_memory().await.unwrap(), [4; 32], base, SessionConfig::offline()).await.unwrap();
    online.core.set_name(name).await.unwrap();
    // The WebView's calls: no audio device in a test.
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

/// Two phones in front, connected to the router and paired.
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

/// Alice calls Bob (the WebView's way) and Bob answers: the call is on.
async fn call_on(alice: &Phone, bob: &Phone) -> String {
    let call = alice.core().place_call(&bob.id(), false).await.expect("alice calls");
    let (core, id) = (alice.core().clone(), call.clone());
    tokio::spawn(async move { core.offer_call(&id, "v=0 offer").await });
    until("bob's phone rings", || async {
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

/// Bob's network changes under the app: his socket to the router and his direct connection with
/// Alice go quiet on a path that is gone, and neither side has noticed yet.
async fn network_changes_under(fake: &Fake, alice: &Phone, bob: &Phone) {
    fake.silence(&bob.id());
    bob.online.network.vanish(&alice.id()).await;
    alice.online.network.vanish(&bob.id()).await;
}

/// Alice hangs up, in the background (the end waits for a connection), and the moment she did.
fn hang_up(alice: &Phone, call: &str) -> Instant {
    let (core, id) = (alice.core().clone(), call.to_owned());
    tokio::spawn(async move { core.end_call(&id, false).await });
    Instant::now()
}

async fn ended_as_answered(phone: &Phone, call: &str) -> bool {
    phone.core().current_call().await.unwrap().is_none()
        && phone.core().store().call(call).await.unwrap().is_some_and(|record| record.outcome == Some(CallOutcome::Answered))
}

// Nothing on the phone says its network changed (the same addresses, or a path that died on its
// own): the quiet socket is noticed by its unanswered pings and opened again, and the hang-up,
// which keeps trying while a call would, gets through on it. Before, the socket was never noticed
// and the hang-up never arrived: the iPhone counted on and logged the call as failed.
#[tokio::test(flavor = "multi_thread")]
async fn a_hang_up_gets_through_once_a_quiet_router_socket_is_opened_again() {
    let (fake, alice, bob) = two_phones().await;
    let call = call_on(&alice, &bob).await;

    network_changes_under(&fake, &alice, &bob).await;
    let hung_up = hang_up(&alice, &call);
    until_within("bob's call ends as a hang-up", CALL_REACH, || ended_as_answered(&bob, &call)).await;
    eprintln!("bob heard the hang-up {} ms after it", hung_up.elapsed().as_millis());
    assert_eq!(fake.opened(&bob.id()), 2, "through a socket opened again");
}
