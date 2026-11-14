//! Signalling is Olm-encrypted, which binds each DTLS fingerprint to the contact so the router can't sit in the middle.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Result};
use async_trait::async_trait;
use ft_protocol::{Body, Signal, SignalKind, PROTOCOL_VERSION};
use ft_push::{RouterClient, RouterEvent, Signalled, TurnGrant};
use ft_webrtc::{Inbox, Role, Session, SessionConfig, Signal as Description, TurnServer};
use tokio::sync::{mpsc, watch, Mutex};

use crate::timings::CallStage;
use crate::{Core, Event, Peer, Transport};

const RESUME_QUIET: Duration = Duration::from_secs(2);

pub const CONNECT_WAIT: Duration = Duration::from_secs(12);

// Routers from 0.4.0 keep a signal this long for an offline recipient and hand it over after its next welcome
const RETAINED_FOR: Duration = Duration::from_secs(55);

const ANSWER_GRACE: Duration = Duration::from_secs(5);

// Routers before 0.4 lose an offer nobody got, keep it for the next attempt
const SPARE_FOR: Duration = Duration::from_secs(20);

// TURN_TTL in ft-router
pub const TURN_LIFE: Duration = Duration::from_secs(10 * 60);

pub const TURN_RENEW_AFTER: Duration = Duration::from_secs(5 * 60);
pub const TURN_LOOK: Duration = Duration::from_secs(60);

// A TURN user this old could expire before gathering is done
pub const TURN_STALE_AFTER: Duration = Duration::from_secs(8 * 60);

pub const TURN_WAIT: Duration = Duration::from_secs(2);

pub const TURN_ASK_AGAIN: Duration = Duration::from_secs(30);

#[async_trait]
pub trait Relay: Send + Sync {
    async fn signal(&self, to: &str, capability: &[u8; 32], bytes: Vec<u8>) -> Result<bool>;
    async fn signal_call(&self, to: &str, capability: &[u8; 32], bytes: Vec<u8>) -> Result<bool> {
        self.signal(to, capability, bytes).await
    }
    async fn signal_as(&self, to: &str, capability: &[u8; 32], bytes: Vec<u8>, call: bool) -> Result<Signalled> {
        let delivered = if call { self.signal_call(to, capability, bytes).await? } else { self.signal(to, capability, bytes).await? };
        Ok(if delivered { Signalled::Delivered } else { Signalled::NotConnected })
    }
    async fn signal_without_waking(&self, to: &str, capability: &[u8; 32], bytes: Vec<u8>) -> Result<Signalled> {
        self.signal_as(to, capability, bytes, false).await
    }
    async fn deposit(&self, to: &str, capability: &[u8; 32], blob: Vec<u8>) -> Result<()>;
    async fn collect(&self) -> Result<Vec<(String, Vec<u8>)>>;
    async fn acknowledge(&self, id: &str) -> Result<()>;
    async fn turn_credentials(&self) -> Result<TurnGrant> {
        bail!("this relay hands out no TURN users")
    }
}

#[async_trait]
impl Relay for RouterClient {
    async fn signal(&self, to: &str, capability: &[u8; 32], bytes: Vec<u8>) -> Result<bool> {
        Ok(RouterClient::signal(self, to, capability, bytes).await?.delivered())
    }

    async fn signal_call(&self, to: &str, capability: &[u8; 32], bytes: Vec<u8>) -> Result<bool> {
        Ok(RouterClient::signal_call(self, to, capability, bytes).await?.delivered())
    }

    async fn signal_as(&self, to: &str, capability: &[u8; 32], bytes: Vec<u8>, call: bool) -> Result<Signalled> {
        if call {
            RouterClient::signal_call(self, to, capability, bytes).await
        } else {
            RouterClient::signal(self, to, capability, bytes).await
        }
    }

    async fn signal_without_waking(&self, to: &str, capability: &[u8; 32], bytes: Vec<u8>) -> Result<Signalled> {
        RouterClient::signal_without_waking(self, to, capability, bytes).await
    }

    async fn deposit(&self, to: &str, capability: &[u8; 32], blob: Vec<u8>) -> Result<()> {
        RouterClient::deposit(self, to, capability, blob).await
    }

    async fn collect(&self) -> Result<Vec<(String, Vec<u8>)>> {
        Ok(RouterClient::collect(self).await?.into_iter().map(|mail| (mail.id, mail.blob)).collect())
    }

    async fn acknowledge(&self, id: &str) -> Result<()> {
        RouterClient::acknowledge(self, id).await
    }

    async fn turn_credentials(&self) -> Result<TurnGrant> {
        RouterClient::turn_credentials(self).await
    }
}

struct Link {
    id: u64,
    session: Session,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    Call,
    // files, call answers and ends, plugins, none of which may go to the mailbox
    Direct,
    Fallback,
    // a reconnection with nothing to deliver, which must not wake the other phone
    IfAwake,
}

enum Attempt {
    Done(Option<Session>),
    Waiting(Session, Duration),
}

impl Attempt {
    async fn outcome(self) -> Option<Session> {
        match self {
            Attempt::Done(session) => session,
            Attempt::Waiting(session, limit) => wait_open(&session, limit).await.then_some(session),
        }
    }
}

const GATE_LOOK: Duration = Duration::from_millis(50);

struct Spare {
    id: u64,
    session: Session,
    inbox: Inbox,
    sdp: String,
    made: Instant,
}

#[derive(Clone)]
struct Retained {
    id: String,
    session: Session,
    window: Instant,
    until: Instant,
    call: bool,
    wakes: bool,
}

struct Ice {
    stun: Vec<String>,
    turn: Option<TurnGrant>,
    // Wall clock, the monotonic one stops while the phone sleeps
    turn_at: SystemTime,
}

impl Default for Ice {
    fn default() -> Self {
        Self { stun: Vec::new(), turn: None, turn_at: SystemTime::now() }
    }
}

