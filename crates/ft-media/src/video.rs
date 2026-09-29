//! A call's video (native video, 2026-09-29; design in `docs/video-nativo.md`): the platform's
//! camera and display on the call's own peer connection, next to its `Voice`, with
//! `webrtc-engine`'s `VideoCall`.
//!
//! Every native call negotiates an audio and a video line from the start (media version 1, see
//! `ft_protocol::CALL_MEDIA_VERSION`), so turning a camera on or off never needs a new offer: each
//! side runs or stops its own camera, and tells the other with a `CallMedia` packet (the core does
//! that). The devices are made the first time the call needs video (our camera, or theirs) and
//! kept, with their native views, until the call ends.
//!
//! - The engine's `VideoCall` runs while the call is connected and either camera is on
//!   (`VideoState::any`), and stops, keeping the devices, when neither is.
//! - `VideoCall::start` always starts the camera, so it gets a **gated** one: while the gate is
//!   shut `start` opens nothing, and the call is paused right after starting. The other side's
//!   picture never opens ours.
//! - Our camera sends while it is on and not held (`VideoState::sending`): `resume` (with a
//!   keyframe) or `pause`.
//! - The engine boxes the camera and the display inside each `VideoCall`; they are shared here, so
//!   a new `VideoCall` gets the same devices, and so is the other side's stream (its reassembly).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use tokio::sync::{oneshot, watch, Mutex};
use tokio::task::JoinHandle;
use webrtc::peer_connection::PeerConnection;
use webrtc_engine::video::assemble::AssemblerStats;
use webrtc_engine::video::call::{FrameSink, FrameSource, VideoCall, VideoCallConfig, VideoTransport};
pub use webrtc_engine::video::call::VideoStats;
use webrtc_engine::video::rtp::{VideoReceiver, VideoRtpError, VideoSender};
use webrtc_engine::video::{EncodedFrame, FrameSender, VideoConfig};
pub use webrtc_engine::video::{Facing, VideoError, VideoSink, VideoSource};

use crate::session::video_both_ways;
use crate::views::{self, RemoteShape};
use crate::MediaSession;

/// A platform's camera and display for one call.
pub struct VideoDevices {
    pub source: Box<dyn VideoSource>,
    pub sink: Box<dyn VideoSink>,
}

/// Makes the devices for a call, once, the first time it needs video. The platform's factory
/// also publishes their native views (`crate::views`); the tests' factory gives the engine's fakes.
pub type VideoFactory = Arc<dyn Fn() -> Result<VideoDevices, VideoError> + Send + Sync>;

/// A platform's video for calls, as `AudioPlatform` is its audio.
#[derive(Clone)]
pub struct VideoPlatform {
    pub devices: VideoFactory,
}

/// A call's video as it is now: a whole state, never a change, so whoever reads it late (a
/// WebView that comes up, the native call screen) reads it right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VideoState {
    /// The call is connected with a video line both ways: our camera may turn on. Never with an
    /// older app's voice call (no video line).
    pub available: bool,
    /// The user wants our camera on.
    pub camera: bool,
    /// The phone holds our camera although the user wants it (the app in the background, the
    /// phone locked, the call screen left): nothing goes out until it is given back.
    pub paused: bool,
    /// The camera in use, or to use when ours turns on.
    pub facing: Facing,
    /// The other side's camera is on (its last `CallMedia`).
    pub remote: bool,
    /// The other side's camera is on but held by its phone: its last picture is old.
    pub remote_paused: bool,
    /// How to lay the other side's picture out, once a keyframe was decoded.
    pub shape: Option<RemoteShape>,
}

impl VideoState {
    /// Whether our camera's pictures go out now.
    pub fn sending(&self) -> bool {
        self.camera && !self.paused
    }

    /// Whether the call has video at all: either camera on, even held. The call screen shows
    /// the video layout and CallKit's `hasVideo` follows this.
    pub fn any(&self) -> bool {
        self.camera || self.remote
    }
}

/// How often the video looks at what comes on its own: the other side's first frames, and the
/// shape of its picture.
const LOOK_EVERY: Duration = Duration::from_millis(250);

