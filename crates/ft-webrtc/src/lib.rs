//! Peer-to-peer sessions over a WebRTC data channel (Plan §21–22).
//!
//! This crate only knows how to connect two peers and pass text between them. It never decides
//! how the signals travel: the core sends them through push (§13), the tests through a channel.
//!
//! A description is sent once ICE gathering completes or, at most, after a short deadline with the
//! candidates gathered so far, so connecting takes exactly two signals (offer and answer). Fewer,
//! self-contained signals suit the push channel best (§15).

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use bytes::BytesMut;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch, Mutex};
use webrtc::data_channel::{DataChannel, DataChannelEvent};
use webrtc::peer_connection::{
    PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCConfigurationBuilder,
    RTCIceCandidateInit, RTCIceGatheringState, RTCIceServer, RTCPeerConnectionState, RTCIceTransportPolicy, RTCSdpType,
    RTCSessionDescription,
};
use webrtc::runtime::{default_runtime, Runtime};

/// Messages travel on a single ordered, reliable channel; logical channels are multiplexed on top
/// of it by the protocol (§22).
pub const CHANNEL_LABEL: &str = "messages";

/// How long closing waits for the other side to acknowledge the channel's end.
const CLOSE_WAIT: Duration = Duration::from_secs(1);

/// What one peer has to hand to the other to connect. The payload is opaque here: whoever
/// transports it encrypts it (§14).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "payload")]
pub enum Signal {
    /// A session description with its ICE candidates: the offer or the answer.
    Sdp(String),
    /// A single ICE candidate, for trickle ICE (§15). Not emitted yet; accepted if received.
    Ice(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Starts the connection and opens the data channel.
    Caller,
    /// Waits for the offer and for the channel.
    Callee,
}

/// A TURN relay with its credentials. They are temporary, with a random user (§17).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnServer {
    pub url: String,
    pub username: String,
    pub credential: String,
}

#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub stun_servers: Vec<String>,
    /// Fallback relays, used when no direct path works (§17).
    pub turn_servers: Vec<TurnServer>,
    /// Only the TURN relay is used: the contact never learns the user's IP ("Always relay", §17).
    pub relay_only: bool,
    /// Local UDP addresses to listen on, one per network interface.
    pub bind: Vec<String>,
    /// How long to wait for ICE gathering before sending what was gathered so far.
    pub gather_timeout: Duration,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            stun_servers: Vec::new(),
            turn_servers: Vec::new(),
            relay_only: false,
            bind: interface_addresses(),
            gather_timeout: Duration::from_secs(3),
        }
    }
}

impl SessionConfig {
    /// Loopback only and no STUN. Used by the tests, which must not touch the network.
    pub fn offline() -> Self {
        Self { bind: vec!["127.0.0.1:0".to_owned()], ..Self::default() }
    }

    pub fn with_stun(servers: impl IntoIterator<Item = String>) -> Self {
        Self { stun_servers: servers.into_iter().collect(), ..Self::default() }
    }
}

/// The ICE servers and transport policy a session is configured with.
pub fn ice_configuration(config: &SessionConfig) -> (Vec<RTCIceServer>, RTCIceTransportPolicy) {
    let stun = (!config.stun_servers.is_empty())
        .then(|| RTCIceServer { urls: config.stun_servers.clone(), ..Default::default() });
    let turn = config.turn_servers.iter().map(|server| RTCIceServer {
        urls: vec![server.url.clone()],
        username: server.username.clone(),
        credential: server.credential.clone(),
    });
    let policy = if config.relay_only { RTCIceTransportPolicy::Relay } else { RTCIceTransportPolicy::All };
    (stun.into_iter().chain(turn).collect(), policy)
}

/// Every non-loopback IPv4 address of the device (Wi-Fi, mobile data…), with an ephemeral port.
///
/// Never the 0.0.0.0 wildcard: with STUN configured, webrtc 0.21 never finishes ICE gathering on
/// it. IPv6 is left for PoC 2, which measures IPv6-only mobile networks (§89).
pub fn interface_addresses() -> Vec<String> {
    let addresses: Vec<String> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|interface| !interface.is_loopback() && interface.ip().is_ipv4())
        .map(|interface| format!("{}:0", interface.ip()))
        .collect();

    if addresses.is_empty() {
        vec!["127.0.0.1:0".to_owned()]
    } else {
        addresses
    }
}