impl Ice {
    fn turn_older_than(&self, age: Duration) -> bool {
        self.turn.is_some() && !SystemTime::now().duration_since(self.turn_at).is_ok_and(|since| since < age)
    }
}

struct Renewal {
    began: tokio::time::Instant,
    over: watch::Receiver<bool>,
}

pub struct Network {
    relay: Arc<dyn Relay>,
    base: SessionConfig,
    ice: std::sync::Mutex<Ice>,
    renewal: std::sync::Mutex<Option<Renewal>>,
    core: OnceLock<Weak<Core>>,
    this: OnceLock<Weak<Network>>,
    links: Mutex<HashMap<String, Link>>,
    pending: Mutex<HashMap<String, Session>>,
    retained: Mutex<HashMap<String, Retained>>,
    spare: Mutex<HashMap<String, Spare>>,
    gates: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    // so no blob is processed twice
    collecting: Mutex<()>,
    next_link: AtomicU64,
    redial: std::sync::Mutex<Redial>,
}

#[derive(Default)]
struct Redial {
    router_up: bool,
    contacts: Vec<String>,
}

impl Network {
    pub fn new(relay: Arc<dyn Relay>, base: SessionConfig) -> Arc<Self> {
        let network = Arc::new(Self {
            relay,
            base,
            ice: std::sync::Mutex::default(),
            renewal: std::sync::Mutex::default(),
            core: OnceLock::new(),
            this: OnceLock::new(),
            links: Mutex::default(),
            pending: Mutex::default(),
            retained: Mutex::default(),
            spare: Mutex::default(),
            gates: Mutex::default(),
            collecting: Mutex::default(),
            next_link: AtomicU64::new(0),
            redial: std::sync::Mutex::default(),
        });
        let _ = network.this.set(Arc::downgrade(&network));
        network
    }

    pub fn attach(&self, core: &Arc<Core>) {
        let _ = self.core.set(Arc::downgrade(core));
    }

