//! Starting the core online (Plan §106 M3): the router client signs with the core's identity, the
//! core sends through the network and the network talks through that router client. This module
//! ties the knot, registers the device, listens to the router, retries the outbox and resumes
//! stalled file transfers.

use std::sync::{Arc, OnceLock, PoisonError, Weak};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use async_trait::async_trait;
use ft_push::{Feedback, RouterClient, Signer};
use ft_storage::Store;
use ft_webrtc::SessionConfig;

use crate::net::Network;
use crate::{CallUpdate, Core, Event};

/// How often the outbox is checked for messages whose next attempt is due (§26).
pub const RETRY_EVERY: Duration = Duration::from_secs(5);

/// What registers this device with the router: the router client, or a fake in the tests.
#[async_trait]
pub trait Registrar: Send + Sync {
    async fn register(&self, capability_hashes: &[[u8; 32]; 8], silent_slots: u8) -> Result<()>;
    /// A closed client (the phone was erased) stops trying.
    fn is_closed(&self) -> bool;
}

#[async_trait]
impl Registrar for RouterClient {
    async fn register(&self, capability_hashes: &[[u8; 32]; 8], silent_slots: u8) -> Result<()> {
        RouterClient::register(self, capability_hashes, silent_slots).await
    }

    fn is_closed(&self) -> bool {
        RouterClient::is_closed(self)
    }
}

/// The first wait after a registration fails, and the longest.
const REGISTER_RETRY: Duration = Duration::from_secs(2);
const REGISTER_RETRY_MAX: Duration = Duration::from_secs(60);

/// Keeps the router told what it must know of this device (2026-10-01): the eight hashes and
/// which slots are silent. Registers now, and again after every change the core reports; one
/// that fails is tried again until it goes through. Each try reads the core as it is then, so
/// the router always ends up with the newest state.
pub async fn keep_registered(core: std::sync::Weak<Core>, router: Arc<dyn Registrar>, mut stop: tokio::sync::watch::Receiver<bool>) {
    let Some((mut changes, retry)) = core.upgrade().map(|core| (core.registration_changes(), core.registration_retry())) else { return };
    loop {
        changes.borrow_and_update();
        let mut wait = REGISTER_RETRY;
        loop {
            if router.is_closed() || *stop.borrow() {
                return;
            }
            let Some(now) = core.upgrade() else { return };
            let state = async { anyhow::Ok((now.route_capability_hashes().await?, now.silent_slots().await?)) }.await;
            drop(now);
            if let Ok((hashes, silent)) = state {
                if router.register(&hashes, silent).await.is_ok() {
                    break;
                }
            }
            // Tried again on a timer, at once if something changed meanwhile, and as soon as the
            // router can be reached again.
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                changed = changes.changed() => if changed.is_err() { return },
                _ = retry.notified() => {}
                _ = stop.wait_for(|stopped| *stopped) => return,
            }
            wait = (wait * 2).min(REGISTER_RETRY_MAX);
        }
        tokio::select! {
            changed = changes.changed() => if changed.is_err() { return },
            _ = stop.wait_for(|stopped| *stopped) => return,
        }
    }
}

/// How long a core a push started keeps going once the call is over: the busy it answers a
/// refused call with, or the decline, goes out first.
pub const LINGER: Duration = Duration::from_secs(3);

/// How long it waits for a call that never comes. The push lives 45 s (the router's TTL, the
/// notification's `CALL_RING_MS`) and the caller tries for 40 s (`CALL_REACH`): by then nobody calls.
pub const WAIT_FOR_CALL: Duration = Duration::from_secs(50);


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

/// When a core a push started has done its job (Android's push core, 2026-10-01; moved here so
/// that the app's own core out of the foreground follows the same rule, `Presence`).
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

/// Whether the app's core keeps its connection to the router and to other phones (2026-10-01).
///
/// A socket left open by an app that goes to the background looks alive to the router until
/// long after iOS suspends the app (or Android freezes it): the router hands it what comes and
/// sends no push, so calls and notices were missed. So the connection is kept only while the app
/// is in the foreground, while a call is going on, and for what a start or a push came for (the
/// same rule as a core a push starts, `Watch`); otherwise the app lets go at once and a push
/// brings it back.
#[derive(Debug, Clone, Copy)]
pub struct Presence {
    foreground: bool,
    /// What a start or a push came for, until it is done; also the moment after a call ends.
    watch: Option<Watch>,
}

