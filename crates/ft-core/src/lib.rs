//! Client orchestrator (Plan §18–19, §26–27, §35, §106 M1): pairs devices by Contact Card and
//! moves end-to-end encrypted packets between them, directly (DataChannel) when possible and
//! through the recipient's mailbox otherwise.
//!
//! - Pairing: whoever scans a Contact Card opens an Olm channel and sends their own card in the
//!   first encrypted packet; the owner adds them and answers with theirs. Until a contact has
//!   written back, our card travels again before each retry, so a lost first packet never
//!   leaves the other side unable to read us.
//! - Sending: the text is stored `pending` and queued in `pending_outbox` (§26). Each attempt tries
//!   the direct connection; otherwise, only if both sides use it, the recipient's mailbox (§19).
//!   The entry leaves the outbox when the DELIVERED receipt arrives.
//! - Receiving: stored once per `message_id` and always acknowledged (§27). Packets from blocked
//!   contacts are dropped (§35).

pub mod calls;
pub mod circles;
pub mod files;
pub mod moving;
pub mod native_calls;
pub mod plugins;
pub mod timings;
pub mod net;
pub mod vault;
pub mod online;
pub mod web;
mod sessions;

pub use plugins::CATALOGUE_HOME;
pub use web::{Fetch, Web, WebAnswer, WebRequest};

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, ensure, Context, Result};
use async_trait::async_trait;
use ft_billing::{Access, AgeClass, Doing, Plan};
use ft_contacts::{ContactCard, RouteCapability};
use ft_crypto::{accept_first_contact, Channel};
use ft_identity::{DeviceId, EnvelopeKey, Identity};
use ft_protocol::{Body, Envelope, MessageId, Packet, Sealed, ENVELOPE_VERSION};
use ft_storage::{Contact, ContactRules, Conversation, Message, MessageState, NewContact, OutboxEntry, Store, UpdateEntry};
use tokio::sync::{broadcast, Mutex};

/// Unreachable contacts are retried after 5 s, 10 s, 20 s… up to this.
pub const MAX_RETRY_DELAY: Duration = Duration::from_secs(300);
/// After a direct send, how long to wait for the receipt before trying again.
pub(crate) const RECEIPT_WAIT: Duration = Duration::from_secs(30);
/// After leaving it in the mailbox, how long before trying again.
pub(crate) const MAILBOX_WAIT: Duration = Duration::from_secs(600);
/// How often a message already in the mailbox gets a new copy there (2026-10-01): the router keeps
/// a blob for 7 days, and the copy is for a router that lost its table.
pub(crate) const MAILBOX_COPY: Duration = Duration::from_secs(24 * 3600);
/// How long an edit, a taking back or a reaction is tried before it is given up (2026-10-06): as
/// long as the router keeps a blob in the mailbox. An app too old to know them never answers.
pub(crate) const UPDATE_LIFETIME: Duration = Duration::from_secs(7 * 24 * 3600);

const NAME: &str = "name";
const MAILBOX: &str = "mailbox";
/// When the app was first opened on this phone (ms): the free year counts from it (§41).
const INSTALLED_AT: &str = "installed_at";
/// What the user said about their age, as a word; never a date of birth (§30, §43).
const AGE_CLASS: &str = "age_class";
/// Until when the Store says the subscription runs (ms); 0 when there is none (§45).
const PAID_UNTIL: &str = "paid_until";
/// The first year is free (§40–41).
/// What the app says when it stops someone: short, and the same everywhere.
const ASK_FOR_THE_EURO: &str = "a subscription is needed to start something new";

pub const FREE_PERIOD: Duration = Duration::from_secs(365 * 24 * 3600);

/// Whether this attempt may leave a copy in the mailbox: the first time, and then once a day.
fn mail_copy_due(in_mailbox: bool, mailed_at: Option<i64>, now: i64) -> bool {
    !in_mailbox || mailed_at.is_none_or(|at| now - at >= MAILBOX_COPY.as_millis() as i64)
}

pub fn retry_delay(attempts: i64) -> Duration {
    let exponent = attempts.clamp(1, 16) as u32 - 1;
    Duration::from_secs(5 * 2u64.pow(exponent)).min(MAX_RETRY_DELAY)
}

/// Where a packet goes: the contact's device and the capability that lets us reach it (§34).
#[derive(Debug, Clone)]
pub struct Peer {
    pub device_id: String,
    pub capability: RouteCapability,
}

/// The network, implemented over WebRTC and the router (and in memory by the tests).
#[async_trait]
pub trait Transport: Send + Sync {
    /// Hands the packet to a direct connection with the peer, opening one if needed. `false` when
    /// the peer cannot be reached directly right now.
    async fn send_direct(&self, to: &Peer, bytes: Vec<u8>) -> Result<bool>;
    /// The same for what may go to the mailbox instead (2026-09-29): `false` at once when the peer
    /// is not connected to the router, so the mailbox takes it without waiting for the connection.
    /// An offer the router keeps for the peer stays open: the connection may still open later.
    async fn try_direct(&self, to: &Peer, bytes: Vec<u8>) -> Result<bool> {
        self.send_direct(to, bytes).await
    }
    /// The same for a call's offer (2026-09-28): opening the way rings an offline iPhone.
    async fn send_direct_call(&self, to: &Peer, bytes: Vec<u8>) -> Result<bool> {
        self.send_direct(to, bytes).await
    }
    /// Hands the packet to the direct connection already open with the peer, and never opens one
    /// (2026-10-01): `false` without one. What checks that an open connection still reaches the
    /// peer must not make an offer of its own through the router.
    async fn send_open(&self, to: &Peer, bytes: Vec<u8>) -> Result<bool> {
        self.send_direct(to, bytes).await
    }
    /// Opens the way to the peer for a message, sending nothing yet (2026-10-06): `true` once a
    /// direct connection is open, as `send_direct` (or `try_direct`, with `fallback`) would find
    /// it. A message taken back while the connection opened is then not sent. By default `true`:
    /// the send itself finds out.
    async fn reach(&self, _to: &Peer, _fallback: bool) -> Result<bool> {
        Ok(true)
    }
    /// Opens a direct connection for a call, sending nothing yet (2026-09-29): the caller opens
    /// the way while its media offer gathers. `false` when the peer cannot be reached now.
    async fn open_direct_call(&self, _to: &Peer) -> Result<bool> {
        Ok(false)
    }
    /// Leaves the packet, already encrypted, in the peer's mailbox on the router (§19).
    async fn send_mailbox(&self, to: &Peer, bytes: Vec<u8>) -> Result<()>;
    /// Our call to the device is over (2026-10-01): an offer of ours that rang for it rings no more,
    /// so the next call makes its own and the device rings again.
    async fn call_over(&self, _device_id: &str) {}
    /// Closes any direct connection with the device (a blocked contact, §35).
    async fn disconnect(&self, _device_id: &str) {}
    /// Where a native call's media listens and the servers that help it: the router's STUN and
    /// TURN (2026-09-28). The routing is the call's own.
    fn media_config(&self) -> ft_media::MediaConfig {
        ft_media::MediaConfig::default()
    }
}

pub use calls::CallUpdate;
pub use native_calls::{CallPhase, CurrentCall, VideoDetach};
pub use files::MAX_FILE_SIZE;
pub use ft_push::{Feedback, MailboxRejected, RouterClient, TurnGrant};

/// What the UI listens to, to refresh itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    ContactsChanged,
    MessagesChanged { contact: String },
    /// A direct connection with the contact opened or closed.
    ConnectionChanged { contact: String },
    /// The contact is writing to this phone right now (2026-10-05). Heard only over the direct
    /// connection; the UI shows it for a moment and lets it fade.
    Typing { contact: String },
    /// Something happened to a call (§66).
    Call { contact: String, call: String, update: CallUpdate },
    /// Moving to a new phone (§60).
    Move(moving::MoveUpdate),
    /// A plugin was installed, granted something, or removed (issue app#3).
    PluginsChanged,
    /// A circle was made, changed, joined or left (2026-09-27).
    CirclesChanged,
    /// Something was said or happened in the circle.
    CircleMessagesChanged { circle: String },
    /// A plugin on the other side said something to its twin here (2026-09-27, `ft.live`).
    PluginEvent { plugin: String, contact: String, data: Vec<u8> },
    /// A plugin set or cancelled a reminder: the phone's alarm clock is told again.
    RemindersChanged,
    /// The user's cloud (plan-drive): connected, set up, changed, backed up or forgotten.
    VaultChanged,
    /// How far a transfer with the cloud is: (done, total) bytes.
    VaultProgress { done: u64, total: u64 },
    /// An incoming call was refused without a trace (Calls off, a stranger, a blocked contact, a
    /// closed hidden session; §108, §109). Not for the UI: it never shows. The phone's own call
    /// screen, which a push may have set ringing before the core knew who called, stops at once.
    CallRefused,
}

pub(crate) enum Route {
    Direct,
    Mailbox,
    Unreachable,
}

/// How long after sending a text its words can still change (2026-10-05): a day.
const EDIT_WINDOW_MS: i64 = 24 * 60 * 60 * 1000;

/// Whether a text is one emoji, as a reaction is (2026-10-05): short, with nothing blank or
/// invisible in it. Not a check against Unicode's tables: the other side draws what it gets.
fn is_reaction(text: &str) -> bool {
    !text.is_empty() && text.len() <= 32 && text.chars().count() <= 8 && !text.chars().any(|c| c.is_whitespace() || c.is_control())
}