    pub fn listen(self: &Arc<Self>, mut events: mpsc::Receiver<RouterEvent>) {
        let network = self.clone();
        tokio::spawn(async move {
            let mut look = tokio::time::interval(TURN_LOOK);
            look.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    event = events.recv() => match event {
                        Some(event) => network.handle(event).await,
                        None => break,
                    },
                    _ = look.tick() => network.keep_turn_fresh(),
                }
            }
        });
    }

    fn keep_turn_fresh(&self) {
        let up = self.redial.lock().expect("redial poisoned").router_up;
        if up && self.turn_older_than(TURN_RENEW_AFTER) {
            let _ = self.renew_turn();
        }
    }

    // Work runs in the background so an answer never gets stuck behind mail that is waiting for it
    pub async fn handle(&self, event: RouterEvent) {
        let Some(network) = self.this.get().and_then(Weak::upgrade) else { return };
        match event {
            RouterEvent::Connected { stun, turn } => {
                self.welcome(stun, turn);
                let redial = {
                    let mut redial = self.redial.lock().expect("redial poisoned");
                    redial.router_up = true;
                    std::mem::take(&mut redial.contacts)
                };
                self.dial(redial);
                tokio::spawn(async move {
                    if let Ok(core) = network.core() {
                        core.router_reachable();
                    }
                    network.collect_mail().await;
                    if let Ok(core) = network.core() {
                        let _ = core.retry_now().await;
                    }
                });
            }
            RouterEvent::Mail => {
                tokio::spawn(async move { network.collect_mail().await });
            }
            RouterEvent::Signal(bytes) => {
                tokio::spawn(async move {
                    let _ = network.on_signal(&bytes).await;
                });
            }
            RouterEvent::Disconnected => self.redial.lock().expect("redial poisoned").router_up = false,
        }
    }

    fn core(&self) -> Result<Arc<Core>> {
        self.core.get().and_then(Weak::upgrade).ok_or_else(|| anyhow!("the core is gone"))
    }

    fn welcome(&self, stun: Vec<String>, turn: Option<TurnGrant>) {
        *self.ice.lock().expect("ice poisoned") = Ice { stun, turn, turn_at: SystemTime::now() };
    }

    pub fn ice(&self) -> (Vec<String>, Option<TurnGrant>) {
        let ice = self.ice.lock().expect("ice poisoned");
        (ice.stun.clone(), ice.turn.clone())
    }

    // The welcome's TURN user lives ten minutes, a device connected longer would gather no relay
    pub fn set_turn(&self, turn: TurnGrant) {
        let mut ice = self.ice.lock().expect("ice poisoned");
        ice.turn = Some(turn);
        ice.turn_at = SystemTime::now();
    }

    pub async fn fresh_ice(&self) -> (Vec<String>, Option<TurnGrant>) {
        self.fresh_turn().await;
        self.ice()
    }

    #[doc(hidden)]
    pub fn age_turn(&self, by: Duration) {
        let mut ice = self.ice.lock().expect("ice poisoned");
        ice.turn_at = ice.turn_at.checked_sub(by).unwrap_or(UNIX_EPOCH);
    }

    fn turn_older_than(&self, age: Duration) -> bool {
        self.ice.lock().expect("ice poisoned").turn_older_than(age)
    }

    async fn fresh_turn(&self) {
        if !self.turn_older_than(TURN_STALE_AFTER) {
            return;
        }
        let Some((began, mut over)) = self.renew_turn() else { return };
        let _ = tokio::time::timeout_at(began + TURN_WAIT, over.wait_for(|over| *over)).await;
    }

    fn renew_turn(&self) -> Option<(tokio::time::Instant, watch::Receiver<bool>)> {
        let network = self.this.get().and_then(Weak::upgrade)?;
        let mut renewal = self.renewal.lock().expect("renewal poisoned");
        if let Some(last) = renewal.as_ref().filter(|last| last.began.elapsed() < TURN_ASK_AGAIN) {
            return Some((last.began, last.over.clone()));
        }
        let (done, over) = watch::channel(false);
        let began = tokio::time::Instant::now();
        *renewal = Some(Renewal { began, over: over.clone() });
        tokio::spawn(async move {
            if let Ok(turn) = network.relay.turn_credentials().await {
                network.set_turn(turn);
            }
            done.send_replace(true);
        });
        Some((began, over))
    }

    fn session_config(&self) -> SessionConfig {
        let (stun, turn) = self.ice();
        ice_config(&self.base.clone().listening_now(), stun, turn)
    }

    async fn collect_mail(&self) {
        let _one_at_a_time = self.collecting.lock().await;
        let (Ok(core), Ok(mail)) = (self.core(), self.relay.collect().await) else { return };
        for (id, blob) in mail {
            // An unreadable blob will never become readable, drop it too
            let _ = core.receive(&blob).await;
            let _ = self.relay.acknowledge(&id).await;
        }
    }

    async fn open_link(&self, contact: &str) -> Option<Session> {
        let mut links = self.links.lock().await;
        match links.get(contact) {
            Some(link) if link.session.is_open() => Some(link.session.clone()),
            Some(_) => {
                links.remove(contact);
                None
            }
            None => None,
        }
    }

    pub async fn is_connected(&self, contact: &str) -> bool {
        self.links.lock().await.get(contact).is_some_and(|link| link.session.is_open())
    }

    pub async fn connected(&self) -> Vec<String> {
        let links = self.links.lock().await;
        let mut contacts: Vec<String> =
            links.iter().filter(|(_, link)| link.session.is_open()).map(|(contact, _)| contact.clone()).collect();
        contacts.sort();
        contacts
    }

    pub async fn disconnect(&self, contact: &str) {
        let retained = self.retained.lock().await.remove(contact);
        if let Some(retained) = retained {
            let _ = retained.session.close().await;
        }
        let spare = self.spare.lock().await.remove(contact);
        if let Some(spare) = spare {
            let _ = spare.session.close().await;
        }
        let link = self.links.lock().await.remove(contact);
        if let Some(link) = link {
            let _ = link.session.close().await;
            self.announce(contact);
        }
    }

    #[doc(hidden)]
    pub async fn vanish(&self, contact: &str) {
        let link = self.links.lock().await.remove(contact);
        if let Some(link) = link {
            link.session.vanish().await;
        }
    }

    #[doc(hidden)]
    pub async fn lose(&self, contact: &str) {
        if let Some(link) = self.links.lock().await.get(contact) {
            link.session.lose();
        }
    }

    #[doc(hidden)]
    pub async fn offers_waiting(&self) -> usize {
        self.pending.lock().await.len()
    }

    pub(crate) fn redial(&self, contacts: Vec<String>) {
        let now = {
            let mut redial = self.redial.lock().expect("redial poisoned");
            if redial.router_up {
                contacts
            } else {
                redial.contacts.extend(contacts);
                redial.contacts.sort();
                redial.contacts.dedup();
                Vec::new()
            }
        };
        self.dial(now);
    }

    // After a network change direct connections can look open for seconds while losing what is sent, so drop them all
    pub(crate) async fn moved(&self) {
        let links: Vec<(String, Link)> = self.links.lock().await.drain().collect();
        {
            let mut redial = self.redial.lock().expect("redial poisoned");
            redial.router_up = false;
            redial.contacts.extend(links.iter().map(|(contact, _)| contact.clone()));
            redial.contacts.sort();
            redial.contacts.dedup();
        }
        let mut gone: Vec<Session> = self.retained.lock().await.drain().map(|(_, retained)| retained.session).collect();
        gone.extend(self.spare.lock().await.drain().map(|(_, spare)| spare.session));
        gone.extend(self.pending.lock().await.drain().map(|(_, session)| session));
        for (contact, link) in links {
            self.announce(&contact);
            gone.push(link.session);
        }
        // Closing waits for the other side, over a path that may be gone
        tokio::spawn(async move {
            for session in gone {
                let _ = session.close().await;
            }
        });
    }

    // Recheck blocked, stranger and left session, any of them may have changed since
    fn dial(&self, contacts: Vec<String>) {
        let Some(network) = self.this.get().and_then(Weak::upgrade) else { return };
        for contact in contacts {
            let network = network.clone();
            tokio::spawn(async move {
                let Ok(core) = network.core() else { return };
                let Ok(Some(stored)) = core.store().contact(&contact).await else { return };
                if stored.blocked || core.silent(&stored) {
                    return;
                }
                let reach = if core.something_waits_for(&contact).await.unwrap_or(true) { Reach::Fallback } else { Reach::IfAwake };
                if let Ok(peer) = core.peer(&contact).await {
                    if network.open_link(&contact).await.is_none() {
                        let _ = network.reach_as(&peer, reach).await;
                    }
                }
            });
        }
    }

    pub async fn close(&self) {
        let mut contacts: Vec<String> = self.links.lock().await.keys().cloned().collect();
        contacts.extend(self.retained.lock().await.keys().cloned());
        contacts.extend(self.spare.lock().await.keys().cloned());
        contacts.sort();
        contacts.dedup();
        for contact in contacts {
            self.disconnect(&contact).await;
        }
        let pending: Vec<Session> = self.pending.lock().await.drain().map(|(_, session)| session).collect();
        for session in pending {
            let _ = session.close().await;
        }
    }

    fn announce(&self, contact: &str) {
        if let Ok(core) = self.core() {
            let _ = core.events.send(Event::ConnectionChanged { contact: contact.to_owned() });
        }
    }

    async fn gate(&self, contact: &str) -> Arc<Mutex<()>> {
        self.gates.lock().await.entry(contact.to_owned()).or_default().clone()
    }

    // A call never waits on an offer that wasn't sent as a call. An iPhone woken for that only shows
    // a notice without starting the app, so the call would never ring.
    // Nor does anything that has to wake the other phone wait on an offer that woke nobody.
    async fn connect(&self, peer: &Peer, reach: Reach) -> Result<Attempt> {
        let call = reach == Reach::Call;
        let wake = reach != Reach::IfAwake;
        if let Some(retained) = self.retained_offer(&peer.device_id).await {
            let weaker = (call && !retained.call) || (wake && !retained.wakes);
            if weaker && !retained.session.is_open() {
                self.drop_retained(&peer.device_id, &retained).await;
            } else {
                let limit = match reach {
                    Reach::Call => CONNECT_WAIT,
                    Reach::Direct => retained.window.saturating_duration_since(Instant::now()),
                    Reach::Fallback | Reach::IfAwake => Duration::ZERO,
                };
                return Ok(Attempt::Waiting(retained.session, limit));
            }
        }
        let core = self.core()?;
        let (session, inbox, sdp) = match self.take_spare(&peer.device_id).await {
            Some(spare) => (spare.session, spare.inbox, spare.sdp),
            None => {
                self.fresh_turn().await;
                let (signals, mut descriptions) = mpsc::channel(8);
                let (session, inbox) = Session::start(self.session_config(), Role::Caller, signals).await?;
                session.invite().await?;
                let Some(Description::Sdp(sdp)) = descriptions.recv().await else {
                    bail!("no offer was produced");
                };
                (session, inbox, sdp)
            }
        };
        if call {
            core.mark_link_gathering(&session.gathering());
        }

        let contact = core.store().contact(&peer.device_id).await?;
        let introduced = contact.as_ref().is_some_and(|contact| contact.introduced);
        let hidden = contact.as_ref().and_then(|contact| contact.session.clone());
        let card = if introduced { None } else { Some(core.my_card_in(hidden.as_deref()).await?.encode()) };
        let via = Some(peer.capability.hash().to_vec());
        let sealed = core.seal_signal(&peer.device_id, Body::Offer { sdp: sdp.clone(), card, via }).await?;
        let session_id = uuid_like();
        let signal = Signal {
            version: PROTOCOL_VERSION,
            kind: SignalKind::Offer,
            session: session_id.clone(),
            from: core.device_id().to_string(),
            to: peer.device_id.clone(),
            sealed,
        };

        if call {
            core.mark_call_stage(CallStage::LinkOffered);
        }
        self.pending.lock().await.insert(session_id.clone(), session.clone());
        // Sealed so only the recipient learns who sent it
        let wrapped = core.wrap_for(&peer.device_id, signal.encode()).await?;
        let signalled = if wake {
            self.relay.signal_as(&peer.device_id, peer.capability.as_bytes(), wrapped, call).await
        } else {
            self.relay.signal_without_waking(&peer.device_id, peer.capability.as_bytes(), wrapped).await
        };
        if matches!(signalled, Ok(Signalled::Retained)) {
            if call {
                core.mark_call_stage(CallStage::LinkOfferRetained);
            }
            self.keep_retained(&peer.device_id, session_id, session.clone(), inbox, Instant::now(), reach).await;
            let limit = if matches!(reach, Reach::Fallback | Reach::IfAwake) { Duration::ZERO } else { CONNECT_WAIT };
            return Ok(Attempt::Waiting(session, limit));
        }
        let opened = match signalled {
            Ok(Signalled::Delivered) => {
                if call {
                    core.mark_call_stage(CallStage::LinkOfferSent);
                }
                wait_open(&session, CONNECT_WAIT).await
            }
            _ => false,
        };
        self.pending.lock().await.remove(&session_id);
        if matches!(signalled, Ok(Signalled::NotConnected)) {
            self.keep_spare(&peer.device_id, session, inbox, sdp).await;
            return Ok(Attempt::Done(None));
        }

        if opened {
            self.adopt(&peer.device_id, session.clone(), inbox).await;
            Ok(Attempt::Done(Some(session)))
        } else {
            let _ = session.close().await;
            Ok(Attempt::Done(None))
        }
    }

    async fn take_spare(&self, contact: &str) -> Option<Spare> {
        let spare = self.spare.lock().await.remove(contact)?;
        if spare.made.elapsed() < SPARE_FOR {
            return Some(spare);
        }
        let _ = spare.session.close().await;
        None
    }

    async fn keep_spare(&self, contact: &str, session: Session, inbox: Inbox, sdp: String) {
        let id = self.next_link.fetch_add(1, Ordering::Relaxed);
        let made = session.gathering().started.unwrap_or_else(Instant::now);
        let older = self.spare.lock().await.insert(contact.to_owned(), Spare { id, session, inbox, sdp, made });
        if let Some(older) = older {
            let _ = older.session.close().await;
        }
        let network = self.this.get().cloned().unwrap_or_default();
        let contact = contact.to_owned();
        tokio::spawn(async move {
            tokio::time::sleep(SPARE_FOR.saturating_sub(made.elapsed())).await;
            let Some(network) = network.upgrade() else { return };
            let stale = {
                let mut spare = network.spare.lock().await;
                if spare.get(&contact).is_some_and(|spare| spare.id == id) { spare.remove(&contact) } else { None }
            };
            if let Some(stale) = stale {
                let _ = stale.session.close().await;
            }
        });
    }

    async fn retained_offer(&self, contact: &str) -> Option<Retained> {
        let retained = self.retained.lock().await;
        retained.get(contact).filter(|retained| retained.until > Instant::now()).cloned()
    }

    async fn keep_retained(&self, contact: &str, id: String, session: Session, inbox: Inbox, signalled_at: Instant, reach: Reach) {
        let until = signalled_at + RETAINED_FOR;
        let (call, wakes) = (reach == Reach::Call, reach != Reach::IfAwake);
        let retained = Retained { id: id.clone(), session: session.clone(), window: signalled_at + CONNECT_WAIT, until, call, wakes };
        self.retained.lock().await.insert(contact.to_owned(), retained);
        let network = self.this.get().cloned().unwrap_or_default();
        let contact = contact.to_owned();
        tokio::spawn(async move {
            let opened = wait_open(&session, (until + ANSWER_GRACE).saturating_duration_since(Instant::now())).await;
            let Some(network) = network.upgrade() else {
                let _ = session.close().await;
                return;
            };
            network.pending.lock().await.remove(&id);
            {
                let mut retained = network.retained.lock().await;
                if retained.get(&contact).is_some_and(|retained| retained.id == id) {
                    retained.remove(&contact);
                }
            }
            if opened {
                network.adopt(&contact, session, inbox).await;
            } else {
                let _ = session.close().await;
            }
        });
    }

    async fn drop_retained(&self, contact: &str, dropped: &Retained) {
        {
            let mut retained = self.retained.lock().await;
            if retained.get(contact).is_some_and(|retained| retained.id == dropped.id) {
                retained.remove(contact);
            }
        }
        self.pending.lock().await.remove(&dropped.id);
        let _ = dropped.session.close().await;
    }

    async fn on_signal(&self, bytes: &[u8]) -> Result<()> {
        let core = self.core()?;
        let bytes = core.unwrap(bytes).await?;
        let signal = Signal::decode(&bytes)?;
        let Some((from, body)) = core.open_signal(&signal.sealed).await? else {
            // Dropped unread, but a push may already have the system call screen ringing for it
            if signal.kind == SignalKind::Offer {
                core.refused_quietly();
            }
            return Ok(());
        };
        if from != signal.from {
            bail!("the signal is not from who it claims");
        }
        match (signal.kind, body) {
            (SignalKind::Offer, Body::Offer { sdp, .. }) => {
                core.mark_call_stage(CallStage::LinkOfferReceived);
                self.answer(&core, &from, signal.session, sdp).await
            }
            (SignalKind::Answer, Body::Answer { sdp }) => {
                let pending = self.pending.lock().await.get(&signal.session).cloned();
                match pending {
                    Some(session) => {
                        core.mark_call_stage(CallStage::LinkAnswerReceived);
                        session.handle_signal(Description::Sdp(sdp)).await
                    }
                    None => Ok(()),
                }
            }
            _ => Ok(()),
        }
    }

    async fn answer(&self, core: &Core, from: &str, session_id: String, offer: String) -> Result<()> {
        self.fresh_turn().await;
        let (signals, mut descriptions) = mpsc::channel(8);
        let (session, inbox) = Session::start(self.session_config(), Role::Callee, signals).await?;
        session.handle_signal(Description::Sdp(offer)).await?;
        let Some(Description::Sdp(sdp)) = descriptions.recv().await else {
            bail!("no answer was produced");
        };
        core.mark_link_gathering(&session.gathering());
        let sealed = core.seal_signal(from, Body::Answer { sdp }).await?;
        let peer = core.peer(from).await?;
        let signal = Signal {
            version: PROTOCOL_VERSION,
            kind: SignalKind::Answer,
            session: session_id,
            from: core.device_id().to_string(),
            to: from.to_owned(),
            sealed,
        };
        let wrapped = core.wrap_for(from, signal.encode()).await?;
        self.relay.signal(from, peer.capability.as_bytes(), wrapped).await?;
        core.mark_call_stage(CallStage::LinkAnswered);

        let network = self.this.get().and_then(Weak::upgrade).ok_or_else(|| anyhow!("the network is gone"))?;
        let contact = from.to_owned();
        tokio::spawn(async move {
            if tokio::time::timeout(CONNECT_WAIT, session.wait_open()).await.is_ok_and(|open| open.is_ok()) {
                network.adopt(&contact, session, inbox).await;
            } else {
                let _ = session.close().await;
            }
        });
        Ok(())
    }

    // A killed app never closes its side, so the UI also hears about stalls
    async fn adopt(&self, contact: &str, session: Session, mut inbox: Inbox) {
        let id = self.next_link.fetch_add(1, Ordering::Relaxed);
        self.links.lock().await.insert(contact.to_owned(), Link { id, session: session.clone() });
        self.announce(contact);
        if let Ok(core) = self.core() {
            core.mark_call_stage(CallStage::LinkOpened);
        }
        if let Ok(core) = self.core() {
            core.link_opened_in_call(contact);
            let from = contact.to_owned();
            tokio::spawn(async move {
                let _ = core.retry_contact_now(&from).await;
                let _ = core.resume_files_from(&from, RESUME_QUIET).await;
            });
        }

        let (Some(core), Some(network)) = (self.core.get().cloned(), self.this.get().cloned()) else { return };
        let contact = contact.to_owned();
        let mut open = session.open_changes();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    bytes = inbox.next() => {
                        let (Some(bytes), Some(core)) = (bytes, core.upgrade()) else { break };
                        let _ = core.receive(&bytes).await;
                    }
                    Ok(()) = open.changed() => {
                        if let Some(network) = network.upgrade() {
                            network.announce(&contact);
                        }
                    }
                }
            }
            if let Some(network) = network.upgrade() {
                let removed = {
                    let mut links = network.links.lock().await;
                    links.get(&contact).is_some_and(|link| link.id == id) && links.remove(&contact).is_some()
                };
                if removed {
                    network.announce(&contact);
                }
            }
            let _ = session.close().await;
        });
    }
}