impl Presence {
    /// A start counts as a push: the app is taken to be in the foreground until the platform says
    /// otherwise, and a process a push started stays for its call.
    pub fn started(now: Instant) -> Self {
        Self { foreground: true, watch: Some(Watch::new(now)) }
    }

    /// The platform says the app is in the foreground or not. In front, what a push waited for
    /// is forgotten: leaving again lets go at once.
    pub fn set_foreground(&mut self, foreground: bool) {
        self.foreground = foreground;
        if foreground {
            self.watch = None;
        }
    }

    /// A push came: a call or a message is on its way.
    pub fn woken(&mut self, now: Instant) {
        match &mut self.watch {
            Some(watch) => watch.pushed(now),
            None => self.watch = Some(Watch::new(now)),
        }
    }

    /// What the core said: a call that ends keeps the connection a moment longer (`LINGER`).
    pub fn saw(&mut self, event: &Event, now: Instant) {
        match &mut self.watch {
            Some(watch) => watch.saw(event, now),
            None => {
                let mut watch = Watch::new(now);
                watch.saw(event, now);
                if watch.settled.is_some() {
                    self.watch = Some(watch);
                }
            }
        }
    }

    /// Whether to stay connected now. `call_going_on`: a call rings, is being answered, is on, or
    /// ours is being placed.
    pub fn connected(&self, now: Instant, call_going_on: bool) -> bool {
        self.foreground || call_going_on || self.watch.is_some_and(|watch| watch.stops(now, false).is_none())
    }
}

/// How often the client looks whether what kept it connected out of the foreground is over.
const PRESENCE_LOOK: Duration = Duration::from_secs(1);

/// Lets go of the router and the other phones, or comes back to them, as `Presence` says.
pub struct Lifecycle {
    core: Weak<Core>,
    router: Arc<RouterClient>,
    network: Arc<Network>,
    presence: std::sync::Mutex<Presence>,
    /// One look at a time: the last decision is the one that stands.
    looking: tokio::sync::Mutex<()>,
}

impl Lifecycle {
    fn presence(&self) -> std::sync::MutexGuard<'_, Presence> {
        self.presence.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The platform says the app is in the foreground or not (2026-10-01). Returns once done, so
    /// that a phone about to be suspended has let go: `true` if the connection came back.
    pub async fn set_foreground(&self, foreground: bool) -> bool {
        self.presence().set_foreground(foreground);
        self.settle().await
    }

    /// A push came (a call's, or a wake): out of the foreground, the connection comes back for what
    /// it announces. `true` if it came back; if it was there, the caller may still want a fresh one.
    pub async fn woken(&self) -> bool {
        self.presence().woken(Instant::now());
        self.settle().await
    }

    /// Acts on what `Presence` says now: away, the router's socket goes at once (so it pushes what
    /// comes) and the direct connections close (so the other phones see this one gone); back, the
    /// socket opens again and its welcome fetches what waited. `true` if the connection came back.
    async fn settle(&self) -> bool {
        let _one_at_a_time = self.looking.lock().await;
        let now = Instant::now();
        let connected = self.presence().connected(now, false) || {
            let Some(core) = self.core.upgrade() else { return false };
            let going_on = core.current_call().await.ok().flatten().is_some();
            self.presence().connected(now, going_on)
        };
        if connected {
            return self.router.set_away(false);
        }
        if self.router.set_away(true) {
            self.network.close().await;
        }
        false
    }
}

/// Follows the core's calls for `Presence`, and looks again every second: a call that ends out of
/// the foreground lets go a moment later, and a push that brought nothing in a while.
async fn keep_present(lifecycle: Arc<Lifecycle>, mut events: tokio::sync::broadcast::Receiver<crate::Event>, mut stop: tokio::sync::watch::Receiver<bool>) {
    let mut look = tokio::time::interval(PRESENCE_LOOK);
    loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(event) => lifecycle.presence().saw(&event, Instant::now()),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            },
            _ = look.tick() => {}
            _ = stop.wait_for(|stopped| *stopped) => return,
        }
        lifecycle.settle().await;
    }
}

