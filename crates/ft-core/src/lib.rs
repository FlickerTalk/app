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

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, ensure, Context, Result};
use async_trait::async_trait;
use ft_billing::{Access, Plan};
use ft_contacts::{ContactCard, RouteCapability};
use ft_crypto::{accept_first_contact, Channel};
use ft_identity::{DeviceId, EnvelopeKey, Identity};
use ft_protocol::{Body, Envelope, Packet, Sealed, ENVELOPE_VERSION};
use ft_storage::{Contact, ContactRules, Conversation, Message, MessageState, NewContact, OutboxEntry, Store, UpdateEntry};
use tokio::sync::{broadcast, Mutex};

pub const MAX_RETRY_DELAY: Duration = Duration::from_secs(300);
pub(crate) const RECEIPT_WAIT: Duration = Duration::from_secs(30);
pub(crate) const MAILBOX_WAIT: Duration = Duration::from_secs(600);
// The router keeps a blob 7 days, the daily copy covers a router that lost its table
pub(crate) const MAILBOX_COPY: Duration = Duration::from_secs(24 * 3600);
// Matches the router's mailbox TTL. Old apps never ack edits or reactions.
pub(crate) const UPDATE_LIFETIME: Duration = Duration::from_secs(7 * 24 * 3600);

const NAME: &str = "name";
const MAILBOX: &str = "mailbox";
const INSTALLED_AT: &str = "installed_at";
// ms, 0 = none
const PAID_UNTIL: &str = "paid_until";
pub const NEEDS_SUBSCRIPTION: &str = "needs_subscription";
pub const PEER_CANNOT_PRESENT: &str = "peer_cannot_present";

pub use ft_billing::FREE_PERIOD;

const STORE_RETRY: Duration = Duration::from_secs(60 * 60);
const PLAN_LOOK_AT_MOST: Duration = Duration::from_secs(12 * 60 * 60);

// Paid-until in ms, 0 if nothing is paid, Err if the Store can't be asked
#[async_trait]
pub trait Entitlements: Send + Sync {
    async fn until(&self) -> Result<i64>;
}

fn mail_copy_due(in_mailbox: bool, mailed_at: Option<i64>, now: i64) -> bool {
    !in_mailbox || mailed_at.is_none_or(|at| now - at >= MAILBOX_COPY.as_millis() as i64)
}

pub fn retry_delay(attempts: i64) -> Duration {
    let exponent = attempts.clamp(1, 16) as u32 - 1;
    Duration::from_secs(5 * 2u64.pow(exponent)).min(MAX_RETRY_DELAY)
}

#[derive(Debug, Clone)]
pub struct Peer {
    pub device_id: String,
    pub capability: RouteCapability,
}

#[async_trait]
pub trait Transport: Send + Sync {
    async fn send_direct(&self, to: &Peer, bytes: Vec<u8>) -> Result<bool>;
    async fn try_direct(&self, to: &Peer, bytes: Vec<u8>) -> Result<bool> {
        self.send_direct(to, bytes).await
    }
    async fn send_direct_call(&self, to: &Peer, bytes: Vec<u8>) -> Result<bool> {
        self.send_direct(to, bytes).await
    }
    // Never opens one, the liveness check must not send an offer through the router
    async fn send_open(&self, to: &Peer, bytes: Vec<u8>) -> Result<bool> {
        self.send_direct(to, bytes).await
    }
    async fn reach(&self, _to: &Peer, _fallback: bool) -> Result<bool> {
        Ok(true)
    }
    async fn open_direct_call(&self, _to: &Peer) -> Result<bool> {
        Ok(false)
    }
    async fn send_mailbox(&self, to: &Peer, bytes: Vec<u8>) -> Result<()>;
    async fn call_over(&self, _device_id: &str) {}
    async fn disconnect(&self, _device_id: &str) {}
    fn media_config(&self) -> ft_media::MediaConfig {
        ft_media::MediaConfig::default()
    }
    async fn fresh_media_config(&self) -> ft_media::MediaConfig {
        self.media_config()
    }
}

pub use calls::CallUpdate;
pub use ft_protocol::MessageId;
pub use native_calls::{CallPhase, CurrentCall, PresentedBy, Presenting, VideoDetach};
pub use files::MAX_FILE_SIZE;
pub use ft_push::{Feedback, MailboxRejected, RouterClient, TurnGrant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    ContactsChanged,
    MessagesChanged { contact: String },
    ConnectionChanged { contact: String },
    Typing { contact: String },
    Call { contact: String, call: String, update: CallUpdate },
    Move(moving::MoveUpdate),
    PluginsChanged,
    CirclesChanged,
    CircleMessagesChanged { circle: String },
    PluginEvent { plugin: String, contact: String, data: Vec<u8> },
    RemindersChanged,
    VaultChanged,
    VaultProgress { done: u64, total: u64 },
    // Not shown in the UI, it stops a system call screen a push may have set ringing
    CallRefused,
    PlanChanged,
}

pub(crate) enum Route {
    Direct,
    Mailbox,
    Unreachable,
}

const EDIT_WINDOW_MS: i64 = 24 * 60 * 60 * 1000;

// Deliberately not checked against Unicode's emoji tables
fn is_reaction(text: &str) -> bool {
    !text.is_empty() && text.len() <= 32 && text.chars().count() <= 8 && !text.chars().any(|c| c.is_whitespace() || c.is_control())
}

