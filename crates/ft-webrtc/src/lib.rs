use std::sync::atomic::{AtomicBool, Ordering};
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

pub const CHANNEL_LABEL: &str = "messages";

const CLOSE_WAIT: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "payload")]
pub enum Signal {
    Sdp(String),
    // not sent yet, but accepted
    Ice(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Caller,
    Callee,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnServer {
    pub url: String,
    pub username: String,
    pub credential: String,
}

#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub stun_servers: Vec<String>,
    pub turn_servers: Vec<TurnServer>,
    // the contact never learns our IP
    pub relay_only: bool,
    pub bind: Vec<String>,
    // phones change networks with the app open, so a later session re-reads the interfaces
    pub follow_interfaces: bool,
    pub gather_timeout: Duration,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            stun_servers: Vec::new(),
            turn_servers: Vec::new(),
            relay_only: false,
            bind: interface_addresses(),
            follow_interfaces: true,
            gather_timeout: Duration::from_secs(3),
        }
    }
}

impl SessionConfig {
    pub fn offline() -> Self {
        Self { bind: vec!["127.0.0.1:0".to_owned()], follow_interfaces: false, ..Self::default() }
    }

    pub fn with_stun(servers: impl IntoIterator<Item = String>) -> Self {
        Self { stun_servers: servers.into_iter().collect(), ..Self::default() }
    }

    pub fn listening_now(mut self) -> Self {
        if self.follow_interfaces {
            self.bind = interface_addresses();
        }
        self
    }
}

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

// not 0.0.0.0, with STUN configured webrtc 0.21 never finishes ICE gathering on it
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

pub struct Inbox(mpsc::Receiver<Vec<u8>>);

impl Inbox {
    pub async fn next(&mut self) -> Option<Vec<u8>> {
        self.0.recv().await
    }

    pub async fn next_text(&mut self) -> Option<String> {
        loop {
            if let Ok(text) = String::from_utf8(self.next().await?) {
                return Some(text);
            }
        }
    }
}

type SharedChannel = Arc<Mutex<Option<Arc<dyn DataChannel>>>>;

// must never block, long work gets spawned
struct Events {
    runtime: Arc<dyn Runtime>,
    gathered: watch::Sender<Live>,
    open: watch::Sender<bool>,
    opened_once: Arc<AtomicBool>,
    gone: watch::Sender<bool>,
    channel: SharedChannel,
    // the inbox ends when this channel closes because nothing else holds a sender
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

    // a peer that vanishes never closes the channel, only the connection notices. Disconnected
    // isn't the end either, after a stall it can come back with the channel still working
    async fn on_connection_state_change(&self, state: RTCPeerConnectionState) {
        match state {
            RTCPeerConnectionState::Disconnected => {
                let _ = self.open.send(false);
            }
            RTCPeerConnectionState::Connected if self.opened_once.load(Ordering::SeqCst) && !*self.gone.borrow() => {
                let _ = self.open.send(true);
            }
            RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed => {
                let _ = self.open.send(false);
                let _ = self.gone.send(true);
            }
            _ => {}
        }
    }

