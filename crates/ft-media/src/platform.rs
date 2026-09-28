//! The phone's own audio device for calls. iOS: the VoiceProcessingIO unit (echo cancellation),
//! which may start only once CallKit activates the audio session. Android: AAudio with the voice
//! communication preset, at once. Elsewhere (the desktop) there is none: its calls keep the
//! WebView's media.

#[cfg(any(target_os = "ios", target_os = "android"))]
use std::sync::Arc;

#[cfg(any(target_os = "ios", target_os = "android"))]
use webrtc_engine::audio::AudioBackend;

#[cfg(any(target_os = "ios", target_os = "android"))]
use crate::Activation;
use crate::AudioPlatform;

/// This platform's call audio, if calls run natively here.
#[cfg(target_os = "ios")]
pub fn platform_audio() -> Option<AudioPlatform> {
    Some(AudioPlatform {
        backend: Arc::new(|| {
            Ok(Box::new(webrtc_engine::audio::ios::VoiceProcessingBackend::new()?) as Box<dyn AudioBackend>)
        }),
        activation: Activation::WhenSessionActive,
    })
}

/// This platform's call audio, if calls run natively here.
#[cfg(target_os = "android")]
pub fn platform_audio() -> Option<AudioPlatform> {
    Some(AudioPlatform {
        backend: Arc::new(|| Ok(Box::new(webrtc_engine::audio::android::AaudioBackend::new()?) as Box<dyn AudioBackend>)),
        activation: Activation::Immediate,
    })
}

/// This platform's call audio, if calls run natively here.
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub fn platform_audio() -> Option<AudioPlatform> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // The host running the tests is a computer: its calls stay on the WebView.
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    #[test]
    fn the_desktop_has_no_native_call_audio() {
        assert!(platform_audio().is_none());
    }
}