pub struct Core {
    store: Store,
    /// Seals the identity and the Olm sessions at rest.
    key: [u8; 32],
    /// Olm work runs one at a time: a channel is loaded, used and saved back under this lock.
    identity: Mutex<Identity>,
    device_id: DeviceId,
    /// The device's own route capability; a renewed link replaces it (A5).
    capability: RwLock<RouteCapability>,
    /// Opens what the router carried for this device (A1): mail and signals sealed for it.
    envelope: EnvelopeKey,
    /// Wrong PINs in a row (A3): each one makes the next try wait longer.
    transport: Arc<dyn Transport>,
    events: broadcast::Sender<Event>,
    /// Where received files are written (set by the app).
    files_dir: OnceLock<PathBuf>,
    /// Where installed plugins live (issue app#3).
    plugins_dir: OnceLock<PathBuf>,
    /// Incoming file transfers in progress, by message id.
    transfers: std::sync::Mutex<HashMap<String, files::Transfer>>,
    /// The call going on, if any: one at a time.
    active_call: std::sync::Mutex<Option<String>>,
    /// The phone's audio device for native voice calls (2026-09-28); none on the desktop.
    call_audio: RwLock<Option<ft_media::AudioPlatform>>,
    /// The media of the active call, when its voice runs here and not in the WebView.
    native_call: std::sync::Mutex<Option<Arc<native_calls::NativeCall>>>,
    /// One native answer at a time: CallKit and the WebView may both answer the same call.
    native_setup: Mutex<()>,
    /// The offer of the call ringing here: (call, SDP, the caller's call media version), for a
    /// WebView that comes up late and for the native answer.
    ringing_offer: std::sync::Mutex<Option<(String, String, u16)>>,
    /// The phone's camera and display for native calls (native video, 2026-09-29); none on the
    /// desktop.
    call_video: RwLock<Option<ft_media::VideoPlatform>>,
    /// What the app runs before a call's video devices go: the bridge takes the views away.
    video_detach: RwLock<Option<native_calls::VideoDetach>>,
    /// Whether the app is on the screen (the bridge's `Visible`): away, our camera is held.
    app_visible: std::sync::atomic::AtomicBool,
    /// Whether the call screen shows the video (the WebView's layout): off it, our camera is held.
    call_shown: std::sync::atomic::AtomicBool,
    /// Whether the OS has the call's audio session active (CallKit's `didActivate` on iOS).
    call_audio_active: std::sync::atomic::AtomicBool,
    /// The OS answered or declined before any call rang (2026-09-28, 2026-09-29): what the next
    /// call's offer gets as soon as it arrives, and then the call being answered so.
    early_answer: std::sync::Mutex<Option<native_calls::EarlyAnswer>>,
    /// The answer prepared while a call rings here (2026-09-29), and how often it is made again.
    preparation: std::sync::Mutex<native_calls::Preparation>,
    answer_refresh: std::sync::atomic::AtomicU64,
    /// The steps of the call being set up, as they happen (temporary diagnostics).
    call_clock: std::sync::Mutex<Option<timings::CallClock>>,
    /// The newest CallKit call whose audio session was heard of (`set_call_audio_session`).
    audio_generation: std::sync::atomic::AtomicU64,
    /// This core, for work it hands to the background (a waiting answer, a hang-up to deliver).
    this: std::sync::Weak<Core>,
    /// Where a move to a new phone writes its copies (set by the app).
    move_dir: OnceLock<PathBuf>,
    moving: std::sync::Mutex<moving::MoveState>,
    /// The hidden sessions open right now, with their slot. Kept, sealed, across starts
    /// (2026-10-01): a session stays open until the user leaves it (`sessions.rs`).
    open_sessions: std::sync::Mutex<HashMap<String, u8>>,
    /// One write of the kept list at a time, each with the list as it is then.
    keeping_sessions: Mutex<()>,
    /// Bumped whenever the router must be told something new: the hashes or the silent slots.
    registration: tokio::sync::watch::Sender<u64>,
    /// How many pongs each contact has sent us (2026-10-01): a call's offer sent over a direct
    /// connection is known to have arrived once a ping sent after it is answered.
    pongs: tokio::sync::watch::Sender<HashMap<String, u64>>,
    /// Woken when the router can be reached again, so a registration that failed is tried now.
    registration_retry: Arc<tokio::sync::Notify>,
    /// The user's cloud (plan-drive), once connected and open.
    vault: Mutex<Option<Arc<ft_vault::Vault>>>,
    /// How clouds are reached: Google Drive in the app, a memory in the tests.
    cloud: OnceLock<Arc<dyn vault::Cloud>>,
    /// Where the vault keeps what waits to go up (set by the app).
    vault_dir: OnceLock<PathBuf>,
}

/// What a session's PIN is hashed with, so the hash is bound to this phone's key.
const SESSION_PIN_CONTEXT: &str = "flickertalk 2026-09-23 hidden session pin";
const SESSION_PIN_LENGTH: usize = 6;
/// Route capabilities besides the device's own (app#9): the router always sees eight, so at most
/// seven hidden sessions.
const SPARE_SLOTS: u8 = 7;
/// Settings keys: whether new contacts get receipts (app#6), and the weekly hours (app#7).
const RECEIPTS_DEFAULT: &str = "receipts_default";
const QUIET_HOURS: &str = "quiet_hours";
/// Set when the card changed under every contact (a new envelope key, A1): they all get it again.
const CARD_STALE: &str = "card_stale";
/// Set in a backup brought down to be swapped in (plan-recuperacion §5, 2026-09-28): its Olm
/// sessions are those of the day it was made, behind every contact's, so the first start renews
/// them.
pub(crate) const SESSIONS_BEHIND: &str = "sessions_behind";

impl Core {
    /// Opens the device's core; the first run creates the identity and route capability.
    pub async fn open(store: Store, key: [u8; 32], transport: Arc<dyn Transport>) -> Result<Arc<Self>> {
        let (identity, capability, envelope) = match store.identity().await? {
            Some(saved) => {
                let identity = Identity::unseal(&saved.sealed, &key)?;
                // An install from before the envelope gets one now; its contacts learn it with
                // the next card they get (A1).
                let envelope = match saved.envelope {
                    Some(sealed) => EnvelopeKey::unseal_at_rest(&sealed, &key)?,
                    None => {
                        let envelope = EnvelopeKey::generate();
                        store.save_envelope(&envelope.seal_at_rest(&key)).await?;
                        // Every contact holds a card without it: they get the new one once online.
                        store.set_setting(CARD_STALE, "1").await?;
                        envelope
                    }
                };
                if store.setting(SESSIONS_BEHIND).await?.as_deref() == Some("1") {
                    renew_sessions(&store, &identity, &key).await?;
                }
                (identity, RouteCapability::from_bytes(saved.route_capability), envelope)
            }
            None => {
                let identity = Identity::generate();
                let capability = RouteCapability::generate();
                let envelope = EnvelopeKey::generate();
                store.save_identity(&identity.seal(&key), capability.as_bytes()).await?;
                store.save_envelope(&envelope.seal_at_rest(&key)).await?;
                (identity, capability, envelope)
            }
        };
        // A call cannot survive the app stopping.
        store.finish_open_calls(now()).await?;
        // Seven spare route capabilities besides our own, made once (app#9).
        if store.spare_capabilities().await?.is_empty() {
            for slot in 1..=SPARE_SLOTS {
                store.add_spare_capability(slot, RouteCapability::generate().as_bytes()).await?;
            }
        }
        // The sessions the user left open are open again, with no PIN (2026-10-01, §108). An empty
        // one that is not open (a leftover of an app from before) goes now, as if closed (A3).
        let open_sessions = sessions::restore(&store, &key).await?;
        for session in store.empty_sessions().await? {
            if !open_sessions.contains_key(&session) {
                forget_session(&store, &session).await?;
            }
        }
        if store.setting(INSTALLED_AT).await?.is_none() {
            store.set_setting(INSTALLED_AT, &now().to_string()).await?;
        }
        let (events, _) = broadcast::channel(256);
        Ok(Arc::new_cyclic(|this| Self {
            this: this.clone(),
            early_answer: std::sync::Mutex::default(),
            audio_generation: std::sync::atomic::AtomicU64::new(0),
            call_clock: std::sync::Mutex::default(),
            preparation: std::sync::Mutex::default(),
            answer_refresh: std::sync::atomic::AtomicU64::new(native_calls::ANSWER_REFRESH_MS),
            device_id: identity.device_id(),
            identity: Mutex::new(identity),
            store,
            key,
            capability: RwLock::new(capability),
            envelope,
            transport,
            events,
            files_dir: OnceLock::new(),
            plugins_dir: OnceLock::new(),
            transfers: std::sync::Mutex::default(),
            active_call: std::sync::Mutex::default(),
            call_audio: RwLock::new(ft_media::platform_audio()),
            native_call: std::sync::Mutex::default(),
            native_setup: Mutex::new(()),
            ringing_offer: std::sync::Mutex::default(),
            call_video: RwLock::new(ft_media::platform_video()),
            video_detach: RwLock::new(None),
            app_visible: std::sync::atomic::AtomicBool::new(true),
            call_shown: std::sync::atomic::AtomicBool::new(true),
            call_audio_active: std::sync::atomic::AtomicBool::new(false),
            move_dir: OnceLock::new(),
            moving: std::sync::Mutex::default(),
            open_sessions: std::sync::Mutex::new(open_sessions),
            keeping_sessions: Mutex::new(()),
            registration: tokio::sync::watch::Sender::new(0),
            pongs: tokio::sync::watch::Sender::new(HashMap::new()),
            registration_retry: Arc::default(),
            vault: Mutex::new(None),
            cloud: OnceLock::new(),
            vault_dir: OnceLock::new(),
        }))
    }

    pub fn device_id(&self) -> &DeviceId {
        &self.device_id
    }

    pub fn route_capability(&self) -> RouteCapability {
        *self.capability.read().expect("capability poisoned")
    }

    /// The Olm identity key, raw and as base64: what a pre-key message carries in the clear.
    /// For the tests that check nothing of it reaches the router (A1).
    pub async fn exchange_key_bytes(&self) -> [u8; 32] {
        self.identity.lock().await.exchange_key().to_bytes()
    }

    pub async fn exchange_key_base64(&self) -> String {
        self.identity.lock().await.exchange_key().to_base64()
    }

    /// Seals `bytes` for the contact's eyes only, if their card carries an envelope key (A1):
    /// what goes through the router then names nobody. A contact without one gets the bytes
    /// as they are, as an older app would send them.
    pub async fn wrap_for(&self, contact: &str, bytes: Vec<u8>) -> Result<Vec<u8>> {
        let card = ContactCard::decode(&self.contact(contact).await?.card)?;
        match card.envelope_key() {
            Some(key) => Ok(Envelope { envelope: ENVELOPE_VERSION, sealed: key.seal(&bytes)? }.encode()),
            None => Ok(bytes),
        }
    }

    /// Opens what the router brought: an envelope sealed for this device, or bare bytes from an
    /// older app. An envelope that is not for us is an error, never something to read.
    pub async fn unwrap(&self, bytes: &[u8]) -> Result<Vec<u8>> {
        match Envelope::decode(bytes) {
            Ok(envelope) => {
                ensure!(envelope.envelope == ENVELOPE_VERSION, "an envelope of a kind this app does not know");
                self.envelope.open(&envelope.sealed)
            }
            Err(_) => Ok(bytes.to_vec()),
        }
    }

    /// Whether the card every contact holds is out of date (A1): after an upgrade that made the
    /// envelope key, the contacts keep sending bare mail until they get the new card.
    pub async fn card_stale(&self) -> Result<bool> {
        Ok(self.store.setting(CARD_STALE).await?.as_deref() == Some("1"))
    }

    /// Hands every contact the current card, once, and forgets that it was needed. A contact who
    /// cannot be reached now gets it with the next message anyway.
    pub async fn reintroduce(&self) -> Result<()> {
        for contact in self.store.all_contacts().await? {
            if contact.blocked {
                continue;
            }
            let _ = self.introduce(&contact).await;
        }
        self.store.set_setting(CARD_STALE, "0").await
    }

