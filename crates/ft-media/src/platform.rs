#[cfg(any(target_os = "ios", target_os = "android", test))]
use std::sync::Arc;
use std::sync::{Mutex, PoisonError};

use webrtc_engine::audio::{AudioBackend, AudioError, Counter, DeviceIo};
#[cfg(any(target_os = "ios", target_os = "android", test))]
use webrtc_engine::video::{EncodedFrame, Facing, FrameSender, VideoConfig, VideoError, VideoSink, VideoSource};

#[cfg(any(target_os = "ios", target_os = "android"))]
use crate::Activation;
use crate::{AudioPlatform, VideoPlatform};

#[cfg(any(target_os = "ios", target_os = "android"))]
pub fn platform_audio() -> Option<AudioPlatform> {
    let activation = if cfg!(target_os = "ios") { Activation::WhenSessionActive } else { Activation::Immediate };
    Some(AudioPlatform {
        backend: Arc::new(counted_backend),
        activation,
    })
}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub fn platform_audio() -> Option<AudioPlatform> {
    None
}

#[cfg(any(target_os = "ios", target_os = "android"))]
pub fn platform_video() -> Option<VideoPlatform> {
    Some(VideoPlatform { devices: Arc::new(platform_devices) })
}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub fn platform_video() -> Option<VideoPlatform> {
    None
}

#[cfg(any(target_os = "ios", target_os = "android"))]
fn platform_devices() -> Result<crate::VideoDevices, VideoError> {
    let source = Arc::new(Mutex::new(webrtc_engine::video::platform_source()?));
    let sink = Arc::new(Mutex::new(webrtc_engine::video::platform_sink()?));
    crate::views::publish(&source, &sink);
    Ok(crate::VideoDevices { source: Box::new(Shared(source)), sink: Box::new(Shared(sink)) })
}

#[cfg(any(target_os = "ios", target_os = "android", test))]
pub(crate) struct Shared<T>(pub(crate) Arc<Mutex<T>>);

#[cfg(any(target_os = "ios", target_os = "android", test))]
impl<T> Shared<T> {
    fn lock(&self) -> std::sync::MutexGuard<'_, T> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(any(target_os = "ios", target_os = "android", test))]
impl<T: VideoSource> VideoSource for Shared<T> {
    fn start(&mut self, config: VideoConfig, facing: Facing, out: FrameSender) -> Result<(), VideoError> {
        self.lock().start(config, facing, out)
    }

    fn stop(&mut self) -> Result<(), VideoError> {
        self.lock().stop()
    }

    fn request_keyframe(&mut self) {
        self.lock().request_keyframe();
    }

    fn set_bitrate(&mut self, bps: u32) {
        self.lock().set_bitrate(bps);
    }

    fn switch_camera(&mut self, facing: Facing) -> Result<(), VideoError> {
        self.lock().switch_camera(facing)
    }

    fn lost(&self) -> bool {
        self.lock().lost()
    }
}

#[cfg(any(target_os = "ios", target_os = "android", test))]
impl<T: VideoSink> VideoSink for Shared<T> {
    fn start(&mut self) -> Result<(), VideoError> {
        self.lock().start()
    }

    fn push(&mut self, frame: EncodedFrame) -> Result<(), VideoError> {
        self.lock().push(frame)
    }

    fn stop(&mut self) -> Result<(), VideoError> {
        self.lock().stop()
    }

    fn keyframe_needed(&mut self) -> bool {
        self.lock().keyframe_needed()
    }
}

// goes to the device log, numbers only
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeviceStats {
    pub capture_dropped: u64,
    pub playout_underruns: u64,
    pub errors: u64,
}

pub trait Counting {
    // dropped capture, playout underruns, errors
    fn counters(&self) -> [Counter; 3];
}

static RUNNING: Mutex<Option<[Counter; 3]>> = Mutex::new(None);

pub fn device_stats() -> Option<DeviceStats> {
    let running = RUNNING.lock().unwrap_or_else(PoisonError::into_inner);
    running.as_ref().map(|[dropped, underruns, errors]| DeviceStats {
        capture_dropped: dropped.get(),
        playout_underruns: underruns.get(),
        errors: errors.get(),
    })
}

// the engine makes new counters every time the device opens, so publish after each start and upkeep
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
        // Android reopens its streams here, with new counters
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

    #[test]
    fn the_running_devices_counters_are_published() {
        let mut device = Counted(Fake { counters: Default::default() });
        let (io, _engine) = webrtc_engine::audio::audio_io(4);
        device.start(io).expect("starts");
        assert_eq!(device_stats(), Some(DeviceStats::default()));
        device.stop().expect("stops");
        assert_eq!(device_stats(), None);
    }

    #[derive(Default)]
    struct Troubled {
        started: bool,
    }

    impl VideoSource for Troubled {
        fn start(&mut self, _: VideoConfig, _: Facing, _: FrameSender) -> Result<(), VideoError> {
            self.started = true;
            Ok(())
        }
        fn stop(&mut self) -> Result<(), VideoError> {
            self.started = false;
            Ok(())
        }
        fn request_keyframe(&mut self) {}
        fn set_bitrate(&mut self, _: u32) {}
        fn switch_camera(&mut self, _: Facing) -> Result<(), VideoError> {
            Ok(())
        }
        fn lost(&self) -> bool {
            true
        }
    }

    impl VideoSink for Troubled {
        fn start(&mut self) -> Result<(), VideoError> {
            self.started = true;
            Ok(())
        }
        fn push(&mut self, _: EncodedFrame) -> Result<(), VideoError> {
            Ok(())
        }
        fn stop(&mut self) -> Result<(), VideoError> {
            Ok(())
        }
        fn keyframe_needed(&mut self) -> bool {
            true
        }
    }

    #[test]
    fn a_shared_device_says_what_the_device_says() {
        let device = Arc::new(Mutex::new(Troubled::default()));
        let mut source: Box<dyn VideoSource> = Box::new(Shared(device.clone()));
        let (out, _frames) = webrtc_engine::video::frame_channel(1);
        source.start(VideoConfig::default(), Facing::Front, out).expect("starts");
        assert!(device.lock().unwrap().started, "the shared device is the one that starts");
        assert!(source.lost());
        let mut sink: Box<dyn VideoSink> = Box::new(Shared(device));
        assert!(sink.keyframe_needed());
    }

    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    #[test]
    fn the_desktop_has_no_native_call_audio() {
        assert!(platform_audio().is_none());
    }

    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    #[test]
    fn the_desktop_has_no_native_call_video() {
        assert!(platform_video().is_none());
    }
}