#[async_trait]
impl Transport for Network {
    fn media_config(&self) -> ft_media::MediaConfig {
        media_config(&self.session_config())
    }

    async fn fresh_media_config(&self) -> ft_media::MediaConfig {
        self.fresh_turn().await;
        self.media_config()
    }

    async fn send_direct(&self, to: &Peer, bytes: Vec<u8>) -> Result<bool> {
        self.send_direct_as(to, bytes, Reach::Direct).await
    }

    async fn try_direct(&self, to: &Peer, bytes: Vec<u8>) -> Result<bool> {
        self.send_direct_as(to, bytes, Reach::Fallback).await
    }

    async fn send_direct_call(&self, to: &Peer, bytes: Vec<u8>) -> Result<bool> {
        self.send_direct_as(to, bytes, Reach::Call).await
    }

    async fn reach(&self, to: &Peer, fallback: bool) -> Result<bool> {
        if self.open_link(&to.device_id).await.is_some() {
            return Ok(true);
        }
        Ok(self.reach_as(to, if fallback { Reach::Fallback } else { Reach::Direct }).await?.is_some())
    }

    async fn send_open(&self, to: &Peer, bytes: Vec<u8>) -> Result<bool> {
        match self.open_link(&to.device_id).await {
            Some(session) => Ok(session.send_bytes(&bytes).await.is_ok()),
            None => Ok(false),
        }
    }

