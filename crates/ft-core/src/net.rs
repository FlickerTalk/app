//! The real network (Plan §13–19, §106 M2): WebRTC data channels between contacts, signalled
//! through the router, with the recipient's mailbox as the fallback.
//!
//! - Sending: an open data channel is reused; otherwise an offer, encrypted with Olm for the
//!   contact, goes through the router. A first contact adds our Contact Card to the offer so the
//!   other side can read it without the mailbox. If the contact is not connected to the router, or
//!   the channel does not open within `CONNECT_WAIT`, the core falls back to the mailbox.
//! - Receiving: offers from contacts are answered; answers complete our pending offers; each open
//!   data channel feeds the core. The router's mail notice (and every reconnection) triggers a
//!   collection of the mailbox and a retry of the outbox.
//!
//! The signalling is authenticated end to end by Olm, so the DTLS fingerprint in each description
//! is bound to the contact's identity: the router cannot place itself in the middle.

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

/// A transfer quiet for this long when a connection opens is asked for again over it.
const RESUME_QUIET: Duration = Duration::from_secs(2);

/// How long to wait for a data channel before falling back to the mailbox.
pub const CONNECT_WAIT: Duration = Duration::from_secs(12);

/// A router from 0.4.0 keeps a signal for a recipient that is not connected this long, and hands
/// it over right after the recipient's next welcome (2026-09-29).
const RETAINED_FOR: Duration = Duration::from_secs(55);

/// An answer to a retained offer handed over at the last moment still has this long to come.
const ANSWER_GRACE: Duration = Duration::from_secs(5);

/// An offer nobody got (a router before 0.4 loses it) is kept this long for the next attempt.
const SPARE_FOR: Duration = Duration::from_secs(20);

/// The router's TURN users live this long (`TURN_TTL` in ft-router, 2026-10-28).
pub const TURN_LIFE: Duration = Duration::from_secs(10 * 60);

/// While the router's socket is up, a TURN user this old is renewed (half its life), looked at
/// every `TURN_LOOK`.
pub const TURN_RENEW_AFTER: Duration = Duration::from_secs(5 * 60);
/// How often the TURN user's age is looked at while the router's socket is up.
pub const TURN_LOOK: Duration = Duration::from_secs(60);

/// A connection or a call about to gather with a TURN user this old asks for a new one first: it
/// could expire before the gathering is done. Past `TURN_RENEW_AFTER`, so that a call only waits
/// when the periodic renewal could not run (the router did not answer, the phone slept).
pub const TURN_STALE_AFTER: Duration = Duration::from_secs(8 * 60);

/// How long a connection or a call waits for that new user, counted from when it was asked for:
/// then it goes on with the user there is. A call never waits on the router.
pub const TURN_WAIT: Duration = Duration::from_secs(2);

/// After asking for a TURN user, the router is not asked again for this long, whether the ask is
/// still going on or failed: a burst of connections asks once.
pub const TURN_ASK_AGAIN: Duration = Duration::from_secs(30);

/// The router, as the network uses it (ft-push's `RouterClient` in the app, a fake in tests).
#[async_trait]
pub trait Relay: Send + Sync {
    /// `true` if the recipient is connected to the router and got the signal.
    async fn signal(&self, to: &str, capability: &[u8; 32], bytes: Vec<u8>) -> Result<bool>;
    /// The same, for a call (2026-09-28): an offline iPhone is rung through CallKit.
    async fn signal_call(&self, to: &str, capability: &[u8; 32], bytes: Vec<u8>) -> Result<bool> {
        self.signal(to, capability, bytes).await
    }
    /// Either, saying whether a router from 0.4.0 kept the signal for a recipient that is not
    /// connected (2026-09-29). A relay that cannot tell never keeps it.
    async fn signal_as(&self, to: &str, capability: &[u8; 32], bytes: Vec<u8>, call: bool) -> Result<Signalled> {
        let delivered = if call { self.signal_call(to, capability, bytes).await? } else { self.signal(to, capability, bytes).await? };
        Ok(if delivered { Signalled::Delivered } else { Signalled::NotConnected })
    }
    async fn deposit(&self, to: &str, capability: &[u8; 32], blob: Vec<u8>) -> Result<()>;
    /// This device's mail: (id, blob), oldest first.
    async fn collect(&self) -> Result<Vec<(String, Vec<u8>)>>;
    async fn acknowledge(&self, id: &str) -> Result<()>;
    /// A new TURN user (`GET /v1/turn-credentials`, 2026-10-28): the welcome's lives `TURN_LIFE`.
    /// A relay that has none to hand out says so.
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

/// An open data channel with a contact.
struct Link {
    id: u64,
    session: Session,
}

/// What a send to a contact that is not connected may do meanwhile (2026-09-29).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// A call's offer: waits for the connection (and marks the call's timings).
    Call,
    /// Only a direct connection will do (files, calls' answers and ends, plugins): waits for it.
    Direct,
    /// It may go to the mailbox instead: never waits for a contact that is not connected.
    Fallback,
}