/// The video of one call.
pub struct Video {
    inner: Arc<Inner>,
    watcher: StdMutex<Option<JoinHandle<()>>>,
}

struct Inner {
    platform: VideoPlatform,
    connection: Arc<dyn PeerConnection>,
    sender: Arc<VideoSender>,
    /// The other side's stream, read by one `VideoCall` after another.
    remote: Arc<Mutex<VideoReceiver>>,
    /// The other side's first video packet has come.
    arrived: watch::Receiver<bool>,
    state: watch::Sender<VideoState>,
    run: Mutex<Run>,
}

#[derive(Default)]
struct Run {
    connected: bool,
    stopped: bool,
    /// The other side has said something about its camera (a `CallMedia`): what comes no longer
    /// decides.
    remote_said: bool,
    devices: Option<Devices>,
    call: Option<VideoCall>,
    /// The counters of the last `VideoCall` that ran.
    last: Option<VideoStats>,
}

/// The call's camera and display, shared with each `VideoCall` that runs them.
struct Devices {
    source: Arc<StdMutex<Box<dyn VideoSource>>>,
    sink: Arc<StdMutex<Box<dyn VideoSink>>>,
    /// Open while our camera may send.
    gate: Arc<AtomicBool>,
}

impl Devices {
    fn new(made: VideoDevices) -> Self {
        Self {
            source: Arc::new(StdMutex::new(made.source)),
            sink: Arc::new(StdMutex::new(made.sink)),
            gate: Arc::new(AtomicBool::new(false)),
        }
    }
}

fn lock<T: ?Sized>(mutex: &StdMutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Video {
    /// The video of `session`'s call, with the platform's devices. Nothing runs, and no camera
    /// opens, until a camera turns on (ours or theirs) with the call connected.
    pub fn for_session(session: &MediaSession, platform: VideoPlatform) -> Self {
        let sender = session.video_sender();
        // Taken already: nothing will come.
        let track = session.take_remote_video().unwrap_or_else(|| oneshot::channel().1);
        let remote = VideoReceiver::pending(track, &sender);
        Self {
            inner: Arc::new(Inner {
                platform,
                connection: session.connection(),
                sender,
                remote: Arc::new(Mutex::new(remote)),
                arrived: session.remote_video_arrived(),
                state: watch::channel(VideoState::default()).0,
                run: Mutex::new(Run::default()),
            }),
            watcher: StdMutex::new(None),
        }
    }

    /// The connection is up: video may run from now on. `available` says whether the call has a
    /// video line both ways.
    pub async fn connected(&self) -> Result<()> {
        let mut run = self.inner.run.lock().await;
        if run.stopped || run.connected {
            return Ok(());
        }
        run.connected = true;
        let available = video_both_ways(self.inner.connection.as_ref(), &self.inner.sender).await;
        self.inner.change(&mut run, |state| state.available = available).await?;
        drop(run);
        self.watch();
        Ok(())
    }

    /// Our camera on or off, as the user chose. The camera permission is the app's business,
    /// asked before. Returns the new state; an error leaves it as it was.
    pub async fn set_camera(&self, on: bool) -> Result<VideoState> {
        let mut run = self.inner.running().await?;
        if on && !self.state().available {
            bail!("the call has no video both ways");
        }
        self.inner.change(&mut run, |state| state.camera = on).await
    }

    /// The phone holds our camera (`true`: the app left the screen, the phone locked) or gives it
    /// back. The user's choice stays as it was.
    pub async fn set_paused(&self, paused: bool) -> Result<VideoState> {
        let mut run = self.inner.running().await?;
        self.inner.change(&mut run, |state| state.paused = paused).await
    }

    /// The other camera: front to back and back again. Held or off, it opens facing that way
    /// next time.
    pub async fn switch_camera(&self) -> Result<VideoState> {
        let mut run = self.inner.running().await?;
        self.inner
            .change(&mut run, |state| {
                state.facing = match state.facing {
                    Facing::Front => Facing::Back,
                    Facing::Back => Facing::Front,
                };
            })
            .await
    }

    /// What the other side said about its camera (its `CallMedia`): the display runs while it
    /// is on.
    pub async fn set_remote(&self, video: bool, paused: bool) -> Result<VideoState> {
        let mut run = self.inner.running().await?;
        run.remote_said = true;
        self.inner
            .change(&mut run, |state| {
                state.remote = video;
                state.remote_paused = video && paused;
            })
            .await
    }

    pub fn state(&self) -> VideoState {
        *self.inner.state.borrow()
    }

    /// Follows the state: our camera, theirs, and the remote picture's shape. The new receiver
    /// counts the state as it is now as seen: read it first (`borrow_and_update`), then wait
    /// for changes, or what came before (`available`, as the call connects) is never heard.
    pub fn changes(&self) -> watch::Receiver<VideoState> {
        self.inner.state.subscribe()
    }

    /// The counters of the video running now or, if none runs, of the last that ran. Numbers
    /// only: no picture, no identifier.
    pub async fn stats(&self) -> Option<VideoStats> {
        let run = self.inner.run.lock().await;
        run.call.as_ref().map(VideoCall::stats).or_else(|| run.last.clone())
    }

    /// Stops the camera and the display for good; the devices and their views go. On iOS the
    /// app removes the layers first (the bridge's `detach_video`).
    pub async fn stop(&self) {
        if let Some(watcher) = lock(&self.watcher).take() {
            watcher.abort();
        }
        let mut run = self.inner.run.lock().await;
        run.stopped = true;
        let rest = VideoState { facing: self.state().facing, ..VideoState::default() };
        // Stopping only stops things: it cannot fail.
        let _ = self.inner.settle(&mut run, &rest).await;
        self.inner.state.send_if_modified(|state| std::mem::replace(state, rest) != rest);
    }

    /// Looks, now and then, at what comes on its own. Holds the video weakly: dropping it ends
    /// the watch.
    fn watch(&self) {
        let mut watcher = lock(&self.watcher);
        if watcher.is_some() {
            return;
        }
        let inner: Weak<Inner> = Arc::downgrade(&self.inner);
        *watcher = Some(tokio::spawn(async move {
            let mut ticker = tokio::time::interval(LOOK_EVERY);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                let Some(inner) = inner.upgrade() else { break };
                if !inner.look().await {
                    break;
                }
            }
        }));
    }
}