    async fn on_data_channel(&self, channel: Arc<dyn DataChannel>) {
        let Some(messages) = self.messages.lock().unwrap_or_else(PoisonError::into_inner).take() else { return };
        *self.channel.lock().await = Some(channel.clone());
        listen(&self.runtime, channel, self.open.clone(), self.opened_once.clone(), self.gone.subscribe(), messages);
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
    gone: watch::Sender<bool>,
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
        let opened_once = Arc::new(AtomicBool::new(false));
        let gone = watch::Sender::new(false);
        let channel: SharedChannel = Arc::new(Mutex::new(None));

        let (ice_servers, policy) = ice_configuration(&config);
        let events = Arc::new(Events {
            runtime: runtime.clone(),
            gathered: gathered_tx,
            open: open_tx.clone(),
            opened_once: opened_once.clone(),
            gone: gone.clone(),
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
            listen(&runtime, created, open_tx.clone(), opened_once, gone.subscribe(), messages.clone());
        }

        let wanted = Wanted::of(&config);
        let limits = GatherLimits { deadline: config.gather_timeout, ..GATHER_LIMITS };
        let report = Arc::default();
        drop(messages);
        let open = open_tx;
        Ok((Self { connection, signals, channel, gathered, wanted, limits, report, opened, open, gone }, Inbox(inbox)))
    }

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

    pub fn gathering(&self) -> Gathering {
        *self.report.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn gathering_started(&self) {
        let mut report = self.report.lock().unwrap_or_else(PoisonError::into_inner);
        *report = Gathering { started: Some(Instant::now()), ..Gathering::default() };
    }

    fn gathering_finished(&self, sdp: &str) {
        let (host, srflx, relay) = candidate_counts(sdp);
        let complete = self.gathered.borrow().complete;
        let mut report = self.report.lock().unwrap_or_else(PoisonError::into_inner);
        *report = Gathering { finished: Some(Instant::now()), complete, host, srflx, relay, ..*report };
    }

    pub fn is_open(&self) -> bool {
        *self.opened.borrow()
    }

    #[doc(hidden)]
    pub async fn vanish(&self) {
        let _ = self.open.send(false);
        let _ = self.gone.send(true);
        let _ = self.connection.close().await;
    }

    #[doc(hidden)]
    pub fn lose(&self) {
        let _ = self.open.send(false);
        let _ = self.gone.send(true);
    }

    pub fn open_changes(&self) -> watch::Receiver<bool> {
        self.open.subscribe()
    }

    // close the data channel first, closing the connection alone doesn't tell the other side
    pub async fn close(&self) -> Result<()> {
        let channel = self.channel.lock().await.clone();
        if let Some(channel) = channel {
            if channel.close().await.is_ok() {
                let mut opened = self.opened.clone();
                let _ = tokio::time::timeout(CLOSE_WAIT, opened.wait_for(|open| !open)).await;
            }
        }
        let _ = self.open.send(false);
        let _ = self.gone.send(true);
        self.connection.close().await?;
        Ok(())
    }

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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Gathering {
    pub started: Option<Instant>,
    pub finished: Option<Instant>,
    pub complete: bool,
    pub host: u16,
    pub srflx: u16,
    pub relay: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Wanted {
    srflx: bool,
    relay: bool,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Found {
    host: u16,
    srflx: Option<Duration>,
    relay: Option<Duration>,
    complete: bool,
}

#[derive(Debug, Clone, Copy)]
struct GatherLimits {
    settle: Duration,
    cap: Duration,
    deadline: Duration,
}

const GATHER_LIMITS: GatherLimits =
    GatherLimits { settle: Duration::from_millis(100), cap: Duration::from_secs(1), deadline: Duration::from_secs(3) };

// webrtc-rs only completes gathering once every STUN and TURN server answered, don't wait for it
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
        (Some(srflx), Some(relay)) => srflx.max(relay).unwrap_or(Duration::ZERO) + limits.settle,
        // the relay takes two round trips where STUN took one
        (Some(Some(srflx)), None) => limits.cap.max(srflx * 3 + limits.settle),
        _ => limits.cap,
    };
    at.min(limits.deadline)
}

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

// first component only, the RTCP one has the same address
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

// a vanished peer never closes the channel, so the inbox also ends when the connection fails
fn listen(
    runtime: &Arc<dyn Runtime>,
    channel: Arc<dyn DataChannel>,
    open: watch::Sender<bool>,
    opened_once: Arc<AtomicBool>,
    mut gone: watch::Receiver<bool>,
    messages: mpsc::Sender<Vec<u8>>,
) {
    runtime.spawn(Box::pin(async move {
        loop {
            let event = tokio::select! {
                event = channel.poll() => event,
                _ = gone.wait_for(|gone| *gone) => break,
            };
            let Some(event) = event else { break };
            match event {
                DataChannelEvent::OnOpen => {
                    opened_once.store(true, Ordering::SeqCst);
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

    #[test]
    fn sessions_listen_on_concrete_interface_addresses_never_the_wildcard() {
        let config = SessionConfig::with_stun(["stun:one:3478".to_owned()]);
        assert!(!config.bind.is_empty());
        assert!(config.bind.iter().all(|address| !address.starts_with("0.0.0.0")));
    }

    #[test]
    fn a_session_started_later_listens_on_the_interfaces_there_are_then() {
        let stale = SessionConfig { bind: vec!["192.0.2.1:0".to_owned()], ..SessionConfig::with_stun(["stun:one:3478".to_owned()]) };
        assert_eq!(stale.clone().listening_now().bind, interface_addresses());
        assert_eq!(stale.listening_now().stun_servers, ["stun:one:3478"], "only where it listens changes");

        let offline = SessionConfig::offline();
        assert_eq!(offline.clone().listening_now().bind, offline.bind);
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

    #[test]
    fn relay_only_sessions_use_nothing_but_the_turn_relay() {
        let config = SessionConfig { relay_only: true, ..SessionConfig::offline() };
        assert_eq!(ice_configuration(&config).1, RTCIceTransportPolicy::Relay);
    }

    #[test]
    fn counts_the_ice_candidates_carried_by_a_description() {
        let sdp = "v=0\r\na=candidate:1 1 udp 2130706431 192.168.1.2 50000 typ host\r\n\
                   a=mid:0\r\na=candidate:2 1 udp 1694498815 203.0.113.4 50000 typ srflx\r\n";
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

    #[test]
    fn a_complete_gathering_goes_at_once() {
        let complete = Found { complete: true, ..found(1, None, None) };
        assert_eq!(send_at(&complete, BOTH, GATHER_LIMITS), Duration::ZERO);
    }

    #[test]
    fn with_every_wanted_kind_the_description_goes_after_a_short_settle() {
        assert_eq!(send_at(&found(2, Some(40), Some(120)), BOTH, GATHER_LIMITS), ms(220));
        let no_turn = Wanted { srflx: true, relay: false, relay_only: false };
        assert_eq!(send_at(&found(1, Some(50), None), no_turn, GATHER_LIMITS), ms(150), "no relay to wait for");
        let nothing = Wanted { srflx: false, relay: false, relay_only: false };
        assert_eq!(send_at(&found(1, None, None), nothing, GATHER_LIMITS), ms(100), "no servers");
    }

    #[test]
    fn a_server_that_never_answers_holds_the_description_a_second_at_most() {
        assert_eq!(send_at(&found(1, None, None), BOTH, GATHER_LIMITS), ms(1_000));
        assert_eq!(send_at(&found(1, Some(50), None), BOTH, GATHER_LIMITS), ms(1_000));
        assert_eq!(send_at(&found(1, None, Some(80)), BOTH, GATHER_LIMITS), ms(1_000));
    }

    #[test]
    fn on_a_slow_network_the_relay_gets_the_time_stun_says_it_needs() {
        assert_eq!(send_at(&found(1, Some(600), None), BOTH, GATHER_LIMITS), ms(1_900));
        assert_eq!(send_at(&found(1, Some(1_500), None), BOTH, GATHER_LIMITS), ms(3_000), "never past the deadline");
    }

    #[test]
    fn always_relay_waits_for_its_relay() {
        let always = Wanted { srflx: false, relay: true, relay_only: true };
        assert_eq!(send_at(&found(0, None, None), always, GATHER_LIMITS), ms(3_000));
        assert_eq!(send_at(&found(0, None, Some(400)), always, GATHER_LIMITS), ms(500));
    }

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

    #[test]
    fn the_session_s_deadline_bounds_the_wait() {
        let short = GatherLimits { deadline: ms(500), ..GATHER_LIMITS };
        assert_eq!(send_at(&found(1, None, None), BOTH, short), ms(500));
    }

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
