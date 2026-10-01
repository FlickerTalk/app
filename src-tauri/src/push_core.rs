//! The core a call push starts with the app closed (Android, 2026-10-01).
//!
//! On Android the Rust library is loaded, and Tauri runs, only with the app's activity. A call push
//! (`{"t":"call"}`) starts the process for `FtMessagingService` alone, so the core never ran: nothing
//! connected to the router, the call's offer and the caller's hang-up never arrived, nothing was kept,
//! and the phone rang "Someone" until the notification's own limit. Now the push service loads the
//! library and starts the core here, with no Tauri and no WebView (`jni`): it connects at once, takes
//! the offer the router kept, and tells the phone's call screen (the push's notification, through
//! Kotlin's `PushCore`) who calls, when to stop ringing and when a call is refused. When the app opens,
//! it adopts this core (`client.rs`): one core per process. Until then, this core lives only for the
//! call: it stops a moment after the call is over, or when no call came (`Watch`).
//!
//! The rules are plain Rust, tested everywhere; the JNI entries exist on Android only.

#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use ft_core::{CallUpdate, Event};

/// How long a core a push started keeps going once the call is over: the busy it answers a
/// refused call with, or the decline, goes out first.
pub const LINGER: Duration = Duration::from_secs(3);

/// How long it waits for a call that never comes. The push lives 45 s (the router's TTL, the
/// notification's `CALL_RING_MS`) and the caller tries for 40 s (`CALL_REACH`): by then nobody calls.
pub const WAIT_FOR_CALL: Duration = Duration::from_secs(50);

/// Whether a push may start the core: only on a phone with its identity, its storage key sealed by
/// the OS key store and its database. A push never makes an identity.
pub fn may_start_from_push(dir: &Path, sealed_key: &str, database: &str) -> bool {
    dir.join(sealed_key).is_file() && dir.join(database).is_file()
}

/// Why a core a push started stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// The call is over (ended, missed, refused).
    Settled,
    /// No call came.
    NoCall,
}

impl Stop {
    /// The word Kotlin logs (debug builds only); never who called.
    pub fn word(self) -> &'static str {
        match self {
            Stop::Settled => "settled",
            Stop::NoCall => "no call",
        }
    }
}

/// When a core a push started has done its job.
#[derive(Debug, Clone, Copy)]
pub struct Watch {
    pushed: Instant,
    settled: Option<Instant>,
}

impl Watch {
    pub fn new(now: Instant) -> Self {
        Self { pushed: now, settled: None }
    }

    /// Another call push came: a call is on its way.
    pub fn pushed(&mut self, now: Instant) {
        self.pushed = now;
        self.settled = None;
    }

    /// What the core said.
    pub fn saw(&mut self, event: &Event, now: Instant) {
        match event {
            Event::Call { update: CallUpdate::Incoming { .. }, .. } => self.settled = None,
            Event::Call { update: CallUpdate::Ended { .. }, .. } | Event::CallRefused => self.settled = Some(now),
            _ => {}
        }
    }

    /// Whether it stops now. `call_going_on`: the core has a call ringing, being answered or on.
    pub fn stops(&self, now: Instant, call_going_on: bool) -> Option<Stop> {
        if call_going_on {
            return None;
        }
        match self.settled {
            Some(at) => (now >= at + LINGER).then_some(Stop::Settled),
            None => (now >= self.pushed + WAIT_FOR_CALL).then_some(Stop::NoCall),
        }
    }
}

/// The phone's own call screen (2026-10-01): CallKit or the call notification, told the same steps
/// whoever tells it. The app's bridge once the app runs; before, for a core a call push started on
/// Android, Kotlin's `PushCore`, which keeps the push's notification.
pub trait CallScreen: Send + Sync {
    /// A call rings: the screen says who (empty: "Someone") and, for a muted contact, makes no noise.
    fn ring(&self, caller: &str, video: bool, muted: bool);
    fn stop_ringing(&self);
    /// A call refused without a trace (§108, §109): a ring the push started stops.
    fn refused(&self);
    fn answering(&self, caller: &str, video: bool);
    fn connected(&self);
    fn ended(&self);
}

/// Kotlin's `PushCore` (Android): the push's call screen, and what it is told of the core's life.
pub trait PushSide: CallScreen {
    /// The core is up: the notification's decline reaches it from now on.
    fn up(&self);
    /// The app took the core: its own bridge hears the calls from now on.
    fn adopted(&self);
    /// The core stopped (`Stop::word`, or why it never started): the push's ringing goes.
    fn stopped(&self, why: &str);
}