impl Drop for Video {
    fn drop(&mut self) {
        if let Some(watcher) = lock(&self.watcher).take() {
            watcher.abort();
        }
    }
}

impl Inner {
    /// The run, unless the video stopped for good.
    async fn running(&self) -> Result<tokio::sync::MutexGuard<'_, Run>> {
        let run = self.run.lock().await;
        if run.stopped {
            bail!("the call's video has stopped");
        }
        Ok(run)
    }

    /// Applies `change` to the state and runs what the new state asks for. On an error, what ran
    /// goes back to the old state, which stays.
    async fn change(&self, run: &mut Run, change: impl FnOnce(&mut VideoState)) -> Result<VideoState> {
        let before = *self.state.borrow();
        let mut after = before;
        change(&mut after);
        if let Err(error) = self.settle(run, &after).await {
            let _ = self.settle(run, &before).await;
            return Err(error);
        }
        self.state.send_if_modified(|state| std::mem::replace(state, after) != after);
        Ok(after)
    }

    /// Starts, steers or stops the video as `state` says.
    async fn settle(&self, run: &mut Run, state: &VideoState) -> Result<()> {
        let wanted = run.connected && !run.stopped && state.available && state.any();
        if !wanted {
            if let Some(call) = run.call.take() {
                run.last = Some(call.stop().await);
            }
            if let Some(devices) = &run.devices {
                devices.gate.store(false, Ordering::SeqCst);
            }
            if run.stopped {
                run.devices = None;
            }
            return Ok(());
        }
        let sending = state.sending();
        if let (Some(call), Some(devices)) = (&run.call, &run.devices) {
            if call.facing() != state.facing {
                call.switch_camera(state.facing).map_err(|error| anyhow!("the camera cannot switch: {error}"))?;
            }
            if sending && call.is_paused() {
                devices.gate.store(true, Ordering::SeqCst);
                if let Err(error) = call.resume() {
                    devices.gate.store(false, Ordering::SeqCst);
                    bail!("the camera cannot start: {error}");
                }
            } else if !sending && !call.is_paused() {
                // Stopping a camera is best effort: it is off for the call either way.
                let _ = call.pause();
                devices.gate.store(false, Ordering::SeqCst);
            }
            return Ok(());
        }
        let devices = match &mut run.devices {
            Some(devices) => devices,
            None => {
                let made = (self.platform.devices)().map_err(|error| anyhow!("no video devices: {error}"))?;
                run.devices.insert(Devices::new(made))
            }
        };
        devices.gate.store(sending, Ordering::SeqCst);
        let source = GatedSource { source: devices.source.clone(), open: devices.gate.clone() };
        let sink = SharedSink(devices.sink.clone());
        let transport = VideoTransport {
            frames: Outgoing(self.sender.clone()),
            feedback: self.sender.feedback(),
            remote: Incoming(self.remote.clone()),
        };
        let config = VideoCallConfig { video: VideoConfig::default(), facing: state.facing, ..VideoCallConfig::default() };
        let call = VideoCall::start(Box::new(source), Box::new(sink), transport, config);
        let call = call.map_err(|error| {
            devices.gate.store(false, Ordering::SeqCst);
            anyhow!("the video cannot start: {error}")
        })?;
        if !sending {
            // Started through a shut gate: nothing opened, and nothing will until `resume`.
            let _ = call.pause();
        }
        run.call = Some(call);
        Ok(())
    }

    /// What comes on its own: the other side's first frames when it said nothing of its camera
    /// (an older app), and the shape of its picture. `false` once the video has stopped.
    async fn look(&self) -> bool {
        let mut run = self.run.lock().await;
        if run.stopped {
            return false;
        }
        if !run.remote_said && !self.state.borrow().remote && *self.arrived.borrow() {
            // Nothing to do with a display that cannot start: it is tried again next time.
            let _ = self.change(&mut run, |state| state.remote = true).await;
        }
        let shown = run.call.as_ref().is_some_and(|call| call.stats().frames_received > 0);
        let shape = if shown { views::remote_shape() } else { None };
        self.state.send_if_modified(|state| std::mem::replace(&mut state.shape, shape) != shape);
        true
    }
}

