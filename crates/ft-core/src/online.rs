use std::collections::HashMap;
use std::sync::{Arc, OnceLock, PoisonError, Weak};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use async_trait::async_trait;
use ft_push::{Feedback, RouterClient, Signer};
use ft_storage::Store;
use ft_webrtc::SessionConfig;

use crate::net::Network;
use crate::{CallUpdate, Core, Event};

pub const RETRY_EVERY: Duration = Duration::from_secs(5);

#[async_trait]
pub trait Registrar: Send + Sync {
    async fn register(&self, capability_hashes: &[[u8; 32]; 8], silent_slots: u8) -> Result<()>;
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

impl Core {
    pub fn registration_changes(&self) -> tokio::sync::watch::Receiver<u64> {
        self.registration.subscribe()
    }

    pub(crate) fn registration_changed(&self) {
        self.registration.send_modify(|generation| *generation += 1);
    }

    pub fn registration_retry(&self) -> Arc<tokio::sync::Notify> {
        self.registration_retry.clone()
    }

    pub fn router_reachable(&self) {
        self.registration_retry.notify_one();
    }
}

const REGISTER_RETRY: Duration = Duration::from_secs(2);
const REGISTER_RETRY_MAX: Duration = Duration::from_secs(60);

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

// Lets the busy or decline go out before a push-started core stops
pub const LINGER: Duration = Duration::from_secs(3);

// The call push lives 45 s and the caller tries for 40 s
pub const WAIT_FOR_CALL: Duration = Duration::from_secs(50);


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    Settled,
    NoCall,
}

