//! The router client against a small fake router: signatures the real router would accept, and
//! how answers are understood. The ignored test runs against the live api.flickertalk.com.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use base64::engine::general_purpose::STANDARD_NO_PAD;
use base64::Engine;
use ed25519_dalek::{Signature, VerifyingKey};
use ft_identity::Identity;
use ft_push::{canonical, MailboxRejected, RouterClient, RouterEvent, Signalled, Signer};
use serde_json::{json, Value};

/// The device identity, exactly as the app signs (vodozemac).
struct Device(Mutex<Identity>);

#[async_trait]
impl Signer for Device {
    fn device_id(&self) -> String {
        self.0.lock().unwrap().device_id().to_string()
    }

    async fn signing_key(&self) -> String {
        self.0.lock().unwrap().signing_key().to_base64()
    }

    async fn sign(&self, message: &[u8]) -> String {
        self.0.lock().unwrap().sign(message).to_base64()
    }
}

fn device() -> Arc<Device> {
    Arc::new(Device(Mutex::new(Identity::generate())))
}

/// What the fake router saw.
#[derive(Default)]
struct Seen {
    verified: Mutex<Vec<String>>,
    /// The key the device registered, to check its later requests.
    key: Mutex<Option<[u8; 32]>>,
    push: Mutex<Vec<Value>>,
    registration: Mutex<Option<Value>>,
}

/// Checks the signature headers the way ft-router does, with ed25519-dalek.
fn check(key: &[u8; 32], method: &str, path: &str, headers: &HeaderMap, body: &[u8]) -> bool {
    let get = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_owned);
    let (Some(time), Some(nonce), Some(signature)) = (get("ft-time"), get("ft-nonce"), get("ft-signature")) else {
        return false;
    };
    let text = canonical(method, path, time.parse().unwrap(), &nonce, body);
    let signature: [u8; 64] = STANDARD_NO_PAD.decode(signature).unwrap().try_into().unwrap();
    VerifyingKey::from_bytes(key).unwrap().verify_strict(&text, &Signature::from_bytes(&signature)).is_ok()
}

