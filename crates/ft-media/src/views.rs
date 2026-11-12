//! Global because the native side reaches the views with no call at hand, from JNI surfaceCreated
//! say

use std::ffi::c_void;
use std::sync::atomic::{AtomicI32, Ordering};
#[cfg(any(target_os = "ios", target_os = "android", test))]
use std::sync::{Arc, Mutex as StdMutex, Weak};

#[cfg(not(target_os = "android"))]
use anyhow::anyhow;
use anyhow::Result;
#[cfg(any(target_os = "android", target_os = "ios", test))]
use webrtc_engine::video::Rotation;
#[cfg(any(target_os = "ios", target_os = "android"))]
use webrtc_engine::video::{PlatformSink, PlatformSource};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewSlot {
    Remote,
    Local,
}

impl ViewSlot {
    // Kotlin passes 0 = remote, 1 = local
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

/// `window` comes from `ANativeWindow_fromSurface`, or is null once the surface is destroyed (the
/// app calls this before `surfaceDestroyed` returns).
///
/// # Safety
///
/// `window` is null or a live `ANativeWindow`. The registry takes a reference of its own: the
/// caller releases its own right after.
pub unsafe fn set_surface(slot: ViewSlot, window: *mut c_void) -> Result<()> {
    #[cfg(target_os = "android")]
    {
        // SAFETY: null or live (the caller).
        unsafe { android::set_surface(slot, window) }
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (slot, window);
        Err(anyhow!("native video surfaces are Android's"))
    }
}

// addresses of an AVSampleBufferDisplayLayer (remote) and an AVCaptureVideoPreviewLayer (local)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layers {
    pub remote: usize,
    pub local: usize,
}

pub fn layers() -> Option<Layers> {
    #[cfg(target_os = "ios")]
    {
        let (source, sink) = live()?;
        let remote = lock(&sink).layer() as usize;
        let local = lock(&source).preview_layer() as usize;
        Some(Layers { remote, local })
    }
    #[cfg(not(target_os = "ios"))]
    {
        None
    }
}

const NO_ORIENTATION: i32 = i32::MIN;

static ORIENTATION: AtomicI32 = AtomicI32::new(NO_ORIENTATION);

// iOS sends UIDeviceOrientation.rawValue, Android the display rotation in degrees
pub fn set_orientation(raw: i32) {
    ORIENTATION.store(raw, Ordering::SeqCst);
    #[cfg(any(target_os = "ios", target_os = "android"))]
    if let Some((source, _)) = live() {
        apply_orientation(&mut lock(&source), raw);
    }
}

#[cfg(any(target_os = "ios", target_os = "android", test))]
pub(crate) fn orientation() -> Option<i32> {
    Some(ORIENTATION.load(Ordering::SeqCst)).filter(|raw| *raw != NO_ORIENTATION)
}

#[cfg(target_os = "ios")]
fn apply_orientation(source: &mut PlatformSource, raw: i32) {
    use webrtc_engine::video::ios::DeviceOrientation;
    source.set_device_orientation(DeviceOrientation::from_raw(raw as isize));
}

#[cfg(target_os = "android")]
fn apply_orientation(source: &mut PlatformSource, raw: i32) {
    source.set_display_rotation(display_rotation(raw));
}

