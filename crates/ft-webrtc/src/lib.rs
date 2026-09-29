//! Peer-to-peer sessions over a WebRTC data channel (Plan §21–22).
//!
//! This crate only knows how to connect two peers and pass text between them. It never decides
//! how the signals travel: the core sends them through push (§13), the tests through a channel.
//!
//! A description is sent whole, with the candidates gathered so far, so connecting takes exactly
//! two signals (offer and answer). Fewer, self-contained signals suit the push channel best (§15).
//!
//! DataChannel setup time (2026-09-29): a description goes as soon as it has what the routing wants
//! (`send_at`, the same rule as the calls' media in ft-media), not when gathering completes. A STUN
//! or TURN server that never answers, on one interface or all, used to hold the offer and then the
//! answer for the whole 3 s deadline each.

use std::sync::{Arc, PoisonError};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use bytes::BytesMut;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch, Mutex};
use webrtc::data_channel::{DataChannel, DataChannelEvent};
use webrtc::peer_connection::{
    PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCConfigurationBuilder, RTCIceCandidateInit,
    RTCIceCandidateType, RTCIceGatheringState, RTCIceServer, RTCIceTransportPolicy, RTCPeerConnectionIceEvent,
    RTCPeerConnectionState, RTCSdpType, RTCSessionDescription,
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
    gathered: watch::Sender<Live>,
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
            self.gathered.send_modify(|live| live.complete = true);
        }
    }

    async fn on_ice_candidate(&self, event: RTCPeerConnectionIceEvent) {
        let now = Instant::now();
        self.gathered.send_modify(|live| match event.candidate.typ {
            RTCIceCandidateType::Host => live.host = live.host.saturating_add(1),
            RTCIceCandidateType::Srflx => {
                live.srflx.get_or_insert(now);
            }
            RTCIceCandidateType::Relay => {
                live.relay.get_or_insert(now);
            }
            _ => {}
        });
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
        let Some(messages) = self.messages.lock().unwrap_or_else(PoisonError::into_inner).take() else { return };
        *self.channel.lock().await = Some(channel.clone());
        listen(&self.runtime, channel, self.open.clone(), messages);
    }
}

#[derive(Clone)]
pub struct Session {
    connection: Arc<dyn PeerConnection>,
    signals: mpsc::Sender<Signal>,
    channel: SharedChannel,
    gathered: watch::Receiver<Live>,
    wanted: Wanted,
    limits: GatherLimits,
    report: Arc<std::sync::Mutex<Gathering>>,
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
        let (gathered_tx, gathered) = watch::channel(Live::default());
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

        let wanted = Wanted::of(&config);
        let limits = GatherLimits { deadline: config.gather_timeout, ..GATHER_LIMITS };
        let report = Arc::default();
        drop(messages);
        let open = open_tx;
        Ok((Self { connection, signals, channel, gathered, wanted, limits, report, opened, open }, Inbox(inbox)))
    }

    /// Creates the offer and sends it out. Only the caller does this.
    pub async fn invite(&self) -> Result<()> {
        let offer = self.connection.create_offer(None).await?;
        self.connection.set_local_description(offer).await?;
        self.gathering_started();
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
                    self.gathering_started();
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

    /// How the gathering went, so far.
    pub fn gathering(&self) -> Gathering {
        *self.report.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn gathering_started(&self) {
        let mut report = self.report.lock().unwrap_or_else(PoisonError::into_inner);
        *report = Gathering { started: Some(Instant::now()), ..Gathering::default() };
    }

    /// `sdp` is the description taken to be sent.
    fn gathering_finished(&self, sdp: &str) {
        let (host, srflx, relay) = candidate_counts(sdp);
        let complete = self.gathered.borrow().complete;
        let mut report = self.report.lock().unwrap_or_else(PoisonError::into_inner);
        *report = Gathering { finished: Some(Instant::now()), complete, host, srflx, relay, ..*report };
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

    /// Sends the description with the ICE candidates gathered so far: once gathering completes,
    /// once it has what the routing wants (`send_at`) or, at most, when the deadline passes. A
    /// server or an interface that never answers (a VPN tunnel, say) must not hold the whole
    /// connection back (2026-09-29).
    async fn send_local_description(&self) -> Result<()> {
        let started = self.gathering().started.unwrap_or_else(Instant::now);
        let mut gathered = self.gathered.clone();
        loop {
            let found = gathered.borrow_and_update().since(started);
            let at = started + send_at(&found, self.wanted, self.limits);
            if Instant::now() >= at {
                break;
            }
            tokio::select! {
                changed = gathered.changed() => {
                    if changed.is_err() {
                        break;
                    }
                }
                () = tokio::time::sleep_until(at.into()) => {}
            }
        }
        let description = self
            .connection
            .local_description()
            .await
            .ok_or_else(|| anyhow!("no local description"))?;
        self.gathering_finished(&description.sdp);
        if candidate_count(&description.sdp) == 0 {
            return Err(anyhow!("no ICE candidate was gathered"));
        }
        self.signals
            .send(Signal::Sdp(serde_json::to_string(&description)?))
            .await
            .map_err(|_| anyhow!("the signalling channel is closed"))
    }
}

/// How a session's ICE gathering went (DataChannel setup timings, 2026-09-29): numbers only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Gathering {
    /// When our description was set and gathering began.
    pub started: Option<Instant>,
    /// When the description was taken to be sent, with what was gathered by then.
    pub finished: Option<Instant>,
    /// Whether gathering had completed by then, or the wait was cut short.
    pub complete: bool,
    pub host: u16,
    pub srflx: u16,
    pub relay: u16,
}

/// What the routing wants a description to carry. The data channel has no routing setting of its
/// own: it uses the router's STUN and TURN (the TURN only when no direct path works, as "relay
/// when needed"), or the relay alone when the session is relay only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Wanted {
    /// A server reflexive candidate: STUN is configured and the routing uses it.
    srflx: bool,
    /// A relay candidate: TURN is configured.
    relay: bool,
    /// Only relay candidates are usable ("Always relay").
    relay_only: bool,
}

