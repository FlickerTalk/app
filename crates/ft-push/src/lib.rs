//! Client of the router `api.flickertalk.com` (Plan §7, §10, §13–19, §106 M2).
//!
//! Every request is signed with the device's identity (`Signer`): the router checks the
//! signature, the time and a one-time nonce. The signed text is the same on both sides and both
//! pin it with a test vector:
//!
//! ```text
//! FT1\n{METHOD}\n{PATH}\n{time in ms}\n{nonce}\n{hex BLAKE3 of the body}
//! ```
//!
//! Anything addressed to another device (a signal, a blob for its mailbox) carries that device's
//! route capability instead of our identity (§34): the router learns nothing about the sender.
//! This crate transports; it never encrypts (ft-crypto does, before).

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use base64::Engine;
use futures_util::StreamExt;
use reqwest::{Method, RequestBuilder, StatusCode};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

/// Reconnection waits grow up to this.
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// The device identity, which signs the requests (ft-core implements it).
#[async_trait]
pub trait Signer: Send + Sync {
    fn device_id(&self) -> String;
    /// Ed25519 public key, base64 without padding.
    async fn signing_key(&self) -> String;
    /// Ed25519 signature, base64 without padding.
    async fn sign(&self, message: &[u8]) -> String;
}

pub fn canonical(method: &str, path: &str, time_ms: i64, nonce: &str, body: &[u8]) -> Vec<u8> {
    format!("FT1\n{method}\n{path}\n{time_ms}\n{nonce}\n{}", blake3::hash(body).to_hex()).into_bytes()
}

pub fn websocket_base(base: &str) -> String {
    let base = base.trim_end_matches('/');
    match base.strip_prefix("https://") {
        Some(host) => format!("wss://{host}"),
        None => format!("ws://{}", base.trim_start_matches("http://")),
    }
}