pub(crate) fn remote_shape() -> Option<RemoteShape> {
    #[cfg(target_os = "ios")]
    {
        let (_, sink) = live()?;
        let rotation = lock(&sink).rotation();
        Some(layer_shape(rotation))
    }
    #[cfg(target_os = "android")]
    {
        let (_, sink) = live()?;
        let size = lock(&sink).video_size();
        surface_shape(size)
    }
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteShape {
    // 0 when the platform doesn't tell (iOS, the layer keeps the aspect ratio)
    pub width: u32,
    pub height: u32,
    // degrees clockwise the view turns it itself, 0 on Android where the decoder turns the picture
    pub rotation: u16,
}

impl RemoteShape {
    // on iOS lay out with bounds, never frame, under the transform
    pub fn quarter_turn(&self) -> bool {
        self.rotation % 180 == 90
    }
}

#[cfg(any(target_os = "android", test))]
fn display_rotation(degrees: i32) -> Rotation {
    match degrees {
        90 => Rotation::Deg90,
        180 => Rotation::Deg180,
        270 => Rotation::Deg270,
        _ => Rotation::Deg0,
    }
}

#[cfg(any(target_os = "ios", test))]
fn layer_shape(rotation: Rotation) -> RemoteShape {
    RemoteShape { width: 0, height: 0, rotation: rotation.degrees() }
}

#[cfg(any(target_os = "android", test))]
fn surface_shape(size: Option<(u32, u32)>) -> Option<RemoteShape> {
    size.map(|(width, height)| RemoteShape { width, height, rotation: 0 })
}

#[cfg(any(target_os = "ios", target_os = "android", test))]
type Held<T> = Arc<StdMutex<T>>;

#[cfg(any(target_os = "ios", target_os = "android", test))]
type Pair<S, K> = (Held<S>, Held<K>);

#[cfg(any(target_os = "ios", target_os = "android", test))]
struct Registry<S, K> {
    source: Weak<StdMutex<S>>,
    sink: Weak<StdMutex<K>>,
}

#[cfg(any(target_os = "ios", target_os = "android", test))]
impl<S, K> Registry<S, K> {
    const fn new() -> Self {
        Self { source: Weak::new(), sink: Weak::new() }
    }

    fn publish(&mut self, source: &Held<S>, sink: &Held<K>) {
        (self.source, self.sink) = (Arc::downgrade(source), Arc::downgrade(sink));
    }

    fn live(&self) -> Option<Pair<S, K>> {
        Some((self.source.upgrade()?, self.sink.upgrade()?))
    }
}

#[cfg(any(target_os = "ios", target_os = "android"))]
static DEVICES: StdMutex<Registry<PlatformSource, PlatformSink>> = StdMutex::new(Registry::new());

#[cfg(any(target_os = "ios", target_os = "android"))]
fn live() -> Option<Pair<PlatformSource, PlatformSink>> {
    lock(&DEVICES).live()
}

#[cfg(any(target_os = "ios", target_os = "android"))]
pub(crate) fn publish(source: &Held<PlatformSource>, sink: &Held<PlatformSink>) {
    // the surfaces' lock first, set_surface takes them in that order
    #[cfg(target_os = "android")]
    let surfaces = android::surfaces();
    lock(&DEVICES).publish(source, sink);
    #[cfg(target_os = "android")]
    for slot in [ViewSlot::Remote, ViewSlot::Local] {
        let _ = android::apply(slot, surfaces[android::index(slot)].as_ref(), source, sink);
    }
    if let Some(raw) = orientation() {
        apply_orientation(&mut lock(source), raw);
    }
}

#[cfg(any(target_os = "ios", target_os = "android"))]
fn lock<T: ?Sized>(mutex: &StdMutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(target_os = "android")]
mod android {
    use std::ffi::c_void;
    use std::ptr::NonNull;
    use std::sync::{Mutex as StdMutex, MutexGuard};

    use anyhow::{anyhow, Result};
    use webrtc_engine::video::android::ANativeWindow;
    use webrtc_engine::video::{PlatformSink, PlatformSource};

    use super::{live, lock, Held, ViewSlot};

    #[link(name = "android")]
    unsafe extern "C" {
        fn ANativeWindow_acquire(window: *mut ANativeWindow);
        fn ANativeWindow_release(window: *mut ANativeWindow);
    }

    pub(super) struct Window(NonNull<ANativeWindow>);

    // SAFETY: an `ANativeWindow` is reference counted and its functions may be called from any
    // thread.
    unsafe impl Send for Window {}

    impl Window {
        /// # Safety
        ///
        /// `window` is a live `ANativeWindow`.
        unsafe fn acquire(window: NonNull<ANativeWindow>) -> Self {
            // SAFETY: live (the caller); balanced by the release in `drop`.
            unsafe { ANativeWindow_acquire(window.as_ptr()) };
            Self(window)
        }
    }

    impl Drop for Window {
        fn drop(&mut self) {
            // SAFETY: releases the one reference this value holds.
            unsafe { ANativeWindow_release(self.0.as_ptr()) };
        }
    }

    static SURFACES: StdMutex<[Option<Window>; 2]> = StdMutex::new([None, None]);

    pub(super) fn surfaces() -> MutexGuard<'static, [Option<Window>; 2]> {
        lock(&SURFACES)
    }

    pub(super) fn index(slot: ViewSlot) -> usize {
        match slot {
            ViewSlot::Remote => 0,
            ViewSlot::Local => 1,
        }
    }

    /// # Safety
    ///
    /// `window` is null or a live `ANativeWindow`.
    pub(super) unsafe fn set_surface(slot: ViewSlot, window: *mut c_void) -> Result<()> {
        // SAFETY: live if not null (the caller).
        let window = NonNull::new(window.cast::<ANativeWindow>()).map(|window| unsafe { Window::acquire(window) });
        let mut surfaces = surfaces();
        // the device takes the new surface before the old one is released
        let applied = match live() {
            Some((source, sink)) => apply(slot, window.as_ref(), &source, &sink),
            None => Ok(()),
        };
        surfaces[index(slot)] = window;
        applied
    }

    pub(super) fn apply(
        slot: ViewSlot,
        window: Option<&Window>,
        source: &Held<PlatformSource>,
        sink: &Held<PlatformSink>,
    ) -> Result<()> {
        let window = window.map(|window| window.0);
        match slot {
            // SAFETY: the registry holds a reference to the window while it is handed over.
            ViewSlot::Remote => unsafe { lock(sink).set_surface(window) },
            // SAFETY: as above.
            ViewSlot::Local => unsafe { lock(source).set_preview_surface(window) }
                .map_err(|error| anyhow!("the camera preview cannot move: {error}"))?,
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use webrtc_engine::video::fake::{FakeSink, FakeSource};

    use super::*;

    #[test]
    fn android_display_rotations_turn_into_frame_rotations() {
        assert_eq!(display_rotation(0), Rotation::Deg0);
        assert_eq!(display_rotation(90), Rotation::Deg90);
        assert_eq!(display_rotation(180), Rotation::Deg180);
        assert_eq!(display_rotation(270), Rotation::Deg270);
        assert_eq!(display_rotation(45), Rotation::Deg0, "anything else is upright");
    }

    #[test]
    fn the_remote_shape_follows_the_platform_display() {
        let turned = layer_shape(Rotation::Deg90);
        assert_eq!(turned, RemoteShape { width: 0, height: 0, rotation: 90 });
        assert!(turned.quarter_turn());
        assert_eq!(layer_shape(Rotation::Deg180).rotation, 180);
        assert_eq!(surface_shape(Some((480, 640))), Some(RemoteShape { width: 480, height: 640, rotation: 0 }));
        assert_eq!(surface_shape(None), None, "nothing decoded yet");
    }

    #[test]
    fn the_registry_holds_the_devices_weakly() {
        let mut registry = Registry::<FakeSource, FakeSink>::new();
        assert!(registry.live().is_none());
        let (source, sink) = (Arc::new(StdMutex::new(FakeSource::new())), Arc::new(StdMutex::new(FakeSink::new())));
        registry.publish(&source, &sink);
        let (live_source, live_sink) = registry.live().expect("published");
        assert!(Arc::ptr_eq(&live_source, &source) && Arc::ptr_eq(&live_sink, &sink));
        drop((live_source, live_sink, source, sink));
        assert!(registry.live().is_none(), "the call let them go");
    }

    #[test]
    fn the_orientation_is_kept_for_devices_made_later() {
        set_orientation(90);
        assert_eq!(orientation(), Some(90));
    }

    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    #[test]
    fn the_desktop_has_no_native_views() {
        assert_eq!(layers(), None);
        assert_eq!(remote_shape(), None);
        assert!(unsafe { set_surface(ViewSlot::Remote, std::ptr::null_mut()) }.is_err());
    }

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

    #[test]
    fn a_quarter_turn_swaps_the_upright_size() {
        let shape = RemoteShape { width: 640, height: 480, rotation: 90 };
        assert!(shape.quarter_turn());
        assert!(RemoteShape { rotation: 270, ..shape }.quarter_turn());
        assert!(!RemoteShape { rotation: 0, ..shape }.quarter_turn());
        assert!(!RemoteShape { rotation: 180, ..shape }.quarter_turn());
    }
}