    async fn open_direct_call(&self, to: &Peer) -> Result<bool> {
        if self.open_link(&to.device_id).await.is_some() {
            return Ok(true);
        }
        let gate = self.gate(&to.device_id).await;
        let one_at_a_time = gate.lock().await;
        if self.open_link(&to.device_id).await.is_some() {
            return Ok(true);
        }
        let attempt = self.connect(to, Reach::Call).await?;
        drop(one_at_a_time);
        Ok(attempt.outcome().await.is_some())
    }

    async fn send_mailbox(&self, to: &Peer, bytes: Vec<u8>) -> Result<()> {
        self.relay.deposit(&to.device_id, to.capability.as_bytes(), bytes).await
    }

    async fn disconnect(&self, device_id: &str) {
        Network::disconnect(self, device_id).await;
    }

    async fn call_over(&self, device_id: &str) {
        if let Some(retained) = self.retained.lock().await.get_mut(device_id) {
            retained.call = false;
        }
    }
}

impl Network {
    async fn send_direct_as(&self, to: &Peer, bytes: Vec<u8>, reach: Reach) -> Result<bool> {
        if let Some(session) = self.open_link(&to.device_id).await {
            if session.send_bytes(&bytes).await.is_ok() {
                return Ok(true);
            }
        }
        match self.reach_as(to, reach).await? {
            Some(session) => Ok(session.send_bytes(&bytes).await.is_ok()),
            None => Ok(false),
        }
    }

