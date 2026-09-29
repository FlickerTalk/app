//! A call's peer connection: an audio track (Opus) and a video track (H.264), descriptions sent
//! whole with their ICE candidates (no trickle), as the WebView's calls and the core's data
//! channels do.
//!
//! Every call negotiates both lines from the start, voice or video (native video, 2026-09-29;
//! `docs/video-nativo.md`): the video line costs nothing while no camera is on, and turning one on
//! never needs a new offer.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use rtc::rtp_transceiver::rtp_sender::RtpCodecKind;
use tokio::sync::{oneshot, watch};
use webrtc::media_stream::track_remote::TrackRemote;
use webrtc::peer_connection::{
    PeerConnection, PeerConnectionEventHandler, RTCConfigurationBuilder, RTCIceCandidateType, RTCIceGatheringState,
    RTCPeerConnectionIceEvent, RTCPeerConnectionState, RTCSessionDescription,
};
use webrtc_engine::rtp::{add_audio_track, peer_connection_builder, AudioSender};
use webrtc_engine::video::rtp::{add_video_track, h264_payload_type, VideoSender};

use crate::{ice_setup, CallRouting, MediaConfig};

/// How far the connection has come.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkState {
    Connecting,
    Connected,
    /// It could not connect, or it stopped answering for good.
    Failed,
    Closed,
}

type PendingTrack = Mutex<Option<oneshot::Sender<Arc<dyn TrackRemote>>>>;

struct Events {
    gathered: watch::Sender<Live>,
    state: Arc<watch::Sender<LinkState>>,
    track: PendingTrack,
    video: PendingTrack,
    video_arrived: watch::Sender<bool>,
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

    async fn on_connection_state_change(&self, state: RTCPeerConnectionState) {
        let state = match state {
            RTCPeerConnectionState::Connected => LinkState::Connected,
            RTCPeerConnectionState::Failed => LinkState::Failed,
            RTCPeerConnectionState::Closed => LinkState::Closed,
            // Disconnected may come back on its own: ICE keeps trying.
            _ => return,
        };
        let _ = self.state.send(state);
    }

    // webrtc-rs hands each remote track over with its first packet.
    async fn on_track(&self, track: Arc<dyn TrackRemote>) {
        let pending = match track.kind().await {
            RtpCodecKind::Audio => &self.track,
            RtpCodecKind::Video => {
                let _ = self.video_arrived.send(true);
                &self.video
            }
            _ => return,
        };
        let pending = pending.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(pending) = pending {
            let _ = pending.send(track);
        }
    }
}

/// How a session's ICE gathering went (call setup timings, 2026-09-29): numbers only.
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

/// One side of a call's connection.
pub struct MediaSession {
    connection: Arc<dyn PeerConnection>,
    sender: Arc<AudioSender>,
    video: Arc<VideoSender>,
    remote_video: Mutex<Option<oneshot::Receiver<Arc<dyn TrackRemote>>>>,
    video_arrived: watch::Receiver<bool>,
    gathered: watch::Receiver<Live>,
    state: watch::Receiver<LinkState>,
    /// Closing says so itself: webrtc-rs does not report its own close.
    closed: Arc<watch::Sender<LinkState>>,
    remote: Mutex<Option<oneshot::Receiver<Arc<dyn TrackRemote>>>>,
    config: MediaConfig,
    report: Mutex<Gathering>,
}

impl MediaSession {
    /// A connection with our audio and video tracks, ready to offer or to answer.
    pub async fn open(config: &MediaConfig) -> Result<Self> {
        let (servers, policy) = ice_setup(config);
        let (gathered_tx, gathered) = watch::channel(Live::default());
        let (state_tx, state) = watch::channel(LinkState::Connecting);
        let state_tx = Arc::new(state_tx);
        let (track_tx, track) = oneshot::channel();
        let (video_tx, remote_video) = oneshot::channel();
        let (video_arrived_tx, video_arrived) = watch::channel(false);
        let events = Events {
            gathered: gathered_tx,
            state: state_tx.clone(),
            track: Mutex::new(Some(track_tx)),
            video: Mutex::new(Some(video_tx)),
            video_arrived: video_arrived_tx,
        };
        let connection: Arc<dyn PeerConnection> = Arc::new(
            peer_connection_builder()?
                .with_configuration(
                    RTCConfigurationBuilder::new().with_ice_servers(servers).with_ice_transport_policy(policy).build(),
                )
                .with_handler(Arc::new(events))
                .with_udp_addrs(config.bind.clone())
                .build()
                .await?,
        );
        // Audio first: the offer's lines follow the order the tracks were added in.
        let tracks = async {
            let audio = add_audio_track(connection.as_ref()).await?;
            let video = add_video_track(connection.as_ref()).await?;
            anyhow::Ok((audio, video))
        };
        let (sender, video) = match tracks.await {
            Ok(tracks) => tracks,
            Err(error) => {
                let _ = connection.close().await;
                return Err(error);
            }
        };
        Ok(Self {
            connection,
            sender: Arc::new(sender),
            video: Arc::new(video),
            remote_video: Mutex::new(Some(remote_video)),
            video_arrived,
            gathered,
            state,
            closed: state_tx,
            remote: Mutex::new(Some(track)),
            config: config.clone(),
            report: Mutex::new(Gathering::default()),
        })
    }