/// Incoming messages of a session, as the bytes that were sent.
pub struct Inbox(mpsc::Receiver<Vec<u8>>);

impl Inbox {
    pub async fn next(&mut self) -> Option<Vec<u8>> {
        self.0.recv().await
    }

    /// The next message that is valid UTF-8 text (the PoC screen talks in text).
    pub async fn next_text(&mut self) -> Option<String> {
        loop {
            if let Ok(text) = String::from_utf8(self.next().await?) {
                return Some(text);
            }
        }
    }
}

type SharedChannel = Arc<Mutex<Option<Arc<dyn DataChannel>>>>;

/// Receives the connection's events. It must never block: long work is spawned.
struct Events {
    runtime: Arc<dyn Runtime>,
    gathered: watch::Sender<bool>,
    open: watch::Sender<bool>,
    channel: SharedChannel,
    /// The callee's inbox, handed to its one channel: once that channel closes nothing holds a
    /// sender any more, so the inbox ends.
    messages: std::sync::Mutex<Option<mpsc::Sender<Vec<u8>>>>,
}

#[async_trait::async_trait]
impl PeerConnectionEventHandler for Events {
    async fn on_ice_gathering_state_change(&self, state: RTCIceGatheringState) {
        if state == RTCIceGatheringState::Complete {
            let _ = self.gathered.send(true);
        }
    }

    // A peer that vanishes (its app killed, the phone reinstalled) never closes the channel; the
    // connection notices it stopped answering. From then on the session is not open, so what is
    // sent next opens a new connection or goes to the mailbox instead of vanishing too.
    async fn on_connection_state_change(&self, state: RTCPeerConnectionState) {
        if matches!(state, RTCPeerConnectionState::Disconnected | RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed) {
            let _ = self.open.send(false);
        }
    }

    // The callee receives the caller's channel here.
    async fn on_data_channel(&self, channel: Arc<dyn DataChannel>) {
        let Some(messages) = self.messages.lock().expect("messages poisoned").take() else { return };
        *self.channel.lock().await = Some(channel.clone());
        listen(&self.runtime, channel, self.open.clone(), messages);
    }
}

#[derive(Clone)]
pub struct Session {
    connection: Arc<dyn PeerConnection>,
    signals: mpsc::Sender<Signal>,
    channel: SharedChannel,
    gathered: watch::Receiver<bool>,
    gather_timeout: Duration,
    opened: watch::Receiver<bool>,
    open: watch::Sender<bool>,
}

impl Session {
    pub async fn start(
        config: SessionConfig,
        role: Role,
        signals: mpsc::Sender<Signal>,
    ) -> Result<(Self, Inbox)> {
        let runtime = default_runtime().context("no WebRTC runtime available")?;
        let (messages, inbox) = mpsc::channel(64);
        let (gathered_tx, gathered) = watch::channel(false);
        let (open_tx, opened) = watch::channel(false);
        let channel: SharedChannel = Arc::new(Mutex::new(None));

        let (ice_servers, policy) = ice_configuration(&config);
        let events = Arc::new(Events {
            runtime: runtime.clone(),
            gathered: gathered_tx,
            open: open_tx.clone(),
            channel: channel.clone(),
            messages: std::sync::Mutex::new((role == Role::Callee).then(|| messages.clone())),
        });

        let connection: Arc<dyn PeerConnection> = Arc::new(
            PeerConnectionBuilder::new()
                .with_configuration(
                    RTCConfigurationBuilder::new()
                        .with_ice_servers(ice_servers)
                        .with_ice_transport_policy(policy)
                        .build(),
                )
                .with_handler(events)
                .with_runtime(runtime.clone())
                .with_udp_addrs(config.bind.clone())
                .build()
                .await?,
        );

        if role == Role::Caller {
            let created = connection.create_data_channel(CHANNEL_LABEL, None).await?;
            *channel.lock().await = Some(created.clone());
            listen(&runtime, created, open_tx.clone(), messages.clone());
        }

        let gather_timeout = config.gather_timeout;
        drop(messages);
        let open = open_tx;
        Ok((Self { connection, signals, channel, gathered, gather_timeout, opened, open }, Inbox(inbox)))
    }

    /// Creates the offer and sends it out. Only the caller does this.
    pub async fn invite(&self) -> Result<()> {
        let offer = self.connection.create_offer(None).await?;
        self.connection.set_local_description(offer).await?;
        self.send_local_description().await
    }

