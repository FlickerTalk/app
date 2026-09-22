//! The router client against a small fake router: signatures the real router would accept, and
//! how answers are understood. The ignored test runs against the live api.flickertalk.com.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::engine::general_purpose::STANDARD_NO_PAD;
use base64::Engine;
use ed25519_dalek::{Signature, VerifyingKey};
use ft_identity::Identity;
use ft_push::{canonical, RouterClient, Signer};
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
                    ("ft_online", true) => StatusCode::ACCEPTED,
                    (_, true) => StatusCode::NOT_FOUND,
                    _ => StatusCode::FORBIDDEN,
                }
            }),
        )
        .route(
            "/v1/mailbox",
            get(|| async { Json(json!([{ "id": "0190-1", "blob": STANDARD_NO_PAD.encode(b"sealed") }])) }),
        )
        .route(
            "/v1/mailbox/{target}",
            post(|| async { StatusCode::CREATED }).delete(|| async { StatusCode::NO_CONTENT }),
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
    client.register(&[5; 32]).await.expect("registers");
    assert_eq!(seen.verified.lock().unwrap().as_slice(), ["register"]);
}

#[tokio::test]
async fn a_signal_reports_whether_the_recipient_is_online() {
    let (base, _) = fake_router().await;
    let client = RouterClient::new(&base, device()).expect("client");
    assert!(client.signal("ft_online", &[1; 32], b"offer".to_vec()).await.expect("answers"));
    assert!(!client.signal("ft_away", &[1; 32], b"offer".to_vec()).await.expect("answers"));
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

/// Against the real router: `cargo test -p ft-push -- --ignored`.
#[tokio::test]
#[ignore = "needs api.flickertalk.com"]
async fn two_devices_meet_on_the_live_router() {
    use ft_push::RouterEvent;
    let base = "https://api.flickertalk.com";
    let (alice, bob) = (RouterClient::new(base, device()).unwrap(), Arc::new(RouterClient::new(base, device()).unwrap()));
    let bob_capability = [7u8; 32];
    alice.register(&[8; 32]).await.expect("alice registers");
    bob.register(blake3::hash(&bob_capability).as_bytes()).await.expect("bob registers");

    let mut events = bob.listen();
    let Some(RouterEvent::Connected { stun, turn }) = events.recv().await else { panic!("a welcome") };
    assert!(!stun.is_empty());
    assert!(turn.is_some());

    assert!(alice.signal(&bob.device_id(), &bob_capability, b"offer".to_vec()).await.expect("signals"));
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