    /// Our offer, once ICE gathering is over (or its deadline passed).
    pub async fn offer(&self) -> Result<String> {
        let offer = self.connection.create_offer(None).await?;
        self.connection.set_local_description(offer).await?;
        self.gathering_started();
        self.gathered_description().await
    }

    /// Our answer to `offer`, once gathered: the lines the offer has, audio and, if there is one,
    /// video (H.264), both ways.
    pub async fn answer(&self, offer: &str) -> Result<String> {
        let offer = RTCSessionDescription::offer(offer.to_owned()).context("invalid offer")?;
        self.connection.set_remote_description(offer).await?;
        let answer = self.connection.create_answer(None).await?;
        self.connection.set_local_description(answer).await?;
        self.gathering_started();
        self.gathered_description().await
    }

    /// The other side's answer to our offer.
    pub async fn accept(&self, answer: &str) -> Result<()> {
        let answer = RTCSessionDescription::answer(answer.to_owned()).context("invalid answer")?;
        self.connection.set_remote_description(answer).await?;
        Ok(())
    }

    /// The local description with the candidates gathered so far: once gathering completes, once
    /// it has what the routing wants (`send_at`) or, at most, when the deadline passes. A server
    /// or an interface that never answers must not hold the call (2026-09-29).
    async fn gathered_description(&self) -> Result<String> {
        let started = self.gathering().started.unwrap_or_else(Instant::now);
        let wanted = Wanted::of(&self.config);
        let limits = GatherLimits { deadline: self.config.gather_timeout, ..GATHER_LIMITS };
        let mut gathered = self.gathered.clone();
        loop {
            let found = gathered.borrow_and_update().since(started);
            let at = started + send_at(&found, wanted, limits);
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
        let description = self.connection.local_description().await.ok_or_else(|| anyhow!("no local description"))?;
        self.gathering_finished(&description.sdp);
        if !description.sdp.contains("a=candidate:") {
            bail!("no ICE candidate was gathered");
        }
        Ok(description.sdp)
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

    /// How the gathering went, so far.
    pub fn gathering(&self) -> Gathering {
        *self.report.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Follows the connection.
    pub fn state(&self) -> watch::Receiver<LinkState> {
        self.state.clone()
    }

    /// Our voice goes out here.
    pub fn sender(&self) -> Arc<AudioSender> {
        self.sender.clone()
    }

    /// The other side's audio track, handed over when its first packet arrives. Only once.
    pub fn take_remote(&self) -> Option<oneshot::Receiver<Arc<dyn TrackRemote>>> {
        self.remote.lock().unwrap_or_else(PoisonError::into_inner).take()
    }

    /// Our video goes out here.
    pub fn video_sender(&self) -> Arc<VideoSender> {
        self.video.clone()
    }

    /// The other side's video track, handed over when its first packet arrives. Only once.
    pub fn take_remote_video(&self) -> Option<oneshot::Receiver<Arc<dyn TrackRemote>>> {
        self.remote_video.lock().unwrap_or_else(PoisonError::into_inner).take()
    }

    /// Turns `true` when the other side's first video packet arrives.
    pub fn remote_video_arrived(&self) -> watch::Receiver<bool> {
        self.video_arrived.clone()
    }

    /// The connection itself, for what the call's video reads of it.
    pub(crate) fn connection(&self) -> Arc<dyn PeerConnection> {
        self.connection.clone()
    }

    /// Whether the call's video line was negotiated both ways: open, sending and receiving on
    /// each side, with H.264 we can send. Never with an older app's voice call (no video line).
    pub async fn video_both_ways(&self) -> bool {
        video_both_ways(self.connection.as_ref(), &self.video).await
    }

    pub async fn close(&self) {
        let _ = self.connection.close().await;
        let _ = self.closed.send(LinkState::Closed);
    }
}

/// What the routing wants a description to carry (call setup time, 2026-09-29).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Wanted {
    /// A server reflexive candidate: STUN is configured and the routing uses it.
    srflx: bool,
    /// A relay candidate: TURN is configured and the routing allows it.
    relay: bool,
    /// Only relay candidates are usable ("Always relay").
    relay_only: bool,
}

impl Wanted {
    fn of(config: &MediaConfig) -> Self {
        let relay_only = config.routing == CallRouting::Always;
        Self {
            srflx: !config.stun.is_empty() && !relay_only,
            relay: config.turn.is_some() && config.routing != CallRouting::Direct,
            relay_only,
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

const GATHER_LIMITS: GatherLimits =
    GatherLimits { settle: Duration::from_millis(100), cap: Duration::from_secs(1), deadline: Duration::from_secs(3) };

/// When (from the start of gathering) the description goes, if nothing more comes.
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

/// Whether the video line negotiated on `connection` goes both ways (see
/// [`MediaSession::video_both_ways`]).
pub(crate) async fn video_both_ways(connection: &dyn PeerConnection, video: &VideoSender) -> bool {
    let (Some(local), Some(remote)) = (connection.local_description().await, connection.remote_description().await)
    else {
        return false;
    };
    if !video_line_both_ways(&local.sdp) || !video_line_both_ways(&remote.sdp) {
        return false;
    }
    let Ok(parameters) = video.rtp_sender().get_parameters().await else { return false };
    h264_payload_type(&parameters.rtp_parameters.codecs).is_some()
}

/// Whether the first video line of `sdp` is open (a port other than 0) and neither sends nor
/// receives only: `sendrecv`, said or by default.
fn video_line_both_ways(sdp: &str) -> bool {
    let mut lines = sdp.lines().skip_while(|line| !line.starts_with("m=video "));
    let Some(media) = lines.next() else { return false };
    if media.split(' ').nth(1) == Some("0") {
        return false;
    }
    !lines
        .take_while(|line| !line.starts_with("m="))
        .any(|line| matches!(line.trim_end(), "a=sendonly" | "a=recvonly" | "a=inactive"))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const LIMIT: Duration = Duration::from_secs(10);

    async fn wait_until(session: &MediaSession, wanted: LinkState) {
        let mut state = session.state();
        tokio::time::timeout(LIMIT, state.wait_for(|state| *state == wanted))
            .await
            .unwrap_or_else(|_| panic!("never {wanted:?}"))
            .expect("the session is there");
    }

    fn sections(sdp: &str) -> Vec<String> {
        sdp.split("\r\nm=").skip(1).map(|section| format!("m={section}")).collect()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn two_sessions_connect_with_one_offer_and_one_answer() {
        let caller = MediaSession::open(&MediaConfig::default()).await.expect("caller");
        let callee = MediaSession::open(&MediaConfig::default()).await.expect("callee");

        let offer = caller.offer().await.expect("offer");
        assert!(offer.contains("m=audio "), "{offer}");
        assert!(offer.contains("a=rtpmap:111 opus/48000/2"), "{offer}");
        assert!(offer.contains("a=candidate:"), "the candidates travel in the offer: {offer}");

        let answer = callee.answer(&offer).await.expect("answer");
        assert!(answer.contains("a=candidate:"), "{answer}");
        caller.accept(&answer).await.expect("accepted");

        wait_until(&caller, LinkState::Connected).await;
        wait_until(&callee, LinkState::Connected).await;

        caller.close().await;
        wait_until(&caller, LinkState::Closed).await;
        callee.close().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_description_that_is_not_sdp_is_refused() {
        let callee = MediaSession::open(&MediaConfig::default()).await.expect("callee");
        assert!(callee.answer("hello").await.is_err());
        callee.close().await;
    }

    fn section<'a>(sections: &'a [String], kind: &str) -> Option<&'a String> {
        sections.iter().find(|section| section.starts_with(&format!("m={kind} ")))
    }

    // Every native call negotiates video from the start (docs/video-nativo.md, decision 1): Opus
    // and H.264 Constrained Baseline in packetization mode 1, with the orientation extension, both
    // ways, so turning a camera on never needs a new offer.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_offer_carries_opus_and_h264_video_both_ways() {
        let caller = MediaSession::open(&MediaConfig::default()).await.expect("caller");
        let offer = caller.offer().await.expect("offer");
        let sections = sections(&offer);

        let audio = section(&sections, "audio").expect("an audio line");
        assert!(audio.contains("a=rtpmap:111 opus/48000/2"), "{offer}");
        assert!(audio.contains("a=sendrecv"), "{offer}");

        let video = section(&sections, "video").expect("a video line");
        assert!(!video.starts_with("m=video 0 "), "{offer}");
        assert!(video.contains("H264/90000"), "{offer}");
        assert!(video.contains("profile-level-id=42e01f"), "{offer}");
        assert!(video.contains("packetization-mode=1"), "{offer}");
        assert!(video.contains("urn:3gpp:video-orientation"), "{offer}");
        assert!(video.contains("a=sendrecv"), "{offer}");
        assert!(!offer.contains("VP8"), "{offer}");
        caller.close().await;
    }

    // Interop: an older app's WebView offers video with H.264 among its codecs; the native side
    // answers audio and video, both ways, with the WebView's own payload type.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_webview_video_offer_is_answered_with_audio_and_video() {
        let callee = MediaSession::open(&MediaConfig::default()).await.expect("callee");
        let answer = callee.answer(&crate::testing::webview_video_offer()).await.expect("answer");
        let sections = sections(&answer);

        let audio = section(&sections, "audio").expect("an audio line");
        assert!(!audio.starts_with("m=audio 0 "), "{answer}");
        assert!(audio.contains("a=rtpmap:111 opus/48000/2"), "{answer}");
        assert!(audio.contains("a=sendrecv"), "{answer}");

        let video = section(&sections, "video").expect("a video line");
        assert!(!video.starts_with("m=video 0 "), "video is refused:\n{answer}");
        assert!(video.contains("a=rtpmap:106 H264/90000"), "{answer}");
        assert!(video.contains("profile-level-id=42e01f"), "{answer}");
        assert!(video.contains("a=sendrecv"), "{answer}");
        assert!(callee.video_both_ways().await, "video goes both ways");
        callee.close().await;
    }

    // Interop: an older app's WebView offers a voice call with no video line. The native side's
    // video track must not spoil the answer: audio only, and no video either way.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_webview_voice_offer_is_answered_with_audio_only() {
        let callee = MediaSession::open(&MediaConfig::default()).await.expect("callee");
        let answer = callee.answer(&crate::testing::webview_voice_offer()).await.expect("answer");
        let sections = sections(&answer);

        let audio = section(&sections, "audio").expect("an audio line");
        assert!(!audio.starts_with("m=audio 0 "), "{answer}");
        assert!(audio.contains("a=rtpmap:111 opus/48000/2"), "{answer}");
        assert!(audio.contains("a=sendrecv"), "{answer}");
        assert!(section(&sections, "video").is_none(), "{answer}");
        assert!(!callee.video_both_ways().await, "no video line, no video");
        callee.close().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn two_native_sessions_have_video_both_ways_once_negotiated() {
        let caller = MediaSession::open(&MediaConfig::default()).await.expect("caller");
        let callee = MediaSession::open(&MediaConfig::default()).await.expect("callee");
        assert!(!caller.video_both_ways().await, "nothing is negotiated yet");
        let offer = caller.offer().await.expect("offer");
        let answer = callee.answer(&offer).await.expect("answer");
        caller.accept(&answer).await.expect("accepted");
        assert!(caller.video_both_ways().await);
        assert!(callee.video_both_ways().await);
        caller.close().await;
        callee.close().await;
    }

    fn ms(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    fn found(host: u16, srflx: Option<u64>, relay: Option<u64>) -> Found {
        Found { host, srflx: srflx.map(ms), relay: relay.map(ms), complete: false }
    }

    const BOTH: Wanted = Wanted { srflx: true, relay: true, relay_only: false };

    // Call setup time (2026-09-29): a server that never answers (on one interface, say) used to
    // hold every description for the whole deadline. Gathering that completes goes at once.
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
        let direct = Wanted { srflx: true, relay: false, relay_only: false };
        assert_eq!(send_at(&found(1, Some(50), None), direct, GATHER_LIMITS), ms(150), "Direct wants no relay");
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

    // End to end: a STUN server that never answers (a local socket nobody reads) holds the offer
    // a second, not the 3 s deadline, and the offer carries what was gathered.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_offer_does_not_wait_for_a_stun_server_that_never_answers() {
        let silent = std::net::UdpSocket::bind("127.0.0.1:0").expect("a socket");
        let stun = format!("stun:{}", silent.local_addr().expect("its address"));
        let caller = MediaSession::open(&MediaConfig { stun: vec![stun], ..MediaConfig::default() }).await.expect("caller");
        let started = Instant::now();
        let offer = caller.offer().await.expect("offer");
        let waited = started.elapsed();
        assert!(waited >= ms(900) && waited < ms(1_500), "waited {waited:?}");
        let gathering = caller.gathering();
        assert!(!gathering.complete, "cut short");
        assert_eq!((gathering.host, gathering.srflx), (1, 0), "{offer}");
        caller.close().await;
    }

    #[test]
    fn what_a_description_waits_for_follows_the_routing() {
        let config = |routing| MediaConfig {
            stun: vec!["stun:s:3478".to_owned()],
            turn: Some(crate::TurnRelay { urls: vec!["turn:t:3478".to_owned()], username: "u".to_owned(), credential: "c".to_owned() }),
            routing,
            ..MediaConfig::default()
        };
        assert_eq!(Wanted::of(&config(CallRouting::Auto)), BOTH);
        assert_eq!(Wanted::of(&config(CallRouting::Direct)), Wanted { srflx: true, relay: false, relay_only: false });
        assert_eq!(Wanted::of(&config(CallRouting::Always)), Wanted { srflx: false, relay: true, relay_only: true });
        assert_eq!(Wanted::of(&MediaConfig::default()), Wanted { srflx: false, relay: false, relay_only: false });
    }

    // Call setup timings (2026-09-29): when gathering started and ended and what it gave, for
    // the diagnostics (names and numbers only).
    #[tokio::test(flavor = "multi_thread")]
    async fn a_session_reports_its_gathering() {
        let caller = MediaSession::open(&MediaConfig::default()).await.expect("caller");
        assert_eq!(caller.gathering(), Gathering::default(), "nothing gathered before the offer");
        let offer = caller.offer().await.expect("offer");
        let gathering = caller.gathering();
        let (started, finished) = (gathering.started.expect("started"), gathering.finished.expect("finished"));
        assert!(finished >= started);
        assert!(gathering.complete, "loopback with no servers completes at once");
        assert_eq!((gathering.host, gathering.srflx, gathering.relay), (1, 0, 0), "{offer}");
        caller.close().await;
    }

    // A candidate repeated in each bundled line is one candidate, and so is its RTCP twin
    // (component 2, the same address).
    #[test]
    fn candidates_are_counted_once_by_type() {
        let sdp = "v=0\r\n\
            a=candidate:1 1 udp 2130706431 10.0.0.2 5000 typ host\r\n\
            a=candidate:1 2 udp 2130706431 10.0.0.2 5000 typ host\r\n\
            a=candidate:2 1 udp 1694498815 192.0.2.4 6000 typ srflx raddr 10.0.0.2 rport 5000\r\n\
            a=candidate:3 1 udp 16777215 198.51.100.8 7000 typ relay raddr 192.0.2.4 rport 6000\r\n\
            a=candidate:4 1 udp 2130706431 10.0.0.3 5001 typ host\r\n\
            m=video 9 UDP/TLS/RTP/SAVPF 102\r\n\
            a=candidate:1 1 udp 2130706431 10.0.0.2 5000 typ host\r\n";
        assert_eq!(candidate_counts(sdp), (2, 1, 1));
        assert_eq!(candidate_counts("v=0\r\n"), (0, 0, 0));
    }

    // The direction of a description's video line, as the answer or the offer says it.
    #[test]
    fn a_video_line_goes_both_ways_only_when_open_and_sendrecv() {
        let sdp = |video: &str| format!("v=0\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=sendrecv\r\n{video}");
        assert!(video_line_both_ways(&sdp("m=video 9 UDP/TLS/RTP/SAVPF 102\r\na=sendrecv\r\n")));
        assert!(video_line_both_ways(&sdp("m=video 9 UDP/TLS/RTP/SAVPF 102\r\na=mid:1\r\n")), "sendrecv is the default");
        assert!(!video_line_both_ways(&sdp("m=video 0 UDP/TLS/RTP/SAVPF 102\r\na=sendrecv\r\n")), "refused");
        assert!(!video_line_both_ways(&sdp("m=video 9 UDP/TLS/RTP/SAVPF 102\r\na=recvonly\r\n")));
        assert!(!video_line_both_ways(&sdp("m=video 9 UDP/TLS/RTP/SAVPF 102\r\na=sendonly\r\n")));
        assert!(!video_line_both_ways(&sdp("m=video 9 UDP/TLS/RTP/SAVPF 102\r\na=inactive\r\n")));
        assert!(!video_line_both_ways(&sdp("")), "no video line");
    }
}