/// Our camera behind a gate: shut, `start` opens nothing.
struct GatedSource {
    source: Arc<StdMutex<Box<dyn VideoSource>>>,
    open: Arc<AtomicBool>,
}

impl VideoSource for GatedSource {
    fn start(&mut self, config: VideoConfig, facing: Facing, out: FrameSender) -> Result<(), VideoError> {
        if !self.open.load(Ordering::SeqCst) {
            return Ok(());
        }
        lock(&self.source).start(config, facing, out)
    }

    fn stop(&mut self) -> Result<(), VideoError> {
        lock(&self.source).stop()
    }

    fn request_keyframe(&mut self) {
        lock(&self.source).request_keyframe();
    }

    fn set_bitrate(&mut self, bps: u32) {
        lock(&self.source).set_bitrate(bps);
    }

    fn switch_camera(&mut self, facing: Facing) -> Result<(), VideoError> {
        lock(&self.source).switch_camera(facing)
    }

    fn lost(&self) -> bool {
        self.open.load(Ordering::SeqCst) && lock(&self.source).lost()
    }
}

/// The call's display, kept across `VideoCall`s.
struct SharedSink(Arc<StdMutex<Box<dyn VideoSink>>>);

impl VideoSink for SharedSink {
    fn start(&mut self) -> Result<(), VideoError> {
        lock(&self.0).start()
    }

    fn push(&mut self, frame: EncodedFrame) -> Result<(), VideoError> {
        lock(&self.0).push(frame)
    }

    fn stop(&mut self) -> Result<(), VideoError> {
        lock(&self.0).stop()
    }

    fn keyframe_needed(&mut self) -> bool {
        lock(&self.0).keyframe_needed()
    }
}

/// Where our frames go: the call's video track.
struct Outgoing(Arc<VideoSender>);