pub fn encode(bytes: &[u8]) -> String {
    STANDARD_NO_PAD.encode(bytes)
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// A temporary TURN user from the router (§17).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TurnGrant {
    pub urls: Vec<String>,
    pub username: String,
    pub credential: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mail {
    pub id: String,
    pub blob: Vec<u8>,
}

/// What the router's WebSocket brings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouterEvent {
    /// (Re)connected: the ICE servers to use now.
    Connected { stun: Vec<String>, turn: Option<TurnGrant> },
    /// A signal addressed to this device (an encoded `ft_protocol::Signal`).
    Signal(Vec<u8>),
    /// Something arrived in the mailbox.
    Mail,
    Disconnected,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Frame {
    Welcome {
        #[serde(default)]
        stun: Vec<String>,
        #[serde(default)]
        turn: Option<TurnGrant>,
    },
    Signal {
        signal: String,
    },
    Mail,
}

pub struct RouterClient {
    base: String,
    http: reqwest::Client,
    signer: Arc<dyn Signer>,
}

impl RouterClient {
    pub fn new(base: &str, signer: Arc<dyn Signer>) -> Result<Self> {
        let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .context("no TLS versions")?
            .with_root_certificates(roots)
            .with_no_client_auth();
        let http = reqwest::Client::builder()
            .use_preconfigured_tls(tls)
            .timeout(Duration::from_secs(20))
            .build()
            .context("cannot build the HTTP client")?;
        Ok(Self { base: base.trim_end_matches('/').to_owned(), http, signer })
    }

    pub fn device_id(&self) -> String {
        self.signer.device_id()
    }

    async fn signed_headers(&self, method: &str, path: &str, body: &[u8]) -> Vec<(&'static str, String)> {
        let (time_ms, nonce) = (now_ms(), uuid::Uuid::now_v7().to_string());
        let signature = self.signer.sign(&canonical(method, path, time_ms, &nonce, body)).await;
        vec![
            ("ft-device", self.signer.device_id()),
            ("ft-time", time_ms.to_string()),
            ("ft-nonce", nonce),
            ("ft-signature", signature),
        ]
    }

    async fn signed(&self, method: Method, path: &str, body: Vec<u8>) -> Result<reqwest::Response> {
        let mut request = self.http.request(method.clone(), format!("{}{path}", self.base));
        for (name, value) in self.signed_headers(method.as_str(), path, &body).await {
            request = request.header(name, value);
        }
        send(request.body(body)).await
    }

    /// Registers (again) this device and the hash of its route capability.
    pub async fn register(&self, capability_hash: &[u8; 32]) -> Result<()> {
        let body = json!({ "signing_key": self.signer.signing_key().await, "capability_hash": encode(capability_hash) });
        expect(self.signed(Method::POST, "/v1/device/register", serde_json::to_vec(&body)?).await?, StatusCode::NO_CONTENT)
    }

    /// Where the router can wake this device when it is not connected (§8); `provider` is "fcm".
    pub async fn set_push(&self, provider: &str, token: &str) -> Result<()> {
        let body = json!({ "provider": provider, "token": token });
        expect(self.signed(Method::PUT, "/v1/device/push", serde_json::to_vec(&body)?).await?, StatusCode::NO_CONTENT)
    }

    /// Removes this device and its mail from the router.
    pub async fn forget(&self) -> Result<()> {
        expect(self.signed(Method::DELETE, "/v1/device", Vec::new()).await?, StatusCode::NO_CONTENT)
    }

    pub async fn turn_credentials(&self) -> Result<TurnGrant> {
        let response = self.signed(Method::GET, "/v1/turn-credentials", Vec::new()).await?;
        if response.status() != StatusCode::OK {
            bail!("the router refused the TURN credentials: {}", response.status());
        }
        Ok(response.json().await?)
    }

    /// `true` if the recipient is connected and got it; `false` if it is offline.
    pub async fn signal(&self, to: &str, capability: &[u8; 32], bytes: Vec<u8>) -> Result<bool> {
        let request = self.http.post(format!("{}/v1/signal/{to}", self.base)).header("ft-capability", encode(capability));
        match send(request.body(bytes)).await?.status() {
            StatusCode::ACCEPTED => Ok(true),
            StatusCode::NOT_FOUND => Ok(false),
            status => bail!("the router refused the signal: {status}"),
        }
    }

    /// Leaves an already encrypted blob in the recipient's mailbox (§19).
    pub async fn deposit(&self, to: &str, capability: &[u8; 32], blob: Vec<u8>) -> Result<()> {
        let request = self.http.post(format!("{}/v1/mailbox/{to}", self.base)).header("ft-capability", encode(capability));
        expect(send(request.body(blob)).await?, StatusCode::CREATED)
    }

    pub async fn collect(&self) -> Result<Vec<Mail>> {
        #[derive(Deserialize)]
        struct Listed {
            id: String,
            blob: String,
        }
        let response = self.signed(Method::GET, "/v1/mailbox", Vec::new()).await?;
        if response.status() != StatusCode::OK {
            bail!("the router refused the mailbox: {}", response.status());
        }
        let listed: Vec<Listed> = response.json().await?;
        listed
            .into_iter()
            .map(|mail| Ok(Mail { id: mail.id, blob: STANDARD_NO_PAD.decode(mail.blob.trim_end_matches('='))? }))
            .collect()
    }

    /// The blob is stored on the phone: the router can delete it.
    pub async fn acknowledge(&self, id: &str) -> Result<()> {
        let response = self.signed(Method::DELETE, &format!("/v1/mailbox/{id}"), Vec::new()).await?;
        match response.status() {
            StatusCode::NO_CONTENT | StatusCode::NOT_FOUND => Ok(()),
            status => bail!("the router refused the acknowledgement: {status}"),
        }
    }

    /// Keeps a signed WebSocket open with the router, reconnecting when it drops, until the
    /// receiver is dropped.
    pub fn listen(self: &Arc<Self>) -> mpsc::Receiver<RouterEvent> {
        let (events, receiver) = mpsc::channel(64);
        let client = self.clone();
        tokio::spawn(async move {
            let mut backoff = Duration::from_secs(1);
            loop {
                match client.connection(&events).await {
                    Ok(()) => backoff = Duration::from_secs(1),
                    Err(_) => backoff = (backoff * 2).min(MAX_BACKOFF),
                }
                if events.send(RouterEvent::Disconnected).await.is_err() {
                    return;
                }
                tokio::time::sleep(backoff).await;
            }
        });
        receiver
    }

    async fn connection(&self, events: &mpsc::Sender<RouterEvent>) -> Result<()> {
        let path = "/v1/connect";
        let query: Vec<String> = self
            .signed_headers("GET", path, b"")
            .await
            .into_iter()
            .map(|(name, value)| format!("{name}={}", query_escape(&value)))
            .collect();
        let url = format!("{}{path}?{}", websocket_base(&self.base), query.join("&"));
        let (mut socket, _) = connect_async(url.as_str()).await.context("cannot reach the router")?;
        while let Some(message) = socket.next().await {
            let Message::Text(text) = message? else { continue };
            let event = match serde_json::from_str::<Frame>(text.as_str()) {
                Ok(Frame::Welcome { stun, turn }) => RouterEvent::Connected { stun, turn },
                Ok(Frame::Signal { signal }) => RouterEvent::Signal(STANDARD_NO_PAD.decode(signal.trim_end_matches('='))?),
                Ok(Frame::Mail) => RouterEvent::Mail,
                Err(_) => continue,
            };
            if events.send(event).await.is_err() {
                return Ok(());
            }
        }
        Ok(())
    }
}

async fn send(request: RequestBuilder) -> Result<reqwest::Response> {
    request.send().await.map_err(|error| anyhow!("cannot reach the router: {error}"))
}

fn expect(response: reqwest::Response, status: StatusCode) -> Result<()> {
    if response.status() == status {
        Ok(())
    } else {
        bail!("the router answered {}", response.status())
    }
}

fn query_escape(value: &str) -> String {
    value.replace('%', "%25").replace('+', "%2B").replace('/', "%2F").replace('=', "%3D").replace('&', "%26")
}

#[cfg(test)]
mod tests {
    use super::*;

    // The very same vector as ft-router's auth tests.
    #[test]
    fn the_signed_text_is_pinned() {
        let expected = format!("FT1\nPOST\n/v1/mailbox/ft_x\n1700000000000\nn1\n{}", blake3::hash(b"blob").to_hex());
        assert_eq!(canonical("POST", "/v1/mailbox/ft_x", 1_700_000_000_000, "n1", b"blob"), expected.as_bytes());
    }

    #[test]
    fn the_router_base_becomes_the_websocket_base() {
        assert_eq!(websocket_base("https://api.flickertalk.com"), "wss://api.flickertalk.com");
        assert_eq!(websocket_base("http://127.0.0.1:8787/"), "ws://127.0.0.1:8787");
    }

    #[test]
    fn capabilities_travel_as_unpadded_base64() {
        assert_eq!(encode(&[0xff; 3]), "////");
        assert_eq!(encode(&[1, 2]), "AQI");
    }

    #[test]
    fn router_frames_are_understood() {
        let welcome: Frame = serde_json::from_str(
            r#"{"kind":"welcome","stun":["stun:a:3478"],"turn":{"urls":["turn:a:3478"],"username":"1:x","credential":"c"}}"#,
        )
        .unwrap();
        let Frame::Welcome { stun, turn } = welcome else { panic!("a welcome") };
        assert_eq!(stun, ["stun:a:3478"]);
        assert_eq!(turn.expect("turn").username, "1:x");

        let signal: Frame = serde_json::from_str(r#"{"kind":"signal","signal":"AQI"}"#).unwrap();
        assert_eq!(signal, Frame::Signal { signal: "AQI".to_owned() });
        assert_eq!(serde_json::from_str::<Frame>(r#"{"kind":"mail"}"#).unwrap(), Frame::Mail);
    }
}
