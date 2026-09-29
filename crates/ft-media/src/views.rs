//! The native views of the call on the screen (native video, 2026-09-29; `docs/video-nativo.md`).
//! One call at a time, so one set of views for the whole process, like the device counters of
//! `platform.rs`: the native side reaches them with no call at hand (a JNI callback from
//! `surfaceCreated`, say).
//!
//! - **Android**: the app's Kotlin code owns two `SurfaceView`s behind the WebView and hands their
//!   `Surface`s over JNI (`set_surface`); they are kept while they live and given to the call's
//!   display (remote) and camera preview (local) whenever those exist, now or later.
//! - **iOS**: the engine's devices own their `CALayer`s; `layers` gives their addresses for the
//!   Swift code to add under the WebView, on the main thread, and to remove before the call's
//!   video devices go.
//!
//! **Contract only (2026-09-29)**: what the bridge builds on; the registry is still to come.

use std::ffi::c_void;

use anyhow::{anyhow, Result};

/// Which picture a native view shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewSlot {
    /// The other side's picture.
    Remote,
    /// Our camera's preview.
    Local,
}

impl ViewSlot {
    /// From the number Kotlin passes: 0 remote, 1 local.
    pub fn from_raw(raw: i32) -> Option<Self> {
        match raw {
            0 => Some(Self::Remote),
            1 => Some(Self::Local),
            _ => None,
        }
    }

    pub fn as_raw(self) -> i32 {
        match self {
            Self::Remote => 0,
            Self::Local => 1,
        }
    }
}

/// Android: the app's surface for `slot`, an `ANativeWindow*` from `ANativeWindow_fromSurface`,
/// or null when the surface is destroyed (the app calls it before `surfaceDestroyed` returns).
///
/// # Safety
///
/// `window` is null or a live `ANativeWindow`. The registry takes a reference of its own: the
/// caller releases its own right after.
pub unsafe fn set_surface(_slot: ViewSlot, _window: *mut c_void) -> Result<()> {
    Err(anyhow!("native video is not there yet"))
}

/// iOS: the call's layers, as addresses of `CALayer`s owned by its video devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layers {
    /// The `AVSampleBufferDisplayLayer` with the other side's picture.
    pub remote: usize,
    /// The `AVCaptureVideoPreviewLayer` with our camera (the front one mirrored).
    pub local: usize,
}

/// iOS: the layers of the call's video, while it has its devices.
pub fn layers() -> Option<Layers> {
    None
}

/// The phone's orientation, for the rotation our frames carry: on iOS
/// `UIDeviceOrientation.rawValue`, on Android the display's rotation in degrees.
pub fn set_orientation(_raw: i32) {}

/// How the platform's display shows the other side's picture now, once it has shown one.
pub(crate) fn remote_shape() -> Option<RemoteShape> {
    None
}

/// How the native side lays the other side's picture out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteShape {
    /// The picture's size as it is shown, upright; 0 when the platform does not tell (iOS: the
    /// layer keeps the aspect ratio itself).
    pub width: u32,
    pub height: u32,
    /// The clockwise turn the view applies itself, in degrees: on iOS the layer's transform, 0 on
    /// Android (the decoder turns the picture).
    pub rotation: u16,
}

impl RemoteShape {
    /// Whether the view lays the picture out with width and height swapped (iOS: `bounds`, never
    /// `frame`, under the transform).
    pub fn quarter_turn(&self) -> bool {
        self.rotation % 180 == 90
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The numbers Kotlin passes over JNI (`FtVideoSurfaces.nativeSurface(slot, surface)`).
    #[test]
    fn view_slots_match_the_numbers_kotlin_passes() {
        assert_eq!(ViewSlot::from_raw(0), Some(ViewSlot::Remote));
        assert_eq!(ViewSlot::from_raw(1), Some(ViewSlot::Local));
        assert_eq!(ViewSlot::from_raw(2), None);
        assert_eq!(ViewSlot::from_raw(-1), None);
        for slot in [ViewSlot::Remote, ViewSlot::Local] {
            assert_eq!(ViewSlot::from_raw(slot.as_raw()), Some(slot));
        }
    }

    // A quarter turn shows the picture on its side: the view lays it out with width and height
    // swapped (iOS: the layer's bounds; the engine's `DisplaySink::rotation`).
    #[test]
    fn a_quarter_turn_swaps_the_upright_size() {
        let shape = RemoteShape { width: 640, height: 480, rotation: 90 };
        assert!(shape.quarter_turn());
        assert!(RemoteShape { rotation: 270, ..shape }.quarter_turn());
        assert!(!RemoteShape { rotation: 0, ..shape }.quarter_turn());
        assert!(!RemoteShape { rotation: 180, ..shape }.quarter_turn());
    }
}