/// Who a running core answers to: the app, once it has the core; before that, for a core a call
/// push started, the push's call screen and the watch that ends it.
pub struct Host<A> {
    app: OnceLock<A>,
    push: Option<Arc<dyn PushSide>>,
    watch: Mutex<Option<Watch>>,
}

impl<A: Clone> Host<A> {
    /// A core the app started.
    pub fn for_app(app: A) -> Self {
        Self { app: OnceLock::from(app), push: None, watch: Mutex::new(None) }
    }

    /// A core a call push started, with the push's call screen.
    pub fn for_push(screen: Arc<dyn PushSide>, now: Instant) -> Self {
        Self { app: OnceLock::new(), push: Some(screen), watch: Mutex::new(Some(Watch::new(now))) }
    }

    /// The app takes the core a push started; `false` if it had it already.
    pub fn adopt(&self, app: A) -> bool {
        self.app.set(app).is_ok()
    }

    pub fn app(&self) -> Option<&A> {
        self.app.get()
    }

    /// The call screen to tell now: the app's (made by `bridge`) once it has the core, else the push's.
    pub fn screen(&self, bridge: impl FnOnce(&A) -> Arc<dyn CallScreen>) -> Option<Arc<dyn CallScreen>> {
        match self.app.get() {
            Some(app) => Some(bridge(app)),
            None => self.push.clone().map(|push| push as Arc<dyn CallScreen>),
        }
    }

    /// Kotlin's side of a core a push started, if this is one (taken by the app or not).
    pub fn push_side(&self) -> Option<&Arc<dyn PushSide>> {
        self.push.as_ref()
    }

    /// Another call push came for this core.
    pub fn pushed(&self, now: Instant) {
        if let Some(watch) = self.watch.lock().unwrap_or_else(PoisonError::into_inner).as_mut() {
            watch.pushed(now);
        }
    }

    /// What the core said, for the watch.
    pub fn saw(&self, event: &Event, now: Instant) {
        if let Some(watch) = self.watch.lock().unwrap_or_else(PoisonError::into_inner).as_mut() {
            watch.saw(event, now);
        }
    }

    /// Whether a core a push started, and the app has not taken, stops now. Never one the app has.
    pub fn stops(&self, now: Instant, call_going_on: bool) -> Option<Stop> {
        if self.app.get().is_some() {
            return None;
        }
        self.watch.lock().unwrap_or_else(PoisonError::into_inner).as_ref()?.stops(now, call_going_on)
    }
}