async fn fake_router() -> (String, Arc<Seen>) {
    let seen = Arc::new(Seen::default());
    let router = Router::new()
        .route(
            "/v1/device/register",
            post(|State(seen): State<Arc<Seen>>, headers: HeaderMap, body: Bytes| async move {
                let registration: Value = serde_json::from_slice(&body).unwrap();
                let key: [u8; 32] = STANDARD_NO_PAD.decode(registration["signing_key"].as_str().unwrap()).unwrap().try_into().unwrap();
                if check(&key, "POST", "/v1/device/register", &headers, &body) {
                    seen.verified.lock().unwrap().push("register".to_owned());
                    *seen.key.lock().unwrap() = Some(key);
                    *seen.registration.lock().unwrap() = Some(registration.clone());
                    StatusCode::NO_CONTENT
                } else {
                    StatusCode::UNAUTHORIZED
                }
            }),
        )
        .route(
            "/v1/device/push",
            put(|State(seen): State<Arc<Seen>>, headers: HeaderMap, body: Bytes| async move {
                let key = seen.key.lock().unwrap().expect("registered first");
                if check(&key, "PUT", "/v1/device/push", &headers, &body) {
                    seen.push.lock().unwrap().push(serde_json::from_slice(&body).unwrap());
                    StatusCode::NO_CONTENT
                } else {
                    StatusCode::UNAUTHORIZED
                }
            }),
        )
        .route(
            "/v1/signal/{to}",
            post(|Path(to): Path<String>, headers: HeaderMap| async move {
                match (to.as_str(), headers.get("ft-capability").is_some()) {
                    ("ft_online", true) => StatusCode::ACCEPTED.into_response(),
                    // Router 0.4.0: not connected, but the signal waits for it in memory.
                    ("ft_sleeping", true) => (StatusCode::NOT_FOUND, [("ft-retained", "1")]).into_response(),
                    (_, true) => StatusCode::NOT_FOUND.into_response(),
                    _ => StatusCode::FORBIDDEN.into_response(),
                }
            }),
        )
        .route(
            "/v1/mailbox",
            get(|| async { Json(json!([{ "id": "0190-1", "blob": STANDARD_NO_PAD.encode(b"sealed") }])) }),
        )
        .route(
            "/v1/mailbox/{target}",
            // The router's answers to a deposit (ft-router `deposit`), one recipient each.
            post(|Path(target): Path<String>| async move {
                match target.as_str() {
                    "ft_gone" => StatusCode::FORBIDDEN,
                    "ft_busy" => StatusCode::TOO_MANY_REQUESTS,
                    "ft_huge" => StatusCode::PAYLOAD_TOO_LARGE,
                    "ft_broken" => StatusCode::INTERNAL_SERVER_ERROR,
                    "ft_full" => StatusCode::INSUFFICIENT_STORAGE,
                    "ft_moved" => StatusCode::NOT_FOUND,
                    _ => StatusCode::CREATED,
                }
            })
            .delete(|| async { StatusCode::NO_CONTENT }),
        )
        .with_state(seen.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (format!("http://{address}"), seen)
}

// The identity signs with vodozemac; the router verifies with ed25519-dalek: they must agree.
#[tokio::test]
async fn registration_is_signed_so_the_router_accepts_it() {
    let (base, seen) = fake_router().await;
    let client = RouterClient::new(&base, device()).expect("client");
    client.register(&eight(), 0).await.expect("registers");
    assert_eq!(seen.verified.lock().unwrap().as_slice(), ["register"]);
}

/// Eight capability hashes, the first the device's own.
fn eight() -> [[u8; 32]; 8] {
    std::array::from_fn(|slot| [slot as u8 + 5; 32])
}

// app#9: always eight capabilities, the first also as the one routers before knew.
#[tokio::test]
async fn registration_hands_over_eight_capabilities() {
    let (base, seen) = fake_router().await;
    let client = RouterClient::new(&base, device()).expect("client");
    client.register(&eight(), 0).await.expect("registers");
    let registration = seen.registration.lock().unwrap().clone().expect("registered");
    let hashes = registration["capability_hashes"].as_array().expect("a list").clone();
    assert_eq!(hashes.len(), 8);
    assert_eq!(registration["capability_hash"], hashes[0]);
    assert_eq!(hashes[3], json!(STANDARD_NO_PAD.encode([8u8; 32])));
}

// 2026-10-01 (§108): the router sends no push for a silent slot. Bit i of `silent_slots` is slot i;
// the phone sets it for a session that is closed (and at random for a spare one), never for the
// main list. Every registration replaces the last one, so it always goes, 0 included.
#[tokio::test]
async fn registration_says_which_slots_are_silent() {
    let (base, seen) = fake_router().await;
    let client = RouterClient::new(&base, device()).expect("client");
    client.register(&eight(), 0b1010_0100).await.expect("registers");
    let registration = seen.registration.lock().unwrap().clone().expect("registered");
    assert_eq!(registration["silent_slots"], json!(0b1010_0100));

    client.register(&eight(), 0).await.expect("registers again");
    let registration = seen.registration.lock().unwrap().clone().expect("registered");
    assert_eq!(registration["silent_slots"], json!(0), "nothing silent is said too");
}

// The main list (slot 0) is never silent, whatever the caller passes.
#[tokio::test]
async fn the_main_list_is_never_registered_as_silent() {
    let (base, seen) = fake_router().await;
    let client = RouterClient::new(&base, device()).expect("client");
    client.register(&eight(), 0b1111_1111).await.expect("registers");
    let registration = seen.registration.lock().unwrap().clone().expect("registered");
    assert_eq!(registration["silent_slots"], json!(0b1111_1110));
}

// §8: where this device can be woken, signed like every request.
#[tokio::test]
async fn the_push_token_is_left_signed() {
    let (base, seen) = fake_router().await;
    let client = RouterClient::new(&base, device()).expect("client");
    client.register(&eight(), 0).await.expect("registers");
    client.set_push("fcm", "fcm-token-1").await.expect("leaves the token");
    assert_eq!(seen.push.lock().unwrap().as_slice(), [json!({ "provider": "fcm", "token": "fcm-token-1" })]);
}

#[tokio::test]
async fn a_signal_reports_whether_the_recipient_is_online() {
    let (base, _) = fake_router().await;
    let client = RouterClient::new(&base, device()).expect("client");
    assert_eq!(client.signal("ft_online", &[1; 32], b"offer".to_vec()).await.expect("answers"), Signalled::Delivered);
    assert_eq!(client.signal("ft_away", &[1; 32], b"offer".to_vec()).await.expect("answers"), Signalled::NotConnected);
}

// Router 0.4.0 (2026-09-29): a signal for a recipient that is not connected is still a 404, now
// with `ft-retained: 1` when the router keeps it for the recipient's next connection. A router
// before 0.4 sends no such header: the signal is lost, as before.
#[tokio::test]
async fn a_signal_the_router_keeps_for_later_says_so() {
    let (base, _) = fake_router().await;
    let client = RouterClient::new(&base, device()).expect("client");
    assert_eq!(client.signal("ft_sleeping", &[1; 32], b"offer".to_vec()).await.expect("answers"), Signalled::Retained);
    assert_eq!(client.signal_call("ft_sleeping", &[1; 32], b"offer".to_vec()).await.expect("answers"), Signalled::Retained);
    assert!(!Signalled::Retained.delivered() && !Signalled::NotConnected.delivered() && Signalled::Delivered.delivered());
}

#[tokio::test]
async fn mail_is_deposited_collected_and_acknowledged() {
    let (base, _) = fake_router().await;
    let client = RouterClient::new(&base, device()).expect("client");
    client.deposit("ft_bob", &[1; 32], b"sealed".to_vec()).await.expect("deposits");
    let mail = client.collect().await.expect("collects");
    assert_eq!(mail.len(), 1);
    assert_eq!(mail[0].blob, b"sealed");
    client.acknowledge(&mail[0].id).await.expect("acknowledges");
}

// A 403 is the router refusing the recipient for good: the core takes the message out of its
// queue. Anything else that fails may work later and is tried again.
#[tokio::test]
async fn only_a_refused_recipient_is_a_rejection() {
    let (base, _) = fake_router().await;
    let client = RouterClient::new(&base, device()).expect("client");
    let refused = client.deposit("ft_gone", &[1; 32], b"sealed".to_vec()).await.expect_err("refused");
    assert_eq!(refused.downcast_ref::<MailboxRejected>(), Some(&MailboxRejected));
    for passing in ["ft_busy", "ft_huge", "ft_broken", "ft_full", "ft_moved"] {
        let failed = client.deposit(passing, &[1; 32], b"sealed".to_vec()).await.expect_err("fails");
        assert!(failed.downcast_ref::<MailboxRejected>().is_none(), "{passing} may work later");
    }
    let unreachable = RouterClient::new("http://127.0.0.1:9", device()).expect("client");
    let offline = unreachable.deposit("ft_bob", &[1; 32], b"sealed".to_vec()).await.expect_err("no network");
    assert!(offline.downcast_ref::<MailboxRejected>().is_none());
}

/// Against the real router: `cargo test -p ft-push -- --ignored`.
#[tokio::test]
#[ignore = "needs api.flickertalk.com"]
async fn two_devices_meet_on_the_live_router() {
    use ft_push::RouterEvent;
    let base = "https://api.flickertalk.com";
    let (alice, bob) = (RouterClient::new(base, device()).unwrap(), Arc::new(RouterClient::new(base, device()).unwrap()));
    let bob_capability = [7u8; 32];
    alice.register(&[[8; 32]; 8], 0).await.expect("alice registers");
    bob.register(&[*blake3::hash(&bob_capability).as_bytes(); 8], 0).await.expect("bob registers");

    let mut events = bob.listen();
    let Some(RouterEvent::Connected { stun, turn }) = events.recv().await else { panic!("a welcome") };
    assert!(!stun.is_empty());
    assert!(turn.is_some());

    assert_eq!(alice.signal(&bob.device_id(), &bob_capability, b"offer".to_vec()).await.expect("signals"), Signalled::Delivered);
    assert_eq!(events.recv().await, Some(RouterEvent::Signal(b"offer".to_vec())));

    alice.deposit(&bob.device_id(), &bob_capability, b"sealed".to_vec()).await.expect("deposits");
    assert_eq!(events.recv().await, Some(RouterEvent::Mail));
    let mail = bob.collect().await.expect("collects");
    assert_eq!(mail[0].blob, b"sealed");
    bob.acknowledge(&mail[0].id).await.expect("acknowledges");
    assert!(bob.collect().await.expect("collects").is_empty());

    alice.forget().await.expect("forgets");
    bob.forget().await.expect("forgets");
}

// iOS suspends the app and cuts its socket, and the phone may not notice for a long while
// (2026-09-28): coming back to the screen, or tapping a push, reconnects at once and so fetches
// what waits, without waiting for a dead socket or the backoff.
#[tokio::test]
async fn the_socket_is_opened_again_at_once_when_asked() {
    use axum::extract::ws::{Message as WsMessage, WebSocketUpgrade};
    let connections = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = connections.clone();
    let router = Router::new().route(
        "/v1/connect",
        get(move |upgrade: WebSocketUpgrade| {
            let counted = counted.clone();
            async move {
                upgrade.on_upgrade(move |mut socket| async move {
                    counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let welcome = json!({ "kind": "welcome", "stun": [], "turn": null }).to_string();
                    let _ = socket.send(WsMessage::Text(welcome.into())).await;
                    // Never closes: like a socket iOS cut while the app slept, it just goes quiet.
                    tokio::time::sleep(Duration::from_secs(60)).await;
                })
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    let client = Arc::new(RouterClient::new(&format!("http://{address}"), device()).unwrap());
    let mut events = client.listen();
    let connected = |event: Option<RouterEvent>| matches!(event, Some(RouterEvent::Connected { .. }));
    assert!(connected(tokio::time::timeout(Duration::from_secs(5), events.recv()).await.unwrap()));

    client.reconnect_now();
    let again = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if connected(events.recv().await) {
                return;
            }
        }
    })
    .await;
    assert!(again.is_ok(), "connected again at once, not after the socket died");
    assert_eq!(connections.load(std::sync::atomic::Ordering::SeqCst), 2);
}

/// Whether a welcome comes within a few seconds.
async fn welcomed(events: &mut tokio::sync::mpsc::Receiver<RouterEvent>) -> bool {
    let waiting = async {
        loop {
            if matches!(events.recv().await, Some(RouterEvent::Connected { .. })) {
                return;
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(5), waiting).await.is_ok()
}

// A call push wakes a suspended iPhone, and CallKit's answer and audio activation follow within
// seconds (2026-09-28): each asks for a fresh socket, but a socket that is being opened or was
// just opened is not dropped again, or the call's offer could be lost in the gap.
#[tokio::test]
async fn a_fresh_socket_is_kept_and_an_old_one_is_opened_again() {
    use axum::extract::ws::{Message as WsMessage, WebSocketUpgrade};
    let connections = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = connections.clone();
    let router = Router::new().route(
        "/v1/connect",
        get(move |upgrade: WebSocketUpgrade| {
            let counted = counted.clone();
            async move {
                upgrade.on_upgrade(move |mut socket| async move {
                    counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    // A slow welcome: the socket is still being opened meanwhile.
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    let welcome = json!({ "kind": "welcome", "stun": [], "turn": null }).to_string();
                    let _ = socket.send(WsMessage::Text(welcome.into())).await;
                    tokio::time::sleep(Duration::from_secs(60)).await;
                })
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    let client = Arc::new(RouterClient::new(&format!("http://{address}"), device()).unwrap());
    let mut events = client.listen();
    tokio::time::sleep(Duration::from_millis(200)).await;
    client.reconnect_unless_fresh(Duration::from_secs(60));
    assert!(welcomed(&mut events).await, "the socket being opened goes on");
    client.reconnect_unless_fresh(Duration::from_secs(60));
    tokio::time::sleep(Duration::from_millis(1_000)).await;
    assert_eq!(connections.load(std::sync::atomic::Ordering::SeqCst), 1, "a fresh socket is kept");

    // An older one (here, anything older than nothing) is opened again at once.
    client.reconnect_unless_fresh(Duration::ZERO);
    assert!(welcomed(&mut events).await, "connected again");
    assert_eq!(connections.load(std::sync::atomic::Ordering::SeqCst), 2);
}

// A call's signal says it is one, and nothing else does (2026-09-28): the router rings an
// offline iPhone only for a call.
#[tokio::test]
async fn only_a_call_signal_says_it_is_a_call() {
    let marked = Arc::new(Mutex::new(Vec::<bool>::new()));
    let seen = marked.clone();
    let router = Router::new().route(
        "/v1/signal/{to}",
        post(move |headers: HeaderMap| {
            let seen = seen.clone();
            async move {
                seen.lock().unwrap().push(headers.get("ft-call").is_some_and(|value| value == "1"));
                StatusCode::NOT_FOUND
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = RouterClient::new(&format!("http://{address}"), device()).unwrap();
    assert_eq!(client.signal("ft_bob", &[1; 32], b"offer".to_vec()).await.unwrap(), Signalled::NotConnected);
    assert_eq!(client.signal_call("ft_bob", &[1; 32], b"offer".to_vec()).await.unwrap(), Signalled::NotConnected);
    assert_eq!(*marked.lock().unwrap(), [false, true]);
}

// Erasing the phone (2026-09-30): iOS cannot start the app again, so the old identity's client is
// closed in the running app. Its socket goes and is not opened again, and it asks nothing more.
#[tokio::test]
async fn a_closed_client_lets_its_socket_go_and_asks_nothing_more() {
    use axum::extract::ws::{Message as WsMessage, WebSocketUpgrade};
    let connections = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (counted, registered) = (connections.clone(), Arc::new(std::sync::atomic::AtomicUsize::new(0)));
    let registrations = registered.clone();
    let router = Router::new()
        .route(
            "/v1/connect",
            get(move |upgrade: WebSocketUpgrade| {
                let counted = counted.clone();
                async move {
                    upgrade.on_upgrade(move |mut socket| async move {
                        counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        let welcome = json!({ "kind": "welcome", "stun": [], "turn": null }).to_string();
                        let _ = socket.send(WsMessage::Text(welcome.into())).await;
                        tokio::time::sleep(Duration::from_secs(60)).await;
                    })
                }
            }),
        )
        .route(
            "/v1/device/register",
            post(move || {
                let registrations = registrations.clone();
                async move {
                    registrations.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    StatusCode::NO_CONTENT
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    let client = Arc::new(RouterClient::new(&format!("http://{address}"), device()).unwrap());
    let mut events = client.listen();
    assert!(welcomed(&mut events).await);

    client.close();
    assert!(client.is_closed());
    let ended = tokio::time::timeout(Duration::from_secs(3), async { while events.recv().await.is_some() {} }).await;
    assert!(ended.is_ok(), "the socket's events end");
    client.reconnect_now();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(connections.load(std::sync::atomic::Ordering::SeqCst), 1, "never opened again");
    assert!(client.register(&eight(), 0).await.is_err());
    assert_eq!(registered.load(std::sync::atomic::Ordering::SeqCst), 0, "nothing reaches the router");
}
