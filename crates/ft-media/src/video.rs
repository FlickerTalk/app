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
//! **Contract only (2026-09-29).** The types and signatures here are what the core and the native
//! bridge build on; the pipeline is still to come, and until then everything that would start
//! video says so with an error.

use std::sync::Arc;

use anyhow::{anyhow, Result};
use tokio::sync::watch;
pub use webrtc_engine::video::{Facing, VideoError, VideoSink, VideoSource};

use crate::views::RemoteShape;
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

/// The video of one call.
pub struct Video {
    state: watch::Sender<VideoState>,
}

fn not_yet() -> anyhow::Error {
    anyhow!("native video is not there yet")
}

impl Video {
    /// The video of `session`'s call, with the platform's devices. Nothing runs, and no camera
    /// opens, until a camera turns on (ours or theirs) with the call connected.
    pub fn for_session(_session: &MediaSession, _platform: VideoPlatform) -> Self {
        Self { state: watch::channel(VideoState::default()).0 }
    }

    /// The connection is up: video may run from now on.
    pub async fn connected(&self) -> Result<()> {
        Ok(())
    }

    /// Our camera on or off, as the user chose. The camera permission is the app's business,
    /// asked before. Returns the new state.
    pub async fn set_camera(&self, _on: bool) -> Result<VideoState> {
        Err(not_yet())
    }

    /// The phone holds our camera (`true`: the app left the screen, the phone locked) or gives it
    /// back. The user's choice stays as it was.
    pub async fn set_paused(&self, _paused: bool) -> Result<VideoState> {
        Err(not_yet())
    }

    /// The other camera: front to back and back again. Held or off, it opens facing that way
    /// next time.
    pub async fn switch_camera(&self) -> Result<VideoState> {
        Err(not_yet())
    }

    /// What the other side said about its camera (its `CallMedia`): the display runs while it
    /// is on.
    pub async fn set_remote(&self, _video: bool, _paused: bool) -> Result<VideoState> {
        Err(not_yet())
    }

    pub fn state(&self) -> VideoState {
        *self.state.borrow()
    }

    /// Follows the state: our camera, theirs, and the remote picture's shape.
    pub fn changes(&self) -> watch::Receiver<VideoState> {
        self.state.subscribe()
    }

    /// Stops the camera and the display for good; the devices and their views go.
    pub async fn stop(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

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
