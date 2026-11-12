//! Anything addressed to another device carries its route capability, never our identity, so the
//! router can't tell who sent it.

use std::sync::{Arc, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use reqwest::{Method, RequestBuilder, StatusCode};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

const MAX_BACKOFF: Duration = Duration::from_secs(30);

// the router only pings once a minute, too seldom to notice a dead socket
pub const PING_EVERY: Duration = Duration::from_secs(20);

// after a network change the old socket isn't closed, it just goes quiet while the router keeps
// writing to it
pub const SILENT_FOR: Duration = Duration::from_secs(40);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MailboxRejected;

impl std::fmt::Display for MailboxRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the router refuses mail for that recipient")
    }
}

impl std::error::Error for MailboxRejected {}

#[async_trait]
pub trait Signer: Send + Sync {
    fn device_id(&self) -> String;
    // both base64 without padding
    async fn signing_key(&self) -> String;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouterEvent {
    Connected { stun: Vec<String>, turn: Option<TurnGrant> },
    Signal(Vec<u8>),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signalled {
    Delivered,
    Retained,
    NotConnected,
}

impl Signalled {
    pub fn delivered(self) -> bool {
        self == Self::Delivered
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feedback {
    Sent,
    TooMany,
    Failed,
}

pub struct RouterClient {
    base: String,
    http: reqwest::Client,
    signer: Arc<dyn Signer>,
    again: Arc<tokio::sync::Notify>,
    link: std::sync::Mutex<Link>,
    closed: tokio::sync::watch::Sender<bool>,
    away: tokio::sync::watch::Sender<bool>,
    liveness: std::sync::Mutex<(Duration, Duration)>,
}

#[derive(Debug, Clone, Copy)]
enum Link {
    Down,
    Opening(SystemTime),
    Open(SystemTime),
}

// phones have no rustls crypto provider installed, and building a client without one aborts the app
pub fn initialize_tls() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

pub fn https_client(timeout: Duration) -> Result<reqwest::Client> {
    let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    let tls = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .context("no TLS versions")?
        .with_root_certificates(roots)
        .with_no_client_auth();
    reqwest::Client::builder()
        .use_preconfigured_tls(tls)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .build()
        .context("cannot build the HTTP client")
}

impl RouterClient {
    pub fn new(base: &str, signer: Arc<dyn Signer>) -> Result<Self> {
        let http = https_client(Duration::from_secs(20))?;
        Ok(Self {
            base: base.trim_end_matches('/').to_owned(),
            http,
            signer,
            again: Arc::default(),
            link: std::sync::Mutex::new(Link::Down),
            closed: tokio::sync::watch::Sender::new(false),
            away: tokio::sync::watch::Sender::new(false),
            liveness: std::sync::Mutex::new((PING_EVERY, SILENT_FOR)),
        })
    }

    // iOS cuts a suspended app's socket and the phone can take long to notice
    pub fn reconnect_now(&self) {
        if self.is_away() {
            return;
        }
        self.again.notify_one();
    }

    // a call push, CallKit's answer and its audio activation all ask within seconds, and dropping
    // the new socket again can lose the offer. Wall clock, a sleeping iPhone's monotonic one stops
    pub fn reconnect_unless_fresh(&self, fresh: Duration) {
        let since = match *self.link.lock().unwrap_or_else(PoisonError::into_inner) {
            Link::Down => None,
            Link::Opening(at) | Link::Open(at) => Some(at),
        };
        let young = since.and_then(|at| SystemTime::now().duration_since(at).ok()).is_some_and(|age| age < fresh);
        if !young {
            self.reconnect_now();
        }
    }

    // iOS can't restart the app, so the old identity's client is shut down in place
    pub fn close(&self) {
        self.closed.send_replace(true);
    }

    pub fn is_closed(&self) -> bool {
        *self.closed.borrow()
    }

    // with no socket the router pushes instead of writing to one a suspended app can't read
    pub fn set_away(&self, away: bool) -> bool {
        self.away.send_if_modified(|now| std::mem::replace(now, away) != away)
    }

    pub fn is_away(&self) -> bool {
        *self.away.borrow()
    }

    pub fn away_changes(&self) -> tokio::sync::watch::Receiver<bool> {
        self.away.subscribe()
    }

    #[doc(hidden)]
    pub fn set_liveness(&self, ping_every: Duration, silent_for: Duration) {
        *self.liveness.lock().unwrap_or_else(PoisonError::into_inner) = (ping_every, silent_for);
    }

    async fn send(&self, request: RequestBuilder) -> Result<reqwest::Response> {
        if self.is_closed() {
            bail!("the router client is closed");
        }
        send(request).await
    }

    fn set_link(&self, link: Link) {
        *self.link.lock().unwrap_or_else(PoisonError::into_inner) = link;
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
        self.send(request.body(body)).await
    }

    // bit i of silent_slots mutes pushes for slot i, bit 0 is the main list and never muted
    pub async fn register(&self, capability_hashes: &[[u8; 32]; 8], silent_slots: u8) -> Result<()> {
        let hashes: Vec<String> = capability_hashes.iter().map(|hash| encode(hash)).collect();
        let body = json!({
            "signing_key": self.signer.signing_key().await,
            "capability_hash": hashes[0],
            "capability_hashes": hashes,
            "silent_slots": silent_slots & !1,
        });
        expect(self.signed(Method::POST, "/v1/device/register", serde_json::to_vec(&body)?).await?, StatusCode::NO_CONTENT)
    }

    pub async fn set_push(&self, provider: &str, token: &str) -> Result<()> {
        let body = json!({ "provider": provider, "token": token });
        expect(self.signed(Method::PUT, "/v1/device/push", serde_json::to_vec(&body)?).await?, StatusCode::NO_CONTENT)
    }

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

    pub async fn signal(&self, to: &str, capability: &[u8; 32], bytes: Vec<u8>) -> Result<Signalled> {
        self.signal_as(to, capability, bytes, false).await
    }

    // the router only learns that it's a call
    pub async fn signal_call(&self, to: &str, capability: &[u8; 32], bytes: Vec<u8>) -> Result<Signalled> {
        self.signal_as(to, capability, bytes, true).await
    }

    async fn signal_as(&self, to: &str, capability: &[u8; 32], bytes: Vec<u8>, call: bool) -> Result<Signalled> {
        let mut request = self.http.post(format!("{}/v1/signal/{to}", self.base)).header("ft-capability", encode(capability));
        if call {
            request = request.header("ft-call", "1");
        }
        let response = self.send(request.body(bytes)).await?;
        let retained = response.headers().get("ft-retained").is_some_and(|value| value == "1");
        match response.status() {
            StatusCode::ACCEPTED => Ok(Signalled::Delivered),
            StatusCode::NOT_FOUND if retained => Ok(Signalled::Retained),
            StatusCode::NOT_FOUND => Ok(Signalled::NotConnected),
            status => bail!("the router refused the signal: {status}"),
        }
    }

    // blob must already be encrypted
    pub async fn deposit(&self, to: &str, capability: &[u8; 32], blob: Vec<u8>) -> Result<()> {
        let request = self.http.post(format!("{}/v1/mailbox/{to}", self.base)).header("ft-capability", encode(capability));
        let response = self.send(request.body(blob)).await?;
        if response.status() == StatusCode::FORBIDDEN {
            return Err(MailboxRejected.into());
        }
        expect(response, StatusCode::CREATED)
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

    // only once the blob is stored, the router deletes it
    pub async fn acknowledge(&self, id: &str) -> Result<()> {
        let response = self.signed(Method::DELETE, &format!("/v1/mailbox/{id}"), Vec::new()).await?;
        match response.status() {
            StatusCode::NO_CONTENT | StatusCode::NOT_FOUND => Ok(()),
            status => bail!("the router refused the acknowledgement: {status}"),
        }
    }

    // nothing in the mail the router sends may name this device
    pub async fn feedback(&self, text: &str, app: &str, platform: &str) -> Feedback {
        let body = json!({ "text": text, "app": app, "platform": platform });
        let Ok(body) = serde_json::to_vec(&body) else { return Feedback::Failed };
        match self.signed(Method::POST, "/v1/feedback", body).await.map(|response| response.status()) {
            Ok(StatusCode::NO_CONTENT) => Feedback::Sent,
            Ok(StatusCode::TOO_MANY_REQUESTS) => Feedback::TooMany,
            _ => Feedback::Failed,
        }
    }

    pub fn listen(self: &Arc<Self>) -> mpsc::Receiver<RouterEvent> {
        let (events, receiver) = mpsc::channel(64);
        let client = self.clone();
        let mut closed = self.closed.subscribe();
        let mut away = self.away.subscribe();
        tokio::spawn(async move {
            let mut backoff = Duration::from_secs(1);
            while !client.is_closed() {
                if client.is_away() {
                    tokio::select! {
                        _ = away.wait_for(|away| !*away) => backoff = Duration::from_secs(1),
                        _ = client.again.notified() => {}
                        _ = closed.wait_for(|closed| *closed) => return,
                    }
                    continue;
                }
                client.set_link(Link::Opening(SystemTime::now()));
                let asked = tokio::select! {
                    result = client.connection(&events) => {
                        match result {
                            Ok(()) => backoff = Duration::from_secs(1),
                            Err(_) => backoff = (backoff * 2).min(MAX_BACKOFF),
                        }
                        false
                    }
                    _ = client.again.notified() => true,
                    _ = away.wait_for(|away| *away) => true,
                    _ = closed.wait_for(|closed| *closed) => false,
                };
                client.set_link(Link::Down);
                if client.is_closed() || events.send(RouterEvent::Disconnected).await.is_err() {
                    return;
                }
                if asked {
                    backoff = Duration::from_secs(1);
                    continue;
                }
                tokio::select! {
                    _ = tokio::time::sleep(backoff) => {}
                    _ = client.again.notified() => backoff = Duration::from_secs(1),
                    _ = away.wait_for(|away| *away) => {}
                    _ = closed.wait_for(|closed| *closed) => return,
                }
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
        let (ping_every, silent_for) = *self.liveness.lock().unwrap_or_else(PoisonError::into_inner);
        let opening = tokio::time::timeout(silent_for, connect_async(url.as_str()));
        let (mut socket, _) = opening.await.map_err(|_| anyhow!("the router did not answer"))?.context("cannot reach the router")?;
        let mut ping = tokio::time::interval_at(tokio::time::Instant::now() + ping_every, ping_every);
        let mut unanswered: Option<tokio::time::Instant> = None;
        loop {
            let message = tokio::select! {
                message = socket.next() => message,
                due = ping.tick() => {
                    match unanswered {
                        Some(since) if due.duration_since(since) >= silent_for => bail!("the router stopped answering"),
                        Some(_) => {}
                        None => unanswered = Some(due),
                    }
                    socket.send(Message::Ping(Default::default())).await?;
                    continue;
                }
            };
            let Some(message) = message else { return Ok(()) };
            unanswered = None;
            let Message::Text(text) = message? else { continue };
            let event = match serde_json::from_str::<Frame>(text.as_str()) {
                Ok(Frame::Welcome { stun, turn }) => {
                    self.set_link(Link::Open(SystemTime::now()));
                    RouterEvent::Connected { stun, turn }
                }
                Ok(Frame::Signal { signal }) => RouterEvent::Signal(STANDARD_NO_PAD.decode(signal.trim_end_matches('='))?),
                Ok(Frame::Mail) => RouterEvent::Mail,
                Err(_) => continue,
            };
            if events.send(event).await.is_err() {
                return Ok(());
            }
        }
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

    #[tokio::test]
    async fn https_client_initializes_tls_for_the_mobile_dev_proxy() {
        assert!(rustls::crypto::CryptoProvider::get_default().is_none());
        initialize_tls();
        initialize_tls();
        https_client(Duration::from_secs(20)).expect("HTTPS client builds without global TLS setup");
        reqwest::Client::builder().build().expect("Tauri's mobile development proxy can build its client");
    }

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