/// The running client: the core, its router client and its network.
pub struct Online {
    pub core: Arc<Core>,
    pub router: Arc<RouterClient>,
    pub network: Arc<Network>,
    /// Set once, by `shutdown`: the background work of this client ends.
    stop: tokio::sync::watch::Sender<bool>,
    lifecycle: Arc<Lifecycle>,
}

/// The router client is created before the core, so it reaches the identity through this slot.
struct CoreSigner(Arc<OnceLock<Arc<Core>>>);

impl CoreSigner {
    fn core(&self) -> &Core {
        self.0.get().expect("the core is set before the router is used")
    }
}

#[async_trait]
impl Signer for CoreSigner {
    fn device_id(&self) -> String {
        self.core().device_id().to_string()
    }

    async fn signing_key(&self) -> String {
        self.core().signing_key().await
    }

    async fn sign(&self, message: &[u8]) -> String {
        self.core().sign(message).await
    }
}

/// Opens the core over `store` and connects it to the router at `router` (for example
/// `https://api.flickertalk.com`). WebRTC listens as `base` says; STUN and TURN come from the
/// router. Registering runs in the background (`keep_registered`): nothing waits for the network.
pub async fn start(store: Store, key: [u8; 32], router: &str, base: SessionConfig) -> Result<Online> {
    let slot = Arc::new(OnceLock::new());
    let router = Arc::new(RouterClient::new(router, Arc::new(CoreSigner(slot.clone())))?);
    let network = Network::new(router.clone(), base);
    let core = Core::open(store, key, network.clone()).await.context("cannot open the core")?;
    let _ = slot.set(core.clone());
    network.attach(&core);

    let stop = tokio::sync::watch::Sender::new(false);
    // Always eight capabilities, our own and seven for hidden sessions, used or not (app#9), and
    // which slots are silent (2026-10-01): now, after every change, and until the router has it.
    tokio::spawn(keep_registered(Arc::downgrade(&core), router.clone(), stop.subscribe()));
    network.listen(router.listen());

    // An upgrade that made the envelope key (A1): every contact gets the card with it, so that
    // what they send through the router from now on names nobody.
    if core.card_stale().await.unwrap_or(false) {
        let stale = core.clone();
        tokio::spawn(async move {
            let _ = stale.reintroduce().await;
        });
    }

    // Out of the foreground the app lets go of the router and the other phones (2026-10-01).
    let lifecycle = Arc::new(Lifecycle {
        core: Arc::downgrade(&core),
        router: router.clone(),
        network: network.clone(),
        presence: std::sync::Mutex::new(Presence::started(Instant::now())),
        looking: tokio::sync::Mutex::new(()),
    });
    tokio::spawn(keep_present(lifecycle.clone(), core.events(), stop.subscribe()));

    let mut stopping = stop.subscribe();
    let retrying = Arc::downgrade(&core);
    let away = router.clone();
    tokio::spawn(async move {
        let mut every = tokio::time::interval(RETRY_EVERY);
        loop {
            tokio::select! {
                _ = every.tick() => {}
                _ = stopping.wait_for(|stopped| *stopped) => break,
            }
            let Some(core) = retrying.upgrade() else { break };
            // Away, nothing is retried: a retry would open a connection the other phone sees as
            // alive, and have the router wake this phone for the answer. The welcome on coming
            // back retries everything. A text written for later goes at its time all the same
            // (2026-10-06): the user chose when, and Android runs the core in the background.
            if away.is_away() {
                let _ = core.send_scheduled_due().await;
                continue;
            }
            let _ = core.retry_due().await;
            let _ = core.resume_files().await;
            // Issue app#1: histories that expire and read messages that burn.
            let _ = core.sweep_history().await;
        }
    });

    Ok(Online { core, router, network, stop, lifecycle })
}

impl Online {
    /// Stops this client for good (erasing the phone, 2026-09-30). iOS cannot start the app
    /// again, so the running client stops in place and a new one starts from what is left on
    /// disk: a call going on ends, the router socket and the direct connections close, the
    /// background work ends and the database is closed, so that it can be deleted.
    pub async fn shutdown(&self) {
        if let Ok(Some(call)) = self.core.current_call().await {
            let _ = self.core.end_call(&call.call, false).await;
        }
        self.stop.send_replace(true);
        self.router.close();
        self.network.close().await;
        self.core.store().close().await;
    }