impl Wanted {
    fn of(config: &SessionConfig) -> Self {
        Self {
            srflx: !config.stun_servers.is_empty() && !config.relay_only,
            relay: !config.turn_servers.is_empty(),
            relay_only: config.relay_only,
        }
    }
}

/// What gathering has found so far: host candidates, and when (from the start of gathering) the
/// first server reflexive and relay ones came.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Found {
    host: u16,
    srflx: Option<Duration>,
    relay: Option<Duration>,
    complete: bool,
}

/// How long a description waits for its candidates.
#[derive(Debug, Clone, Copy)]
struct GatherLimits {
    /// After every wanted kind came: for the other interfaces' candidates, which travel together.
    settle: Duration,
    /// With something usable, a server that never answers holds the description this long at most.
    cap: Duration,
    /// Without anything usable (no relay yet with "Always relay"): the whole deadline.
    deadline: Duration,
}

/// The same limits as the calls' media (ft-media, 2026-09-29).
const GATHER_LIMITS: GatherLimits =
    GatherLimits { settle: Duration::from_millis(100), cap: Duration::from_secs(1), deadline: Duration::from_secs(3) };

/// When (from the start of gathering) the description goes, if nothing more comes. webrtc-rs only
/// completes gathering once every STUN and TURN server answered, so one that never does (an
/// interface with no route, a lost packet) must not hold the description for the whole deadline.
fn send_at(found: &Found, wanted: Wanted, limits: GatherLimits) -> Duration {
    if found.complete {
        return Duration::ZERO;
    }
    let usable = if wanted.relay_only {
        found.relay.is_some()
    } else {
        found.host > 0 || found.srflx.is_some() || found.relay.is_some()
    };
    if !usable {
        return limits.deadline;
    }
    let srflx = if wanted.srflx { found.srflx.map(Some) } else { Some(None) };
    let relay = if wanted.relay { found.relay.map(Some) } else { Some(None) };
    let at = match (srflx, relay) {
        // Every wanted kind is here: from the last of them.
        (Some(srflx), Some(relay)) => srflx.max(relay).unwrap_or(Duration::ZERO) + limits.settle,
        // The relay takes two round trips where STUN took one.
        (Some(Some(srflx)), None) => limits.cap.max(srflx * 3 + limits.settle),
        _ => limits.cap,
    };
    at.min(limits.deadline)
}

/// What gathering has found so far, as it happens: when the first candidates of each kind came.
#[derive(Debug, Clone, Copy, Default)]
struct Live {
    host: u16,
    srflx: Option<Instant>,
    relay: Option<Instant>,
    complete: bool,
}