    async fn reach_as(&self, to: &Peer, reach: Reach) -> Result<Option<Session>> {
        let gate = self.gate(&to.device_id).await;
        let one_at_a_time = if matches!(reach, Reach::Fallback | Reach::IfAwake) {
            loop {
                if let Ok(guard) = tokio::time::timeout(GATE_LOOK, gate.clone().lock_owned()).await {
                    break guard;
                }
                if self.open_link(&to.device_id).await.is_none() && self.retained_offer(&to.device_id).await.is_some() {
                    return Ok(None);
                }
            }
        } else {
            gate.clone().lock_owned().await
        };
        // another send may have connected while this one waited
        let attempt = match self.open_link(&to.device_id).await {
            Some(session) => Attempt::Done(Some(session)),
            None => self.connect(to, reach).await?,
        };
        drop(one_at_a_time);
        Ok(attempt.outcome().await)
    }
}

async fn wait_open(session: &Session, limit: Duration) -> bool {
    if session.is_open() {
        return true;
    }
    tokio::time::timeout(limit, session.wait_open()).await.is_ok_and(|open| open.is_ok())
}

pub fn ice_config(base: &SessionConfig, stun: Vec<String>, turn: Option<TurnGrant>) -> SessionConfig {
    let mut config = base.clone();
    if !stun.is_empty() {
        config.stun_servers = stun;
    }
    if let Some(grant) = turn {
        config.turn_servers = grant
            .urls
            .into_iter()
            .map(|url| TurnServer { url, username: grant.username.clone(), credential: grant.credential.clone() })
            .collect();
    }
    config
}

pub fn media_config(session: &SessionConfig) -> ft_media::MediaConfig {
    let turn = session.turn_servers.first().map(|first| ft_media::TurnRelay {
        urls: session.turn_servers.iter().map(|server| server.url.clone()).collect(),
        username: first.username.clone(),
        credential: first.credential.clone(),
    });
    ft_media::MediaConfig {
        stun: session.stun_servers.clone(),
        turn,
        bind: session.bind.clone(),
        routing: ft_media::CallRouting::default(),
        gather_timeout: session.gather_timeout,
    }
}