/// What a connection attempt came to, as `connect` leaves it.
enum Attempt {
    /// Settled while holding the gate: the open connection, or none.
    Done(Option<Session>),
    /// Our offer waits at the router for a contact that is not connected: its connection may
    /// open within this long. Waited for after letting go of the gate (2026-10-01), so that a
    /// call coming meanwhile makes its own offer, marked as a call, at once.
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

/// Between looks at whether our offer is waiting at the router, while another send holds the gate.
const GATE_LOOK: Duration = Duration::from_millis(50);

/// An offer made and gathered that nobody got, ready for the next attempt with the same contact.
struct Spare {
    id: u64,
    session: Session,
    inbox: Inbox,
    sdp: String,
    made: Instant,
}

/// Our offer, kept by the router for a contact that is not connected, still waiting for its answer.
#[derive(Clone)]
struct Retained {
    /// Its signalling session id.
    id: String,
    session: Session,
    /// The end of the connection window of the send that made it.
    window: Instant,
    /// When the router drops it: from then on a new offer is needed.
    until: Instant,
    /// Whether it rang the contact for the call going on: sent marked as a call, and that call not
    /// over yet (`call_over`).
    call: bool,
}

/// The router's STUN servers and TURN user, and when that user came.
struct Ice {
    stun: Vec<String>,
    turn: Option<TurnGrant>,
    /// Wall-clock time: a sleeping phone's monotonic clock stops, and a night asleep must count.
    turn_at: SystemTime,
}

impl Default for Ice {
    fn default() -> Self {
        Self { stun: Vec::new(), turn: None, turn_at: SystemTime::now() }
    }
}

impl Ice {
    /// Whether there is a TURN user at least `age` old. With the clock set back, its age is
    /// unknown: old.
    fn turn_older_than(&self, age: Duration) -> bool {
        self.turn.is_some() && !SystemTime::now().duration_since(self.turn_at).is_ok_and(|since| since < age)
    }
}

/// The last ask for a new TURN user: when it began, and whether it is over.
struct Renewal {
    began: tokio::time::Instant,
    over: watch::Receiver<bool>,
}

pub struct Network {
    relay: Arc<dyn Relay>,
    /// Where to listen; STUN and TURN come from the router's welcome, and newer TURN users from
    /// the router on demand (2026-10-28).
    base: SessionConfig,
    ice: std::sync::Mutex<Ice>,
    renewal: std::sync::Mutex<Option<Renewal>>,
    core: OnceLock<Weak<Core>>,
    this: OnceLock<Weak<Network>>,
    links: Mutex<HashMap<String, Link>>,
    /// Our offers waiting for their answer, by signalling session id.
    pending: Mutex<HashMap<String, Session>>,
    /// By contact, our offer the router keeps for them while they are not connected.
    retained: Mutex<HashMap<String, Retained>>,
    /// By contact, our offer a router before 0.4 lost, for the next attempt.
    spare: Mutex<HashMap<String, Spare>>,
    /// One connection attempt at a time per contact.
    gates: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    /// One mailbox collection at a time: a blob must not be processed twice.
    collecting: Mutex<()>,
    next_link: AtomicU64,
    /// The contacts to dial again at the router's next welcome (`redial`).
    redial: std::sync::Mutex<Redial>,
}

/// Contacts to dial again, and whether the router's socket is up, as its events say.
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