impl Live {
    fn since(&self, started: Instant) -> Found {
        Found {
            host: self.host,
            srflx: self.srflx.map(|at| at.saturating_duration_since(started)),
            relay: self.relay.map(|at| at.saturating_duration_since(started)),
            complete: self.complete,
        }
    }
}

/// The distinct candidates of `sdp` by type: (host, srflx, relay). Only the first component: the
/// RTCP one is the same address.
fn candidate_counts(sdp: &str) -> (u16, u16, u16) {
    let distinct: std::collections::BTreeSet<&str> =
        sdp.lines().filter_map(|line| line.trim_end().strip_prefix("a=candidate:")).collect();
    let mut counts = (0u16, 0u16, 0u16);
    for candidate in distinct {
        if candidate.split(' ').nth(1) != Some("1") {
            continue;
        }
        let kind = candidate.split(' ').skip_while(|word| *word != "typ").nth(1);
        match kind {
            Some("host") => counts.0 = counts.0.saturating_add(1),
            Some("srflx") => counts.1 = counts.1.saturating_add(1),
            Some("relay") => counts.2 = counts.2.saturating_add(1),
            _ => {}
        }
    }
    counts
}

fn candidate_count(sdp: &str) -> usize {
    sdp.lines().filter(|line| line.starts_with("a=candidate:")).count()
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

    fn ms(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    fn found(host: u16, srflx: Option<u64>, relay: Option<u64>) -> Found {
        Found { host, srflx: srflx.map(ms), relay: relay.map(ms), complete: false }
    }

    const BOTH: Wanted = Wanted { srflx: true, relay: true, relay_only: false };

    // DataChannel setup time (2026-09-29), as the calls' media (ft-media's `send_at`): a server
    // that never answers used to hold every description for the whole deadline. Gathering that
    // completes goes at once.
    #[test]
    fn a_complete_gathering_goes_at_once() {
        let complete = Found { complete: true, ..found(1, None, None) };
        assert_eq!(send_at(&complete, BOTH, GATHER_LIMITS), Duration::ZERO);
    }

    // Once every kind the routing wants is there, a short settle lets the other interfaces'
    // candidates, which travel at the same time, come in too.
    #[test]
    fn with_every_wanted_kind_the_description_goes_after_a_short_settle() {
        assert_eq!(send_at(&found(2, Some(40), Some(120)), BOTH, GATHER_LIMITS), ms(220));
        let no_turn = Wanted { srflx: true, relay: false, relay_only: false };
        assert_eq!(send_at(&found(1, Some(50), None), no_turn, GATHER_LIMITS), ms(150), "no relay to wait for");
        let nothing = Wanted { srflx: false, relay: false, relay_only: false };
        assert_eq!(send_at(&found(1, None, None), nothing, GATHER_LIMITS), ms(100), "no servers");
    }

    // A server that never answers holds the description a second at most, when something usable
    // was gathered.
    #[test]
    fn a_server_that_never_answers_holds_the_description_a_second_at_most() {
        assert_eq!(send_at(&found(1, None, None), BOTH, GATHER_LIMITS), ms(1_000));
        assert_eq!(send_at(&found(1, Some(50), None), BOTH, GATHER_LIMITS), ms(1_000));
        assert_eq!(send_at(&found(1, None, Some(80)), BOTH, GATHER_LIMITS), ms(1_000));
    }

    // A slow network: the relay takes two round trips where STUN took one. Its first answer says
    // how slow, and the relay gets three times that, within the deadline.
    #[test]
    fn on_a_slow_network_the_relay_gets_the_time_stun_says_it_needs() {
        assert_eq!(send_at(&found(1, Some(600), None), BOTH, GATHER_LIMITS), ms(1_900));
        assert_eq!(send_at(&found(1, Some(1_500), None), BOTH, GATHER_LIMITS), ms(3_000), "never past the deadline");
    }

    // "Always relay": only a relay candidate is usable; without one the description waits for the
    // whole deadline (and then fails: no candidate).
    #[test]
    fn always_relay_waits_for_its_relay() {
        let always = Wanted { srflx: false, relay: true, relay_only: true };
        assert_eq!(send_at(&found(0, None, None), always, GATHER_LIMITS), ms(3_000));
        assert_eq!(send_at(&found(0, None, Some(400)), always, GATHER_LIMITS), ms(500));
    }

    // The data channel has no routing setting of its own: STUN and TURN from the router, and
    // "Always relay" when the session is relay only.
    #[test]
    fn what_a_description_waits_for_follows_the_session_s_servers() {
        let turn = TurnServer { url: "turn:t:3478".to_owned(), username: "u".to_owned(), credential: "c".to_owned() };
        let both = SessionConfig { stun_servers: vec!["stun:s:3478".to_owned()], turn_servers: vec![turn], ..SessionConfig::offline() };
        assert_eq!(Wanted::of(&both), BOTH);
        let relay_only = SessionConfig { relay_only: true, ..both.clone() };
        assert_eq!(Wanted::of(&relay_only), Wanted { srflx: false, relay: true, relay_only: true });
        let no_turn = SessionConfig { turn_servers: Vec::new(), ..both };
        assert_eq!(Wanted::of(&no_turn), Wanted { srflx: true, relay: false, relay_only: false });
        assert_eq!(Wanted::of(&SessionConfig::offline()), Wanted { srflx: false, relay: false, relay_only: false });
    }

    // The deadline a session is configured with bounds every wait.
    #[test]
    fn the_session_s_deadline_bounds_the_wait() {
        let short = GatherLimits { deadline: ms(500), ..GATHER_LIMITS };
        assert_eq!(send_at(&found(1, None, None), BOTH, short), ms(500));
    }

    // DataChannel setup timings (2026-09-29): when gathering started and ended and what it gave,
    // for the diagnostics (names and numbers only).
    #[tokio::test(flavor = "multi_thread")]
    async fn a_session_reports_its_gathering() {
        let (signals, mut descriptions) = mpsc::channel(8);
        let (session, _) = Session::start(SessionConfig::offline(), Role::Caller, signals).await.expect("starts");
        assert_eq!(session.gathering(), Gathering::default(), "nothing gathered before the offer");
        session.invite().await.expect("offers");
        assert!(descriptions.recv().await.is_some(), "the offer went out");
        let gathering = session.gathering();
        let (started, finished) = (gathering.started.expect("started"), gathering.finished.expect("finished"));
        assert!(finished >= started);
        assert!(gathering.complete, "loopback with no servers completes at once");
        assert_eq!((gathering.host, gathering.srflx, gathering.relay), (1, 0, 0));
        let _ = session.close().await;
    }

    // End to end: a STUN server that never answers (a local socket nobody reads) holds the offer
    // a second, not the 3 s deadline.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_offer_does_not_wait_for_a_stun_server_that_never_answers() {
        let silent = std::net::UdpSocket::bind("127.0.0.1:0").expect("a socket");
        let stun = format!("stun:{}", silent.local_addr().expect("its address"));
        let config = SessionConfig { stun_servers: vec![stun], ..SessionConfig::offline() };
        let (signals, mut descriptions) = mpsc::channel(8);
        let (session, _) = Session::start(config, Role::Caller, signals).await.expect("starts");
        let started = std::time::Instant::now();
        session.invite().await.expect("offers");
        let waited = started.elapsed();
        assert!(descriptions.recv().await.is_some(), "the offer went out");
        assert!(waited >= ms(900) && waited < ms(1_500), "waited {waited:?}");
        assert!(!session.gathering().complete, "cut short");
        let _ = session.close().await;
    }

    // The distinct candidates by type: each bundled line and each RTCP twin count once.
    #[test]
    fn candidates_are_counted_once_by_type() {
        let sdp = "v=0\r\n\
            a=candidate:1 1 udp 2130706431 10.0.0.2 5000 typ host\r\n\
            a=candidate:1 2 udp 2130706431 10.0.0.2 5000 typ host\r\n\
            a=candidate:2 1 udp 1694498815 192.0.2.4 6000 typ srflx raddr 10.0.0.2 rport 5000\r\n\
            a=candidate:3 1 udp 16777215 198.51.100.8 7000 typ relay raddr 192.0.2.4 rport 6000\r\n\
            a=candidate:4 1 udp 2130706431 10.0.0.3 5001 typ host\r\n\
            a=candidate:1 1 udp 2130706431 10.0.0.2 5000 typ host\r\n";
        assert_eq!(candidate_counts(sdp), (2, 1, 1));
        assert_eq!(candidate_counts("v=0\r\n"), (0, 0, 0));
    }

    #[test]
    fn stun_servers_are_kept_in_order() {
        let config = SessionConfig::with_stun(["stun:one:3478".to_owned(), "stun:two:3478".to_owned()]);
        assert_eq!(config.stun_servers, ["stun:one:3478", "stun:two:3478"]);
    }
}