pub struct Core {
    store: Store,
    key: [u8; 32],
    // held while an Olm channel is loaded, used and saved back
    identity: Mutex<Identity>,
    device_id: DeviceId,
    capability: RwLock<RouteCapability>,
    envelope: EnvelopeKey,
    transport: Arc<dyn Transport>,
    events: broadcast::Sender<Event>,
    files_dir: OnceLock<PathBuf>,
    plugins_dir: OnceLock<PathBuf>,
    transfers: std::sync::Mutex<HashMap<String, files::Transfer>>,
    active_call: std::sync::Mutex<Option<String>>,
    call_audio: RwLock<Option<ft_media::AudioPlatform>>,
    native_call: std::sync::Mutex<Option<Arc<native_calls::NativeCall>>>,
    call_media_ceiling: std::sync::atomic::AtomicU16,
    tells_mailbox: std::sync::atomic::AtomicBool,
    native_setup: Mutex<()>,
    ringing_offer: std::sync::Mutex<Option<(String, String, u16)>>,
    call_video: RwLock<Option<ft_media::VideoPlatform>>,
    video_detach: RwLock<Option<native_calls::VideoDetach>>,
    app_visible: std::sync::atomic::AtomicBool,
    call_shown: std::sync::atomic::AtomicBool,
    call_audio_active: std::sync::atomic::AtomicBool,
    early_answer: std::sync::Mutex<Option<native_calls::EarlyAnswer>>,
    preparation: std::sync::Mutex<native_calls::Preparation>,
    answer_refresh: std::sync::atomic::AtomicU64,
    call_clock: std::sync::Mutex<Option<timings::CallClock>>,
    audio_generation: std::sync::atomic::AtomicU64,
    this: std::sync::Weak<Core>,
    move_dir: OnceLock<PathBuf>,
    moving: std::sync::Mutex<moving::MoveState>,
    open_sessions: std::sync::Mutex<HashMap<String, u8>>,
    keeping_sessions: Mutex<()>,
    seen_access: std::sync::Mutex<Option<Access>>,
    plan_poke: Arc<tokio::sync::Notify>,
    registration: tokio::sync::watch::Sender<u64>,
    pongs: tokio::sync::watch::Sender<HashMap<String, u64>>,
    registration_retry: Arc<tokio::sync::Notify>,
    vault: Mutex<Option<Arc<ft_vault::Vault>>>,
    cloud: OnceLock<Arc<dyn vault::Cloud>>,
    vault_dir: OnceLock<PathBuf>,
}

const SESSION_PIN_CONTEXT: &str = "flickertalk 2026-09-23 hidden session pin";
const SESSION_PIN_LENGTH: usize = 6;
const SPARE_SLOTS: u8 = 7;
const RECEIPTS_DEFAULT: &str = "receipts_default";
const QUIET_HOURS: &str = "quiet_hours";
const CARD_STALE: &str = "card_stale";
// Set on a restored backup, whose Olm sessions are behind every contact's
pub(crate) const SESSIONS_BEHIND: &str = "sessions_behind";
// "due" until the contact is added, then its id. Older installs have neither.
const BUNDLED_CONTACT: &str = "bundled_contact";
const BUNDLED_DUE: &str = "due";