    /// The core is created with the network as its transport, then attached here.
    pub fn attach(&self, core: &Arc<Core>) {
        let _ = self.core.set(Arc::downgrade(core));
    }

    /// Feeds the router's events until the channel closes (each is handled in the background).
    /// Meanwhile, while the router's socket is up, a TURN user past `TURN_RENEW_AFTER` is renewed
    /// (2026-10-28): the welcome's lives `TURN_LIFE`, and a phone stays connected for longer.
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

    /// Renews the TURN user in the background if it is past `TURN_RENEW_AFTER` and the router's
    /// socket is up. Down, the next welcome brings a new one.
    fn keep_turn_fresh(&self) {
        let up = self.redial.lock().expect("redial poisoned").router_up;
        if up && self.turn_older_than(TURN_RENEW_AFTER) {
            let _ = self.renew_turn();
        }
    }

    /// Returns at once: the work runs in the background, so that an answer arriving meanwhile
    /// is never stuck behind the mail whose processing is waiting for it.
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
                        // A registration that failed offline goes now (2026-10-01).
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

    /// The router's welcome: its STUN servers and a TURN user, new.
    fn welcome(&self, stun: Vec<String>, turn: Option<TurnGrant>) {
        *self.ice.lock().expect("ice poisoned") = Ice { stun, turn, turn_at: SystemTime::now() };
    }

    /// The router's STUN servers and TURN user, from its last welcome or a newer one.
    pub fn ice(&self) -> (Vec<String>, Option<TurnGrant>) {
        let ice = self.ice.lock().expect("ice poisoned");
        (ice.stun.clone(), ice.turn.clone())
    }

    /// A fresh TURN user (`RouterClient::turn_credentials`) for the connections and calls made from
    /// now on (2026-10-28). The welcome's lives ten minutes: a device that stays connected longer
    /// would otherwise gather no relay at all. The STUN servers stay.
    pub fn set_turn(&self, turn: TurnGrant) {
        let mut ice = self.ice.lock().expect("ice poisoned");
        ice.turn = Some(turn);
        ice.turn_at = SystemTime::now();
    }

    /// The router's STUN servers and a TURN user young enough for a call (2026-10-28): the WebView's
    /// calls gather with them. Waits for a new user `TURN_WAIT` at most.
    pub async fn fresh_ice(&self) -> (Vec<String>, Option<TurnGrant>) {
        self.fresh_turn().await;
        self.ice()
    }

    /// The TURN user came `by` earlier than it did. For tests.
    #[doc(hidden)]
    pub fn age_turn(&self, by: Duration) {
        let mut ice = self.ice.lock().expect("ice poisoned");
        ice.turn_at = ice.turn_at.checked_sub(by).unwrap_or(UNIX_EPOCH);
    }

    fn turn_older_than(&self, age: Duration) -> bool {
        self.ice.lock().expect("ice poisoned").turn_older_than(age)
    }

    /// Before a connection or a call gathers (2026-10-28): a TURN user past `TURN_STALE_AFTER` is
    /// replaced by a new one from the router, waited for until `TURN_WAIT` after it was asked for.
    /// Then it goes on with the user there is: a call never waits on the router.
    async fn fresh_turn(&self) {
        if !self.turn_older_than(TURN_STALE_AFTER) {
            return;
        }
        let Some((began, mut over)) = self.renew_turn() else { return };
        let _ = tokio::time::timeout_at(began + TURN_WAIT, over.wait_for(|over| *over)).await;
    }

    /// Asks the router for a new TURN user in the background, unless an ask began less than
    /// `TURN_ASK_AGAIN` ago, still going on or failed: then that one stands. When the ask began,
    /// and when it is over.
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