fn uuid_like() -> String {
    ft_protocol::MessageId::new().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_router_supplies_the_stun_and_turn_servers() {
        let base = SessionConfig { relay_only: true, ..SessionConfig::offline() };
        let grant = TurnGrant { urls: vec!["turn:t:3478".to_owned()], username: "u".to_owned(), credential: "c".to_owned() };
        let config = ice_config(&base, vec!["stun:t:3478".to_owned()], Some(grant));
        assert_eq!(config.stun_servers, ["stun:t:3478"]);
        assert_eq!(config.turn_servers[0].url, "turn:t:3478");
        assert_eq!(config.turn_servers[0].username, "u");
        assert!(config.relay_only, "the listening setup is kept");
        assert_eq!(config.bind, base.bind);
    }

    #[test]
    fn a_native_call_uses_the_same_servers_and_addresses() {
        let base = SessionConfig::offline();
        let grant = TurnGrant { urls: vec!["turn:t:3478".to_owned(), "turns:t:5349".to_owned()], username: "u".to_owned(), credential: "c".to_owned() };
        let media = media_config(&ice_config(&base, vec!["stun:t:3478".to_owned()], Some(grant)));
        assert_eq!(media.stun, ["stun:t:3478"]);
        let turn = media.turn.expect("a relay");
        assert_eq!(turn.urls, ["turn:t:3478", "turns:t:5349"]);
        assert_eq!((turn.username.as_str(), turn.credential.as_str()), ("u", "c"));
        assert_eq!(media.bind, base.bind);
        assert_eq!(media.gather_timeout, base.gather_timeout);
        assert!(media_config(&base).turn.is_none());
    }

    #[test]
    fn without_a_welcome_the_base_configuration_stays() {
        let base = SessionConfig::with_stun(["stun:fallback:19302".to_owned()]);
        assert_eq!(ice_config(&base, vec![], None).stun_servers, ["stun:fallback:19302"]);
    }

    struct NoRouter;

    #[async_trait]
    impl Relay for NoRouter {
        async fn signal(&self, _: &str, _: &[u8; 32], _: Vec<u8>) -> Result<bool> {
            Ok(false)
        }

        async fn deposit(&self, _: &str, _: &[u8; 32], _: Vec<u8>) -> Result<()> {
            Ok(())
        }

        async fn collect(&self) -> Result<Vec<(String, Vec<u8>)>> {
            Ok(Vec::new())
        }

        async fn acknowledge(&self, _: &str) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn fresh_turn_credentials_replace_the_welcome_s_for_what_comes_next() {
        let network = Network::new(Arc::new(NoRouter), SessionConfig::offline());
        let welcome = TurnGrant { urls: vec!["turn:t:3478".to_owned()], username: "old".to_owned(), credential: "c1".to_owned() };
        network.welcome(vec!["stun:t:3478".to_owned()], Some(welcome));
        let fresh = TurnGrant { urls: vec!["turn:t:3478".to_owned()], username: "new".to_owned(), credential: "c2".to_owned() };
        network.set_turn(fresh);
        assert_eq!(network.session_config().turn_servers[0].username, "new");
        assert_eq!(network.media_config().turn.expect("a relay").credential, "c2");
        assert_eq!(network.ice().0, ["stun:t:3478"]);
    }

    fn grant(username: &str) -> TurnGrant {
        TurnGrant { urls: vec!["turn:t:3478".to_owned()], username: username.to_owned(), credential: format!("{username}-key") }
    }

    #[derive(Default)]
    struct TurnDesk {
        asked: std::sync::atomic::AtomicUsize,
        takes: Duration,
        hangs: bool,
        refuses: bool,
    }

    impl TurnDesk {
        fn asked(&self) -> usize {
            self.asked.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl Relay for TurnDesk {
        async fn signal(&self, _: &str, _: &[u8; 32], _: Vec<u8>) -> Result<bool> {
            Ok(false)
        }

        async fn deposit(&self, _: &str, _: &[u8; 32], _: Vec<u8>) -> Result<()> {
            Ok(())
        }

        async fn collect(&self) -> Result<Vec<(String, Vec<u8>)>> {
            Ok(Vec::new())
        }

        async fn acknowledge(&self, _: &str) -> Result<()> {
            Ok(())
        }

        async fn turn_credentials(&self) -> Result<TurnGrant> {
            let n = self.asked.fetch_add(1, Ordering::SeqCst);
            if self.hangs {
                std::future::pending::<()>().await;
            }
            tokio::time::sleep(self.takes).await;
            if self.refuses {
                bail!("too many requests");
            }
            Ok(grant(&format!("fresh-{n}")))
        }
    }

    async fn welcomed(desk: &Arc<TurnDesk>) -> Arc<Network> {
        let network = Network::new(desk.clone(), SessionConfig::offline());
        network.handle(RouterEvent::Connected { stun: vec!["stun:t:3478".to_owned()], turn: Some(grant("welcome")) }).await;
        network
    }

    fn turn_user(media: ft_media::MediaConfig) -> String {
        media.turn.expect("a relay").username
    }

    #[tokio::test]
    async fn a_connection_or_a_call_with_an_old_turn_user_takes_a_new_one_first() {
        let desk = Arc::new(TurnDesk::default());
        let network = welcomed(&desk).await;
        network.age_turn(TURN_RENEW_AFTER);
        assert_eq!(turn_user(network.fresh_media_config().await), "welcome", "young enough for a call");
        assert_eq!(desk.asked(), 0);

        network.age_turn(TURN_LIFE);
        assert_eq!(turn_user(network.fresh_media_config().await), "fresh-0", "a native call's media");
        assert_eq!(network.session_config().turn_servers[0].username, "fresh-0", "the direct connections");
        assert_eq!(network.fresh_ice().await.1.expect("a relay").username, "fresh-0", "the WebView's calls");
        assert_eq!(network.ice().0, ["stun:t:3478"], "the STUN servers stay");
        assert_eq!(desk.asked(), 1, "once: the new one is young");
    }

    #[tokio::test(start_paused = true)]
    async fn a_router_that_does_not_answer_holds_a_call_back_two_seconds_at_most() {
        let desk = Arc::new(TurnDesk { hangs: true, ..TurnDesk::default() });
        let network = welcomed(&desk).await;
        network.age_turn(TURN_LIFE);
        let started = tokio::time::Instant::now();
        let media = tokio::time::timeout(Duration::from_secs(60), network.fresh_media_config()).await.expect("never stuck");
        assert_eq!(turn_user(media), "welcome", "what there is");
        assert!(started.elapsed() <= TURN_WAIT, "{:?}", started.elapsed());
        assert_eq!(desk.asked(), 1, "the router was asked");

        let again = tokio::time::Instant::now();
        network.fresh_media_config().await;
        assert!(again.elapsed() < Duration::from_millis(10), "{:?}", again.elapsed());
        assert_eq!(desk.asked(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_burst_of_connections_asks_the_router_once() {
        let desk = Arc::new(TurnDesk { takes: Duration::from_millis(300), ..TurnDesk::default() });
        let network = welcomed(&desk).await;
        network.age_turn(TURN_LIFE);
        let burst: Vec<_> = (0..10)
            .map(|_| {
                let network = network.clone();
                tokio::spawn(async move { turn_user(network.fresh_media_config().await) })
            })
            .collect();
        for each in burst {
            assert_eq!(each.await.unwrap(), "fresh-0");
        }
        assert_eq!(desk.asked(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn a_router_that_refuses_is_not_asked_again_at_once() {
        let desk = Arc::new(TurnDesk { refuses: true, ..TurnDesk::default() });
        let network = welcomed(&desk).await;
        network.age_turn(TURN_LIFE);
        for _ in 0..5 {
            assert_eq!(turn_user(network.fresh_media_config().await), "welcome");
        }
        assert_eq!(desk.asked(), 1);
        tokio::time::sleep(TURN_ASK_AGAIN).await;
        network.fresh_media_config().await;
        assert_eq!(desk.asked(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn while_connected_to_the_router_the_turn_user_is_renewed_before_it_expires() {
        let desk = Arc::new(TurnDesk::default());
        let network = Network::new(desk.clone(), SessionConfig::offline());
        let (events, incoming) = mpsc::channel(8);
        network.listen(incoming);
        events.send(RouterEvent::Connected { stun: vec![], turn: Some(grant("welcome")) }).await.unwrap();
        tokio::time::sleep(TURN_LOOK * 3).await;
        assert_eq!(desk.asked(), 0, "a young one stays");

        network.age_turn(TURN_RENEW_AFTER);
        tokio::time::sleep(TURN_LOOK).await;
        assert_eq!(desk.asked(), 1);
        assert_eq!(network.ice().1.expect("a relay").username, "fresh-0");
        tokio::time::sleep(TURN_LOOK * 3).await;
        assert_eq!(desk.asked(), 1, "the new one is young");

        events.send(RouterEvent::Disconnected).await.unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
        network.age_turn(TURN_LIFE);
        tokio::time::sleep(TURN_LOOK * 3).await;
        assert_eq!(desk.asked(), 1, "not while the socket is down");
    }

    #[tokio::test(start_paused = true)]
    async fn without_turn_in_the_welcome_none_is_asked_for() {
        let desk = Arc::new(TurnDesk::default());
        let network = Network::new(desk.clone(), SessionConfig::offline());
        let (events, incoming) = mpsc::channel(8);
        network.listen(incoming);
        events.send(RouterEvent::Connected { stun: vec![], turn: None }).await.unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;
        network.age_turn(TURN_LIFE);
        assert!(network.fresh_media_config().await.turn.is_none());
        tokio::time::sleep(TURN_LOOK * 3).await;
        assert_eq!(desk.asked(), 0);
    }

    #[derive(Default)]
    struct Offers(std::sync::Mutex<Vec<bool>>);

    impl Offers {
        fn waking(&self) -> Vec<bool> {
            self.0.lock().expect("offers poisoned").clone()
        }

        async fn until_more_than(&self, count: usize) -> Vec<bool> {
            for _ in 0..500 {
                if self.waking().len() > count {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            self.waking()
        }
    }

    #[async_trait]
    impl Relay for Offers {
        async fn signal(&self, _: &str, _: &[u8; 32], _: Vec<u8>) -> Result<bool> {
            self.0.lock().expect("offers poisoned").push(true);
            Ok(false)
        }

        async fn signal_without_waking(&self, _: &str, _: &[u8; 32], _: Vec<u8>) -> Result<Signalled> {
            self.0.lock().expect("offers poisoned").push(false);
            Ok(Signalled::NotConnected)
        }

        async fn deposit(&self, _: &str, _: &[u8; 32], _: Vec<u8>) -> Result<()> {
            Ok(())
        }

        async fn collect(&self) -> Result<Vec<(String, Vec<u8>)>> {
            Ok(Vec::new())
        }

        async fn acknowledge(&self, _: &str) -> Result<()> {
            Ok(())
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_redial_wakes_the_contact_only_when_something_waits_for_it() {
        let offers = Arc::new(Offers::default());
        let network = Network::new(offers.clone(), SessionConfig::offline());
        let core = Core::open(ft_storage::Store::open_in_memory().await.unwrap(), [4; 32], network.clone()).await.unwrap();
        network.attach(&core);
        let other = Core::open(ft_storage::Store::open_in_memory().await.unwrap(), [4; 32], Network::new(Arc::new(NoRouter), SessionConfig::offline()))
            .await
            .unwrap();
        let contact = core.add_contact(&other.my_card().await.unwrap().to_link(), None).await.unwrap().device_id;
        network.handle(RouterEvent::Connected { stun: vec![], turn: None }).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        let before = offers.waking().len();

        network.redial(vec![contact.clone()]);
        assert_eq!(offers.until_more_than(before).await[before..], [false], "nothing waits for the contact");

        core.schedule_text(&contact, "later", crate::now() + 3_600_000, None).await.unwrap();
        network.redial(vec![contact.clone()]);
        assert_eq!(offers.until_more_than(before + 1).await[before..], [false, false], "a text for later is not due");

        core.queue_text(&contact, "hello", None).await.unwrap();
        network.redial(vec![contact.clone()]);
        assert_eq!(offers.until_more_than(before + 2).await[before..], [false, false, true], "a text waits for the contact");
    }

    #[test]
    fn new_connections_and_calls_listen_on_the_interfaces_there_are_now() {
        let started = SessionConfig { bind: vec!["192.0.2.1:0".to_owned()], ..SessionConfig::default() };
        let network = Network::new(Arc::new(NoRouter), started);
        assert_eq!(network.session_config().bind, ft_webrtc::interface_addresses());
        assert_eq!(network.media_config().bind, ft_webrtc::interface_addresses());

        let offline = Network::new(Arc::new(NoRouter), SessionConfig::offline());
        assert_eq!(offline.session_config().bind, ["127.0.0.1:0"], "the tests' loopback stays");
    }
}