    /// Retires the link of the main list (or of a hidden session) and makes a new one (A5):
    /// whoever kept the old one can no longer reach this phone, and every contact of that list
    /// gets the new card at once, or with their next message. Returns the eight hashes the
    /// router must be told; the app re-registers with them.
    pub async fn renew_link(&self, session: Option<&str>) -> Result<[[u8; 32]; 8]> {
        let fresh = RouteCapability::generate();
        match session {
            None => {
                self.store.set_route_capability(fresh.as_bytes()).await?;
                *self.capability.write().expect("capability poisoned") = fresh;
            }
            Some(session) => {
                let slot = self.store.session_slot(session).await?.ok_or_else(|| anyhow!("the session has no slot"))?;
                self.store.set_spare_capability(slot, fresh.as_bytes()).await?;
            }
        }
        let mut contacts = match session {
            None => self.store.contacts().await?,
            Some(session) => self.store.session_contacts(session).await?,
        };
        // Whoever knows us through a circle of that list holds the old link too. The strangers
        // waiting in the requests do not get the new one: retiring the old is the point.
        let mut known: std::collections::HashSet<String> = contacts.iter().map(|contact| contact.device_id.clone()).collect();
        for circle in self.store.circles(session).await?.into_iter().filter(|circle| !circle.left) {
            for member in self.store.circle_members(&circle.id).await? {
                if member.device_id == self.device_id.as_str() || !known.insert(member.device_id.clone()) {
                    continue;
                }
                if let Some(contact) = self.store.contact(&member.device_id).await?.filter(|contact| !contact.blocked) {
                    contacts.push(contact);
                }
            }
        }
        for contact in contacts {
            let _ = self.introduce(&contact).await;
        }
        let _ = self.events.send(Event::ContactsChanged);
        self.registration_changed();
        self.route_capability_hashes().await
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn events(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }

    /// Signs a request to the router with the identity (§7).
    pub async fn sign(&self, message: &[u8]) -> String {
        self.identity.lock().await.sign(message).to_base64()
    }

    pub async fn signing_key(&self) -> String {
        self.identity.lock().await.signing_key().to_base64()
    }

    pub async fn name(&self) -> Result<Option<String>> {
        self.store.setting(NAME).await
    }

    pub async fn set_name(&self, name: &str) -> Result<()> {
        self.store.set_setting(NAME, name.trim()).await
    }

    /// Until when (ms) the app is free: a year from the first time it opened on this phone,
    /// counted only here (§41, strategy A).
    pub async fn free_until(&self) -> Result<i64> {
        let installed = self.store.setting(INSTALLED_AT).await?.and_then(|at| at.parse::<i64>().ok()).unwrap_or_else(now);
        Ok(installed + FREE_PERIOD.as_millis() as i64)
    }

    /// Where this phone stands: the free year, the age the user declared and what the Store says
    /// about the subscription. None of it leaves the phone (§45–§47).
    pub async fn plan(&self) -> Result<Plan> {
        Ok(Plan {
            free_until: self.free_until().await?,
            age: self.age_class().await?,
            paid_until: self.store.setting(PAID_UNTIL).await?.and_then(|at| at.parse().ok()).unwrap_or(0),
        })
    }

    pub async fn access(&self) -> Result<Access> {
        Ok(Access::of(now(), self.plan().await?))
    }

    pub async fn age_class(&self) -> Result<AgeClass> {
        Ok(AgeClass::of(self.store.setting(AGE_CLASS).await?.as_deref().unwrap_or_default()))
    }

    /// What the user answered about their age. Under 21 is always free (§40).
    pub async fn set_age_class(&self, age: AgeClass) -> Result<()> {
        self.store.set_setting(AGE_CLASS, age.as_str()).await
    }

    /// What the Store said about the subscription, checked by the platform bridge (§45).
    pub async fn set_entitlement(&self, until: i64) -> Result<()> {
        self.store.set_setting(PAID_UNTIL, &until.max(0).to_string()).await
    }

    /// Refuses what the plan does not allow. Receiving is never refused: a message that arrives is
    /// delivered whatever the plan says (§1).
    async fn allowed(&self, doing: Doing) -> Result<()> {
        ensure!(self.access().await?.may(doing), "the free year is over: {}", ASK_FOR_THE_EURO);
        Ok(())
    }

    /// Whether this phone uses the mailbox (§19); on by default.
    pub async fn mailbox(&self) -> Result<bool> {
        Ok(self.store.setting(MAILBOX).await?.as_deref() != Some("0"))
    }

    /// Stores the preference and tells every contact, who need it to decide where to send.
    pub async fn set_mailbox(&self, enabled: bool) -> Result<()> {
        self.store.set_setting(MAILBOX, if enabled { "1" } else { "0" }).await?;
        for contact in self.store.all_contacts().await? {
            let _ = self.send_control(&contact, Body::MailboxPreference { enabled }).await;
        }
        Ok(())
    }

    /// This device's Contact Card, to show as a QR code or share as a link.
    pub async fn my_card(&self) -> Result<ContactCard> {
        self.my_card_in(None).await
    }

    /// Our card as a hidden session hands it out: with the session's own route capability, so
    /// what its contacts send is known to be for it (app#9).
    pub async fn my_card_in(&self, session: Option<&str>) -> Result<ContactCard> {
        self.card_in(session, true).await
    }

    /// Our card as an app from before the envelope would make it: for the tests of the
    /// compatibility with such contacts (A1).
    pub async fn my_card_without_envelope(&self) -> Result<ContactCard> {
        self.card_in(None, false).await
    }

    async fn card_in(&self, session: Option<&str>, with_envelope: bool) -> Result<ContactCard> {
        let capability = self.capability_of(session).await?;
        let (name, mailbox) = (self.name().await?, self.mailbox().await?);
        let envelope = with_envelope.then(|| self.envelope.public_key());
        let mut identity = self.identity.lock().await;
        let card = ContactCard::create(&mut identity, name, capability, mailbox, envelope);
        // Creating the first card adds the Olm fallback key to the account.
        self.store.save_identity(&identity.seal(&self.key), self.route_capability().as_bytes()).await?;
        Ok(card)
    }

    /// Adds the owner of a scanned card and introduces ourselves to them.
    pub async fn add_contact(&self, link: &str, name: Option<String>) -> Result<Contact> {
        self.add_contact_in(link, name, None).await
    }

    /// Adds the owner of a card to the main list or, given one, to a hidden session. A contact
    /// already known stays where they were.
    pub async fn add_contact_in(&self, link: &str, name: Option<String>, session: Option<&str>) -> Result<Contact> {
        if let Some(session) = session {
            ensure!(self.open_sessions.lock().expect("sessions poisoned").contains_key(session), "that session is not open");
        }
        let card = ContactCard::from_link(link)?;
        let device_id = card.device_id();
        if device_id == self.device_id {
            bail!("that is your own contact card");
        }
        let name = name
            .map(|name| clean_name(&name))
            .filter(|name| !name.is_empty())
            .or_else(|| card.name().map(clean_name).filter(|name| !name.is_empty()))
            .unwrap_or_else(|| short_name(&device_id));
        // Scanning someone is choosing them: a stranger waiting in the requests is accepted.
        self.store
            .add_contact(&NewContact {
                device_id: device_id.to_string(),
                name,
                card: card.encode(),
                mailbox: card.mailbox(),
                session: session.map(str::to_owned),
                receipts: self.receipts_default().await?,
                accepted: true,
            })
            .await?;
        {
            let identity = self.identity.lock().await;
            // They may have scanned us first: then a channel exists already.
            if self.store.channel(device_id.as_str()).await?.is_none() {
                let channel = Channel::open(&identity, &card.contact_keys())?;
                self.store.save_channel(device_id.as_str(), &channel.seal(&self.key)).await?;
            }
        }
        let contact = self.contact(device_id.as_str()).await?;
        self.introduce(&contact).await?;
        let _ = self.events.send(Event::ContactsChanged);
        Ok(contact)
    }

    fn pin_hash(&self, pin: &str) -> Result<[u8; 32]> {
        ensure!(pin.len() == SESSION_PIN_LENGTH && pin.bytes().all(|byte| byte.is_ascii_digit()), "a PIN is six digits");
        Ok(*blake3::keyed_hash(&blake3::derive_key(SESSION_PIN_CONTEXT, &self.key), pin.as_bytes()).as_bytes())
    }

    /// Opens the hidden session whose PIN this is or, if none has it, a new empty one, and
    /// returns its id: every PIN is valid and nothing tells the two apart (A3). An empty one goes
    /// when it is closed. `None` only when the seven slots are all taken: the screen shows an
    /// empty session all the same.
    pub async fn open_session(&self, pin: &str) -> Result<Option<String>> {
        let hash = self.pin_hash(pin)?;
        if let Some(id) = self.open_session_with(&hash).await? {
            self.keep_open_sessions().await?;
            return Ok(Some(id));
        }
        let Some(slot) = self.free_slot().await? else { return Ok(None) };
        let id = MessageId::new().to_string();
        self.store.add_session(&id, &hash, slot).await?;
        sessions::redraw(&self.store, slot).await?;
        self.open_sessions.lock().expect("sessions poisoned").insert(id.clone(), slot);
        self.keep_open_sessions().await?;
        Ok(Some(id))
    }

    /// Keeps which sessions are open, for the next start (2026-10-01, §108).
    /// The router hears of it too: which slots are silent changed with it.
    async fn keep_open_sessions(&self) -> Result<()> {
        {
            let _one_at_a_time = self.keeping_sessions.lock().await;
            sessions::keep(&self.store, &self.key, &self.open_sessions()).await?;
        }
        self.registration_changed();
        // What plugins set to ring inside a session rings only while it is open (§108): the
        // phone's alarm clock is told again.
        let _ = self.events.send(Event::RemindersChanged);
        Ok(())
    }

    async fn open_session_with(&self, hash: &[u8; 32]) -> Result<Option<String>> {
        let Some(id) = self.store.session_by_pin(hash).await? else { return Ok(None) };
        let slot = match self.store.session_slot(&id).await? {
            Some(slot) => slot,
            None => {
                // Made before slots: it gets one now and tells its contacts the new way in.
                let slot = self.free_slot().await?.ok_or_else(|| anyhow!("no room for another session"))?;
                self.store.set_session_slot(&id, slot).await?;
                sessions::redraw(&self.store, slot).await?;
                self.open_sessions.lock().expect("sessions poisoned").insert(id.clone(), slot);
                for contact in self.store.session_contacts(&id).await? {
                    let _ = self.introduce(&contact).await;
                }
                slot
            }
        };
        self.open_sessions.lock().expect("sessions poisoned").insert(id.clone(), slot);
        Ok(Some(id))
    }

    /// Takes a hidden session away for good (A3): its contacts, their history and files, and
    /// its slot, which a new session may use again with a new link. The router must get the new
    /// hashes.
    pub async fn remove_session(&self, session: &str) -> Result<()> {
        ensure!(self.open_sessions.lock().expect("sessions poisoned").contains_key(session), "that session is not open");
        let mut files = Vec::new();
        for contact in self.store.session_contacts(session).await? {
            files.extend(self.store.files(&contact.device_id).await?);
        }
        self.cut_off(session).await?;
        forget_session(&self.store, session).await?;
        self.forget_bytes(files).await;
        self.open_sessions.lock().expect("sessions poisoned").remove(session);
        self.keep_open_sessions().await?;
        let _ = self.events.send(Event::ContactsChanged);
        Ok(())
    }

    async fn free_slot(&self) -> Result<Option<u8>> {
        let used = self.store.used_slots().await?;
        Ok((1..=SPARE_SLOTS).find(|slot| !used.contains(slot)))
    }

    /// Leaves the session: from now on it receives in silence, until its PIN opens it again; only
    /// this closes it (2026-10-01: a start does not). One with nobody in it goes for good, with
    /// its link, so that PINs never fill the slots (A3). Returns whether it went: then the router
    /// must get the new hashes.
    pub async fn close_session(&self, session: &str) -> Result<bool> {
        self.open_sessions.lock().expect("sessions poisoned").remove(session);
        self.keep_open_sessions().await?;
        self.cut_off(session).await?;
        if !self.store.empty_sessions().await?.iter().any(|empty| empty == session) {
            return Ok(false);
        }
        forget_session(&self.store, session).await?;
        self.registration_changed();
        Ok(true)
    }

    /// A session left or deleted closes its direct connections at once (2026-10-01): the router
    /// stops what comes for a silent slot, but cannot close a connection already open. A call with
    /// one of its contacts is hung up first, so that its end still goes out by that connection;
    /// from now on the router would hold back their side of it.
    async fn cut_off(&self, session: &str) -> Result<()> {
        let all = self.store.all_contacts().await?;
        let contacts: Vec<String> = all.into_iter().filter(|contact| contact.session.as_deref() == Some(session)).map(|contact| contact.device_id).collect();
        self.hang_up_with(&contacts).await?;
        for contact in &contacts {
            self.transport.disconnect(contact).await;
        }
        Ok(())
    }

    /// The sessions open right now, oldest first.
    pub fn open_sessions(&self) -> Vec<String> {
        let mut open: Vec<String> = self.open_sessions.lock().expect("sessions poisoned").keys().cloned().collect();
        open.sort();
        open
    }

    /// The slots of the sessions open right now: a wake-up for any other slot stays quiet.
    pub fn open_slots(&self) -> Vec<u8> {
        let mut slots: Vec<u8> = self.open_sessions.lock().expect("sessions poisoned").values().copied().collect();
        slots.sort();
        slots
    }

    /// Changes whenever the router must be told something new (2026-10-01): the hashes or the
    /// silent slots.
    pub fn registration_changes(&self) -> tokio::sync::watch::Receiver<u64> {
        self.registration.subscribe()
    }

    fn registration_changed(&self) {
        self.registration.send_modify(|generation| *generation += 1);
    }

    /// What a registration that failed waits on besides its timer: the router is back.
    pub fn registration_retry(&self) -> Arc<tokio::sync::Notify> {
        self.registration_retry.clone()
    }

    /// The router can be reached again (its socket connected): a registration waiting to be
    /// retried goes now.
    pub fn router_reachable(&self) {
        self.registration_retry.notify_one();
    }

    /// The slots the router must not push for (2026-10-01, §108): bit i for slot i.
    pub async fn silent_slots(&self) -> Result<u8> {
        let (used, noise) = (self.store.used_slots().await?, sessions::noise(&self.store).await?);
        Ok(sessions::silent_mask(&used, &self.open_slots(), noise))
    }

    /// The hashes the router gets (app#9): our own capability first, then the seven spares,
    /// used or not. Always the same eight.
    pub async fn route_capability_hashes(&self) -> Result<[[u8; 32]; 8]> {
        let mut hashes = [self.route_capability().hash(); 8];
        for (slot, capability) in self.store.spare_capabilities().await? {
            hashes[slot as usize] = RouteCapability::from_bytes(capability).hash();
        }
        Ok(hashes)
    }

    /// The route capability a session hands out, or our own for the main list.
    async fn capability_of(&self, session: Option<&str>) -> Result<RouteCapability> {
        let Some(session) = session else { return Ok(self.route_capability()) };
        let slot = self.store.session_slot(session).await?.ok_or_else(|| anyhow!("the session has no slot"))?;
        let spare = self.store.spare_capabilities().await?.into_iter().find(|(spare, _)| *spare == slot);
        spare.map(|(_, capability)| RouteCapability::from_bytes(capability)).ok_or_else(|| anyhow!("no capability for the slot"))
    }

    /// The session a sender belongs to, from the hash of our capability they used. A hash of
    /// no link of ours (a session gone, a link renewed) reaches nobody (A3, A5): not the session
    /// that takes the slot next, and not the main list.
    async fn session_via(&self, via: Option<&[u8]>) -> Result<Option<String>> {
        let Some(via) = via else { return Ok(None) };
        let hashes = self.route_capability_hashes().await?;
        if hashes[0].as_slice() == via {
            return Ok(None);
        }
        let Some(slot) = (1..8).find(|slot| hashes[*slot].as_slice() == via) else { bail!("that link is retired") };
        match self.store.session_with_slot(slot as u8).await? {
            Some(session) => Ok(Some(session)),
            None => bail!("that link is retired"),
        }
    }

    /// Whether what this contact sends must make no noise: they belong to a closed session, or
    /// they are a stranger waiting in the requests (A5).
    pub(crate) fn silent(&self, contact: &Contact) -> bool {
        !contact.accepted
            || contact.session.as_deref().is_some_and(|session| !self.open_sessions.lock().expect("sessions poisoned").contains_key(session))
    }

    /// The strangers who wrote first and wait for a yes (A5), with what they said, newest first.
    pub async fn requests(&self) -> Result<Vec<Conversation>> {
        self.store.request_conversations(None).await
    }

    /// The same, for a hidden session.
    pub async fn session_requests(&self, session: &str) -> Result<Vec<Conversation>> {
        self.store.request_conversations(Some(session)).await
    }

    /// Yes to a stranger (A5): they join the list, get our card, and what they sent that waited
    /// (a file, say) is pulled like anyone else's.
    pub async fn accept_contact(&self, contact: &str) -> Result<()> {
        let stored = self.contact(contact).await?;
        if stored.accepted {
            return Ok(());
        }
        self.store.set_accepted(contact, true).await?;
        let _ = self.events.send(Event::ContactsChanged);
        let _ = self.events.send(Event::MessagesChanged { contact: contact.to_owned() });
        let accepted = self.contact(contact).await?;
        let _ = self.introduce(&accepted).await;
        let _ = self.resume_files_from(contact, Duration::ZERO).await;
        Ok(())
    }

    /// No to a stranger (A5): blocked, so nothing more of theirs reaches this phone, and out of
    /// the requests. What they sent stays with the block, for a report.
    pub async fn decline_contact(&self, contact: &str) -> Result<()> {
        self.contact(contact).await?;
        self.block(contact, true).await
    }

    /// Writing to someone is choosing them: a stranger in the requests is accepted by an answer.
    async fn chosen(&self, contact: &str) -> Result<Contact> {
        let stored = self.contact(contact).await?;
        if !stored.accepted {
            self.accept_contact(contact).await?;
            return self.contact(contact).await;
        }
        Ok(stored)
    }

    /// The call history the screen may show: the main list's calls and those of open sessions.
    /// A closed session's calls stay stored and come back when its PIN opens it again.
    pub async fn visible_calls(&self, limit: i64) -> Result<Vec<ft_storage::CallRecord>> {
        let mut visible = Vec::new();
        for call in self.store.calls(limit).await? {
            let shown = match self.store.contact(&call.contact).await? {
                Some(contact) => !self.silent(&contact),
                None => true,
            };
            if shown {
                visible.push(call);
            }
        }
        Ok(visible)
    }

    /// How a stored packet is acknowledged: delivered, or only received when this phone tells
    /// the contact nothing (app#6). Either way the sender stops retrying (§27).
    pub(crate) fn acknowledgement(&self, contact: &Contact, id: MessageId) -> Body {
        if contact.rules.receipts {
            Body::Delivered { ids: vec![id] }
        } else {
            Body::Received { ids: vec![id] }
        }
    }

    /// Tells the UI the conversation changed, unless the contact's session is closed.
    pub(crate) fn announce_messages(&self, contact: &Contact) {
        if !self.silent(contact) {
            let _ = self.events.send(Event::MessagesChanged { contact: contact.device_id.clone() });
        }
    }

    /// What this phone takes from a contact and tells them (issues app#4–#6). Only here.
    pub async fn set_rules(&self, contact: &str, rules: ContactRules) -> Result<()> {
        self.contact(contact).await?;
        self.store.set_rules(contact, &rules).await?;
        let _ = self.events.send(Event::ContactsChanged);
        Ok(())
    }

    /// Tells the contact the user is writing to them (2026-10-05). Only over a direct connection
    /// already open: it never opens one, never waits in the mailbox and never reaches the server;
    /// with no connection it says nothing. Nothing goes to a blocked contact, to a stranger still in
    /// the requests, or to a contact whose `typing` rule is off. Returns whether it was sent.
    pub async fn typing(&self, contact: &str) -> Result<bool> {
        let contact = self.contact(contact).await?;
        if contact.blocked || !contact.accepted || !contact.rules.typing {
            return Ok(false);
        }
        self.transmit_open(&contact, &Packet::new(Body::Typing)).await
    }

    /// Whether contacts added from now on are told their messages arrived and were read.
    pub async fn receipts_default(&self) -> Result<bool> {
        Ok(self.store.setting(RECEIPTS_DEFAULT).await?.as_deref() != Some("0"))
    }

    pub async fn set_receipts_default(&self, receipts: bool) -> Result<()> {
        self.store.set_setting(RECEIPTS_DEFAULT, if receipts { "1" } else { "0" }).await
    }

    /// The weekly hours when this phone may make noise (app#7), as JSON; `None` when off.
    pub async fn quiet_hours(&self) -> Result<Option<String>> {
        Ok(self.store.setting(QUIET_HOURS).await?.filter(|hours| !hours.is_empty()))
    }

    /// The weekly hours as the native side reads them; empty when off.
    pub async fn quiet_week(&self) -> Result<String> {
        Ok(match self.quiet_hours().await? {
            Some(json) => serde_json::from_str::<Week>(&json)?.compact(),
            None => String::new(),
        })
    }

    /// Keeps the weekly hours after checking them, or turns them off with `None`.
    pub async fn set_quiet_hours(&self, hours: Option<&str>) -> Result<()> {
        let kept = match hours {
            Some(json) => {
                let week: Week = serde_json::from_str(json).context("unreadable hours")?;
                week.check()?;
                serde_json::to_string(&week)?
            }
            None => String::new(),
        };
        self.store.set_setting(QUIET_HOURS, &kept).await
    }

    /// Safety number with a contact (§29), the same on both phones.
    pub async fn fingerprint(&self, contact: &str) -> Result<String> {
        let card = ContactCard::decode(&self.contact(contact).await?.card)?;
        let mine = self.identity.lock().await.signing_key();
        Ok(ft_contacts::fingerprint(&mine, &card.signing_key()))
    }

    pub async fn rename_contact(&self, contact: &str, name: &str) -> Result<()> {
        self.store.rename_contact(contact, name.trim()).await?;
        let _ = self.events.send(Event::ContactsChanged);
        Ok(())
    }

    /// Removes a contact and its local conversation from this phone.
    pub async fn remove_contact(&self, contact: &str) -> Result<()> {
        self.contact(contact).await?;
        self.store.remove_contact(contact).await?;
        let _ = self.events.send(Event::ContactsChanged);
        Ok(())
    }

    /// How long this phone keeps the conversation with a contact, and how long a read message
    /// stays after being read; both in seconds, 0 for forever and never (issue app#1). It is a
    /// choice of this phone: nothing of it travels.
    pub async fn set_history(&self, contact: &str, keep_for: i64, burn_after_read: i64) -> Result<()> {
        self.store.set_history(contact, keep_for.max(0), burn_after_read.max(0)).await?;
        let _ = self.events.send(Event::ContactsChanged);
        self.sweep_history().await
    }

    /// Applies the history rules, deleting the bytes of the files that go with the messages.
    pub async fn sweep_history(&self) -> Result<()> {
        self.sweep_history_at(now()).await
    }

    pub async fn sweep_history_at(&self, at: i64) -> Result<()> {
        let watched: Vec<String> = self
            .store
            .contacts()
            .await?
            .into_iter()
            .filter(|contact| contact.keep_for > 0 || contact.burn_after_read > 0)
            .map(|contact| contact.device_id)
            .collect();
        let mut theirs = Vec::new();
        for contact in &watched {
            theirs.extend(self.store.files(contact).await?);
        }

        for contact in self.store.sweep(at).await? {
            let _ = self.events.send(Event::MessagesChanged { contact });
            let _ = self.events.send(Event::ContactsChanged);
        }
        // The bytes of the files whose message is gone.
        self.forget_bytes(theirs).await;
        Ok(())
    }

    /// Sends a message on to someone else (§61): the same text, or the same file from the bytes
    /// this phone already has. It becomes a message of its own; nothing of where it came from
    /// travels with it.
    pub async fn forward(&self, message_id: &str, to: &str) -> Result<String> {
        let stored = self.store.message(message_id).await?.context("that message is not here")?;
        match self.store.file(message_id).await? {
            Some(file) => {
                // A file of ours has its bytes here from the start; one that is arriving only
                // when it has arrived whole (§62). "Complete" is about the transfer, not the bytes.
                ensure!(stored.outgoing || file.complete, "that file is not here whole yet");
                let path = self.file_path(&file);
                ensure!(path.exists(), "the bytes of that file are no longer here");
                self.send_file(to, &path, &file.name, &file.mime).await
            }
            None => self.send_text(to, &stored.body).await,
        }
    }

    /// Erases one message from this phone, with its file and its place in the outbox (§61). It is
    /// our copy: nothing is sent to the other side, which keeps theirs.
    pub async fn forget_message(&self, message_id: &str) -> Result<()> {
        let stored = self.store.message(message_id).await?.context("that message is not here")?;
        let file = self.store.file(message_id).await?;
        if !self.store.forget_message(message_id).await? {
            bail!("that message is not here");
        }
        self.forget_bytes(file.into_iter().collect()).await;
        let _ = self.events.send(Event::MessagesChanged { contact: stored.contact });
        Ok(())
    }

    /// Writes a text now to be sent at `send_at` (2026-10-06, milliseconds): it waits in this
    /// phone's outbox until then and goes like any other text, from this phone, if it is awake
    /// (Android runs the core in the background; an iPhone sends it when it wakes the app).
    /// Between a minute and a year from now. Deleting the message here before its time cancels it.
    pub async fn schedule_text(&self, contact: &str, text: &str, send_at: i64, reply_to: Option<&str>) -> Result<String> {
        let text = text.trim();
        ensure!(!text.is_empty(), "nothing to send");
        let soonest = now() + 60_000;
        ensure!(send_at >= soonest, "that time is too soon");
        ensure!(send_at <= now() + 366 * 24 * 60 * 60 * 1000, "that time is too far away");
        self.chosen(contact).await?;
        let reply_to = match reply_to {
            Some(quoted) => {
                let quoted_message = self.store.message(quoted).await?.context("the message answered is not here")?;
                ensure!(quoted_message.contact == contact, "the message answered is from another conversation");
                Some(quoted.to_owned())
            }
            None => None,
        };
        let started = !self.store.messages(contact, 1).await?.is_empty();
        self.allowed(if started { Doing::Reply } else { Doing::Start }).await?;
        let message_id = MessageId::new().to_string();
        self.store
            .insert_message(&Message {
                message_id: message_id.clone(),
                contact: contact.to_owned(),
                outgoing: true,
                body: text.to_owned(),
                sent_at: send_at,
                received_at: now(),
                state: MessageState::Pending,
            })
            .await?;
        if let Some(quoted) = reply_to {
            self.store.set_reply(&message_id, &quoted).await?;
        }
        self.store.enqueue_at(&message_id, contact, now(), send_at).await?;
        let _ = self.events.send(Event::MessagesChanged { contact: contact.to_owned() });
        Ok(message_id)
    }

    /// The messages of the conversation with these words (2026-10-05), newest first, at most fifty.
    /// On this phone only: nothing of the search leaves it.
    pub async fn search(&self, contact: &str, query: &str) -> Result<Vec<Message>> {
        self.contact(contact).await?;
        self.store.search_messages(contact, query, 50).await
    }

    /// Says one of our texts again with other words (2026-10-05): here at once, and told to the
    /// contact through the update queue (`tell`), directly or through the mailbox. Only our own text, not a file,
    /// not one taken back, and not one the router refused; within a day of sending it.
    pub async fn edit_message(&self, message_id: &str, text: &str) -> Result<()> {
        let text = text.trim();
        ensure!(!text.is_empty(), "nothing to say");
        let message = self.store.message(message_id).await?.context("that message is not here")?;
        ensure!(message.outgoing, "only our own words can change");
        ensure!(message.state != MessageState::NotSent, "a message that was not sent is sent again, not edited");
        ensure!(self.store.file(message_id).await?.is_none(), "a file has no words to change");
        ensure!(!self.store.is_deleted(message_id).await?, "that message was taken back");
        ensure!(now() - message.sent_at <= EDIT_WINDOW_MS, "too late to change it");
        let contact = self.contact(&message.contact).await?;
        let packet = Packet::new(Body::Edit { of: MessageId::parse(message_id)?, text: text.to_owned() });
        self.store.edit_text(message_id, text, packet.sent_at as i64).await?;
        let _ = self.events.send(Event::MessagesChanged { contact: contact.device_id.clone() });
        self.tell(&contact, &packet).await
    }

    /// Takes one of our messages back for both sides (2026-10-05): its words and its file go
    /// here, the mark that it was there stays, and the contact is told through the update queue. A phone
    /// that already has it shows the mark too; an older app keeps the message. One still waiting
    /// here is cancelled; the contact is told anyway (2026-10-06), because a try may be on its way
    /// or may already have reached them, and a phone that never had it drops the taking back. Only
    /// one written for later whose time has not come never went anywhere: it goes from this phone
    /// altogether, with no mark (2026-10-06), and nothing is told.
    pub async fn delete_for_everyone(&self, message_id: &str) -> Result<()> {
        let message = self.store.message(message_id).await?.context("that message is not here")?;
        ensure!(message.outgoing, "only our own messages can be taken back from the other side");
        let contact = self.contact(&message.contact).await?;
        if self.store.scheduled_all().await?.get(message_id).is_some_and(|send_at| *send_at > now()) {
            return self.forget_message(message_id).await;
        }
        let file = self.store.file(message_id).await?;
        self.store.mark_deleted(message_id).await?;
        self.forget_bytes(file.into_iter().collect()).await;
        let _ = self.events.send(Event::MessagesChanged { contact: contact.device_id.clone() });
        let packet = Packet::new(Body::Delete { of: MessageId::parse(message_id)? });
        self.tell(&contact, &packet).await
    }

    /// Pins a message on this phone, or unpins it (2026-10-05): a choice of this phone, like the
    /// contact's name; nothing of it travels.
    pub async fn pin_message(&self, message_id: &str, pinned: bool) -> Result<()> {
        let message = self.store.message(message_id).await?.context("that message is not here")?;
        self.store.set_pinned(message_id, pinned).await?;
        let _ = self.events.send(Event::MessagesChanged { contact: message.contact });
        Ok(())
    }

    /// One emoji on a message of the conversation (2026-10-05), theirs or ours; `None` takes it
    /// back. Shown here at once, and told to the contact through the update queue: directly, or
    /// through the mailbox when there is one, and again until they have it.
    pub async fn react(&self, contact: &str, message_id: &str, emoji: Option<&str>) -> Result<()> {
        let contact = self.contact(contact).await?;
        ensure!(contact.accepted && !contact.blocked, "not a conversation to react in");
        let message = self.store.message(message_id).await?.context("that message is not here")?;
        ensure!(message.contact == contact.device_id, "that message is from another conversation");
        let emoji = emoji.map(str::trim).filter(|emoji| !emoji.is_empty());
        if let Some(emoji) = emoji {
            ensure!(is_reaction(emoji), "a reaction is one emoji");
        }
        let packet = Packet::new(Body::Reaction { to: MessageId::parse(message_id)?, emoji: emoji.unwrap_or("").to_owned() });
        self.store.set_reaction(message_id, true, emoji, packet.sent_at as i64).await?;
        let _ = self.events.send(Event::MessagesChanged { contact: contact.device_id.clone() });
        self.tell(&contact, &packet).await
    }

    pub async fn block(&self, contact: &str, blocked: bool) -> Result<()> {
        self.store.set_blocked(contact, blocked).await?;
        if blocked {
            self.transport.disconnect(contact).await;
        }
        let _ = self.events.send(Event::ContactsChanged);
        Ok(())
    }

    /// Stores the text and tries to deliver it right away; returns its message id.
    pub async fn send_text(&self, contact: &str, text: &str) -> Result<String> {
        self.send_text_replying(contact, text, None).await
    }

    /// Like `send_text`, answering one of the conversation's messages (2026-10-05): the other
    /// phone shows the quote over the text. The quoted message has to be in this conversation.
    pub async fn send_text_replying(&self, contact: &str, text: &str, reply_to: Option<&str>) -> Result<String> {
        let text = text.trim();
        if text.is_empty() {
            bail!("nothing to send");
        }
        self.chosen(contact).await?;
        let reply_to = match reply_to {
            Some(quoted) => {
                let quoted_message = self.store.message(quoted).await?.context("the message answered is not here")?;
                ensure!(quoted_message.contact == contact, "the message answered is from another conversation");
                Some(MessageId::parse(quoted)?)
            }
            None => None,
        };
        // Answering is always allowed; writing to someone for the first time is starting (§42).
        let started = !self.store.messages(contact, 1).await?.is_empty();
        self.allowed(if started { Doing::Reply } else { Doing::Start }).await?;
        let packet = Packet::new(Body::Message { text: text.to_owned(), reply_to });
        let message_id = packet.id.to_string();
        self.store
            .insert_message(&Message {
                message_id: message_id.clone(),
                contact: contact.to_owned(),
                outgoing: true,
                body: text.to_owned(),
                sent_at: packet.sent_at as i64,
                received_at: packet.sent_at as i64,
                state: MessageState::Pending,
            })
            .await?;
        if let Some(quoted) = reply_to {
            self.store.set_reply(&message_id, &quoted.to_string()).await?;
        }
        self.store.enqueue(&message_id, contact, now()).await?;
        let _ = self.events.send(Event::MessagesChanged { contact: contact.to_owned() });

        if let Some(entry) = self.store.outbox().await?.into_iter().find(|e| e.message_id == message_id) {
            self.attempt(&entry).await;
        }
        Ok(message_id)
    }

    /// Queues again a message the router refused (§84), with the same id: the recipient keeps
    /// it once even if an earlier try did reach them. Only a message that was not sent goes again.
    pub async fn resend(&self, message_id: &str) -> Result<()> {
        let message = self.store.message(message_id).await?.context("that message is not here")?;
        ensure!(message.outgoing && message.state == MessageState::NotSent, "only a message that was not sent goes again");
        self.store.advance(std::slice::from_ref(&message.message_id), MessageState::Pending).await?;
        self.store.enqueue(message_id, &message.contact, now()).await?;
        let _ = self.events.send(Event::MessagesChanged { contact: message.contact.clone() });
        if let Some(entry) = self.store.outbox().await?.into_iter().find(|e| e.message_id == message_id) {
            self.attempt(&entry).await;
        }
        Ok(())
    }

    /// Sends one packet as it is, once, by whichever way works. For tests and tools that need
    /// to put a given packet on the wire (an offer with an impossible size, an old clock).
    pub async fn send_raw_to(&self, contact: &str, packet: &Packet) -> Result<()> {
        let contact = self.contact(contact).await?;
        self.transmit(&contact, packet).await?;
        Ok(())
    }

    /// Retries the outbox entries that are due, of contacts and of circles.
    pub async fn retry_due(&self) -> Result<()> {
        for entry in self.store.due(now()).await? {
            self.attempt(&entry).await;
        }
        for entry in self.store.updates_due(now()).await? {
            self.attempt_update(&entry).await;
        }
        for entry in self.store.circle_due(now()).await? {
            let _ = self.deliver_circle(&entry).await;
        }
        Ok(())
    }

    /// Sends the texts written for later whose time has come (2026-10-06), and nothing else: what
    /// the app does out of the foreground, where the rest of the outbox waits for it to come back.
    pub async fn send_scheduled_due(&self) -> Result<()> {
        let later = self.store.scheduled_all().await?;
        for entry in self.store.due(now()).await? {
            if later.contains_key(&entry.message_id) {
                self.attempt(&entry).await;
            }
        }
        Ok(())
    }

    /// A direct connection with the contact opened (2026-09-29): what was already tried and still
    /// waits for its receipt goes over it now, even if it is in the mailbox too; the recipient
    /// shows it once. What is being sent for the first time is left to its own send.
    pub(crate) async fn retry_contact_now(&self, contact: &str) -> Result<()> {
        for entry in self.store.outbox().await? {
            if entry.contact == contact && entry.attempts > 0 {
                self.attempt(&entry).await;
            }
        }
        for entry in self.store.update_outbox().await? {
            if entry.contact == contact && entry.attempts > 0 {
                self.attempt_update(&entry).await;
            }
        }
        for entry in self.store.circle_outbox().await? {
            if entry.contact == contact && entry.attempts > 0 {
                let _ = self.deliver_circle(&entry).await;
            }
        }
        Ok(())
    }

    /// Retries every pending message now (the contact came online, say). A message written for
    /// later (2026-10-06) keeps waiting for its time.
    pub async fn retry_now(&self) -> Result<()> {
        let later = self.store.scheduled_all().await?;
        let moment = now();
        for entry in self.store.outbox().await? {
            if later.get(&entry.message_id).is_some_and(|send_at| *send_at > moment) {
                continue;
            }
            self.attempt(&entry).await;
        }
        for entry in self.store.update_outbox().await? {
            self.attempt_update(&entry).await;
        }
        self.deliver_circle_queue().await;
        Ok(())
    }

    /// Marks the conversation as read and tells the sender (§38).
    pub async fn mark_read(&self, contact: &str) -> Result<()> {
        let unread = self.store.unread(contact).await?;
        if unread.is_empty() {
            return Ok(());
        }
        self.store.advance(&unread, MessageState::Read).await?;
        let _ = self.events.send(Event::MessagesChanged { contact: contact.to_owned() });
        let ids = unread.iter().filter_map(|id| MessageId::parse(id).ok()).collect();
        let contact = self.contact(contact).await?;
        if contact.rules.receipts {
            let _ = self.send_control(&contact, Body::Read { ids }).await;
        }
        Ok(())
    }

    /// A packet from the network: the DataChannel or the mailbox. Mail comes in an envelope
    /// sealed for this device (A1); what a direct connection brings is bare.
    pub async fn receive(&self, bytes: &[u8]) -> Result<()> {
        let bytes = self.unwrap(bytes).await?;
        let Some((contact, packet, first_contact)) = self.open_sealed(&bytes).await? else {
            return Ok(());
        };
        self.handle(&contact, packet, first_contact).await
    }

    /// Encrypts a signal body (offer or answer) for a contact: its plaintext is a `Packet`.
    pub async fn seal_signal(&self, contact: &str, body: Body) -> Result<Vec<u8>> {
        let contact = self.contact(contact).await?;
        let _identity = self.identity.lock().await;
        let mut channel = self.channel(&contact.device_id).await?;
        let sealed = channel.encrypt(&self.device_id, &Packet::new(body).encode())?;
        self.store.save_channel(&contact.device_id, &channel.seal(&self.key)).await?;
        Ok(sealed.encode())
    }

    /// Decrypts a signal from a contact, or from someone whose offer carries their card (a first
    /// contact). Returns the sender and the body; `None` if the sender is blocked.
    pub async fn open_signal(&self, bytes: &[u8]) -> Result<Option<(String, Body)>> {
        Ok(self.open_sealed(bytes).await?.map(|(contact, packet, first_contact)| {
            if first_contact {
                let _ = self.events.send(Event::ContactsChanged);
            }
            (contact.device_id, packet.body)
        }))
    }

    /// Where to reach a contact.
    pub async fn peer(&self, contact: &str) -> Result<Peer> {
        let contact = self.contact(contact).await?;
        let card = ContactCard::decode(&contact.card)?;
        Ok(Peer { device_id: contact.device_id, capability: card.route_capability() })
    }

    /// Decrypts an incoming `Sealed`, adding the sender if it is a first contact that presents a
    /// valid card. `None` when the sender is blocked (dropped silently, §35).
    async fn open_sealed(&self, bytes: &[u8]) -> Result<Option<(Contact, Packet, bool)>> {
        let sealed = Sealed::decode(bytes)?;
        let from = DeviceId::parse(&sealed.from)?;
        let known = self.store.contact(from.as_str()).await?;
        if known.as_ref().is_some_and(|contact| contact.blocked) {
            return Ok(None);
        }

        let (packet, first_contact) = {
            let mut identity = self.identity.lock().await;
            match &known {
                Some(contact) => {
                    let card = ContactCard::decode(&contact.card)?;
                    let mut channel = self.channel(from.as_str()).await?;
                    let plaintext = channel.decrypt(&mut identity, card.contact_keys().exchange_key, &sealed)?;
                    self.store.save_channel(from.as_str(), &channel.seal(&self.key)).await?;
                    (Packet::decode(&plaintext)?, false)
                }
                None => {
                    let (channel, plaintext, sender_key) = accept_first_contact(&mut identity, &sealed)?;
                    let packet = Packet::decode(&plaintext)?;
                    let (card, via) = match &packet.body {
                        Body::ContactCard { card, via } | Body::Offer { card: Some(card), via, .. } => {
                            (ContactCard::decode(card)?, via.clone())
                        }
                        _ => bail!("a first contact must introduce itself with its contact card"),
                    };
                    // Scanned from a hidden session's card: they belong to that session (app#9).
                    let session = self.session_via(via.as_deref()).await?;
                    if card.device_id() != from || card.contact_keys().exchange_key != sender_key {
                        bail!("the contact card does not match its sender");
                    }
                    let name = card.name().map(clean_name).filter(|name| !name.is_empty()).unwrap_or_else(|| short_name(&from));
                    // They wrote first with our link: they wait in the requests until the user
                    // says yes (A5). Unless the user scanned them already, which is a yes.
                    self.store
                        .add_contact(&NewContact {
                            device_id: from.to_string(),
                            name,
                            card: card.encode(),
                            mailbox: card.mailbox(),
                            session,
                            receipts: self.receipts_default().await?,
                            accepted: false,
                        })
                        .await?;
                    self.store.save_channel(from.as_str(), &channel.seal(&self.key)).await?;
                    (packet, true)
                }
            }
        };
        self.store.set_introduced(from.as_str()).await?;
        Ok(Some((self.contact(from.as_str()).await?, packet, first_contact)))
    }

    async fn handle(&self, contact: &Contact, packet: Packet, first_contact: bool) -> Result<()> {
        let id = contact.device_id.as_str();
        match packet.body {
            Body::Message { .. } if !contact.rules.accepts_chat => {
                // Chat off (app#5): nothing is kept; they stop retrying and see only sent.
                let _ = self.send_control(contact, Body::Received { ids: vec![packet.id] }).await;
            }
            Body::Message { text, reply_to } => {
                let stored = self
                    .store
                    .insert_message(&Message {
                        message_id: packet.id.to_string(),
                        contact: id.to_owned(),
                        outgoing: false,
                        body: text,
                        sent_at: packet.sent_at as i64,
                        received_at: now(),
                        state: MessageState::Delivered,
                    })
                    .await?;
                if stored {
                    // What it answers (2026-10-05), as the sender says; the quote shows if the
                    // message is still here.
                    if let Some(quoted) = reply_to {
                        self.store.set_reply(&packet.id.to_string(), &quoted.to_string()).await?;
                    }
                    self.announce_messages(contact);
                }
                // Always acknowledged, even a duplicate: the sender is waiting for it (§27).
                let _ = self.send_control(contact, self.acknowledgement(contact, packet.id)).await;
            }
            Body::Delivered { ids } => self.receipt(id, ids, MessageState::Delivered).await?,
            Body::Received { ids } => self.receipt(id, ids, MessageState::Sent).await?,
            Body::Read { ids } => self.receipt(id, ids, MessageState::Read).await?,
            Body::ContactCard { card, .. } => {
                let card = ContactCard::decode(&card)?;
                if card.device_id().as_str() != id {
                    bail!("a contact sent someone else's card");
                }
                self.store
                    .add_contact(&NewContact {
                        device_id: id.to_owned(),
                        name: contact.name.clone(),
                        card: card.encode(),
                        mailbox: card.mailbox(),
                        session: None,
                        receipts: contact.rules.receipts,
                        accepted: contact.accepted,
                    })
                    .await?;
                if first_contact {
                    // They scanned us: answer with our card so they know we have theirs.
                    self.introduce(contact).await?;
                    let _ = self.events.send(Event::ContactsChanged);
                }
            }
            Body::MailboxPreference { enabled } => {
                self.store.set_contact_mailbox(id, enabled).await?;
                let _ = self.events.send(Event::ContactsChanged);
            }
            Body::Ping => {
                let _ = self.send_control(contact, Body::Pong).await;
            }
            Body::Pong => self.pongs.send_modify(|pongs| *pongs.entry(id.to_owned()).or_default() += 1),
            Body::File { name, size, mime, hash, chunk } => {
                self.offered(contact, packet.id, packet.sent_at, name, size, mime, hash, chunk).await?
            }
            Body::FileRequest { file, from, count } => self.serve_chunks(contact, file, from, count).await?,
            Body::FileChunk { file, index, data } => self.take_chunk(contact, file, index, data).await?,
            Body::FileDone { file } => self.file_done(contact, file).await?,
            Body::FileFailed { file } => self.file_failed(contact, file).await?,
            Body::CallOffer { call, sdp, video, media } => self.call_offered(contact, call, sdp, video, media).await?,
            Body::CallAnswer { call, sdp, media } => self.call_answered(contact, call, sdp, media).await?,
            Body::CallMedia { call, seq, video, paused } => self.call_media_received(contact, call, seq, video, paused).await?,
            Body::CallEnd { call, reason } => self.call_ended(contact, call, reason).await?,
            Body::MoveOffer { proof, key, size, hash } => self.move_offered(contact, proof, key, size, hash).await?,
            Body::MoveRequest { from, count } => self.move_requested(contact, from, count).await?,
            Body::MoveChunk { index, data } => self.move_chunk(contact, index, data).await?,
            Body::MoveDone => self.move_finished(contact).await?,
            Body::CircleCard { card } => self.circle_card_received(contact, packet.id, &card).await?,
            Body::CircleMessage { circle, text } => self.circle_text_received(contact, packet.id, packet.sent_at, &circle, text).await?,
            Body::CircleLeave { circle } => self.circle_leave_received(contact, packet.id, &circle).await?,
            Body::PluginEvent { plugin, data } => self.plugin_event_received(contact, plugin, data).await?,
            // They are writing (2026-10-05): shown only for someone whose messages this phone keeps.
            Body::Typing if contact.rules.accepts_chat && contact.accepted => {
                let _ = self.events.send(Event::Typing { contact: id.to_string() });
            }
            // Their emoji on a message of this conversation (2026-10-05); on anything else, nothing.
            // One made before the one already here changes nothing (2026-10-06).
            Body::Reaction { to, emoji } => {
                let to = to.to_string();
                if contact.rules.accepts_chat && contact.accepted && self.store.message(&to).await?.is_some_and(|message| message.contact == id) {
                    let emoji = emoji.trim();
                    let emoji = (!emoji.is_empty() && is_reaction(emoji)).then_some(emoji);
                    self.store.set_reaction(&to, false, emoji, packet.sent_at as i64).await?;
                    self.announce_messages(contact);
                }
                // Always acknowledged, taken or not: the sender's queue waits for it (2026-10-06).
                let _ = self.send_control(contact, self.acknowledgement(contact, packet.id)).await;
            }
            // Their own text, said again with other words (2026-10-05): only theirs, only a text,
            // and within the same day as the sender's app allows. Measured with the sender's
            // clock on both ends (the message's stamp and the edit's), so an edit that waited
            // for this phone to be reachable still counts from when it was made. One made before
            // the edit already here changes nothing (2026-10-06).
            Body::Edit { of, text } => {
                let of = of.to_string();
                let text = text.trim();
                let theirs = contact.rules.accepts_chat
                    && contact.accepted
                    && self.store.message(&of).await?.is_some_and(|message| {
                        message.contact == id && !message.outgoing && packet.sent_at as i64 - message.sent_at <= EDIT_WINDOW_MS
                    });
                if theirs
                    && !text.is_empty()
                    && self.store.file(&of).await?.is_none()
                    && !self.store.is_deleted(&of).await?
                    && self.store.edit_text(&of, text, packet.sent_at as i64).await?
                {
                    self.announce_messages(contact);
                }
                let _ = self.send_control(contact, self.acknowledgement(contact, packet.id)).await;
            }
            // They take their own message back (2026-10-05): its words and its file go, the mark stays.
            Body::Delete { of } => {
                let of = of.to_string();
                let theirs = contact.rules.accepts_chat
                    && contact.accepted
                    && self.store.message(&of).await?.is_some_and(|message| message.contact == id && !message.outgoing);
                if theirs {
                    let file = self.store.file(&of).await?;
                    self.store.mark_deleted(&of).await?;
                    self.forget_bytes(file.into_iter().collect()).await;
                    self.announce_messages(contact);
                }
                let _ = self.send_control(contact, self.acknowledgement(contact, packet.id)).await;
            }
            // Offers and answers travel as signals (see `open_signal`), never as packets.
            Body::Typing | Body::Block | Body::Offer { .. } | Body::Answer { .. } | Body::Unknown => {}
        }
        Ok(())
    }

    async fn receipt(&self, contact: &str, ids: Vec<MessageId>, state: MessageState) -> Result<()> {
        let mut ours = Vec::new();
        for id in ids.iter().map(ToString::to_string) {
            // An edit's, a taking back's or a reaction's receipt only takes it out of its queue.
            if self.store.dequeue_update(&id).await? {
                continue;
            }
            // A circle packet's receipt clears that member's entry alone (2026-09-27).
            if !self.circle_receipt(contact, &id, state).await? {
                ours.push(id);
            }
        }
        if ours.is_empty() {
            return Ok(());
        }
        self.store.advance(&ours, state).await?;
        for id in &ours {
            self.store.dequeue(id).await?;
        }
        let _ = self.events.send(Event::MessagesChanged { contact: contact.to_owned() });
        Ok(())
    }

    /// One delivery attempt that never holds back the rest of the queue. A router that refuses
    /// the contact for good takes the message out of the queue; a pending one is shown as not
    /// sent, for the user to send again (§84), while one already shown as sent stays so. Anything
    /// else is tried again later, like a contact that cannot be reached.
    pub(crate) async fn attempt(&self, entry: &OutboxEntry) {
        let Err(error) = self.deliver(entry).await else {
            // Its time came and it went (2026-10-06): no longer "for later".
            let _ = self.store.unschedule(&entry.message_id).await;
            return;
        };
        if error.downcast_ref::<MailboxRejected>().is_some() {
            if let Ok(true) = self.store.mark_not_sent(&entry.message_id).await {
                let _ = self.events.send(Event::MessagesChanged { contact: entry.contact.clone() });
            }
            return;
        }
        let attempts = entry.attempts + 1;
        let next = now() + retry_delay(attempts).as_millis() as i64;
        let _ = self.store.reschedule(&entry.message_id, attempts, next, entry.in_mailbox).await;
    }

    /// One delivery attempt for an outbox entry.
    async fn deliver(&self, entry: &OutboxEntry) -> Result<()> {
        let Some(message) = self.store.message(&entry.message_id).await? else {
            return self.store.dequeue(&entry.message_id).await;
        };
        // Taken back before it went out: there is nothing left to send.
        if self.store.is_deleted(&entry.message_id).await? {
            return self.store.dequeue(&entry.message_id).await;
        }
        let contact = self.contact(&entry.contact).await?;
        if !contact.introduced {
            self.introduce(&contact).await?;
        }
        if let Some(file) = self.store.file(&entry.message_id).await? {
            return self.deliver_file(entry, &contact, &message, &file).await;
        }
        let reply_to = match self.store.reply_to(&message.message_id).await? {
            Some(quoted) => Some(MessageId::parse(&quoted)?),
            None => None,
        };
        let packet = Packet::resend(MessageId::parse(&message.message_id)?, message.sent_at as u64, Body::Message { text: message.body, reply_to });
        let attempts = entry.attempts + 1;
        let copy = mail_copy_due(entry.in_mailbox, entry.mailed_at, now());
        // Taken back while the connection opened (2026-10-06): it goes nowhere.
        let Some(route) = self.transmit_unless_taken_back(&contact, &packet, copy, &entry.message_id).await? else {
            return self.store.dequeue(&entry.message_id).await;
        };
        if copy && matches!(route, Route::Mailbox) {
            self.store.mailed(&entry.message_id, now()).await?;
        }
        let (next, in_mailbox) = match route {
            Route::Direct => (RECEIPT_WAIT, entry.in_mailbox),
            Route::Mailbox => (MAILBOX_WAIT, true),
            Route::Unreachable => (retry_delay(attempts), entry.in_mailbox),
        };
        if next != retry_delay(attempts) || in_mailbox {
            // Only moves forward: a receipt that arrived meanwhile is kept.
            self.store.advance(std::slice::from_ref(&entry.message_id), MessageState::Sent).await?;
            let _ = self.events.send(Event::MessagesChanged { contact: contact.device_id.clone() });
        }
        if self.store.outbox().await?.iter().any(|e| e.message_id == entry.message_id) {
            self.store.reschedule(&entry.message_id, attempts, now() + next.as_millis() as i64, in_mailbox).await?;
        }
        Ok(())
    }

    /// Tells the contact of an edit, a taking back or a reaction (2026-10-06): queued like a
    /// message and tried again until their receipt, so that this phone never shows as done what
    /// the other has not heard of (§84). The first try is now.
    async fn tell(&self, contact: &Contact, packet: &Packet) -> Result<()> {
        let at = now();
        let entry = UpdateEntry {
            packet_id: packet.id.to_string(),
            contact: contact.device_id.clone(),
            packet: packet.encode(),
            created_at: at,
            attempts: 0,
            next_attempt: at,
            in_mailbox: false,
        };
        self.store.enqueue_update(&entry.packet_id, &entry.contact, &entry.packet, at).await?;
        self.attempt_update(&entry).await;
        Ok(())
    }

    /// One try of a queued update, like `attempt` for a message: waits for the receipt after a
    /// direct send, for longer after the mailbox, and backs off while the contact is out of reach.
    /// A router that refuses the contact for good, or a week gone by, gives it up.
    pub(crate) async fn attempt_update(&self, entry: &UpdateEntry) {
        if now() - entry.created_at > UPDATE_LIFETIME.as_millis() as i64 {
            let _ = self.store.dequeue_update(&entry.packet_id).await;
            return;
        }
        let attempts = entry.attempts + 1;
        let (next, in_mailbox) = match self.deliver_update(entry).await {
            // An old app never answers it: a direct send backs off too.
            Ok(Route::Direct) => (retry_delay(attempts).max(RECEIPT_WAIT), entry.in_mailbox),
            Ok(Route::Mailbox) => (MAILBOX_WAIT, true),
            Ok(Route::Unreachable) => (retry_delay(attempts), entry.in_mailbox),
            Err(error) if error.downcast_ref::<MailboxRejected>().is_some() => {
                let _ = self.store.dequeue_update(&entry.packet_id).await;
                return;
            }
            Err(_) => (retry_delay(attempts), entry.in_mailbox),
        };
        // A receipt that arrived meanwhile already took it out: this changes nothing then.
        let _ = self.store.reschedule_update(&entry.packet_id, attempts, now() + next.as_millis() as i64, in_mailbox).await;
    }

    async fn deliver_update(&self, entry: &UpdateEntry) -> Result<Route> {
        let contact = self.contact(&entry.contact).await?;
        let packet = Packet::decode(&entry.packet)?;
        // Once in the mailbox, a retry goes directly or not at all: the copy there is enough.
        self.transmit_as(&contact, &packet, !entry.in_mailbox).await
    }

    /// Sends our Contact Card, so the contact can read us and reach us.
    async fn introduce(&self, contact: &Contact) -> Result<()> {
        let card = self.my_card_in(contact.session.as_deref()).await?;
        let via = self.via(contact)?;
        self.transmit(contact, &Packet::new(Body::ContactCard { card: card.encode(), via })).await?;
        Ok(())
    }

    /// The hash of the contact's capability we use, so they know where we belong (app#9).
    pub(crate) fn via(&self, contact: &Contact) -> Result<Option<Vec<u8>>> {
        Ok(Some(ContactCard::decode(&contact.card)?.route_capability().hash().to_vec()))
    }

    /// Control packets (receipts, cards, preferences) are sent once, by whichever way works.
    async fn send_control(&self, contact: &Contact, body: Body) -> Result<()> {
        self.transmit(contact, &Packet::new(body)).await?;
        Ok(())
    }

    /// Encrypts a packet for the contact with Olm.
    async fn seal_for(&self, contact: &Contact, packet: &Packet) -> Result<Vec<u8>> {
        let _identity = self.identity.lock().await;
        let mut channel = self.channel(&contact.device_id).await?;
        let sealed = channel.encrypt(&self.device_id, &packet.encode())?;
        self.store.save_channel(&contact.device_id, &channel.seal(&self.key)).await?;
        Ok(sealed.encode())
    }

    /// Sends over a direct connection only (files, §62); `false` if the contact cannot be reached.
    async fn transmit_direct(&self, contact: &Contact, packet: &Packet) -> Result<bool> {
        let bytes = self.seal_for(contact, packet).await?;
        let card = ContactCard::decode(&contact.card)?;
        let peer = Peer { device_id: contact.device_id.clone(), capability: card.route_capability() };
        Ok(self.transport.send_direct(&peer, bytes).await.unwrap_or(false))
    }

    /// Like `transmit_direct`, for a call's offer: an offline iPhone is rung (2026-09-28).
    pub(crate) async fn transmit_direct_call(&self, contact: &Contact, packet: &Packet) -> Result<bool> {
        let bytes = self.seal_for(contact, packet).await?;
        let card = ContactCard::decode(&contact.card)?;
        let peer = Peer { device_id: contact.device_id.clone(), capability: card.route_capability() };
        Ok(self.transport.send_direct_call(&peer, bytes).await.unwrap_or(false))
    }

    /// Sends over the direct connection already open with the contact, never opening one.
    pub(crate) async fn transmit_open(&self, contact: &Contact, packet: &Packet) -> Result<bool> {
        let bytes = self.seal_for(contact, packet).await?;
        let card = ContactCard::decode(&contact.card)?;
        let peer = Peer { device_id: contact.device_id.clone(), capability: card.route_capability() };
        Ok(self.transport.send_open(&peer, bytes).await.unwrap_or(false))
    }

    /// The pongs the contact has sent so far, and a way to wait for the next.
    pub(crate) fn pongs_from(&self, contact: &str) -> (u64, tokio::sync::watch::Receiver<HashMap<String, u64>>) {
        let pongs = self.pongs.subscribe();
        let heard = pongs.borrow().get(contact).copied().unwrap_or(0);
        (heard, pongs)
    }

    /// Drops the direct connection with the contact (one found dead), so that the next send opens
    /// a new one through the router.
    pub(crate) async fn drop_connection(&self, contact: &str) {
        self.transport.disconnect(contact).await;
    }

    /// Opens the way to the contact for a call's offer, sending nothing yet.
    pub(crate) async fn open_direct_call(&self, contact: &Contact) -> Result<bool> {
        let card = ContactCard::decode(&contact.card)?;
        let peer = Peer { device_id: contact.device_id.clone(), capability: card.route_capability() };
        Ok(self.transport.open_direct_call(&peer).await.unwrap_or(false))
    }

    async fn transmit(&self, contact: &Contact, packet: &Packet) -> Result<Route> {
        self.transmit_as(contact, packet, true).await
    }

    /// Like `transmit`; with `copy` false, a packet already in the mailbox gets no new copy there,
    /// and `Mailbox` says it is still there.
    async fn transmit_as(&self, contact: &Contact, packet: &Packet, copy: bool) -> Result<Route> {
        Ok(self.transmit_checked(contact, packet, copy, None).await?.unwrap_or(Route::Unreachable))
    }

    /// Like `transmit_as`, for one of our messages (2026-10-06): taken back while its connection
    /// opened, or before the mailbox took it, it is not sent, and the answer is `None`. Its
    /// delivery may have started seconds before (a connection takes up to 12 s to open).
    async fn transmit_unless_taken_back(&self, contact: &Contact, packet: &Packet, copy: bool, message_id: &str) -> Result<Option<Route>> {
        self.transmit_checked(contact, packet, copy, Some(message_id)).await
    }

    /// Whether our message is still to be sent: here, and not taken back.
    async fn still_to_send(&self, message_id: &str) -> Result<bool> {
        Ok(self.store.message(message_id).await?.is_some() && !self.store.is_deleted(message_id).await?)
    }

    async fn transmit_checked(&self, contact: &Contact, packet: &Packet, copy: bool, message: Option<&str>) -> Result<Option<Route>> {
        let bytes = self.seal_for(contact, packet).await?;
        let card = ContactCard::decode(&contact.card)?;
        let peer = Peer { device_id: contact.device_id.clone(), capability: card.route_capability() };

        // The message gets delivered first (§17, §19): with the mailbox allowed on both sides, a
        // peer that is not connected does not hold it back (2026-09-29).
        let mailbox = contact.mailbox && self.mailbox().await?;
        let direct = match message {
            // Opened first, then looked at again right before sending.
            Some(message_id) => {
                let open = self.transport.reach(&peer, mailbox).await.unwrap_or(false);
                if !self.still_to_send(message_id).await? {
                    return Ok(None);
                }
                if open { self.transport.send_open(&peer, bytes.clone()).await } else { Ok(false) }
            }
            None if mailbox => self.transport.try_direct(&peer, bytes.clone()).await,
            None => self.transport.send_direct(&peer, bytes.clone()).await,
        };
        if direct.unwrap_or(false) {
            return Ok(Some(Route::Direct));
        }
        if mailbox && !copy {
            return Ok(Some(Route::Mailbox));
        }
        if mailbox {
            if let Some(message_id) = message {
                if !self.still_to_send(message_id).await? {
                    return Ok(None);
                }
            }
            // Through the router it goes in an envelope (A1): the router sees only who it is for.
            let mail = self.wrap_for(&contact.device_id, bytes).await?;
            self.transport.send_mailbox(&peer, mail).await.context("the mailbox is not reachable")?;
            return Ok(Some(Route::Mailbox));
        }
        Ok(Some(Route::Unreachable))
    }

    async fn channel(&self, contact: &str) -> Result<Channel> {
        match self.store.channel(contact).await? {
            Some(sealed) => Channel::unseal(&sealed, &self.key),
            None => Ok(Channel::default()),
        }
    }

    async fn contact(&self, device_id: &str) -> Result<Contact> {
        self.store.contact(device_id).await?.ok_or_else(|| anyhow!("unknown contact"))
    }
}

/// The router client signs with the device identity.
#[async_trait]
impl ft_push::Signer for Core {
    fn device_id(&self) -> String {
        self.device_id.to_string()
    }