impl Stop {
    // Logged by Kotlin, so never who called
    pub fn word(self) -> &'static str {
        match self {
            Stop::Settled => "settled",
            Stop::NoCall => "no call",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Watch {
    pushed: Instant,
    settled: Option<Instant>,
}

impl Watch {
    pub fn new(now: Instant) -> Self {
        Self { pushed: now, settled: None }
    }

    pub fn pushed(&mut self, now: Instant) {
        self.pushed = now;
        self.settled = None;
    }

    pub fn saw(&mut self, event: &Event, now: Instant) {
        match event {
            Event::Call { update: CallUpdate::Incoming { .. }, .. } => self.settled = None,
            Event::Call { update: CallUpdate::Ended { .. }, .. } | Event::CallRefused => self.settled = Some(now),
            _ => {}
        }
    }

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

// A suspended iOS app (or frozen Android one) keeps a socket that looks alive to the router, so no push goes out and calls get missed
#[derive(Debug, Clone, Copy)]
pub struct Presence {
    foreground: bool,
    watch: Option<Watch>,
}

impl Presence {
    pub fn started(now: Instant) -> Self {
        Self { foreground: true, watch: Some(Watch::new(now)) }
    }

    pub fn set_foreground(&mut self, foreground: bool) {
        self.foreground = foreground;
        if foreground {
            self.watch = None;
        }
    }

    pub fn woken(&mut self, now: Instant) {
        match &mut self.watch {
            Some(watch) => watch.pushed(now),
            None => self.watch = Some(Watch::new(now)),
        }
    }

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

    pub fn in_front(&self) -> bool {
        self.foreground
    }

    pub fn connected(&self, now: Instant, call_going_on: bool) -> bool {
        self.foreground || call_going_on || self.watch.is_some_and(|watch| watch.stops(now, false).is_none())
    }
}

const PRESENCE_LOOK: Duration = Duration::from_secs(1);

// Past this the contact has probably left too, and the router would wake their phone for nothing
pub const REDIAL_WITHIN: Duration = Duration::from_secs(120);

// Wall clock on purpose, the monotonic clock stops while the phone sleeps
#[derive(Debug, Default)]
struct LeftBehind {
    closed: HashMap<String, SystemTime>,
}

impl LeftBehind {
    fn closed(&mut self, contacts: Vec<String>, at: SystemTime) {
        for contact in contacts {
            self.closed.insert(contact, at);
        }
    }

    fn take(&mut self, now: SystemTime, within: Duration) -> Vec<String> {
        let mut recent: Vec<String> = self
            .closed
            .drain()
            .filter(|(_, at)| now.duration_since(*at).is_ok_and(|ago| ago < within))
            .map(|(contact, _)| contact)
            .collect();
        recent.sort();
        recent
    }
}

#[derive(Debug)]
struct Addresses(Vec<String>);

impl Addresses {
    fn new(mut seen: Vec<String>) -> Self {
        seen.sort();
        Self(seen)
    }

    fn changed(&mut self, mut now: Vec<String>) -> bool {
        now.sort();
        if now == self.0 {
            return false;
        }
        self.0 = now;
        true
    }
}

pub struct Lifecycle {
    core: Weak<Core>,
    router: Arc<RouterClient>,
    network: Arc<Network>,
    presence: std::sync::Mutex<Presence>,
    looking: tokio::sync::Mutex<()>,
    left: std::sync::Mutex<LeftBehind>,
    redial_within: std::sync::Mutex<Duration>,
}

impl Lifecycle {
    fn presence(&self) -> std::sync::MutexGuard<'_, Presence> {
        self.presence.lock().unwrap_or_else(PoisonError::into_inner)
    }

    // A leave and return faster than the socket loop polls leaves the socket up with no new welcome, so retry registration here
    pub async fn set_foreground(&self, foreground: bool) -> bool {
        let back = self.change(|presence| presence.set_foreground(foreground)).await;
        if foreground {
            if let Some(core) = self.core.upgrade() {
                core.router_reachable();
            }
        }
        back
    }

    #[doc(hidden)]
    pub fn set_redial_within(&self, within: Duration) {
        *self.redial_within.lock().unwrap_or_else(PoisonError::into_inner) = within;
    }

    // The old socket can sit quiet on a dead path while the router keeps feeding it
    pub async fn network_changed(&self) {
        let _one_at_a_time = self.looking.lock().await;
        if self.router.is_away() {
            return;
        }
        self.network.moved().await;
        self.router.reconnect_now();
    }

    pub async fn woken(&self) -> bool {
        self.change(|presence| presence.woken(Instant::now())).await
    }

    // One look, otherwise the presence loop can reconnect first and the caller opens a second socket,
    // dropping the call offer the router handed to the first
    async fn change(&self, change: impl FnOnce(&mut Presence)) -> bool {
        let one_at_a_time = self.looking.lock().await;
        change(&mut self.presence());
        self.settle_holding(&one_at_a_time).await
    }

    #[doc(hidden)]
    pub async fn look(&self) -> bool {
        self.settle().await
    }

    async fn settle(&self) -> bool {
        let one_at_a_time = self.looking.lock().await;
        self.settle_holding(&one_at_a_time).await
    }

    async fn settle_holding(&self, _one_at_a_time: &tokio::sync::MutexGuard<'_, ()>) -> bool {
        let now = Instant::now();
        let connected = self.presence().connected(now, false) || {
            let Some(core) = self.core.upgrade() else { return false };
            let going_on = core.current_call().await.ok().flatten().is_some();
            self.presence().connected(now, going_on)
        };
        if connected {
            if self.presence().in_front() {
                let within = *self.redial_within.lock().unwrap_or_else(PoisonError::into_inner);
                let left = self.left.lock().unwrap_or_else(PoisonError::into_inner).take(SystemTime::now(), within);
                self.network.redial(left);
            }
            return self.router.set_away(false);
        }
        if self.router.set_away(true) {
            let open = self.network.connected().await;
            self.left.lock().unwrap_or_else(PoisonError::into_inner).closed(open, SystemTime::now());
            self.network.close().await;
        }
        false
    }
}

const NETWORK_LOOK: Duration = Duration::from_secs(3);

async fn watch_addresses<Fut>(
    mut addresses: impl FnMut() -> Vec<String>,
    mut changed: impl FnMut() -> Fut,
    mut away: tokio::sync::watch::Receiver<bool>,
    mut stop: tokio::sync::watch::Receiver<bool>,
) where
    Fut: std::future::Future<Output = ()>,
{
    loop {
        tokio::select! {
            back = away.wait_for(|away| !*away) => if back.is_err() { return },
            _ = stop.wait_for(|stopped| *stopped) => return,
        }
        let mut seen = Addresses::new(addresses());
        let mut look = tokio::time::interval_at(tokio::time::Instant::now() + NETWORK_LOOK, NETWORK_LOOK);
        loop {
            tokio::select! {
                _ = look.tick() => {}
                gone = away.wait_for(|away| *away) => if gone.is_err() { return } else { break },
                _ = stop.wait_for(|stopped| *stopped) => return,
            }
            if seen.changed(addresses()) {
                changed().await;
            }
        }
    }
}

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

pub struct Online {
    pub core: Arc<Core>,
    pub router: Arc<RouterClient>,
    pub network: Arc<Network>,
    stop: tokio::sync::watch::Sender<bool>,
    lifecycle: Arc<Lifecycle>,
}

// The router client exists before the core, hence the slot
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

pub async fn start(store: Store, key: [u8; 32], router: &str, base: SessionConfig) -> Result<Online> {
    let slot = Arc::new(OnceLock::new());
    let router = Arc::new(RouterClient::new(router, Arc::new(CoreSigner(slot.clone())))?);
    let follows_the_network = base.follow_interfaces;
    let network = Network::new(router.clone(), base);
    let core = Core::open(store, key, network.clone()).await.context("cannot open the core")?;
    let _ = slot.set(core.clone());
    network.attach(&core);

    let stop = tokio::sync::watch::Sender::new(false);
    // always all eight capabilities, used or not
    tokio::spawn(keep_registered(Arc::downgrade(&core), router.clone(), stop.subscribe()));
    network.listen(router.listen());

    // Hand out the card with the envelope key so their mail through the router names nobody
    if core.card_stale().await.unwrap_or(false) {
        let stale = core.clone();
        tokio::spawn(async move {
            let _ = stale.reintroduce().await;
        });
    }

    let lifecycle = Arc::new(Lifecycle {
        core: Arc::downgrade(&core),
        router: router.clone(),
        network: network.clone(),
        presence: std::sync::Mutex::new(Presence::started(Instant::now())),
        looking: tokio::sync::Mutex::new(()),
        left: std::sync::Mutex::default(),
        redial_within: std::sync::Mutex::new(REDIAL_WITHIN),
    });
    tokio::spawn(keep_present(lifecycle.clone(), core.events(), stop.subscribe()));
    if follows_the_network {
        let moved = lifecycle.clone();
        tokio::spawn(watch_addresses(
            ft_webrtc::interface_addresses,
            move || {
                let lifecycle = moved.clone();
                async move { lifecycle.network_changed().await }
            },
            router.away_changes(),
            stop.subscribe(),
        ));
    }

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
            // A retry while away would open a connection the other phone sees as alive
            if away.is_away() {
                let _ = core.send_scheduled_due().await;
                continue;
            }
            let _ = core.retry_due().await;
            let _ = core.resume_files().await;
            let _ = core.sweep_history().await;
        }
    });

    Ok(Online { core, router, network, stop, lifecycle })
}

