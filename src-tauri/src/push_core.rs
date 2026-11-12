#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Instant;

use ft_core::Event;
pub use ft_core::online::{Stop, Watch};

/// A push must never create an identity, only start a phone that is already set up.
pub fn may_start_from_push(dir: &Path, sealed_key: &str, database: &str) -> bool {
    dir.join(sealed_key).is_file() && dir.join(database).is_file()
}

pub trait CallScreen: Send + Sync {
    /// empty caller shows "Someone"
    fn ring(&self, caller: &str, video: bool, muted: bool);
    fn stop_ringing(&self);
    fn refused(&self);
    fn answering(&self, caller: &str, video: bool);
    fn connected(&self);
    fn ended(&self);
}

pub trait PushSide: CallScreen {
    fn up(&self);
    fn adopted(&self);
    fn stopped(&self, why: &str);
}

pub struct Host<A> {
    app: OnceLock<A>,
    push: Option<Arc<dyn PushSide>>,
    watch: Mutex<Option<Watch>>,
}

impl<A: Clone> Host<A> {
    pub fn for_app(app: A) -> Self {
        Self { app: OnceLock::from(app), push: None, watch: Mutex::new(None) }
    }

    pub fn for_push(screen: Arc<dyn PushSide>, now: Instant) -> Self {
        Self { app: OnceLock::new(), push: Some(screen), watch: Mutex::new(Some(Watch::new(now))) }
    }

    pub fn adopt(&self, app: A) -> bool {
        self.app.set(app).is_ok()
    }

    pub fn app(&self) -> Option<&A> {
        self.app.get()
    }

    pub fn screen(&self, bridge: impl FnOnce(&A) -> Arc<dyn CallScreen>) -> Option<Arc<dyn CallScreen>> {
        match self.app.get() {
            Some(app) => Some(bridge(app)),
            None => self.push.clone().map(|push| push as Arc<dyn CallScreen>),
        }
    }

    pub fn push_side(&self) -> Option<&Arc<dyn PushSide>> {
        self.push.as_ref()
    }

    pub fn pushed(&self, now: Instant) {
        if let Some(watch) = self.watch.lock().unwrap_or_else(PoisonError::into_inner).as_mut() {
            watch.pushed(now);
        }
    }

    pub fn saw(&self, event: &Event, now: Instant) {
        if let Some(watch) = self.watch.lock().unwrap_or_else(PoisonError::into_inner).as_mut() {
            watch.saw(event, now);
        }
    }

    pub fn stops(&self, now: Instant, call_going_on: bool) -> Option<Stop> {
        if self.app.get().is_some() {
            return None;
        }
        self.watch.lock().unwrap_or_else(PoisonError::into_inner).as_ref()?.stops(now, call_going_on)
    }
}

#[cfg(target_os = "android")]
mod android {
    use std::path::PathBuf;
    use std::sync::{Arc, OnceLock};

    use anyhow::anyhow;
    use jni::objects::{GlobalRef, JByteArray, JClass, JString, JValue};
    use jni::sys::{jboolean, JNI_FALSE, JNI_TRUE};
    use jni::{JNIEnv, JavaVM};

    use super::{CallScreen, PushSide};
    use crate::client::{self, KeyVault};

    // Cached on the first start, a thread the JVM didn't create can't find app classes by name.
    static KOTLIN: OnceLock<(JavaVM, GlobalRef)> = OnceLock::new();