impl FrameSink for Outgoing {
    type Error = VideoRtpError;

    async fn send(&mut self, frame: &EncodedFrame) -> Result<(), VideoRtpError> {
        self.0.send(frame).await
    }
}

/// The other side's frames, from the stream every `VideoCall` of the call reads in turn.
struct Incoming(Arc<Mutex<VideoReceiver>>);

impl FrameSource for Incoming {
    type Error = VideoRtpError;

    async fn recv(&mut self) -> Result<Option<EncodedFrame>, VideoRtpError> {
        self.0.lock().await.recv().await
    }

    async fn request_keyframe(&mut self) -> Result<(), VideoRtpError> {
        self.0.lock().await.request_keyframe().await
    }

    fn stats(&self) -> AssemblerStats {
        self.0.try_lock().map(|remote| remote.stats()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::testing::{fake_video, webview_voice_offer, VideoProbe};
    use crate::{LinkState, MediaConfig};

    const LIMIT: Duration = Duration::from_secs(10);

    struct Side {
        session: MediaSession,
        video: Video,
        probe: VideoProbe,
    }

    impl Side {
        async fn open() -> Self {
            let session = MediaSession::open(&MediaConfig::default()).await.expect("a session");
            let (platform, probe) = fake_video();
            let video = Video::for_session(&session, platform);
            Self { session, video, probe }
        }

        async fn hang_up(&self) {
            self.video.stop().await;
            self.session.close().await;
        }

        fn shown(&self) -> usize {
            self.probe.display().shown.len()
        }
    }

    async fn connected(side: &Side) {
        let mut state = side.session.state();
        tokio::time::timeout(LIMIT, state.wait_for(|state| *state == LinkState::Connected))
            .await
            .expect("connected in time")
            .expect("the session is there");
        side.video.connected().await.expect("video may run");
    }

    /// Two phones in a call over a real loopback connection, each with fake video devices.
    async fn call() -> (Side, Side) {
        let (alice, bob) = (Side::open().await, Side::open().await);
        let offer = alice.session.offer().await.expect("offer");
        let answer = bob.session.answer(&offer).await.expect("answer");
        alice.session.accept(&answer).await.expect("accepted");
        connected(&alice).await;
        connected(&bob).await;
        (alice, bob)
    }

    async fn until(what: &str, check: impl Fn() -> bool) {
        let waited = tokio::time::timeout(LIMIT, async {
            while !check() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        });
        waited.await.unwrap_or_else(|_| panic!("never: {what}"));
    }

    // Turning a camera on in the middle of a call needs no new offer: the other side shows its
    // frames (once its core hears of it), and its own camera stays shut (the gated source).
    #[tokio::test(flavor = "multi_thread")]
    async fn our_camera_on_shows_its_frames_on_the_other_side_and_leaves_theirs_alone() {
        let (alice, bob) = call().await;
        assert!(alice.video.state().available && bob.video.state().available, "video both ways");
        assert_eq!((alice.probe.made(), bob.probe.made()), (0, 0), "no camera on, no devices");
        let changes = bob.video.changes();

        let state = alice.video.set_camera(true).await.expect("alice's camera turns on");
        assert!(state.camera && state.sending() && !state.remote);
        assert_eq!(alice.video.state(), state);
        // What bob's core does with alice's `CallMedia`.
        let seen = bob.video.set_remote(true, false).await.expect("bob hears of it");
        assert!(seen.remote && !seen.camera && seen.any());
        assert!(changes.has_changed().expect("the state is there"), "the change is published");

        until("bob sees alice", || bob.shown() >= 10).await;
        assert!(alice.probe.camera().running);
        assert_eq!(bob.probe.display().broken, 0, "every frame decodes");
        let theirs = bob.probe.camera();
        assert!(!theirs.running && theirs.sent == 0, "the other side's picture does not open our camera");
        assert!(!bob.video.state().camera);
        assert!(alice.probe.display().shown.is_empty(), "nothing comes to alice");
        assert_eq!((alice.probe.made(), bob.probe.made()), (1, 1));

        let stats = alice.video.stats().await.expect("alice's video runs");
        assert!(stats.frames_sent > 0, "{stats:?}");
        alice.hang_up().await;
        bob.hang_up().await;
    }

    // Off, the camera stops and nothing goes out; on again, the other side gets a keyframe first.
    // The devices are made once for the whole call.
    #[tokio::test(flavor = "multi_thread")]
    async fn off_and_on_again_resumes_with_a_keyframe() {
        let (alice, bob) = call().await;
        alice.video.set_camera(true).await.expect("on");
        bob.video.set_remote(true, false).await.expect("bob hears of it");
        until("bob sees alice", || bob.shown() >= 10).await;

        let off = alice.video.set_camera(false).await.expect("off");
        assert!(!off.camera && !off.sending());
        assert!(!alice.probe.camera().running, "the camera is shut");
        // Bob has not heard yet (a late `CallMedia`): his display still runs, and nothing comes.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let shown = bob.shown();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(bob.shown(), shown, "nothing comes while the camera is off");

        alice.video.set_camera(true).await.expect("on again");
        until("bob sees alice again", || bob.shown() >= shown + 5).await;
        let display = bob.probe.display();
        assert!(display.shown[shown].keyframe, "the first frame after the camera comes back is a keyframe");
        assert_eq!(display.broken, 0);
        assert_eq!(alice.probe.made(), 1, "the devices are made once per call");
        alice.hang_up().await;
        bob.hang_up().await;
    }

    // The phone holds the camera (the app left the screen) and gives it back: the user's choice
    // stays, the frames stop and come back from a keyframe.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_held_camera_stops_and_resumes_with_a_keyframe_without_turning_off() {
        let (alice, bob) = call().await;
        alice.video.set_camera(true).await.expect("on");
        bob.video.set_remote(true, false).await.expect("bob hears of it");
        until("bob sees alice", || bob.shown() >= 10).await;

        let held = alice.video.set_paused(true).await.expect("held");
        assert!(held.camera && held.paused && !held.sending() && held.any());
        assert!(!alice.probe.camera().running);
        bob.video.set_remote(true, true).await.expect("bob hears it is held");
        assert!(bob.video.state().remote_paused);
        tokio::time::sleep(Duration::from_millis(300)).await;
        let shown = bob.shown();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(bob.shown(), shown, "nothing comes while the camera is held");

        let back = alice.video.set_paused(false).await.expect("given back");
        assert!(back.camera && !back.paused && back.sending());
        until("bob sees alice again", || bob.shown() >= shown + 5).await;
        let display = bob.probe.display();
        assert!(display.shown[shown].keyframe, "the camera comes back with a keyframe");
        assert_eq!(display.broken, 0);

        // Held while off: nothing opens when it is given back.
        alice.video.set_camera(false).await.expect("off");
        alice.video.set_paused(true).await.expect("held");
        let back = alice.video.set_paused(false).await.expect("given back");
        assert!(!back.camera && !back.sending());
        assert!(!alice.probe.camera().running);
        alice.hang_up().await;
        bob.hang_up().await;
    }

    // Front to back and back again, with the camera off (it opens facing the new way) or on
    // (the frames keep coming, decodable).
    #[tokio::test(flavor = "multi_thread")]
    async fn switching_the_camera_turns_it_around_on_or_off() {
        let (alice, bob) = call().await;
        let state = alice.video.switch_camera().await.expect("switched while off");
        assert_eq!(state.facing, Facing::Back);
        assert_eq!(alice.probe.made(), 0, "switching opens no camera");

        alice.video.set_camera(true).await.expect("on");
        bob.video.set_remote(true, false).await.expect("bob hears of it");
        until("bob sees alice", || bob.shown() >= 5).await;
        assert_eq!(alice.probe.camera().facing, Some(Facing::Back), "it opened facing back");

        let state = alice.video.switch_camera().await.expect("switched while on");
        assert_eq!(state.facing, Facing::Front);
        assert_eq!(alice.probe.camera().facing, Some(Facing::Front));
        let shown = bob.shown();
        until("bob sees the other camera", || bob.shown() >= shown + 5).await;
        assert_eq!(bob.probe.display().broken, 0);
        alice.hang_up().await;
        bob.hang_up().await;
    }

    // An older app's voice call has no video line: no camera may turn on, and none opens.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_voice_call_without_a_video_line_has_no_video() {
        let callee = Side::open().await;
        callee.session.answer(&webview_voice_offer()).await.expect("answered");
        callee.video.connected().await.expect("connected");
        assert!(!callee.video.state().available, "the video button is not available");
        assert!(callee.video.set_camera(true).await.is_err());
        assert!(!callee.video.state().camera);
        assert_eq!(callee.probe.made(), 0);
        callee.hang_up().await;
    }