impl Online {
    // iOS can't relaunch the app, so stop in place and close the database so it can be deleted
    pub async fn shutdown(&self) {
        if let Ok(Some(call)) = self.core.current_call().await {
            let _ = self.core.end_call(&call.call, false).await;
        }
        self.stop.send_replace(true);
        self.router.close();
        self.network.close().await;
        self.core.store().close().await;
    }

    pub fn lifecycle(&self) -> Arc<Lifecycle> {
        self.lifecycle.clone()
    }

    pub async fn set_foreground(&self, foreground: bool) -> bool {
        self.lifecycle.set_foreground(foreground).await
    }

    pub async fn woken(&self) -> bool {
        self.lifecycle.woken().await
    }

    // Nothing of it is kept. `app_version` is the app's, not a crate's.
    pub async fn send_feedback(&self, text: &str, app_version: &str) -> Feedback {
        self.router.feedback(text.trim(), app_version, std::env::consts::OS).await
    }

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

    #[test]
    fn with_no_call_it_stops_once_nobody_can_be_calling() {
        let start = Instant::now();
        let watch = Watch::new(start);
        assert_eq!(watch.stops(start, false), None);
        assert_eq!(watch.stops(start + WAIT_FOR_CALL - Duration::from_millis(1), false), None);
        assert_eq!(watch.stops(start + WAIT_FOR_CALL, false), Some(Stop::NoCall));
    }

    #[test]
    fn it_never_stops_while_a_call_goes_on() {
        let start = Instant::now();
        let mut watch = Watch::new(start);
        watch.saw(&incoming(), start + Duration::from_secs(3));
        assert_eq!(watch.stops(start + WAIT_FOR_CALL * 3, true), None);
    }

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