/// The JNI side (Android): Kotlin's `PushCore` starts the core and hands it the notification's
/// buttons; the core calls back into `PushCore` for the key store and the call screen. Thin glue:
/// what it decides is above, in `client::start_for_push` and in Kotlin's tested functions.
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

    /// The JVM and Kotlin's `PushCore` class, from the first start: the core's threads call back
    /// through them (a thread the JVM did not make finds no app class by name).
    static KOTLIN: OnceLock<(JavaVM, GlobalRef)> = OnceLock::new();

    /// Runs `call` against `PushCore` on this thread; a Kotlin exception is cleared, never thrown on.
    fn kotlin<T>(call: impl FnOnce(&mut JNIEnv, &JClass) -> jni::errors::Result<T>) -> Option<T> {
        let (vm, class) = KOTLIN.get()?;
        let mut env = vm.attach_current_thread_as_daemon().ok()?;
        let answer = call(&mut env, <&JClass>::from(class.as_obj()));
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }
        answer.ok()
    }

    /// A `PushCore` method with no arguments and nothing back.
    fn tell(name: &str) {
        let _ = kotlin(|env, class| env.call_static_method(class, name, "()V", &[]).map(drop));
    }

    /// The Keystore through Kotlin: bytes in, bytes out.
    fn through_keystore(name: &str, input: &[u8]) -> anyhow::Result<Vec<u8>> {
        kotlin(|env, class| {
            let input = env.byte_array_from_slice(input)?;
            let output = env.call_static_method(class, name, "([B)[B", &[JValue::Object(&input)])?.l()?;
            env.convert_byte_array(JByteArray::from(output))
        })
        .ok_or_else(|| anyhow!("the key store did not answer"))
    }

    /// The Android Keystore, as the app's bridge has it, with no activity (§94).
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

    /// The push's notification, kept by Kotlin's `PushCore`.
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

        // Answering and connecting go through the app, which the notification's answer opens.
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

    /// `PushCore.nativeStart(dataDir)`: a call push came. Returns at once; the core starts in the
    /// background (or is told that a call is on its way). Never unwinds into the JVM.
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

    /// `PushCore.nativeCallEvent(json)`: a notification button for a core a push started. `false`
    /// if none listens: the event waits in Kotlin's queue for the app.
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

    // A push never makes an identity: with no sealed key or no database (a phone not set up, or
    // erased), nothing starts; the app makes them when the user opens it.
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

    // No call came (the caller's phone died, or the push was late): the core lets go once nobody
    // can be calling any more.
    #[test]
    fn with_no_call_it_stops_once_nobody_can_be_calling() {
        let start = Instant::now();
        let watch = Watch::new(start);
        assert_eq!(watch.stops(start, false), None);
        assert_eq!(watch.stops(start + WAIT_FOR_CALL - Duration::from_millis(1), false), None);
        assert_eq!(watch.stops(start + WAIT_FOR_CALL, false), Some(Stop::NoCall));
    }

    // Never while a call rings, is answered or goes on: the app's own limits end it.
    #[test]
    fn it_never_stops_while_a_call_goes_on() {
        let start = Instant::now();
        let mut watch = Watch::new(start);
        watch.saw(&incoming(), start + Duration::from_secs(3));
        assert_eq!(watch.stops(start + WAIT_FOR_CALL * 3, true), None);
    }

    // The call is over (the caller hung up, it was declined or refused): a moment for what the
    // core still sends, then it stops.
    #[test]
    fn once_the_call_is_over_it_stops_a_moment_later() {
        for over in [ended(), Event::CallRefused] {
            let start = Instant::now();
            let mut watch = Watch::new(start);
            let at = start + Duration::from_secs(5);
            watch.saw(&incoming(), start + Duration::from_secs(3));
            watch.saw(&over, at);
            assert_eq!(watch.stops(at, false), None);
            assert_eq!(watch.stops(at + LINGER - Duration::from_millis(1), false), None);
            assert_eq!(watch.stops(at + LINGER, false), Some(Stop::Settled));
        }
    }

    // A second call push, or a call that rings after another was refused: the core waits for it.
    #[test]
    fn a_new_push_or_a_new_call_waits_again() {
        let start = Instant::now();
        let mut watch = Watch::new(start);
        let refused = start + Duration::from_secs(2);
        watch.saw(&Event::CallRefused, refused);
        let again = refused + Duration::from_secs(1);
        watch.pushed(again);
        assert_eq!(watch.stops(refused + LINGER, false), None, "a call is on its way");
        assert_eq!(watch.stops(again + WAIT_FOR_CALL, false), Some(Stop::NoCall));

        let mut watch = Watch::new(start);
        watch.saw(&Event::CallRefused, refused);
        watch.saw(&incoming(), again);
        assert_eq!(watch.stops(refused + LINGER, false), None);
    }

    // What else the core says changes nothing.
    #[test]
    fn other_events_change_nothing() {
        let start = Instant::now();
        let mut watch = Watch::new(start);
        watch.saw(&Event::ContactsChanged, start);
        watch.saw(&Event::MessagesChanged { contact: "ft_a".to_owned() }, start);
        let busy = Event::Call { contact: "ft_b".to_owned(), call: "d".to_owned(), update: CallUpdate::MissedWhileBusy };
        watch.saw(&busy, start);
        assert_eq!(watch.stops(start + LINGER, false), None);
    }


    /// A call screen that writes down what it was told, as `who: step`.
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

    // One core per process (2026-10-01): until the app opens, what the call screen must hear goes
    // to the push's notification; once the app takes the core, to the app's bridge, and only there
    // (never both: no second ring). The app takes it once.
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

    // A core the app started answers to the app from the start.
    #[test]
    fn a_core_the_app_started_tells_the_app_s_screen() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let host = Host::for_app("app");
        assert!(!host.adopt("app"));
        let made = log.clone();
        host.screen(move |app: &&'static str| -> Arc<dyn CallScreen> { Arc::new(Told(app, made)) }).expect("the app's").refused();
        assert_eq!(*log.lock().unwrap(), vec!["app: refused".to_owned()]);
    }

    // The watch ends only a core a push started and the app has not taken: the app's core lives
    // as long as the app.
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

    // A second call push restarts the wait of a core the push started.
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
