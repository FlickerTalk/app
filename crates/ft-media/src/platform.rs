//! The phone's own audio device for calls: `webrtc-engine`'s backend for the platform. iOS: the
//! VoiceProcessingIO unit (echo cancellation), which may start only once CallKit activates the
//! audio session. Android: AAudio with the voice communication preset, at once. Elsewhere (the
//! desktop) there is none: its calls keep the WebView's media.

#[cfg(any(target_os = "ios", target_os = "android"))]
use std::sync::Arc;
use std::sync::{Mutex, PoisonError};

use webrtc_engine::audio::{AudioBackend, AudioError, Counter, DeviceIo};

#[cfg(any(target_os = "ios", target_os = "android"))]
use crate::Activation;
use crate::{AudioPlatform, VideoPlatform};

/// This platform's call audio, if calls run natively here.
#[cfg(any(target_os = "ios", target_os = "android"))]
pub fn platform_audio() -> Option<AudioPlatform> {
    let activation = if cfg!(target_os = "ios") { Activation::WhenSessionActive } else { Activation::Immediate };
    Some(AudioPlatform {
        backend: Arc::new(counted_backend),
        activation,
    })
}

/// This platform's call audio, if calls run natively here.
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub fn platform_audio() -> Option<AudioPlatform> {
    None
}

/// This platform's call video, if video calls run natively here (native video, 2026-09-29):
/// the engine's camera and display on iOS and Android, none on the desktop. Contract only for
/// now: none anywhere until the pipeline is there (`docs/video-nativo.md`).
pub fn platform_video() -> Option<VideoPlatform> {
    None
}

// ---- Temporary call diagnostics (2026-09-28) ----

/// What the running device counts, for the device log: samples lost on the way in, speaker
/// callbacks that played silence, and device errors. Numbers only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeviceStats {
    pub capture_dropped: u64,
    pub playout_underruns: u64,
    pub errors: u64,
}

/// A backend whose counters can be read while it runs.
pub trait Counting {
    /// Dropped capture, playout underruns and errors, as the backend counts them now.
    fn counters(&self) -> [Counter; 3];
}

/// The running device's counters: one call at a time, so one device.
static RUNNING: Mutex<Option<[Counter; 3]>> = Mutex::new(None);

/// What the running device has counted, if one runs.
pub fn device_stats() -> Option<DeviceStats> {
    let running = RUNNING.lock().unwrap_or_else(PoisonError::into_inner);
    running.as_ref().map(|[dropped, underruns, errors]| DeviceStats {
        capture_dropped: dropped.get(),
        playout_underruns: underruns.get(),
        errors: errors.get(),
    })
}

/// Wraps a backend so its counters are published after each start and upkeep: the engine makes
/// new ones each time the device opens.
pub struct Counted<B>(pub B);

impl<B: Counting> Counted<B> {
    fn publish(&self, running: bool) {
        *RUNNING.lock().unwrap_or_else(PoisonError::into_inner) = running.then(|| self.0.counters());
    }
}

impl<B: AudioBackend + Counting> AudioBackend for Counted<B> {
    fn start(&mut self, io: DeviceIo) -> Result<(), AudioError> {
        self.0.start(io)?;
        self.publish(true);
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        self.publish(false);
        self.0.stop()
    }

    fn maintain(&mut self) -> Result<(), AudioError> {
        let kept = self.0.maintain();
        // Android reopens its streams here, with new counters.
        self.publish(true);
        kept
    }
}

#[cfg(target_os = "ios")]
impl Counting for webrtc_engine::audio::ios::VoiceProcessingBackend {
    fn counters(&self) -> [Counter; 3] {
        [self.capture_dropped(), self.playout_underruns(), self.capture_errors()]
    }
}

#[cfg(target_os = "android")]
impl Counting for webrtc_engine::audio::android::AaudioBackend {
    fn counters(&self) -> [Counter; 3] {
        [self.capture_dropped(), self.playout_underruns(), self.stream_errors()]
    }
}

/// The platform's backend, counted (the same one the engine's `platform_backend` makes).
#[cfg(target_os = "ios")]
fn counted_backend() -> Result<Box<dyn AudioBackend>, AudioError> {
    Ok(Box::new(Counted(webrtc_engine::audio::ios::VoiceProcessingBackend::new()?)))
}

#[cfg(target_os = "android")]
fn counted_backend() -> Result<Box<dyn AudioBackend>, AudioError> {
    Ok(Box::new(Counted(webrtc_engine::audio::android::AaudioBackend::new()?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        counters: [Counter; 3],
    }

    impl AudioBackend for Fake {
        fn start(&mut self, _io: DeviceIo) -> Result<(), AudioError> {
            // Like the engine: new counters each time the device opens.
            self.counters = Default::default();
            Ok(())
        }
        fn stop(&mut self) -> Result<(), AudioError> {
            Ok(())
        }
    }

    impl Counting for Fake {
        fn counters(&self) -> [Counter; 3] {
            self.counters.clone()
        }
    }

    // The counters of the device that runs are readable; none once it stops.
    #[test]
    fn the_running_devices_counters_are_published() {
        let mut device = Counted(Fake { counters: Default::default() });
        let (io, _engine) = webrtc_engine::audio::audio_io(4);
        device.start(io).expect("starts");
        assert_eq!(device_stats(), Some(DeviceStats::default()));
        device.stop().expect("stops");
        assert_eq!(device_stats(), None);
    }

    // The host running the tests is a computer: its calls stay on the WebView.
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    #[test]
    fn the_desktop_has_no_native_call_audio() {
        assert!(platform_audio().is_none());
    }

    // Nor native video: the desktop's calls keep the WebView's camera and display.
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    #[test]
    fn the_desktop_has_no_native_call_video() {
        assert!(platform_video().is_none());
    }
}