    #[test]
    fn contacts_are_dialled_again_only_after_a_short_absence() {
        let at = SystemTime::now();
        let mut left = LeftBehind::default();
        left.closed(vec!["ft_b".to_owned(), "ft_a".to_owned()], at);
        assert_eq!(left.take(at + REDIAL_WITHIN - Duration::from_secs(1), REDIAL_WITHIN), ["ft_a", "ft_b"]);
        assert!(left.take(at, REDIAL_WITHIN).is_empty(), "taken once");

        left.closed(vec!["ft_a".to_owned()], at);
        assert!(left.take(at + REDIAL_WITHIN, REDIAL_WITHIN).is_empty());
        assert!(left.take(at, REDIAL_WITHIN).is_empty(), "forgotten all the same");
    }

    #[test]
    fn each_contact_counts_from_its_own_let_go() {
        let at = SystemTime::now();
        let mut left = LeftBehind::default();
        left.closed(vec!["ft_a".to_owned()], at);
        left.closed(vec!["ft_c".to_owned()], at + Duration::from_secs(100));
        assert_eq!(left.take(at + Duration::from_secs(150), REDIAL_WITHIN), ["ft_c"]);
    }

    #[test]
    fn a_clock_set_back_dials_nobody() {
        let at = SystemTime::now();
        let mut left = LeftBehind::default();
        left.closed(vec!["ft_a".to_owned()], at);
        assert!(left.take(at - Duration::from_secs(1), REDIAL_WITHIN).is_empty());
    }

    fn addresses(list: &[&str]) -> Vec<String> {
        list.iter().map(|address| (*address).to_owned()).collect()
    }

    #[test]
    fn a_network_change_is_a_change_of_the_addresses() {
        let mut seen = Addresses::new(addresses(&["10.0.0.7:0", "192.168.1.20:0"]));
        assert!(!seen.changed(addresses(&["192.168.1.20:0", "10.0.0.7:0"])), "the same, in another order");
        assert!(seen.changed(addresses(&["10.0.0.7:0"])), "Wi-Fi left");
        assert!(!seen.changed(addresses(&["10.0.0.7:0"])), "seen once");
        assert!(seen.changed(addresses(&["10.0.0.7:0", "192.168.5.3:0"])), "another Wi-Fi joined");
        assert!(seen.changed(addresses(&["10.0.0.9:0", "192.168.5.3:0"])), "a new mobile data address");
    }

    #[tokio::test(start_paused = true)]
    async fn the_watcher_tells_each_change_of_the_addresses_once_and_never_while_away() {
        use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
        assert_eq!(NETWORK_LOOK, Duration::from_secs(3));
        let now = Arc::new(std::sync::Mutex::new(addresses(&["10.0.0.7:0"])));
        let (looks, told) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let (away, stop) = (tokio::sync::watch::Sender::new(false), tokio::sync::watch::Sender::new(false));
        let (looked, counted, seen) = (now.clone(), told.clone(), looks.clone());
        let watching = tokio::spawn(watch_addresses(
            move || {
                seen.fetch_add(1, SeqCst);
                looked.lock().unwrap().clone()
            },
            move || {
                counted.fetch_add(1, SeqCst);
                async {}
            },
            away.subscribe(),
            stop.subscribe(),
        ));

        tokio::time::sleep(NETWORK_LOOK * 3).await;
        assert_eq!(told.load(SeqCst), 0, "nothing changed");
        *now.lock().unwrap() = addresses(&["10.0.0.7:0", "192.168.1.20:0"]);
        tokio::time::sleep(NETWORK_LOOK + Duration::from_millis(1)).await;
        assert_eq!(told.load(SeqCst), 1, "Wi-Fi joined");
        tokio::time::sleep(NETWORK_LOOK * 3).await;
        assert_eq!(told.load(SeqCst), 1, "told once");

        away.send_replace(true);
        tokio::time::sleep(Duration::from_millis(1)).await;
        let before = looks.load(SeqCst);
        *now.lock().unwrap() = addresses(&["10.0.0.9:0"]);
        tokio::time::sleep(NETWORK_LOOK * 20).await;
        assert_eq!(looks.load(SeqCst), before, "nothing looked at while away");
        away.send_replace(false);
        tokio::time::sleep(NETWORK_LOOK * 3).await;
        assert!(looks.load(SeqCst) > before, "looking again");
        assert_eq!(told.load(SeqCst), 1, "the change made while away is not followed again");
        *now.lock().unwrap() = addresses(&["10.0.0.9:0", "192.168.1.20:0"]);
        tokio::time::sleep(NETWORK_LOOK + Duration::from_millis(1)).await;
        assert_eq!(told.load(SeqCst), 2, "a change once back is");

        stop.send_replace(true);
        tokio::time::timeout(NETWORK_LOOK, watching).await.expect("it stops").unwrap();
    }