    async fn signing_key(&self) -> String {
        Core::signing_key(self).await
    }

    async fn sign(&self, message: &[u8]) -> String {
        Core::sign(self, message).await
    }
}

/// Takes a hidden session away and gives its slot a new link (A3): whoever kept the old QR
/// reaches nobody, not even the session that takes the slot next. What was in it is the caller's.
async fn forget_session(store: &Store, session: &str) -> Result<()> {
    if let Some(slot) = store.session_slot(session).await? {
        store.set_spare_capability(slot, RouteCapability::generate().as_bytes()).await?;
    }
    store.remove_session(session).await
}

/// A readable default name for a contact whose card has none.
fn short_name(device_id: &DeviceId) -> String {
    device_id.as_str().chars().take(9).collect()
}

/// The most a contact's name may run to on this phone.
pub const NAME_LIMIT: usize = 40;

/// A name as a card brings it, tamed before it is shown (B6): no control characters, no
/// bidirectional overrides that could turn "Mamá" into something else, one space between words,
/// and no more than `NAME_LIMIT` characters. Empty if nothing readable is left.
pub fn clean_name(name: &str) -> String {
    let readable = name
        .chars()
        .filter(|c| !c.is_control())
        .filter(|c| !matches!(*c, '\u{200e}' | '\u{200f}' | '\u{061c}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}'))
        .collect::<String>();
    readable.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(NAME_LIMIT).collect::<String>().trim_end().to_owned()
}

/// A phone restored from a backup (plan-recuperacion §5): a new outgoing session with every
/// contact, from their card, which encrypts from now on. Our card goes to each of them over it
/// (`card_stale`, sent by `online::start`); being a pre-key message, they take the session up, and
/// no key of the backup's sessions is used again for sending.
async fn renew_sessions(store: &Store, identity: &Identity, key: &[u8; 32]) -> Result<()> {
    for contact in store.all_contacts().await? {
        if contact.blocked {
            continue;
        }
        let Ok(card) = ContactCard::decode(&contact.card) else { continue };
        let mut channel = match store.channel(&contact.device_id).await? {
            Some(sealed) => Channel::unseal(&sealed, key)?,
            None => Channel::default(),
        };
        channel.renew(identity, &card.contact_keys())?;
        store.save_channel(&contact.device_id, &channel.seal(key)).await?;
    }
    store.set_setting(CARD_STALE, "1").await?;
    store.forget_setting(SESSIONS_BEHIND).await
}

/// This phone's clock, in milliseconds since the Unix epoch.
pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// The weekly hours when the phone may make noise (app#7): Monday first, seven days. Outside
/// them messages still arrive and calls still show, but nothing sounds, vibrates or notifies.
/// Evaluated on the phone's own clock and time zone by the native side.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Week {
    pub days: Vec<Day>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum Day {
    /// `"all"` (the whole day) or `"none"` (never).
    Whole(String),
    /// A stretch, `"HH:MM"` to `"HH:MM"`; past midnight when `to` comes before `from`.
    Hours { from: String, to: String },
}

impl Week {
    /// What the native side reads (app#7): seven `;`-separated days, Monday first, each `all`,
    /// `none` or `FROM-TO` in minutes of the day.
    pub fn compact(&self) -> String {
        let day = |day: &Day| match day {
            Day::Whole(which) => which.clone(),
            Day::Hours { from, to } => format!("{}-{}", minutes(from).unwrap_or(0), minutes(to).unwrap_or(0)),
        };
        self.days.iter().map(day).collect::<Vec<_>>().join(";")
    }