    fn kotlin<T>(call: impl FnOnce(&mut JNIEnv, &JClass) -> jni::errors::Result<T>) -> Option<T> {
        let (vm, class) = KOTLIN.get()?;
        let mut env = vm.attach_current_thread_as_daemon().ok()?;
        let answer = call(&mut env, <&JClass>::from(class.as_obj()));
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }
        answer.ok()
    }

    fn tell(name: &str) {
        let _ = kotlin(|env, class| env.call_static_method(class, name, "()V", &[]).map(drop));
    }

    fn through_keystore(name: &str, input: &[u8]) -> anyhow::Result<Vec<u8>> {
        kotlin(|env, class| {
            let input = env.byte_array_from_slice(input)?;
            let output = env.call_static_method(class, name, "([B)[B", &[JValue::Object(&input)])?.l()?;
            env.convert_byte_array(JByteArray::from(output))
        })
        .ok_or_else(|| anyhow!("the key store did not answer"))
    }

    struct KotlinVault;

    impl KeyVault for KotlinVault {
        fn seal(&self, key: &[u8; 32]) -> anyhow::Result<Vec<u8>> {
            through_keystore("sealKey", key)
        }

        fn open(&self, sealed: &[u8]) -> anyhow::Result<[u8; 32]> {
            through_keystore("openKey", sealed)?.try_into().map_err(|_| anyhow!("the key store gave something that is not a key"))
        }

        fn forget(&self) -> anyhow::Result<()> {
            Err(anyhow!("a push never erases the phone"))
        }
    }

    struct KotlinSide;

    impl CallScreen for KotlinSide {
        fn ring(&self, caller: &str, video: bool, muted: bool) {
            let _ = kotlin(|env, class| {
                let caller = env.new_string(caller)?;
                let args = [JValue::Object(&caller), JValue::Bool(video.into()), JValue::Bool(muted.into())];
                env.call_static_method(class, "ring", "(Ljava/lang/String;ZZ)V", &args).map(drop)
            });
        }

        fn stop_ringing(&self) {
            tell("stopRinging");
        }

        fn refused(&self) {
            tell("refused");
        }

        // The notification's answer opens the app, which handles answering and connecting.
        fn answering(&self, _caller: &str, _video: bool) {}

        fn connected(&self) {}

        fn ended(&self) {
            tell("ended");
        }
    }

    impl PushSide for KotlinSide {
        fn up(&self) {
            tell("up");
        }

        fn adopted(&self) {
            tell("adopted");
        }

        fn stopped(&self, why: &str) {
            let _ = kotlin(|env, class| {
                let why = env.new_string(why)?;
                env.call_static_method(class, "stopped", "(Ljava/lang/String;)V", &[JValue::Object(&why)]).map(drop)
            });
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_com_flickertalk_platform_PushCore_nativeStart(mut env: JNIEnv, class: JClass, dir: JString) -> jboolean {
        let started = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Option<()> {
            if KOTLIN.get().is_none() {
                let vm = env.get_java_vm().ok()?;
                let class = env.new_global_ref(&class).ok()?;
                let _ = KOTLIN.set((vm, class));
            }
            let dir: String = env.get_string(&dir).ok()?.into();
            tauri::async_runtime::spawn(client::start_for_push(PathBuf::from(dir), Box::new(KotlinVault), Arc::new(KotlinSide)));
            Some(())
        }));
        if matches!(started, Ok(Some(()))) {
            JNI_TRUE
        } else {
            JNI_FALSE
        }
    }

    /// Returns false when no core is listening, so Kotlin keeps the event for the app.
    #[no_mangle]
    pub extern "system" fn Java_com_flickertalk_platform_PushCore_nativeCallEvent(mut env: JNIEnv, _class: JClass, json: JString) -> jboolean {
        let taken = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let json: String = env.get_string(&json).ok()?.into();
            Some(tauri_plugin_ft_platform::parse_call_event(&json).is_some_and(client::push_event))
        }));
        if matches!(taken, Ok(Some(true))) {
            JNI_TRUE
        } else {
            JNI_FALSE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use ft_core::online::{LINGER, WAIT_FOR_CALL};
    use ft_core::CallUpdate;
    use ft_storage::CallOutcome;

    fn ended() -> Event {
        Event::Call { contact: "ft_a".to_owned(), call: "c".to_owned(), update: CallUpdate::Ended { outcome: CallOutcome::Missed } }
    }

    fn incoming() -> Event {
        Event::Call { contact: "ft_a".to_owned(), call: "c".to_owned(), update: CallUpdate::Incoming { video: false, sdp: String::new() } }
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ft-push-core-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_push_starts_the_core_only_on_a_phone_with_its_identity() {
        let dir = scratch("identity");
        assert!(!may_start_from_push(&dir, "storage.key.sealed", "flickertalk.db"), "nothing there");
        std::fs::write(dir.join("flickertalk.db"), b"").unwrap();
        assert!(!may_start_from_push(&dir, "storage.key.sealed", "flickertalk.db"), "no sealed key");
        std::fs::write(dir.join("storage.key"), [1; 32]).unwrap();
        assert!(!may_start_from_push(&dir, "storage.key.sealed", "flickertalk.db"), "a key in the clear is the app's to seal");
        std::fs::write(dir.join("storage.key.sealed"), [2; 60]).unwrap();
        assert!(may_start_from_push(&dir, "storage.key.sealed", "flickertalk.db"));
        std::fs::remove_file(dir.join("flickertalk.db")).unwrap();
        assert!(!may_start_from_push(&dir, "storage.key.sealed", "flickertalk.db"), "no database");
        assert!(!may_start_from_push(&dir.join("missing"), "storage.key.sealed", "flickertalk.db"));
    }

    struct Told(&'static str, Arc<Mutex<Vec<String>>>);

    impl CallScreen for Told {
        fn ring(&self, caller: &str, video: bool, muted: bool) {
            self.1.lock().unwrap().push(format!("{}: ring {caller} video={video} muted={muted}", self.0));
        }
        fn stop_ringing(&self) {
            self.1.lock().unwrap().push(format!("{}: stop", self.0));
        }
        fn refused(&self) {
            self.1.lock().unwrap().push(format!("{}: refused", self.0));
        }
        fn answering(&self, caller: &str, _video: bool) {
            self.1.lock().unwrap().push(format!("{}: answering {caller}", self.0));
        }
        fn connected(&self) {
            self.1.lock().unwrap().push(format!("{}: connected", self.0));
        }
        fn ended(&self) {
            self.1.lock().unwrap().push(format!("{}: ended", self.0));
        }
    }

    impl PushSide for Told {
        fn up(&self) {}
        fn adopted(&self) {}
        fn stopped(&self, _why: &str) {}
    }

    #[test]
    fn the_push_s_screen_hears_the_call_until_the_app_takes_the_core() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let host = Host::for_push(Arc::new(Told("push", log.clone())), Instant::now());
        let bridge = |log: &Arc<Mutex<Vec<String>>>| {
            let log = log.clone();
            move |app: &&'static str| -> Arc<dyn CallScreen> { Arc::new(Told(app, log)) }
        };
        assert!(host.app().is_none());
        host.screen(bridge(&log)).expect("the push's").ring("Ana", false, true);
        assert!(host.adopt("app"));
        assert!(!host.adopt("app"), "taken once");
        assert_eq!(host.app(), Some(&"app"));
        host.screen(bridge(&log)).expect("the app's").stop_ringing();
        assert_eq!(*log.lock().unwrap(), vec!["push: ring Ana video=false muted=true".to_owned(), "app: stop".to_owned()]);
    }

    #[test]
    fn a_core_the_app_started_tells_the_app_s_screen() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let host = Host::for_app("app");
        assert!(!host.adopt("app"));
        let made = log.clone();
        host.screen(move |app: &&'static str| -> Arc<dyn CallScreen> { Arc::new(Told(app, made)) }).expect("the app's").refused();
        assert_eq!(*log.lock().unwrap(), vec!["app: refused".to_owned()]);
    }

    #[test]
    fn only_a_core_the_push_started_and_the_app_has_not_taken_stops() {
        let start = Instant::now();
        let over = start + Duration::from_secs(4);
        let push = Host::<&str>::for_push(Arc::new(Told("push", Arc::default())), start);
        push.saw(&ended(), over);
        assert_eq!(push.stops(over + LINGER, false), Some(Stop::Settled));
        assert_eq!(push.stops(over + LINGER, true), None, "a call going on");

        let taken = Host::<&str>::for_push(Arc::new(Told("push", Arc::default())), start);
        taken.saw(&ended(), over);
        assert!(taken.adopt("app"));
        assert_eq!(taken.stops(over + LINGER, false), None);

        let app = Host::for_app("app");
        app.saw(&ended(), over);
        assert_eq!(app.stops(start + WAIT_FOR_CALL * 2, false), None);
        app.pushed(start);
        assert_eq!(app.stops(start + WAIT_FOR_CALL * 2, false), None);
    }

    #[test]
    fn a_second_push_restarts_the_wait() {
        let start = Instant::now();
        let host = Host::<&str>::for_push(Arc::new(Told("push", Arc::default())), start);
        let again = start + Duration::from_secs(30);
        host.pushed(again);
        assert_eq!(host.stops(start + WAIT_FOR_CALL, false), None);
        assert_eq!(host.stops(again + WAIT_FOR_CALL, false), Some(Stop::NoCall));
    }

    #[test]
    fn the_reasons_kotlin_logs_are_words() {
        assert_eq!(Stop::Settled.word(), "settled");
        assert_eq!(Stop::NoCall.word(), "no call");
    }
}