    #[test]
    fn in_the_foreground_it_stays_connected() {
        let start = Instant::now();
        let mut presence = Presence::started(start);
        presence.set_foreground(true);
        assert!(presence.connected(start + WAIT_FOR_CALL * 10, false));
        presence.saw(&ended(), start);
        assert!(presence.connected(start + WAIT_FOR_CALL * 10, false));
    }

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

    #[tokio::test(flavor = "multi_thread")]
    async fn a_start_registers_the_hashes_and_the_silent_slots() {
        let (core, router) = (core().await, Arc::new(Router::default()));
        let _stop = registering(&core, &router);
        until("registered", || router.count() == 1).await;
        let expected = (core.route_capability_hashes().await.unwrap(), core.silent_slots().await.unwrap());
        assert_eq!(router.last(), Some(expected));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn opening_or_leaving_a_session_registers_again() {
        let (core, router) = (core().await, Arc::new(Router::default()));
        let _stop = registering(&core, &router);
        until("registered", || router.count() == 1).await;

        let session = core.open_session("123456").await.unwrap().unwrap();
        let slot = core.store().session_slot(&session).await.unwrap().unwrap();
        until("registered with the session open", || router.last().is_some_and(|(_, mask)| mask & 1 << slot == 0) && router.count() >= 2).await;
        assert_eq!(router.last().unwrap().1, core.silent_slots().await.unwrap());

        let before = core.route_capability_hashes().await.unwrap();
        core.close_session(&session).await.unwrap();
        let after = core.route_capability_hashes().await.unwrap();
        assert_ne!(after, before);
        until("registered with the new hash", || router.last().is_some_and(|(hashes, _)| hashes == after)).await;
        assert_eq!(router.last().unwrap().1, core.silent_slots().await.unwrap());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_registration_that_fails_is_tried_again_with_the_newest_state() {
        let (core, router) = (core().await, Arc::new(Router::default()));
        let _stop = registering(&core, &router);
        until("registered", || router.count() == 1).await;

        router.failing.store(2, std::sync::atomic::Ordering::SeqCst);
        let first = core.open_session("111111").await.unwrap().unwrap();
        until("a try failed", || router.tries.load(std::sync::atomic::Ordering::SeqCst) >= 2).await;
        let second = core.open_session("222222").await.unwrap().unwrap();
        core.router_reachable();
        let wanted = core.silent_slots().await.unwrap();
        until("the router has the newest state", || router.last().is_some_and(|(_, mask)| mask == wanted) && router.count() >= 2).await;
        for session in [first, second] {
            let slot = core.store().session_slot(&session).await.unwrap().unwrap();
            assert_eq!(router.last().unwrap().1 & 1 << slot, 0, "both open, so neither silent");
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_failed_registration_is_retried_on_its_own() {
        let (core, router) = (core().await, Arc::new(Router::default()));
        router.failing.store(1, std::sync::atomic::Ordering::SeqCst);
        let _stop = registering(&core, &router);
        tokio::time::sleep(REGISTER_RETRY + Duration::from_millis(500)).await;
        assert_eq!(router.count(), 1, "the second try went through");
    }

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

    #[tokio::test(flavor = "multi_thread")]
    async fn a_stopped_client_lets_go_of_the_router_and_the_database() {
        let store = Store::open_in_memory().await.unwrap();
        let online = start(store, [7; 32], "http://127.0.0.1:9", SessionConfig::default()).await.unwrap();
        assert!(online.core.store().contacts().await.is_ok());

        online.shutdown().await;
        assert!(online.router.is_closed());
        assert!(online.core.store().contacts().await.is_err(), "the database is closed");
        let stopped = tokio::time::timeout(Duration::from_secs(1), online.stopped()).await;
        assert!(stopped.is_ok(), "whoever waits for the stop hears it");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_stopped_client_leaves_no_task_behind() {
        let store = Store::open_in_memory().await.unwrap();
        let online = start(store, [7; 32], "http://127.0.0.1:9", SessionConfig::default()).await.unwrap();
        assert!(Arc::strong_count(&online.lifecycle) > 1, "the watcher and the presence loop run");

        online.shutdown().await;
        until("every task let go", || Arc::strong_count(&online.lifecycle) == 1 && Arc::strong_count(&online.router) == 3).await;
    }
}