    /// What follows the app's foreground and the pushes (`Lifecycle`), for whoever hears them.
    pub fn lifecycle(&self) -> Arc<Lifecycle> {
        self.lifecycle.clone()
    }

    /// See `Lifecycle::set_foreground`.
    pub async fn set_foreground(&self, foreground: bool) -> bool {
        self.lifecycle.set_foreground(foreground).await
    }

    /// See `Lifecycle::woken`.
    pub async fn woken(&self) -> bool {
        self.lifecycle.woken().await
    }

    /// Sends a suggestion to the project's mailbox (2026-10-02), without the spaces around it,
    /// with the app's version (the app's, not a crate's) and the platform it runs on. Here and not
    /// on `Core`: the router client lives with the running client. Nothing of it is kept.
    pub async fn send_feedback(&self, text: &str, app_version: &str) -> Feedback {
        self.router.feedback(text.trim(), app_version, std::env::consts::OS).await
    }

    /// Resolves once the client is stopped: whoever follows its events lets them go.
    pub fn stopped(&self) -> impl std::future::Future<Output = ()> + Send + 'static {
        let mut stop = self.stop.subscribe();
        async move {
            let _ = stop.wait_for(|stopped| *stopped).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ended() -> Event {
        Event::Call { contact: "ft_a".to_owned(), call: "c".to_owned(), update: CallUpdate::Ended { outcome: ft_storage::CallOutcome::Missed } }
    }

    fn incoming() -> Event {
        Event::Call { contact: "ft_a".to_owned(), call: "c".to_owned(), update: CallUpdate::Incoming { video: false, sdp: String::new() } }
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


    // In front, the app is always connected, call or not.
    #[test]
    fn in_the_foreground_it_stays_connected() {
        let start = Instant::now();
        let mut presence = Presence::started(start);
        presence.set_foreground(true);
        assert!(presence.connected(start + WAIT_FOR_CALL * 10, false));
        presence.saw(&ended(), start);
        assert!(presence.connected(start + WAIT_FOR_CALL * 10, false));
    }

    // Leaving the foreground with no call lets go at once: the router pushes what comes, and the
    // other phones see this one gone.
    #[test]
    fn leaving_the_foreground_lets_go_at_once() {
        let start = Instant::now();
        let mut presence = Presence::started(start);
        presence.set_foreground(true);
        let left = start + Duration::from_secs(1);
        presence.set_foreground(false);
        assert!(!presence.connected(left, false));
        presence.set_foreground(false);
        assert!(!presence.connected(left, false), "leaving twice is leaving");
    }

    // A call (ringing, being answered, on, or ours being placed) keeps the connection whatever
    // the app does; once it is over, a moment for what still goes out (the hang-up), then it goes.
    #[test]
    fn a_call_keeps_it_out_of_the_foreground_until_a_moment_after_it_ends() {
        let start = Instant::now();
        let mut presence = Presence::started(start);
        presence.set_foreground(true);
        presence.set_foreground(false);
        assert!(presence.connected(start + WAIT_FOR_CALL * 3, true));
        let over = start + WAIT_FOR_CALL * 3;
        presence.saw(&ended(), over);
        assert!(presence.connected(over + LINGER - Duration::from_millis(1), false));
        assert!(!presence.connected(over + LINGER, false));
    }

    // A process a push started out of the foreground (iOS PushKit, with no scene): the start is
    // for a call, so it stays until that call is over, or until nobody can be calling any more.
    #[test]
    fn a_start_out_of_the_foreground_waits_for_its_call() {
        let start = Instant::now();
        let mut presence = Presence::started(start);
        presence.set_foreground(false);
        assert!(presence.connected(start + WAIT_FOR_CALL - Duration::from_millis(1), false));
        assert!(!presence.connected(start + WAIT_FOR_CALL, false), "no call came");

        let mut presence = Presence::started(start);
        presence.set_foreground(false);
        presence.saw(&incoming(), start + Duration::from_secs(2));
        let over = start + Duration::from_secs(9);
        presence.saw(&ended(), over);
        assert!(presence.connected(over + LINGER - Duration::from_millis(1), false));
        assert!(!presence.connected(over + LINGER, false), "the call is over");
    }

    // Out of the foreground and let go, a push (a call's, or a wake) brings the connection back
    // for what it announces, as a start does: a redial rings.
    #[test]
    fn a_push_brings_it_back_for_what_it_announces() {
        let start = Instant::now();
        let mut presence = Presence::started(start);
        presence.set_foreground(true);
        presence.set_foreground(false);
        let pushed = start + Duration::from_secs(20);
        assert!(!presence.connected(pushed, false));
        presence.woken(pushed);
        assert!(presence.connected(pushed, false));
        assert!(presence.connected(pushed + WAIT_FOR_CALL - Duration::from_millis(1), false));
        assert!(!presence.connected(pushed + WAIT_FOR_CALL, false));

        presence.woken(pushed);
        let over = pushed + Duration::from_secs(8);
        presence.saw(&incoming(), pushed + Duration::from_secs(1));
        presence.saw(&ended(), over);
        assert!(!presence.connected(over + LINGER, false), "the redial is over");
    }

    // Back in front, whatever a push or the start was waiting for is forgotten: leaving again
    // lets go at once.
    #[test]
    fn coming_back_forgets_what_a_push_waited_for() {
        let start = Instant::now();
        let mut presence = Presence::started(start);
        presence.set_foreground(false);
        presence.woken(start);
        presence.set_foreground(true);
        presence.set_foreground(true);
        assert!(presence.connected(start, false));
        presence.set_foreground(false);
        assert!(!presence.connected(start + Duration::from_millis(1), false));
    }

    #[test]
    fn due_messages_are_checked_every_few_seconds() {
        assert!(RETRY_EVERY <= Duration::from_secs(5));
    }

    /// A router that records each registration and refuses the first `failing` ones.
    #[derive(Default)]
    struct Router {
        failing: std::sync::atomic::AtomicUsize,
        tries: std::sync::atomic::AtomicUsize,
        registered: std::sync::Mutex<Vec<([[u8; 32]; 8], u8)>>,
        closed: std::sync::atomic::AtomicBool,
    }

    #[async_trait]
    impl Registrar for Router {
        async fn register(&self, hashes: &[[u8; 32]; 8], silent_slots: u8) -> Result<()> {
            use std::sync::atomic::Ordering::SeqCst;
            self.tries.fetch_add(1, SeqCst);
            if self.failing.load(SeqCst) > 0 {
                self.failing.fetch_sub(1, SeqCst);
                anyhow::bail!("no network");
            }
            self.registered.lock().unwrap().push((*hashes, silent_slots));
            Ok(())
        }

        fn is_closed(&self) -> bool {
            self.closed.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl Router {
        fn last(&self) -> Option<([[u8; 32]; 8], u8)> {
            self.registered.lock().unwrap().last().copied()
        }

        fn count(&self) -> usize {
            self.registered.lock().unwrap().len()
        }
    }

    struct Nowhere;

    #[async_trait]
    impl crate::Transport for Nowhere {
        async fn send_direct(&self, _: &crate::Peer, _: Vec<u8>) -> Result<bool> {
            Ok(false)
        }

        async fn send_mailbox(&self, _: &crate::Peer, _: Vec<u8>) -> Result<()> {
            Ok(())
        }
    }

    async fn core() -> Arc<Core> {
        Core::open(Store::open_in_memory().await.unwrap(), [7; 32], Arc::new(Nowhere)).await.unwrap()
    }

    async fn until(what: &str, condition: impl Fn() -> bool) {
        for _ in 0..500 {
            if condition() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("timed out waiting until {what}");
    }

    fn registering(core: &Arc<Core>, router: &Arc<Router>) -> tokio::sync::watch::Sender<bool> {
        let stop = tokio::sync::watch::Sender::new(false);
        tokio::spawn(keep_registered(Arc::downgrade(core), router.clone(), stop.subscribe()));
        stop
    }

    // Every start registers, with what the core says now: the hashes and the silent slots.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_start_registers_the_hashes_and_the_silent_slots() {
        let (core, router) = (core().await, Arc::new(Router::default()));
        let _stop = registering(&core, &router);
        until("registered", || router.count() == 1).await;
        let expected = (core.route_capability_hashes().await.unwrap(), core.silent_slots().await.unwrap());
        assert_eq!(router.last(), Some(expected));
    }

    // 2026-10-01: a session opened or left changes what the router must stay silent for, and the
    // phone registers again at once.
    #[tokio::test(flavor = "multi_thread")]
    async fn opening_or_leaving_a_session_registers_again() {
        let (core, router) = (core().await, Arc::new(Router::default()));
        let _stop = registering(&core, &router);
        until("registered", || router.count() == 1).await;

        let session = core.open_session("123456").await.unwrap().unwrap();
        let slot = core.store().session_slot(&session).await.unwrap().unwrap();
        until("registered with the session open", || router.last().is_some_and(|(_, mask)| mask & 1 << slot == 0) && router.count() >= 2).await;
        assert_eq!(router.last().unwrap().1, core.silent_slots().await.unwrap());

        // Empty, it goes when left: its slot gets a new link, and the router a new hash.
        let before = core.route_capability_hashes().await.unwrap();
        core.close_session(&session).await.unwrap();
        let after = core.route_capability_hashes().await.unwrap();
        assert_ne!(after, before);
        until("registered with the new hash", || router.last().is_some_and(|(hashes, _)| hashes == after)).await;
        assert_eq!(router.last().unwrap().1, core.silent_slots().await.unwrap());
    }

    // No network: the change is not lost. It is tried again until the router has it, and what
    // goes is the newest state, not the one of the failed try.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_registration_that_fails_is_tried_again_with_the_newest_state() {
        let (core, router) = (core().await, Arc::new(Router::default()));
        let _stop = registering(&core, &router);
        until("registered", || router.count() == 1).await;

        router.failing.store(2, std::sync::atomic::Ordering::SeqCst);
        let first = core.open_session("111111").await.unwrap().unwrap();
        until("a try failed", || router.tries.load(std::sync::atomic::Ordering::SeqCst) >= 2).await;
        let second = core.open_session("222222").await.unwrap().unwrap();
        // The router is back: no need to wait for the timer.
        core.router_reachable();
        let wanted = core.silent_slots().await.unwrap();
        until("the router has the newest state", || router.last().is_some_and(|(_, mask)| mask == wanted) && router.count() >= 2).await;
        for session in [first, second] {
            let slot = core.store().session_slot(&session).await.unwrap().unwrap();
            assert_eq!(router.last().unwrap().1 & 1 << slot, 0, "both open, so neither silent");
        }
    }

    // A failed registration is retried on a timer too, without anything else happening.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_failed_registration_is_retried_on_its_own() {
        let (core, router) = (core().await, Arc::new(Router::default()));
        router.failing.store(1, std::sync::atomic::Ordering::SeqCst);
        let _stop = registering(&core, &router);
        tokio::time::sleep(REGISTER_RETRY + Duration::from_millis(500)).await;
        assert_eq!(router.count(), 1, "the second try went through");
    }

    // An erased phone (a closed router client) or a stopped client tries no more.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_closed_router_or_a_stopped_client_is_not_registered_with() {
        let (core, router) = (core().await, Arc::new(Router::default()));
        let stop = registering(&core, &router);
        until("registered", || router.count() == 1).await;
        stop.send_replace(true);
        tokio::time::sleep(Duration::from_millis(50)).await;
        core.open_session("123456").await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(router.count(), 1);

        let (core, router) = (self::core().await, Arc::new(Router::default()));
        router.closed.store(true, std::sync::atomic::Ordering::SeqCst);
        let _stop = registering(&core, &router);
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(router.tries.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    // Erasing the phone (2026-09-30): iOS cannot start the app again, so the running core stops
    // in place. Nothing of the old identity may reach the router or the database afterwards.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_stopped_client_lets_go_of_the_router_and_the_database() {
        let store = Store::open_in_memory().await.unwrap();
        // Nothing listens there: registering is left retrying in the background.
        let online = start(store, [7; 32], "http://127.0.0.1:9", SessionConfig::default()).await.unwrap();
        assert!(online.core.store().contacts().await.is_ok());

        online.shutdown().await;
        assert!(online.router.is_closed());
        assert!(online.core.store().contacts().await.is_err(), "the database is closed");
        let stopped = tokio::time::timeout(Duration::from_secs(1), online.stopped()).await;
        assert!(stopped.is_ok(), "whoever waits for the stop hears it");
    }
}