impl Core {
    pub async fn open(store: Store, key: [u8; 32], transport: Arc<dyn Transport>) -> Result<Arc<Self>> {
        let (identity, capability, envelope) = match store.identity().await? {
            Some(saved) => {
                let identity = Identity::unseal(&saved.sealed, &key)?;
                let envelope = match saved.envelope {
                    Some(sealed) => EnvelopeKey::unseal_at_rest(&sealed, &key)?,
                    None => {
                        let envelope = EnvelopeKey::generate();
                        store.save_envelope(&envelope.seal_at_rest(&key)).await?;
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
                store.set_setting(BUNDLED_CONTACT, BUNDLED_DUE).await?;
                (identity, capability, envelope)
            }
        };
        store.finish_open_calls(now()).await?;
        if store.spare_capabilities().await?.is_empty() {
            for slot in 1..=SPARE_SLOTS {
                store.add_spare_capability(slot, RouteCapability::generate().as_bytes()).await?;
            }
        }
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
            call_media_ceiling: std::sync::atomic::AtomicU16::new(ft_protocol::CALL_MEDIA_VERSION),
            tells_mailbox: std::sync::atomic::AtomicBool::new(true),
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
            seen_access: std::sync::Mutex::new(None),
            plan_poke: Arc::new(tokio::sync::Notify::new()),
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

    pub async fn exchange_key_bytes(&self) -> [u8; 32] {
        self.identity.lock().await.exchange_key().to_bytes()
    }

    pub async fn exchange_key_base64(&self) -> String {
        self.identity.lock().await.exchange_key().to_base64()
    }

    // A contact on an older app has no envelope key and gets the bytes unsealed
    pub async fn wrap_for(&self, contact: &str, bytes: Vec<u8>) -> Result<Vec<u8>> {
        let card = ContactCard::decode(&self.contact(contact).await?.card)?;
        match card.envelope_key() {
            Some(key) => Ok(Envelope { envelope: ENVELOPE_VERSION, sealed: key.seal(&bytes)? }.encode()),
            None => Ok(bytes),
        }
    }

    // An envelope sealed for someone else is an error, never read as bare bytes
    pub async fn unwrap(&self, bytes: &[u8]) -> Result<Vec<u8>> {
        match Envelope::decode(bytes) {
            Ok(envelope) => {
                ensure!(envelope.envelope == ENVELOPE_VERSION, "an envelope of a kind this app does not know");
                self.envelope.open(&envelope.sealed)
            }
            Err(_) => Ok(bytes.to_vec()),
        }
    }

    pub async fn card_stale(&self) -> Result<bool> {
        Ok(self.store.setting(CARD_STALE).await?.as_deref() == Some("1"))
    }

    pub async fn reintroduce(&self) -> Result<()> {
        for contact in self.store.all_contacts().await? {
            if contact.blocked || self.quiet_until_written(&contact).await {
                continue;
            }
            let _ = self.introduce(&contact).await;
        }
        self.store.set_setting(CARD_STALE, "0").await
    }

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
            if self.quiet_until_written(&contact).await {
                continue;
            }
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

    pub async fn free_until(&self) -> Result<i64> {
        #[cfg(debug_assertions)]
        if trial_over_for_debugging() {
            // Not 0, the app reads 0 as no date yet
            return Ok(now() - 1);
        }
        let installed = self.store.setting(INSTALLED_AT).await?.and_then(|at| at.parse::<i64>().ok()).unwrap_or_else(now);
        Ok(installed + FREE_PERIOD.as_millis() as i64)
    }

    pub async fn plan(&self) -> Result<Plan> {
        Ok(Plan {
            free_until: self.free_until().await?,
            paid_until: self.store.setting(PAID_UNTIL).await?.and_then(|at| at.parse().ok()).unwrap_or(0),
        })
    }

    pub async fn access(&self) -> Result<Access> {
        Ok(Access::of(now(), self.plan().await?))
    }

    pub async fn set_entitlement(&self, until: i64) -> Result<()> {
        let until = if until > now() { until } else { 0 }.to_string();
        if self.store.setting(PAID_UNTIL).await?.as_deref() == Some(until.as_str()) {
            return Ok(());
        }
        self.store.set_setting(PAID_UNTIL, &until).await?;
        let access = self.access().await?;
        *self.seen_access.lock().expect("plan poisoned") = Some(access);
        let _ = self.events.send(Event::PlanChanged);
        Ok(())
    }

    pub async fn recheck_plan(&self, store: &dyn Entitlements) -> Result<()> {
        if self.plan().await?.paid_date_passed(now()) {
            if let Ok(until) = store.until().await {
                self.set_entitlement(until).await?;
            }
        }
        let access = self.access().await?;
        let before = self.seen_access.lock().expect("plan poisoned").replace(access);
        if before.is_some_and(|before| before != access) {
            let _ = self.events.send(Event::PlanChanged);
        }
        Ok(())
    }

    // A phone asleep for hours never ran the timer
    pub fn look_at_plan_again(&self) {
        self.plan_poke.notify_one();
    }

    pub async fn watch_plan(core: std::sync::Weak<Core>, store: Arc<dyn Entitlements>) {
        loop {
            let Some(here) = core.upgrade() else { return };
            let _ = here.recheck_plan(&*store).await;
            let wait = here.next_plan_look().await.unwrap_or(PLAN_LOOK_AT_MOST);
            let poke = here.plan_poke.clone();
            drop(here);
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                _ = poke.notified() => {}
            }
        }
    }

    async fn next_plan_look(&self) -> Result<Duration> {
        let (plan, at) = (self.plan().await?, now());
        let mut wait = PLAN_LOOK_AT_MOST;
        if let Some(change) = plan.next_change(at) {
            // one ms past it so the clock reads it as passed
            wait = wait.min(Duration::from_millis((change - at + 1).max(1) as u64));
        }
        if plan.paid_date_passed(at) {
            wait = wait.min(STORE_RETRY);
        }
        Ok(wait)
    }


    pub async fn mailbox(&self) -> Result<bool> {
        Ok(self.store.setting(MAILBOX).await?.as_deref() != Some("0"))
    }

    #[doc(hidden)]
    pub fn set_tells_mailbox(&self, tells: bool) {
        self.tells_mailbox.store(tells, std::sync::atomic::Ordering::Relaxed);
    }

    pub async fn set_mailbox(&self, enabled: bool) -> Result<()> {
        self.store.set_setting(MAILBOX, if enabled { "1" } else { "0" }).await?;
        for contact in self.store.all_contacts().await? {
            if self.quiet_until_written(&contact).await {
                continue;
            }
            let _ = self.send_control(&contact, Body::MailboxPreference { enabled }).await;
        }
        Ok(())
    }

    pub async fn my_card(&self) -> Result<ContactCard> {
        self.my_card_in(None).await
    }

    pub async fn my_card_in(&self, session: Option<&str>) -> Result<ContactCard> {
        self.card_in(session, true).await
    }

    pub async fn my_card_without_envelope(&self) -> Result<ContactCard> {
        self.card_in(None, false).await
    }

    async fn card_in(&self, session: Option<&str>, with_envelope: bool) -> Result<ContactCard> {
        let capability = self.capability_of(session).await?;
        let (name, mailbox) = (self.name().await?, self.mailbox().await?);
        let envelope = with_envelope.then(|| self.envelope.public_key());
        let mut identity = self.identity.lock().await;
        let card = ContactCard::create(&mut identity, name, capability, mailbox, envelope);
        // Creating the first card adds the Olm fallback key to the account
        self.store.save_identity(&identity.seal(&self.key), self.route_capability().as_bytes()).await?;
        Ok(card)
    }

    pub async fn add_contact(&self, link: &str, name: Option<String>) -> Result<Contact> {
        self.add_contact_in(link, name, None).await
    }

    pub async fn add_contact_in(&self, link: &str, name: Option<String>, session: Option<&str>) -> Result<Contact> {
        if let Some(session) = session {
            ensure!(self.open_sessions.lock().expect("sessions poisoned").contains_key(session), "that session is not open");
        }
        let card = ContactCard::from_link(link)?;
        if card.device_id() == self.device_id {
            bail!("that is your own contact card");
        }
        let contact = self.keep_contact(&card, name, session).await?;
        self.introduce(&contact).await?;
        let _ = self.events.send(Event::ContactsChanged);
        Ok(contact)
    }

    // Stays local, not even our card goes out until the user writes to it
    pub async fn add_bundled_contact(&self, link: &str, name: Option<String>) -> Result<Option<Contact>> {
        if self.store.setting(BUNDLED_CONTACT).await?.as_deref() != Some(BUNDLED_DUE) {
            return Ok(None);
        }
        let Ok(card) = ContactCard::from_link(link) else { return Ok(None) };
        if card.device_id() == self.device_id {
            return Ok(None);
        }
        let contact = self.keep_contact(&card, name, None).await?;
        self.store.set_setting(BUNDLED_CONTACT, &contact.device_id).await?;
        let _ = self.events.send(Event::ContactsChanged);
        Ok(Some(contact))
    }

    async fn quiet_until_written(&self, contact: &Contact) -> bool {
        !contact.introduced && matches!(self.store.setting(BUNDLED_CONTACT).await, Ok(Some(id)) if id == contact.device_id)
    }

    async fn keep_contact(&self, card: &ContactCard, name: Option<String>, session: Option<&str>) -> Result<Contact> {
        let device_id = card.device_id();
        let name = name
            .map(|name| clean_name(&name))
            .filter(|name| !name.is_empty())
            .or_else(|| card.name().map(clean_name).filter(|name| !name.is_empty()))
            .unwrap_or_else(|| short_name(&device_id));
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
            if self.store.channel(device_id.as_str()).await?.is_none() {
                let channel = Channel::open(&identity, &card.contact_keys())?;
                self.store.save_channel(device_id.as_str(), &channel.seal(&self.key)).await?;
            }
        }
        self.contact(device_id.as_str()).await
    }

    fn pin_hash(&self, pin: &str) -> Result<[u8; 32]> {
        ensure!(pin.len() == SESSION_PIN_LENGTH && pin.bytes().all(|byte| byte.is_ascii_digit()), "a PIN is six digits");
        Ok(*blake3::keyed_hash(&blake3::derive_key(SESSION_PIN_CONTEXT, &self.key), pin.as_bytes()).as_bytes())
    }

    // Any PIN opens a session, a new empty one if no session has it, and the two must look alike.
    // Without the subscription every PIN is refused the same way.
    pub async fn open_session(&self, pin: &str) -> Result<Option<String>> {
        let hash = self.pin_hash(pin)?;
        ensure!(self.access().await?.may(ft_billing::Doing::UseSession), NEEDS_SUBSCRIPTION);
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

    async fn keep_open_sessions(&self) -> Result<()> {
        {
            let _one_at_a_time = self.keeping_sessions.lock().await;
            sessions::keep(&self.store, &self.key, &self.open_sessions()).await?;
        }
        self.registration_changed();
        let _ = self.events.send(Event::RemindersChanged);
        Ok(())
    }

    async fn open_session_with(&self, hash: &[u8; 32]) -> Result<Option<String>> {
        let Some(id) = self.store.session_by_pin(hash).await? else { return Ok(None) };
        let slot = match self.store.session_slot(&id).await? {
            Some(slot) => slot,
            None => {
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

    // The router can't close a connection that's already open, so close them here.
    // Hang up a call first so its end still goes out on that connection.
    async fn cut_off(&self, session: &str) -> Result<()> {
        let all = self.store.all_contacts().await?;
        let contacts: Vec<String> = all.into_iter().filter(|contact| contact.session.as_deref() == Some(session)).map(|contact| contact.device_id).collect();
        self.hang_up_with(&contacts).await?;
        for contact in &contacts {
            self.transport.disconnect(contact).await;
        }
        Ok(())
    }

    pub fn open_sessions(&self) -> Vec<String> {
        let mut open: Vec<String> = self.open_sessions.lock().expect("sessions poisoned").keys().cloned().collect();
        open.sort();
        open
    }

    pub fn open_slots(&self) -> Vec<u8> {
        let mut slots: Vec<u8> = self.open_sessions.lock().expect("sessions poisoned").values().copied().collect();
        slots.sort();
        slots
    }

    pub fn registration_changes(&self) -> tokio::sync::watch::Receiver<u64> {
        self.registration.subscribe()
    }

    fn registration_changed(&self) {
        self.registration.send_modify(|generation| *generation += 1);
    }

    pub fn registration_retry(&self) -> Arc<tokio::sync::Notify> {
        self.registration_retry.clone()
    }

    pub fn router_reachable(&self) {
        self.registration_retry.notify_one();
    }

    pub async fn silent_slots(&self) -> Result<u8> {
        let (used, noise) = (self.store.used_slots().await?, sessions::noise(&self.store).await?);
        Ok(sessions::silent_mask(&used, &self.open_slots(), noise))
    }

    pub async fn route_capability_hashes(&self) -> Result<[[u8; 32]; 8]> {
        let mut hashes = [self.route_capability().hash(); 8];
        for (slot, capability) in self.store.spare_capabilities().await? {
            hashes[slot as usize] = RouteCapability::from_bytes(capability).hash();
        }
        Ok(hashes)
    }

    async fn capability_of(&self, session: Option<&str>) -> Result<RouteCapability> {
        let Some(session) = session else { return Ok(self.route_capability()) };
        let slot = self.store.session_slot(session).await?.ok_or_else(|| anyhow!("the session has no slot"))?;
        let spare = self.store.spare_capabilities().await?.into_iter().find(|(spare, _)| *spare == slot);
        spare.map(|(_, capability)| RouteCapability::from_bytes(capability)).ok_or_else(|| anyhow!("no capability for the slot"))
    }

    // An unknown or retired link reaches nobody, not the slot's next session and not the main list
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

    pub(crate) fn silent(&self, contact: &Contact) -> bool {
        !contact.accepted
            || contact.session.as_deref().is_some_and(|session| !self.open_sessions.lock().expect("sessions poisoned").contains_key(session))
    }

    pub async fn requests(&self) -> Result<Vec<Conversation>> {
        self.store.request_conversations(None).await
    }

    pub async fn session_requests(&self, session: &str) -> Result<Vec<Conversation>> {
        self.store.request_conversations(Some(session)).await
    }

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

    pub async fn decline_contact(&self, contact: &str) -> Result<()> {
        self.contact(contact).await?;
        self.block(contact, true).await
    }

    async fn chosen(&self, contact: &str) -> Result<Contact> {
        let stored = self.contact(contact).await?;
        if !stored.accepted {
            self.accept_contact(contact).await?;
            return self.contact(contact).await;
        }
        Ok(stored)
    }

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

    pub(crate) fn acknowledgement(&self, contact: &Contact, id: MessageId) -> Body {
        if contact.rules.receipts {
            Body::Delivered { ids: vec![id] }
        } else {
            Body::Received { ids: vec![id] }
        }
    }

    pub(crate) fn announce_messages(&self, contact: &Contact) {
        if !self.silent(contact) {
            let _ = self.events.send(Event::MessagesChanged { contact: contact.device_id.clone() });
        }
    }

    pub async fn set_rules(&self, contact: &str, rules: ContactRules) -> Result<()> {
        self.contact(contact).await?;
        self.store.set_rules(contact, &rules).await?;
        let _ = self.events.send(Event::ContactsChanged);
        Ok(())
    }

    // Only over an already open direct connection, never the mailbox or the router
    pub async fn typing(&self, contact: &str) -> Result<bool> {
        let contact = self.contact(contact).await?;
        if contact.blocked || !contact.accepted || !contact.rules.typing {
            return Ok(false);
        }
        self.transmit_open(&contact, &Packet::new(Body::Typing)).await
    }

    pub async fn receipts_default(&self) -> Result<bool> {
        Ok(self.store.setting(RECEIPTS_DEFAULT).await?.as_deref() != Some("0"))
    }

    pub async fn set_receipts_default(&self, receipts: bool) -> Result<()> {
        self.store.set_setting(RECEIPTS_DEFAULT, if receipts { "1" } else { "0" }).await
    }

    pub async fn quiet_hours(&self) -> Result<Option<String>> {
        Ok(self.store.setting(QUIET_HOURS).await?.filter(|hours| !hours.is_empty()))
    }

    pub async fn quiet_week(&self) -> Result<String> {
        Ok(match self.quiet_hours().await? {
            Some(json) => serde_json::from_str::<Week>(&json)?.compact(),
            None => String::new(),
        })
    }

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

    pub async fn remove_contact(&self, contact: &str) -> Result<()> {
        self.contact(contact).await?;
        self.store.remove_contact(contact).await?;
        let _ = self.events.send(Event::ContactsChanged);
        Ok(())
    }

    // seconds, 0 = forever / never
    pub async fn set_history(&self, contact: &str, keep_for: i64, burn_after_read: i64) -> Result<()> {
        self.store.set_history(contact, keep_for.max(0), burn_after_read.max(0)).await?;
        let _ = self.events.send(Event::ContactsChanged);
        self.sweep_history().await
    }

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
        self.forget_bytes(theirs).await;
        Ok(())
    }

    pub async fn forward(&self, message_id: &str, to: &str) -> Result<String> {
        let stored = self.store.message(message_id).await?.context("that message is not here")?;
        match self.store.file(message_id).await? {
            Some(file) => {
                // `complete` is about the transfer, our own outgoing files have their bytes from the start
                ensure!(stored.outgoing || file.complete, "that file is not here whole yet");
                let path = self.file_path(&file);
                ensure!(path.exists(), "the bytes of that file are no longer here");
                self.send_file(to, &path, &file.name, &file.mime).await
            }
            None => self.send_text(to, &stored.body).await,
        }
    }

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

    // `send_at` in ms. An iPhone only sends it when something wakes the app.
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

    pub async fn search(&self, contact: &str, query: &str) -> Result<Vec<Message>> {
        self.contact(contact).await?;
        self.store.search_messages(contact, query, 50).await
    }

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

    // Tell the contact even if it never left, a try may already be on its way
    pub async fn delete_for_everyone(&self, message_id: &str) -> Result<()> {
        let message = self.store.message(message_id).await?.context("that message is not here")?;
        ensure!(message.outgoing, "only our own messages can be taken back from the other side");
        let contact = self.contact(&message.contact).await?;
        if self.store.scheduled_all().await?.get(message_id).is_some_and(|send_at| *send_at > now()) {
            return self.forget_message(message_id).await;
        }
        let file = self.store.file(message_id).await?;
        self.store.mark_deleted(message_id).await?;
        if message.state == MessageState::Pending {
            self.store.advance(&[message_id.to_owned()], MessageState::Sent).await?;
        }
        self.forget_bytes(file.into_iter().collect()).await;
        let _ = self.events.send(Event::MessagesChanged { contact: contact.device_id.clone() });
        let packet = Packet::new(Body::Delete { of: MessageId::parse(message_id)? });
        self.tell(&contact, &packet).await
    }

    pub async fn updates_waiting(&self, contact: &str) -> Result<HashSet<String>> {
        let mut waiting = HashSet::new();
        for entry in self.store.update_outbox().await? {
            if entry.contact != contact {
                continue;
            }
            if let Ok(Packet { body: Body::Edit { of, .. } | Body::Delete { of }, .. }) = Packet::decode(&entry.packet) {
                waiting.insert(of.to_string());
            }
        }
        Ok(waiting)
    }

    pub async fn pin_message(&self, message_id: &str, pinned: bool) -> Result<()> {
        let message = self.store.message(message_id).await?.context("that message is not here")?;
        self.store.set_pinned(message_id, pinned).await?;
        let _ = self.events.send(Event::MessagesChanged { contact: message.contact });
        Ok(())
    }

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

    pub async fn send_text(&self, contact: &str, text: &str) -> Result<String> {
        self.send_text_replying(contact, text, None).await
    }

    pub async fn send_text_replying(&self, contact: &str, text: &str, reply_to: Option<&str>) -> Result<String> {
        let message_id = self.queue_text(contact, text, reply_to).await?;
        self.deliver_queued(&message_id).await;
        Ok(message_id)
    }

    pub async fn deliver_queued(&self, message_id: &str) {
        if let Ok(outbox) = self.store.outbox().await {
            if let Some(entry) = outbox.into_iter().find(|e| e.message_id == message_id) {
                self.attempt(&entry).await;
            }
        }
    }

    pub async fn queue_text(&self, contact: &str, text: &str, reply_to: Option<&str>) -> Result<String> {
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
        let packet = Packet::new(Body::Message { text: text.to_owned(), reply_to });
        let message = Message {
            message_id: packet.id.to_string(),
            contact: contact.to_owned(),
            outgoing: true,
            body: text.to_owned(),
            sent_at: packet.sent_at as i64,
            received_at: packet.sent_at as i64,
            state: MessageState::Pending,
        };
        let quoted = reply_to.map(|quoted| quoted.to_string());
        self.store.queue_message(&message, quoted.as_deref(), now()).await?;
        let _ = self.events.send(Event::MessagesChanged { contact: contact.to_owned() });
        Ok(message.message_id)
    }

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

    pub async fn send_raw_to(&self, contact: &str, packet: &Packet) -> Result<()> {
        let contact = self.contact(contact).await?;
        self.transmit(&contact, packet).await?;
        Ok(())
    }

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

    pub async fn send_scheduled_due(&self) -> Result<()> {
        let later = self.store.scheduled_all().await?;
        for entry in self.store.due(now()).await? {
            if later.contains_key(&entry.message_id) {
                self.attempt(&entry).await;
            }
        }
        Ok(())
    }

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

    pub async fn receive(&self, bytes: &[u8]) -> Result<()> {
        let bytes = self.unwrap(bytes).await?;
        let Some((contact, packet, first_contact)) = self.open_sealed(&bytes).await? else {
            return Ok(());
        };
        self.handle(&contact, packet, first_contact).await
    }

    pub async fn seal_signal(&self, contact: &str, body: Body) -> Result<Vec<u8>> {
        let contact = self.contact(contact).await?;
        let plaintext = self.plaintext(&Packet::new(body)).await?;
        let _identity = self.identity.lock().await;
        let mut channel = self.channel(&contact.device_id).await?;
        let sealed = channel.encrypt(&self.device_id, &plaintext)?;
        self.store.save_channel(&contact.device_id, &channel.seal(&self.key)).await?;
        Ok(sealed.encode())
    }

    pub async fn open_signal(&self, bytes: &[u8]) -> Result<Option<(String, Body)>> {
        Ok(self.open_sealed(bytes).await?.map(|(contact, packet, first_contact)| {
            if first_contact {
                let _ = self.events.send(Event::ContactsChanged);
            }
            (contact.device_id, packet.body)
        }))
    }

    pub async fn peer(&self, contact: &str) -> Result<Peer> {
        let contact = self.contact(contact).await?;
        let card = ContactCard::decode(&contact.card)?;
        Ok(Peer { device_id: contact.device_id, capability: card.route_capability() })
    }

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
                    let session = self.session_via(via.as_deref()).await?;
                    if card.device_id() != from || card.contact_keys().exchange_key != sender_key {
                        bail!("the contact card does not match its sender");
                    }
                    let name = card.name().map(clean_name).filter(|name| !name.is_empty()).unwrap_or_else(|| short_name(&from));
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
        let mut contact = self.contact(from.as_str()).await?;
        if let Some(enabled) = packet.mailbox.filter(|enabled| *enabled != contact.mailbox) {
            self.store.set_contact_mailbox(from.as_str(), enabled).await?;
            contact.mailbox = enabled;
            let _ = self.events.send(Event::ContactsChanged);
        }
        Ok(Some((contact, packet, first_contact)))
    }

    async fn handle(&self, contact: &Contact, packet: Packet, first_contact: bool) -> Result<()> {
        let id = contact.device_id.as_str();
        match packet.body {
            Body::Message { .. } if !contact.rules.accepts_chat => {
                let _ = self.send_control(contact, Body::Received { ids: vec![packet.id] }).await;
            }
            Body::Message { .. } if written_while_blocked(contact, packet.sent_at) => {
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
                    if let Some(quoted) = reply_to {
                        self.store.set_reply(&packet.id.to_string(), &quoted.to_string()).await?;
                    }
                    self.announce_messages(contact);
                }
                // ack even a duplicate, the sender is waiting for it
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
            Body::CallPresent { call, seq, plugin, file } => self.call_present_received(contact, call, seq, plugin, file).await?,
            Body::CallEnd { call, reason } => self.call_ended(contact, call, reason).await?,
            Body::MoveOffer { proof, key, size, hash } => self.move_offered(contact, proof, key, size, hash).await?,
            Body::MoveRequest { from, count } => self.move_requested(contact, from, count).await?,
            Body::MoveChunk { index, data } => self.move_chunk(contact, index, data).await?,
            Body::MoveDone => self.move_finished(contact).await?,
            Body::CircleCard { card } => self.circle_card_received(contact, packet.id, &card).await?,
            Body::CircleMessage { circle, text } => self.circle_text_received(contact, packet.id, packet.sent_at, &circle, text).await?,
            Body::CircleLeave { circle } => self.circle_leave_received(contact, packet.id, &circle).await?,
            Body::PluginEvent { plugin, data } => self.plugin_event_received(contact, plugin, data).await?,
            Body::Typing if contact.rules.accepts_chat && contact.accepted => {
                let _ = self.events.send(Event::Typing { contact: id.to_string() });
            }
            Body::Reaction { to, emoji } => {
                let to = to.to_string();
                if contact.rules.accepts_chat && self.store.message(&to).await?.is_some_and(|message| message.contact == id) {
                    let emoji = emoji.trim();
                    let emoji = (!emoji.is_empty() && is_reaction(emoji)).then_some(emoji);
                    self.store.set_reaction(&to, false, emoji, packet.sent_at as i64).await?;
                    self.announce_messages(contact);
                }
                let _ = self.send_control(contact, self.acknowledgement(contact, packet.id)).await;
            }
            // Both stamps come from the sender's clock, so a delayed edit counts from when it was made
            Body::Edit { of, text } => {
                let of = of.to_string();
                let text = text.trim();
                let theirs = contact.rules.accepts_chat
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
            Body::Delete { of } => {
                let of = of.to_string();
                let theirs = contact.rules.accepts_chat
                    && self.store.message(&of).await?.is_some_and(|message| message.contact == id && !message.outgoing);
                if theirs {
                    let file = self.store.file(&of).await?;
                    self.store.mark_deleted(&of).await?;
                    self.forget_bytes(file.into_iter().collect()).await;
                    self.announce_messages(contact);
                }
                let _ = self.send_control(contact, self.acknowledgement(contact, packet.id)).await;
            }
            // offers and answers come as signals, never as packets
            Body::Typing | Body::Block | Body::Offer { .. } | Body::Answer { .. } | Body::Unknown => {}
        }
        Ok(())
    }

    async fn receipt(&self, contact: &str, ids: Vec<MessageId>, state: MessageState) -> Result<()> {
        let mut ours = Vec::new();
        let mut updated = false;
        for id in ids.iter().map(ToString::to_string) {
            if self.store.dequeue_update(&id).await? {
                updated = true;
                continue;
            }
            if !self.circle_receipt(contact, &id, state).await? {
                ours.push(id);
            }
        }
        if ours.is_empty() {
            if updated {
                let _ = self.events.send(Event::MessagesChanged { contact: contact.to_owned() });
            }
            return Ok(());
        }
        self.store.advance(&ours, state).await?;
        for id in &ours {
            self.store.dequeue(id).await?;
        }
        let _ = self.events.send(Event::MessagesChanged { contact: contact.to_owned() });
        Ok(())
    }

    pub(crate) async fn attempt(&self, entry: &OutboxEntry) {
        let Err(error) = self.deliver(entry).await else {
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

    async fn deliver(&self, entry: &OutboxEntry) -> Result<()> {
        let Some(message) = self.store.message(&entry.message_id).await? else {
            return self.store.dequeue(&entry.message_id).await;
        };
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
            // advance only moves forward, so a receipt that came meanwhile is kept
            self.store.advance(std::slice::from_ref(&entry.message_id), MessageState::Sent).await?;
            let _ = self.events.send(Event::MessagesChanged { contact: contact.device_id.clone() });
        }
        if self.store.outbox().await?.iter().any(|e| e.message_id == entry.message_id) {
            self.store.reschedule(&entry.message_id, attempts, now() + next.as_millis() as i64, in_mailbox).await?;
        }
        Ok(())
    }

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

    pub(crate) async fn attempt_update(&self, entry: &UpdateEntry) {
        if now() - entry.created_at > UPDATE_LIFETIME.as_millis() as i64 {
            let _ = self.store.dequeue_update(&entry.packet_id).await;
            return;
        }
        let attempts = entry.attempts + 1;
        let (next, in_mailbox) = match self.deliver_update(entry).await {
            // Old apps never ack updates, so back off on direct sends too
            Ok(Route::Direct) => (retry_delay(attempts).max(RECEIPT_WAIT), entry.in_mailbox),
            Ok(Route::Mailbox) => (MAILBOX_WAIT, true),
            Ok(Route::Unreachable) => (retry_delay(attempts), entry.in_mailbox),
            Err(error) if error.downcast_ref::<MailboxRejected>().is_some() => {
                let _ = self.store.dequeue_update(&entry.packet_id).await;
                return;
            }
            Err(_) => (retry_delay(attempts), entry.in_mailbox),
        };
        let _ = self.store.reschedule_update(&entry.packet_id, attempts, now() + next.as_millis() as i64, in_mailbox).await;
    }

    async fn deliver_update(&self, entry: &UpdateEntry) -> Result<Route> {
        let contact = self.contact(&entry.contact).await?;
        let packet = Packet::decode(&entry.packet)?;
        self.transmit_as(&contact, &packet, !entry.in_mailbox).await
    }

    async fn introduce(&self, contact: &Contact) -> Result<()> {
        let card = self.my_card_in(contact.session.as_deref()).await?;
        let via = self.via(contact)?;
        self.transmit(contact, &Packet::new(Body::ContactCard { card: card.encode(), via })).await?;
        Ok(())
    }

    pub(crate) fn via(&self, contact: &Contact) -> Result<Option<Vec<u8>>> {
        Ok(Some(ContactCard::decode(&contact.card)?.route_capability().hash().to_vec()))
    }

    async fn send_control(&self, contact: &Contact, body: Body) -> Result<()> {
        self.transmit(contact, &Packet::new(body)).await?;
        Ok(())
    }

    async fn plaintext(&self, packet: &Packet) -> Result<Vec<u8>> {
        if !self.tells_mailbox.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(packet.encode());
        }
        Ok(Packet { mailbox: Some(self.mailbox().await?), ..packet.clone() }.encode())
    }

    async fn seal_for(&self, contact: &Contact, packet: &Packet) -> Result<Vec<u8>> {
        let plaintext = self.plaintext(packet).await?;
        let _identity = self.identity.lock().await;
        let mut channel = self.channel(&contact.device_id).await?;
        let sealed = channel.encrypt(&self.device_id, &plaintext)?;
        self.store.save_channel(&contact.device_id, &channel.seal(&self.key)).await?;
        Ok(sealed.encode())
    }

    async fn transmit_direct(&self, contact: &Contact, packet: &Packet) -> Result<bool> {
        let bytes = self.seal_for(contact, packet).await?;
        let card = ContactCard::decode(&contact.card)?;
        let peer = Peer { device_id: contact.device_id.clone(), capability: card.route_capability() };
        Ok(self.transport.send_direct(&peer, bytes).await.unwrap_or(false))
    }

    pub(crate) async fn transmit_direct_call(&self, contact: &Contact, packet: &Packet) -> Result<bool> {
        let bytes = self.seal_for(contact, packet).await?;
        let card = ContactCard::decode(&contact.card)?;
        let peer = Peer { device_id: contact.device_id.clone(), capability: card.route_capability() };
        Ok(self.transport.send_direct_call(&peer, bytes).await.unwrap_or(false))
    }

    pub(crate) async fn transmit_open(&self, contact: &Contact, packet: &Packet) -> Result<bool> {
        let bytes = self.seal_for(contact, packet).await?;
        let card = ContactCard::decode(&contact.card)?;
        let peer = Peer { device_id: contact.device_id.clone(), capability: card.route_capability() };
        Ok(self.transport.send_open(&peer, bytes).await.unwrap_or(false))
    }

    pub(crate) fn pongs_from(&self, contact: &str) -> (u64, tokio::sync::watch::Receiver<HashMap<String, u64>>) {
        let pongs = self.pongs.subscribe();
        let heard = pongs.borrow().get(contact).copied().unwrap_or(0);
        (heard, pongs)
    }

    pub(crate) async fn drop_connection(&self, contact: &str) {
        self.transport.disconnect(contact).await;
    }

    pub(crate) async fn open_direct_call(&self, contact: &Contact) -> Result<bool> {
        let card = ContactCard::decode(&contact.card)?;
        let peer = Peer { device_id: contact.device_id.clone(), capability: card.route_capability() };
        Ok(self.transport.open_direct_call(&peer).await.unwrap_or(false))
    }

    async fn transmit(&self, contact: &Contact, packet: &Packet) -> Result<Route> {
        self.transmit_as(contact, packet, true).await
    }

    async fn transmit_as(&self, contact: &Contact, packet: &Packet, copy: bool) -> Result<Route> {
        Ok(self.transmit_checked(contact, packet, copy, None).await?.unwrap_or(Route::Unreachable))
    }

    async fn transmit_unless_taken_back(&self, contact: &Contact, packet: &Packet, copy: bool, message_id: &str) -> Result<Option<Route>> {
        self.transmit_checked(contact, packet, copy, Some(message_id)).await
    }

    async fn still_to_send(&self, message_id: &str) -> Result<bool> {
        Ok(self.store.message(message_id).await?.is_some() && !self.store.is_deleted(message_id).await?)
    }

    async fn transmit_checked(&self, contact: &Contact, packet: &Packet, copy: bool, message: Option<&str>) -> Result<Option<Route>> {
        let bytes = self.seal_for(contact, packet).await?;
        let card = ContactCard::decode(&contact.card)?;
        let peer = Peer { device_id: contact.device_id.clone(), capability: card.route_capability() };

        let mailbox = contact.mailbox && self.mailbox().await?;
        let direct = match message {
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
            // sealed so the router only sees who it's for
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

// New capability so the old QR reaches nobody, not even the slot's next session
async fn forget_session(store: &Store, session: &str) -> Result<()> {
    if let Some(slot) = store.session_slot(session).await? {
        store.set_spare_capability(slot, RouteCapability::generate().as_bytes()).await?;
    }
    store.remove_session(session).await
}

fn short_name(device_id: &DeviceId) -> String {
    device_id.as_str().chars().take(9).collect()
}

pub const NAME_LIMIT: usize = 40;

// Drops control chars and bidi overrides, which can make a name display as something else
pub fn clean_name(name: &str) -> String {
    let readable = name
        .chars()
        .filter(|c| !c.is_control())
        .filter(|c| !matches!(*c, '\u{200e}' | '\u{200f}' | '\u{061c}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}'))
        .collect::<String>();
    readable.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(NAME_LIMIT).collect::<String>().trim_end().to_owned()
}

// The card goes out as a pre-key message on the new sessions, so no key of the backup's sessions sends again
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

// FT_DEBUG_TRIAL_OVER=1 makes the free days count as over in debug builds, nothing is written
#[cfg(debug_assertions)]
fn trial_over_for_debugging() -> bool {
    std::env::var("FT_DEBUG_TRIAL_OVER").as_deref() == Ok("1")
}

// Compares their clock with ours, so clock skew moves the cutoff
pub(crate) fn written_while_blocked(contact: &Contact, sent_at: u64) -> bool {
    contact.unblocked_at.is_some_and(|unblocked_at| (sent_at as i64) < unblocked_at)
}

// ms since the Unix epoch
pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Week {
    pub days: Vec<Day>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum Day {
    // "all" or "none"
    Whole(String),
    // "HH:MM", wraps past midnight when to < from
    Hours { from: String, to: String },
}

impl Week {
    // Parsed by the native side. Seven `;`-separated days from Monday, each `all`, `none` or `FROM-TO` in minutes.
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
    #[test]
    fn the_week_is_handed_to_the_phone_in_minutes() {
        let week: Week = serde_json::from_str(
            r#"{"days":[{"from":"18:00","to":"22:00"},"none",{"from":"22:00","to":"02:00"},"all","all","all","all"]}"#,
        )
        .unwrap();
        assert_eq!(week.compact(), "1080-1320;none;1320-120;all;all;all;all");
    }

    use super::*;

    #[test]
    fn names_are_tamed() {
        assert_eq!(clean_name("  Bob   Smith "), "Bob Smith");
        assert_eq!(clean_name("Mam\u{202e}á\u{0000}"), "Mamá");
        assert_eq!(clean_name(&"x".repeat(100)).chars().count(), NAME_LIMIT);
        assert_eq!(clean_name("\t\n"), "");
        assert_eq!(clean_name("Zoë 😀"), "Zoë 😀");
    }

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