    fn check(&self) -> Result<()> {
        ensure!(self.days.len() == 7, "a week has seven days");
        for day in &self.days {
            match day {
                Day::Whole(which) => ensure!(which == "all" || which == "none", "a day is all, none or hours"),
                Day::Hours { from, to } => ensure!(minutes(from).is_some() && minutes(to).is_some(), "hours are HH:MM"),
            }
        }
        Ok(())
    }
}

fn minutes(time: &str) -> Option<u32> {
    let (hours, minutes) = time.split_once(':')?;
    let (hours, minutes): (u32, u32) = (hours.parse().ok()?, minutes.parse().ok()?);
    (hours < 24 && minutes < 60 && time.len() == 5).then_some(hours * 60 + minutes)
}

#[cfg(test)]
mod tests {
    // app#7: what the native side reads, Monday first, minutes of the day.
    #[test]
    fn the_week_is_handed_to_the_phone_in_minutes() {
        let week: Week = serde_json::from_str(
            r#"{"days":[{"from":"18:00","to":"22:00"},"none",{"from":"22:00","to":"02:00"},"all","all","all","all"]}"#,
        )
        .unwrap();
        assert_eq!(week.compact(), "1080-1320;none;1320-120;all;all;all;all");
    }

    use super::*;

    // B6: what a card calls its owner is tamed before it is shown.
    #[test]
    fn names_are_tamed() {
        assert_eq!(clean_name("  Bob   Smith "), "Bob Smith");
        assert_eq!(clean_name("Mam\u{202e}á\u{0000}"), "Mamá");
        assert_eq!(clean_name(&"x".repeat(100)).chars().count(), NAME_LIMIT);
        assert_eq!(clean_name("\t\n"), "");
        assert_eq!(clean_name("Zoë 😀"), "Zoë 😀");
    }

    // 2026-10-01: a message the mailbox took gets a new copy there once a day, not at every retry.
    #[test]
    fn a_message_in_the_mailbox_gets_a_new_copy_there_once_a_day() {
        let day = MAILBOX_COPY.as_millis() as i64;
        assert!(mail_copy_due(false, None, 1_000), "the first copy goes as always");
        assert!(mail_copy_due(true, None, 1_000), "queued before the time was kept: a copy now");
        assert!(!mail_copy_due(true, Some(1_000), 1_000 + MAILBOX_WAIT.as_millis() as i64));
        assert!(!mail_copy_due(true, Some(1_000), 1_000 + day - 1));
        assert!(mail_copy_due(true, Some(1_000), 1_000 + day));
    }

    #[test]
    fn unreachable_contacts_are_retried_ever_more_slowly_up_to_a_limit() {
        assert!(retry_delay(1) < retry_delay(2));
        assert!(retry_delay(2) < retry_delay(4));
        assert_eq!(retry_delay(50), MAX_RETRY_DELAY);
    }
}