    /// A new session's setup: listening on the interfaces there are now (app#200), with the
    /// router's STUN and TURN.
    fn session_config(&self) -> SessionConfig {
        let (stun, turn) = self.ice();
        ice_config(&self.base.clone().listening_now(), stun, turn)
    }

    async fn collect_mail(&self) {
        let _one_at_a_time = self.collecting.lock().await;
        let (Ok(core), Ok(mail)) = (self.core(), self.relay.collect().await) else { return };
        for (id, blob) in mail {
            // A blob that cannot be read is dropped too: it would never become readable.
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

    /// Whether a direct connection with the contact is open now.
    pub async fn is_connected(&self, contact: &str) -> bool {
        self.links.lock().await.get(contact).is_some_and(|link| link.session.is_open())
    }

    /// The contacts with a direct connection open now.
    pub async fn connected(&self) -> Vec<String> {
        let links = self.links.lock().await;
        let mut contacts: Vec<String> =
            links.iter().filter(|(_, link)| link.session.is_open()).map(|(contact, _)| contact.clone()).collect();
        contacts.sort();
        contacts
    }

    /// Closes the direct connection with the contact, if any (the other side sees it close too),
    /// and drops our offer waiting for them.
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

    /// Drops the direct connection with the contact without telling them, as an app that is
    /// killed or suspended does. For tests of what the contact sees then.
    #[doc(hidden)]
    pub async fn vanish(&self, contact: &str) {
        let link = self.links.lock().await.remove(contact);
        if let Some(link) = link {
            link.session.vanish().await;
        }
    }

    /// The direct connection with the contact fails on this side while the contact's side still
    /// answers. For tests.
    #[doc(hidden)]
    pub async fn lose(&self, contact: &str) {
        if let Some(link) = self.links.lock().await.get(contact) {
            link.session.lose();
        }
    }

    /// How many of our offers wait for their answer. For tests.
    #[doc(hidden)]
    pub async fn offers_waiting(&self) -> usize {
        self.pending.lock().await.len()
    }

    /// Dials the contacts again, sending nothing (app#152): the direct connections the app let go
    /// of when it left the foreground. At the router's next welcome, so that the answers find its
    /// socket, or at once if the socket is up (a leave and return too quick for it to drop).
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

    /// The phone's network changed (app#200, 2026-10-20) and the router's socket is about to be
    /// opened again. The direct connections may run over a path that is gone and still look open
    /// for seconds, losing what is sent into them, and the offers still waiting carry the old
    /// network's addresses: all of it goes, and the contacts it was connected to are dialled again
    /// at the router's next welcome, over the network there is now.
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
        // Closing waits for the other side to hear it, over a path that may be gone.
        tokio::spawn(async move {
            for session in gone {
                let _ = session.close().await;
            }
        });
    }

    /// Opens a direct connection with each contact, in the background, without waiting for one
    /// that is not connected to the router. Not with a contact blocked, a stranger waiting for a
    /// yes, nor one of a session left (§35, A5, §108): any of them may have changed since.
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
                if let Ok(peer) = core.peer(&contact).await {
                    let _ = network.reach(&peer, true).await;
                }
            });
        }
    }

    /// Closes every direct connection and every offer still waiting (erasing the phone,
    /// 2026-09-30).
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

    /// Opens a data channel with the contact, or says it cannot be reached now. Called with the
    /// contact's gate held; an offer waiting at the router is waited for after it (`Attempt`).
    ///
    /// A contact that is not connected to the router (a closed app): a router from 0.4.0 keeps our
    /// offer and wakes them, so the offer stays open for their answer until the router drops it,
    /// and the connection is kept when it opens. Meanwhile no new offer is made. What may go to the
    /// mailbox never waits for it (the message gets delivered first, §17); a call waits the usual
    /// window each time, and what needs the connection waits the window of the offer's first send.
    ///
    /// A call never waits on an offer that was not sent as a call (2026-10-01): the router only
    /// woke the contact for it (an iPhone shows a notice and does not start the app), so the call
    /// would never ring; nor on the offer of a call that is over (a redial rings again). That
    /// offer is dropped and the call makes its own, marked as a call. One
    /// offer per contact at a time keeps it to one connection: the dropped one cannot open, since
    /// our side of it is closed.
    async fn connect(&self, peer: &Peer, reach: Reach) -> Result<Attempt> {
        let call = reach == Reach::Call;
        if let Some(retained) = self.retained_offer(&peer.device_id).await {
            if call && !retained.call && !retained.session.is_open() {
                self.drop_retained(&peer.device_id, &retained).await;
            } else {
                let limit = match reach {
                    Reach::Call => CONNECT_WAIT,
                    Reach::Direct => retained.window.saturating_duration_since(Instant::now()),
                    Reach::Fallback => Duration::ZERO,
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
        // A contact of a hidden session gets that session's card (app#9).
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
        // Through the router the signal goes in an envelope (A1): it names its sender to the
        // recipient alone.
        let wrapped = core.wrap_for(&peer.device_id, signal.encode()).await?;
        let signalled = self.relay.signal_as(&peer.device_id, peer.capability.as_bytes(), wrapped, call).await;
        if matches!(signalled, Ok(Signalled::Retained)) {
            if call {
                core.mark_call_stage(CallStage::LinkOfferRetained);
            }
            self.keep_retained(&peer.device_id, session_id, session.clone(), inbox, Instant::now(), call).await;
            let limit = if reach == Reach::Fallback { Duration::ZERO } else { CONNECT_WAIT };
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
            // Nobody got it: the next attempt goes with it instead of gathering again.
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

    /// The offer a router before 0.4 lost for the contact, if still fresh.
    async fn take_spare(&self, contact: &str) -> Option<Spare> {
        let spare = self.spare.lock().await.remove(contact)?;
        if spare.made.elapsed() < SPARE_FOR {
            return Some(spare);
        }
        let _ = spare.session.close().await;
        None
    }

    /// Keeps an offer nobody got for the contact's next attempt, for a while.
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

    /// Our offer the router still keeps for the contact, if any.
    async fn retained_offer(&self, contact: &str) -> Option<Retained> {
        let retained = self.retained.lock().await;
        retained.get(contact).filter(|retained| retained.until > Instant::now()).cloned()
    }

    /// Keeps a retained offer open until the router drops it (and a last answer has had time to
    /// come): the contact may wake and answer it at any moment, and its connection is kept then.
    async fn keep_retained(&self, contact: &str, id: String, session: Session, inbox: Inbox, signalled_at: Instant, call: bool) {
        let until = signalled_at + RETAINED_FOR;
        let retained = Retained { id: id.clone(), session: session.clone(), window: signalled_at + CONNECT_WAIT, until, call };
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

    /// Gives up our offer the router keeps for the contact: an answer to it is ignored from now on
    /// and its connection can no longer open. The router may still hand it over; the contact's
    /// side of it then never opens either, and is closed after `CONNECT_WAIT`.
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
            // A blocked sender (§35): dropped unread. Its offer may be a call that a push set the
            // phone's own call screen ringing for, before the core could know who called.
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

    /// Keeps the open channel for the contact and feeds what arrives on it to the core.
    ///
    /// The UI hears whenever the connection stalls (a contact whose app was killed never closes
    /// it, app#151) and whenever it comes back (app#152). The inbox ends when the channel closes or
    /// the connection fails: the link goes, the UI hears of it, and the session is closed, so the
    /// contact's side, if it still answers, sees it close too.
    async fn adopt(&self, contact: &str, session: Session, mut inbox: Inbox) {
        let id = self.next_link.fetch_add(1, Ordering::Relaxed);
        self.links.lock().await.insert(contact.to_owned(), Link { id, session: session.clone() });
        self.announce(contact);
        if let Ok(core) = self.core() {
            core.mark_call_stage(CallStage::LinkOpened);
        }
        // File transfers stopped by the last connection go on over this one (§63), and what
        // waits for its receipt goes over it too, even if it is in the mailbox (2026-09-29).
        if let Ok(core) = self.core() {
            // What we present in a call with them, in case a word went with the last connection.
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
        // A send that comes meanwhile waits here, and then finds the connection open.
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

    /// The offer stays, as one that never rang: what still has to reach the contact (the call's
    /// end) goes by it if they wake, without another offer through the router, which would wake
    /// them again; the next call drops it and rings with its own (`connect`).
    async fn call_over(&self, device_id: &str) {
        if let Some(retained) = self.retained.lock().await.get_mut(device_id) {
            retained.call = false;
        }
    }
}

impl Network {
    /// Sends over the open link, or opens one first; a call's signal is marked as such
    /// (2026-09-28), so an offline iPhone rings.
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

    /// The open connection with the peer, opening one first if needed (2026-10-06, split from
    /// `send_direct_as` so that a message can be looked at again before it is sent).
    async fn reach_as(&self, to: &Peer, reach: Reach) -> Result<Option<Session>> {
        let gate = self.gate(&to.device_id).await;
        let one_at_a_time = if reach == Reach::Fallback {
            // Behind a call waiting for a contact that is not connected, a message would wait too:
            // once our offer is known to wait at the router, it goes to the mailbox instead.
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
        // Another send may have connected while this one waited.
        let attempt = match self.open_link(&to.device_id).await {
            Some(session) => Attempt::Done(Some(session)),
            None => self.connect(to, reach).await?,
        };
        drop(one_at_a_time);
        Ok(attempt.outcome().await)
    }
}

/// Whether the session's channel opens within `limit` (at once if it is open).
async fn wait_open(session: &Session, limit: Duration) -> bool {
    if session.is_open() {
        return true;
    }
    tokio::time::timeout(limit, session.wait_open()).await.is_ok_and(|open| open.is_ok())
}

/// The listening addresses of `base` with the router's STUN and TURN.
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

/// A native call's media setup from the data channels' one: the same addresses and servers.
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

/// A random id for one signalling exchange.
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

    // A native call listens where the data channels do and uses the router's STUN and TURN.
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

    // The router's TURN users live ten minutes (2026-10-28): a device connected for longer (the
    // support contact) takes fresh ones, and every connection and call made afterwards uses them.
    // The STUN servers of the welcome stay.
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

    /// A router that hands out TURN users, `fresh-0`, `fresh-1`…, and counts the asks. It may take
    /// its time, never answer, or refuse.
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

    /// A network over `desk`, welcomed by the router with the TURN user `welcome`.
    async fn welcomed(desk: &Arc<TurnDesk>) -> Arc<Network> {
        let network = Network::new(desk.clone(), SessionConfig::offline());
        network.handle(RouterEvent::Connected { stun: vec!["stun:t:3478".to_owned()], turn: Some(grant("welcome")) }).await;
        network
    }

    fn turn_user(media: ft_media::MediaConfig) -> String {
        media.turn.expect("a relay").username
    }

    // The router's TURN users live ten minutes (2026-10-28) and the app kept the welcome's: a phone
    // connected for longer called with an expired one, gathered no relay, and a call whose path
    // needed one never connected. A connection or a call about to gather with a user that old asks
    // the router for a new one first. A younger one is used as it is, without waiting.
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

    // A router that does not answer holds a call back `TURN_WAIT` at most: it goes on with the user
    // there is. The next one, while that ask still hangs, does not wait for it again.
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

    // A burst of connections and calls with an old user (a phone back with many contacts to dial)
    // asks the router once, and each of them gathers with the new user.
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

    // A router that refuses (too many requests, no TURN) is not asked again for `TURN_ASK_AGAIN`:
    // the connections in between go at once with the user there is.
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

    // While the router's socket is up, the TURN user is renewed before it expires (past
    // `TURN_RENEW_AFTER`, looked at every `TURN_LOOK`), so that a call seldom waits for one. Not
    // while the socket is down: the next welcome brings one.
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

    // A router with no TURN (none in its welcome) has none to hand out either: nothing is asked.
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

    // The phone changed networks with the app open (app#200, 2026-10-20): the addresses it had
    // when the app started may be gone. A direct connection or a call's media made afterwards
    // listens on the interfaces there are now; before, nothing could bind once they were all gone.
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