    // An older app's video call says nothing about its camera (no `CallMedia`): the first
    // frames that come show it.
    #[tokio::test(flavor = "multi_thread")]
    async fn video_that_comes_unannounced_shows_the_other_camera() {
        let (alice, bob) = call().await;
        alice.video.set_camera(true).await.expect("on");
        until("bob takes alice's camera as on", || bob.video.state().remote).await;
        until("bob sees alice", || bob.shown() >= 5).await;
        alice.hang_up().await;
        bob.hang_up().await;
    }

    // What the other side said wins over what comes: a camera it said is off stays off.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_camera_said_off_stays_off_whatever_comes() {
        let (alice, bob) = call().await;
        bob.video.set_remote(false, false).await.expect("bob hears alice's camera is off");
        alice.video.set_camera(true).await.expect("on");
        until("alice sends", || alice.probe.camera().sent >= 10).await;
        tokio::time::sleep(Duration::from_millis(600)).await;
        assert!(!bob.video.state().remote);
        assert_eq!(bob.probe.made(), 0);
        alice.hang_up().await;
        bob.hang_up().await;
    }

    // The call ends: the camera and the display stop and the devices go; nothing turns on again.
    #[tokio::test(flavor = "multi_thread")]
    async fn stop_ends_the_video_and_lets_the_devices_go() {
        let (alice, bob) = call().await;
        alice.video.set_camera(true).await.expect("on");
        bob.video.set_remote(true, false).await.expect("bob hears of it");
        until("bob sees alice", || bob.shown() >= 5).await;

        alice.video.stop().await;
        bob.video.stop().await;
        assert!(!alice.probe.camera().running && !alice.probe.display().running);
        assert!(!bob.probe.display().running);
        until("the devices go", || alice.probe.released() && bob.probe.released()).await;
        assert!(!alice.video.state().any());
        assert!(alice.video.set_camera(true).await.is_err(), "stopped is for good");
        assert_eq!(alice.probe.made(), 1);
        alice.session.close().await;
        bob.session.close().await;
    }

    // A call starts with no picture either way, and the selfie camera ready for when ours turns on.
    #[test]
    fn a_call_starts_with_no_video_and_the_front_camera() {
        let state = VideoState::default();
        assert!(!state.available, "until the call is connected with a video line both ways");
        assert!(!state.camera && !state.paused && !state.remote && !state.remote_paused);
        assert_eq!(state.facing, Facing::Front);
        assert_eq!(state.shape, None);
        assert!(!state.sending());
        assert!(!state.any());
    }

    // Our camera sends only when the user wants it on and the phone does not hold it (§ docs/video-nativo.md).
    #[test]
    fn our_camera_sends_only_when_wanted_and_not_held() {
        let on = VideoState { camera: true, ..VideoState::default() };
        assert!(on.sending());
        assert!(!VideoState { paused: true, ..on }.sending(), "held by the phone: nothing goes out");
        assert!(!VideoState { paused: true, ..VideoState::default() }.sending());
    }

    // The call shows video (and CallKit says so) while either camera is on, even held.
    #[test]
    fn a_call_has_video_while_either_camera_is_on() {
        assert!(VideoState { camera: true, paused: true, ..VideoState::default() }.any());
        assert!(VideoState { remote: true, ..VideoState::default() }.any());
        assert!(VideoState { remote: true, remote_paused: true, ..VideoState::default() }.any());
        assert!(!VideoState { remote_paused: true, ..VideoState::default() }.any(), "a pause alone is not a camera");
    }
}