    pub async fn handle_signal(&self, signal: Signal) -> Result<()> {
        match signal {
            Signal::Sdp(payload) => {
                let description: RTCSessionDescription =
                    serde_json::from_str(&payload).context("invalid session description")?;
                let answer_expected = description.sdp_type == RTCSdpType::Offer;
                self.connection.set_remote_description(description).await?;

                if answer_expected {
                    let answer = self.connection.create_answer(None).await?;
                    self.connection.set_local_description(answer).await?;
                    self.send_local_description().await?;
                }
            }
            Signal::Ice(payload) => {
                let candidate: RTCIceCandidateInit =
                    serde_json::from_str(&payload).context("invalid ICE candidate")?;
                self.connection.add_ice_candidate(candidate).await?;
            }
        }
        Ok(())
    }

    /// Resolves once the data channel is usable.
    pub async fn wait_open(&self) -> Result<()> {
        wait_for_ever(self.opened.clone(), "the session was closed").await
    }

    pub async fn send(&self, text: &str) -> Result<()> {
        self.open_channel().await?.send_text(text).await?;
        Ok(())
    }

    pub async fn send_bytes(&self, bytes: &[u8]) -> Result<()> {
        self.open_channel().await?.send(BytesMut::from(bytes)).await?;
        Ok(())
    }

    async fn open_channel(&self) -> Result<Arc<dyn DataChannel>> {
        self.channel.lock().await.clone().ok_or_else(|| anyhow!("the data channel is not open yet"))
    }

    /// Whether the data channel is open right now.
    pub fn is_open(&self) -> bool {
        *self.opened.borrow()
    }

    /// Stops without telling the other side, as a process that is killed does. For tests of what
    /// the other side sees then.
    #[doc(hidden)]
    pub async fn vanish(&self) {
        let _ = self.open.send(false);
        let _ = self.connection.close().await;
    }

    /// Ends the session. The data channel is closed first: that is what tells the other side,
    /// which closing the connection alone does not.
    pub async fn close(&self) -> Result<()> {
        let channel = self.channel.lock().await.clone();
        if let Some(channel) = channel {
            if channel.close().await.is_ok() {
                let mut opened = self.opened.clone();
                let _ = tokio::time::timeout(CLOSE_WAIT, opened.wait_for(|open| !open)).await;
            }
        }
        let _ = self.open.send(false);
        self.connection.close().await?;
        Ok(())
    }

    /// Sends the description with the ICE candidates gathered so far: once gathering completes or,
    /// at most, when the deadline passes. An interface that never answers (a VPN tunnel, say) must
    /// not hold the whole connection back.
    async fn send_local_description(&self) -> Result<()> {
        let _ = wait_for(self.gathered.clone(), self.gather_timeout, "ICE gathering").await;
        let description = self
            .connection
            .local_description()
            .await
            .ok_or_else(|| anyhow!("no local description"))?;
        if candidate_count(&description.sdp) == 0 {
            return Err(anyhow!("no ICE candidate was gathered"));
        }
        self.signals
            .send(Signal::Sdp(serde_json::to_string(&description)?))
            .await
            .map_err(|_| anyhow!("the signalling channel is closed"))
    }
}

fn candidate_count(sdp: &str) -> usize {
    sdp.lines().filter(|line| line.starts_with("a=candidate:")).count()
}

/// Waits until the flag turns true, but never longer than `limit`.
async fn wait_for(flag: watch::Receiver<bool>, limit: Duration, what: &'static str) -> Result<()> {
    tokio::time::timeout(limit, wait_for_ever(flag, "stopped"))
        .await
        .map_err(|_| anyhow!("{what} timed out after {limit:?}"))?
        .with_context(|| what.to_owned())
}

async fn wait_for_ever(mut flag: watch::Receiver<bool>, closed: &'static str) -> Result<()> {
    loop {
        if *flag.borrow() {
            return Ok(());
        }
        flag.changed().await.map_err(|_| anyhow!(closed))?;
    }
}

