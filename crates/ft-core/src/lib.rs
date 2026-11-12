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
mod contacts;
mod sending;
mod sessions;
mod settings;

pub use contacts::{clean_name, NAME_LIMIT};
pub use plugins::CATALOGUE_HOME;
pub use sending::{retry_delay, MAX_RETRY_DELAY};
pub use settings::{Day, Week};
pub use web::{Fetch, Web, WebAnswer, WebRequest};
pub(crate) use sending::{Route, MAILBOX_WAIT, RECEIPT_WAIT};

use contacts::short_name;
use sending::{is_reaction, EDIT_WINDOW_MS};
use sessions::forget_session;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{bail, ensure, Context, Result};
use async_trait::async_trait;
use ft_billing::{Access, Plan};
use ft_contacts::{ContactCard, RouteCapability};
use ft_crypto::{accept_first_contact, Channel};
use ft_identity::{DeviceId, EnvelopeKey, Identity};
use ft_protocol::{Body, Envelope, Packet, Sealed, ENVELOPE_VERSION};
use ft_storage::{Contact, Message, MessageState, NewContact, Store};
use tokio::sync::{broadcast, Mutex};

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

const SPARE_SLOTS: u8 = 7;
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

    pub async fn search(&self, contact: &str, query: &str) -> Result<Vec<Message>> {
        self.contact(contact).await?;
        self.store.search_messages(contact, query, 50).await
    }

    pub async fn pin_message(&self, message_id: &str, pinned: bool) -> Result<()> {
        let message = self.store.message(message_id).await?.context("that message is not here")?;
        self.store.set_pinned(message_id, pinned).await?;
        let _ = self.events.send(Event::MessagesChanged { contact: message.contact });
        Ok(())
    }

    pub async fn receive(&self, bytes: &[u8]) -> Result<()> {
        let bytes = self.unwrap(bytes).await?;
        let Some((contact, packet, first_contact)) = self.open_sealed(&bytes).await? else {
            return Ok(());
        };
        self.handle(&contact, packet, first_contact).await
    }

    pub async fn open_signal(&self, bytes: &[u8]) -> Result<Option<(String, Body)>> {
        Ok(self.open_sealed(bytes).await?.map(|(contact, packet, first_contact)| {
            if first_contact {
                let _ = self.events.send(Event::ContactsChanged);
            }
            (contact.device_id, packet.body)
        }))
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

    async fn channel(&self, contact: &str) -> Result<Channel> {
        match self.store.channel(contact).await? {
            Some(sealed) => Channel::unseal(&sealed, &self.key),
            None => Ok(Channel::default()),
        }
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
