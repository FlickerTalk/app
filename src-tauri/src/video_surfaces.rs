#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::ffi::c_void;

use anyhow::{anyhow, Result};
use ft_media::ViewSlot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HandOff {
    Show(ViewSlot),
    Gone(ViewSlot),
    Unknown,
}

fn hand_off(slot: i32, surface: bool) -> HandOff {
    match ViewSlot::from_raw(slot) {
        Some(slot) if surface => HandOff::Show(slot),
        Some(slot) => HandOff::Gone(slot),
        None => HandOff::Unknown,
    }
}

fn deliver(
    slot: i32,
    surface: bool,
    window_of: impl FnOnce() -> *mut c_void,
    set: impl FnOnce(ViewSlot, *mut c_void) -> Result<()>,
    release: impl FnOnce(*mut c_void),
) -> Result<()> {
    match hand_off(slot, surface) {
        HandOff::Unknown => Err(anyhow!("no video view in slot {slot}")),
        HandOff::Gone(slot) => set(slot, std::ptr::null_mut()),
        HandOff::Show(slot) => {
            let window = window_of();
            let outcome = set(slot, window);
            if !window.is_null() {
                release(window);
            }
            outcome
        }
    }
}

#[cfg(target_os = "android")]
mod jni {
    use std::ffi::c_void;

    #[link(name = "android")]
    extern "C" {
        fn ANativeWindow_fromSurface(env: *mut c_void, surface: *mut c_void) -> *mut c_void;
        fn ANativeWindow_release(window: *mut c_void);
    }

    // slot 0 is remote, 1 local. A destroyed surface comes as null before surfaceDestroyed
    // returns, so the engine stops drawing into it in time.
    #[no_mangle]
    pub extern "system" fn Java_com_flickertalk_platform_FtVideoSurfaces_nativeSurface(
        env: *mut c_void,
        _class: *mut c_void,
        slot: i32,
        surface: *mut c_void,
    ) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = super::deliver(
                slot,
                !surface.is_null(),
                // SAFETY: `env` is the calling thread's JNIEnv and `surface` a live `Surface`
                // local reference for the length of this call.
                || unsafe { ANativeWindow_fromSurface(env, surface) },
                // SAFETY: the window is null or the one just made, and the registry takes its own
                // reference.
                |slot, window| unsafe { ft_media::views::set_surface(slot, window) },
                // SAFETY: releases the reference `ANativeWindow_fromSurface` gave us, once.
                |window| unsafe { ANativeWindow_release(window) },
            );
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Step {
        Made,
        Set(ViewSlot, usize),
        Released(usize),
    }

    fn run(slot: i32, surface: bool, window: usize, set_fails: bool) -> (Result<()>, Vec<Step>) {
        let steps = RefCell::new(Vec::new());
        let outcome = deliver(
            slot,
            surface,
            || {
                steps.borrow_mut().push(Step::Made);
                window as *mut c_void
            },
            |slot, window| {
                steps.borrow_mut().push(Step::Set(slot, window as usize));
                if set_fails {
                    Err(anyhow!("no registry"))
                } else {
                    Ok(())
                }
            },
            |window| steps.borrow_mut().push(Step::Released(window as usize)),
        );
        (outcome, steps.into_inner())
    }

    #[test]
    fn the_slot_kotlin_passes_names_the_view() {
        assert_eq!(hand_off(0, true), HandOff::Show(ViewSlot::Remote));
        assert_eq!(hand_off(1, true), HandOff::Show(ViewSlot::Local));
        assert_eq!(hand_off(0, false), HandOff::Gone(ViewSlot::Remote));
        assert_eq!(hand_off(1, false), HandOff::Gone(ViewSlot::Local));
        assert_eq!(hand_off(2, true), HandOff::Unknown);
        assert_eq!(hand_off(-1, false), HandOff::Unknown);
    }

    #[test]
    fn a_surface_goes_to_the_registry_and_our_reference_is_released_after() {
        let (outcome, steps) = run(1, true, 0x40, false);
        assert!(outcome.is_ok());
        assert_eq!(steps, vec![Step::Made, Step::Set(ViewSlot::Local, 0x40), Step::Released(0x40)]);
    }

    #[test]
    fn our_reference_is_released_even_when_the_registry_refuses() {
        let (outcome, steps) = run(0, true, 0x80, true);
        assert!(outcome.is_err());
        assert_eq!(steps, vec![Step::Made, Step::Set(ViewSlot::Remote, 0x80), Step::Released(0x80)]);
    }

    #[test]
    fn a_destroyed_surface_clears_the_slot() {
        let (outcome, steps) = run(0, false, 0x80, false);
        assert!(outcome.is_ok());
        assert_eq!(steps, vec![Step::Set(ViewSlot::Remote, 0)]);
    }

    #[test]
    fn a_surface_with_no_window_leaves_the_slot_empty() {
        let (outcome, steps) = run(1, true, 0, false);
        assert!(outcome.is_ok());
        assert_eq!(steps, vec![Step::Made, Step::Set(ViewSlot::Local, 0)]);
    }

    #[test]
    fn an_unknown_slot_touches_nothing() {
        let (outcome, steps) = run(7, true, 0x40, false);
        assert!(outcome.is_err());
        assert!(steps.is_empty());
    }
}