/// Turns the channel's events into the open flag and the inbox. Spawned, never awaited inline.
fn listen(
    runtime: &Arc<dyn Runtime>,
    channel: Arc<dyn DataChannel>,
    open: watch::Sender<bool>,
    messages: mpsc::Sender<Vec<u8>>,
) {
    runtime.spawn(Box::pin(async move {
        while let Some(event) = channel.poll().await {
            match event {
                DataChannelEvent::OnOpen => {
                    let _ = open.send(true);
                }
                DataChannelEvent::OnMessage(message) => {
                    let _ = messages.send(message.data.to_vec()).await;
                }
                DataChannelEvent::OnClose => {
                    let _ = open.send(false);
                    break;
                }
                _ => {}
            }
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signals_survive_a_round_trip_through_the_transport() {
        let signal = Signal::Sdp("{\"type\":\"offer\",\"sdp\":\"v=0\"}".to_owned());
        let encoded = serde_json::to_string(&signal).expect("the signal serialises");
        assert_eq!(serde_json::from_str::<Signal>(&encoded).expect("it parses back"), signal);
    }

    #[test]
    fn roles_arrive_from_the_ui_in_snake_case() {
        assert_eq!(serde_json::from_str::<Role>("\"caller\"").expect("parses"), Role::Caller);
        assert_eq!(serde_json::from_str::<Role>("\"callee\"").expect("parses"), Role::Callee);
    }

    #[test]
    fn an_offline_session_stays_on_loopback_without_stun() {
        let config = SessionConfig::offline();
        assert!(config.stun_servers.is_empty());
        assert_eq!(config.bind, ["127.0.0.1:0"]);
    }

    // With STUN, listening on 0.0.0.0 never finishes ICE gathering in webrtc 0.21: sessions
    // listen on each concrete interface address instead.
    #[test]
    fn sessions_listen_on_concrete_interface_addresses_never_the_wildcard() {
        let config = SessionConfig::with_stun(["stun:one:3478".to_owned()]);
        assert!(!config.bind.is_empty());
        assert!(config.bind.iter().all(|address| !address.starts_with("0.0.0.0")));
    }

    #[test]
    fn stun_servers_need_no_credentials() {
        let (servers, policy) = ice_configuration(&SessionConfig::with_stun(["stun:one:3478".to_owned()]));
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].urls, ["stun:one:3478"]);
        assert!(servers[0].username.is_empty());
        assert_eq!(policy, RTCIceTransportPolicy::All);
    }

    #[test]
    fn turn_servers_carry_their_credentials() {
        let config = SessionConfig {
            turn_servers: vec![TurnServer {
                url: "turn:relay:3478".to_owned(),
                username: "user".to_owned(),
                credential: "secret".to_owned(),
            }],
            ..SessionConfig::offline()
        };
        let (servers, _) = ice_configuration(&config);
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].urls, ["turn:relay:3478"]);
        assert_eq!(servers[0].username, "user");
        assert_eq!(servers[0].credential, "secret");
    }

    // Plan §17: "Always relay" hides the user's IP from the contact.
    #[test]
    fn relay_only_sessions_use_nothing_but_the_turn_relay() {
        let config = SessionConfig { relay_only: true, ..SessionConfig::offline() };
        assert_eq!(ice_configuration(&config).1, RTCIceTransportPolicy::Relay);
    }

    #[test]
    fn counts_the_ice_candidates_carried_by_a_description() {
        let sdp = "v=0\r\na=candidate:1 1 udp 2130706431 192.168.1.2 50000 typ host\r\n\
                   a=mid:0\r\na=candidate:2 1 udp 1694498815 81.2.3.4 50000 typ srflx\r\n";
        assert_eq!(candidate_count(sdp), 2);
        assert_eq!(candidate_count("v=0\r\na=mid:0\r\n"), 0);
    }

    #[test]
    fn gathering_waits_a_few_seconds_at_most_by_default() {
        assert!(SessionConfig::default().gather_timeout <= Duration::from_secs(3));
    }

    #[tokio::test]
    async fn waiting_for_a_flag_gives_up_after_the_limit() {
        let (_flag, never) = watch::channel(false);
        let error = wait_for(never, Duration::from_millis(30), "ICE gathering")
            .await
            .expect_err("it gives up");
        assert!(error.to_string().contains("timed out"));
    }

    #[test]
    fn stun_servers_are_kept_in_order() {
        let config = SessionConfig::with_stun(["stun:one:3478".to_owned(), "stun:two:3478".to_owned()]);
        assert_eq!(config.stun_servers, ["stun:one:3478", "stun:two:3478"]);
    }
}
