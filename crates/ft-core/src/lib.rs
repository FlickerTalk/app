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
pub mod files;
pub mod moving;
pub mod plugins;
pub mod net;
pub mod online;
pub mod web;

pub use plugins::CATALOGUE_HOME;
pub use web::{Fetch, Web, WebAnswer, WebRequest};

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, ensure, Context, Result};
use async_trait::async_trait;
use ft_billing::{Access, AgeClass, Doing, Plan};
use ft_contacts::{ContactCard, RouteCapability};
use ft_crypto::{accept_first_contact, Channel};
use ft_identity::{DeviceId, Identity};
use ft_protocol::{Body, MessageId, Packet, Sealed};
use ft_storage::{Contact, ContactRules, Message, MessageState, NewContact, OutboxEntry, Store};
use tokio::sync::{broadcast, Mutex};

/// Unreachable contacts are retried after 5 s, 10 s, 20 s… up to this.
pub const MAX_RETRY_DELAY: Duration = Duration::from_secs(300);
/// After a direct send, how long to wait for the receipt before trying again.
const RECEIPT_WAIT: Duration = Duration::from_secs(30);
/// After leaving it in the mailbox, how long before trying again.
const MAILBOX_WAIT: Duration = Duration::from_secs(600);

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
    /// Leaves the packet, already encrypted, in the peer's mailbox on the router (§19).
    async fn send_mailbox(&self, to: &Peer, bytes: Vec<u8>) -> Result<()>;
    /// Closes any direct connection with the device (a blocked contact, §35).
    async fn disconnect(&self, _device_id: &str) {}
}

pub use calls::CallUpdate;
pub use ft_push::{RouterClient, TurnGrant};

/// What the UI listens to, to refresh itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    ContactsChanged,
    MessagesChanged { contact: String },
    /// A direct connection with the contact opened or closed.
    ConnectionChanged { contact: String },
    /// Something happened to a call (§66).
    Call { contact: String, call: String, update: CallUpdate },
    /// Moving to a new phone (§60).
    Move(moving::MoveUpdate),
    /// A plugin was installed, granted something, or removed (issue app#3).
    PluginsChanged,
}

enum Route {
    Direct,
    Mailbox,
    Unreachable,
}

pub struct Core {
    store: Store,
    /// Seals the identity and the Olm sessions at rest.
    key: [u8; 32],
    /// Olm work runs one at a time: a channel is loaded, used and saved back under this lock.
    identity: Mutex<Identity>,
    device_id: DeviceId,
    capability: RouteCapability,
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
    /// Where a move to a new phone writes its copies (set by the app).
    move_dir: OnceLock<PathBuf>,
    moving: std::sync::Mutex<moving::MoveState>,
    /// The hidden sessions open right now, with their slot. Only in memory: the app starts with
    /// all of them closed.
    open_sessions: std::sync::Mutex<HashMap<String, u8>>,
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

impl Core {
    /// Opens the device's core; the first run creates the identity and route capability.
    pub async fn open(store: Store, key: [u8; 32], transport: Arc<dyn Transport>) -> Result<Arc<Self>> {
        let (identity, capability) = match store.identity().await? {
            Some(saved) => (Identity::unseal(&saved.sealed, &key)?, RouteCapability::from_bytes(saved.route_capability)),
            None => {
                let identity = Identity::generate();
                let capability = RouteCapability::generate();
                store.save_identity(&identity.seal(&key), capability.as_bytes()).await?;
                (identity, capability)
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
        if store.setting(INSTALLED_AT).await?.is_none() {
            store.set_setting(INSTALLED_AT, &now().to_string()).await?;
        }
        let (events, _) = broadcast::channel(256);
        Ok(Arc::new(Self {
            device_id: identity.device_id(),
            identity: Mutex::new(identity),
            store,
            key,
            capability,
            transport,
            events,
            files_dir: OnceLock::new(),
            plugins_dir: OnceLock::new(),
            transfers: std::sync::Mutex::default(),
            active_call: std::sync::Mutex::default(),
            move_dir: OnceLock::new(),
            moving: std::sync::Mutex::default(),
            open_sessions: std::sync::Mutex::default(),
        }))
    }

    pub fn device_id(&self) -> &DeviceId {
        &self.device_id
    }

    pub fn route_capability(&self) -> RouteCapability {
        self.capability
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
        for contact in self.store.contacts().await? {
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
        let capability = self.capability_of(session).await?;
        let (name, mailbox) = (self.name().await?, self.mailbox().await?);
        let mut identity = self.identity.lock().await;
        let card = ContactCard::create(&mut identity, name, capability, mailbox);
        // Creating the first card adds the Olm fallback key to the account.
        self.store.save_identity(&identity.seal(&self.key), self.capability.as_bytes()).await?;
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
            .filter(|name| !name.trim().is_empty())
            .or_else(|| card.name().map(str::to_owned))
            .unwrap_or_else(|| short_name(&device_id));
        self.store
            .add_contact(&NewContact {
                device_id: device_id.to_string(),
                name,
                card: card.encode(),
                mailbox: card.mailbox(),
                session: session.map(str::to_owned),
                receipts: self.receipts_default().await?,
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

    /// Opens the hidden session whose PIN this is or, if none has it, a new empty one, and
    /// returns its id. Nothing tells the two apart: that is what keeps a session unknowable.
    pub async fn open_session(&self, pin: &str) -> Result<String> {
        ensure!(pin.len() == SESSION_PIN_LENGTH && pin.bytes().all(|byte| byte.is_ascii_digit()), "a PIN is six digits");
        let hash = *blake3::keyed_hash(&blake3::derive_key(SESSION_PIN_CONTEXT, &self.key), pin.as_bytes()).as_bytes();
        let (id, slot) = match self.store.session_by_pin(&hash).await? {
            Some(id) => match self.store.session_slot(&id).await? {
                Some(slot) => (id, slot),
                None => {
                    // Made before slots: it gets one now and tells its contacts the new way in.
                    let slot = self.free_slot().await?;
                    self.store.set_session_slot(&id, slot).await?;
                    self.open_sessions.lock().expect("sessions poisoned").insert(id.clone(), slot);
                    for contact in self.store.session_contacts(&id).await? {
                        let _ = self.introduce(&contact).await;
                    }
                    (id, slot)
                }
            },
            None => {
                let slot = self.free_slot().await?;
                let id = MessageId::new().to_string();
                self.store.add_session(&id, &hash, slot).await?;
                (id, slot)
            }
        };
        self.open_sessions.lock().expect("sessions poisoned").insert(id.clone(), slot);
        Ok(id)
    }

    async fn free_slot(&self) -> Result<u8> {
        let used = self.store.used_slots().await?;
        (1..=SPARE_SLOTS).find(|slot| !used.contains(slot)).ok_or_else(|| anyhow!("no room for another session"))
    }

    /// Leaves the session: from now on it receives in silence, until its PIN opens it again.
    pub fn close_session(&self, session: &str) {
        self.open_sessions.lock().expect("sessions poisoned").remove(session);
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

    /// The hashes the router gets (app#9): our own capability first, then the seven spares,
    /// used or not. Always the same eight.
    pub async fn route_capability_hashes(&self) -> Result<[[u8; 32]; 8]> {
        let mut hashes = [self.capability.hash(); 8];
        for (slot, capability) in self.store.spare_capabilities().await? {
            hashes[slot as usize] = RouteCapability::from_bytes(capability).hash();
        }
        Ok(hashes)
    }

    /// The route capability a session hands out, or our own for the main list.
    async fn capability_of(&self, session: Option<&str>) -> Result<RouteCapability> {
        let Some(session) = session else { return Ok(self.capability) };
        let slot = self.store.session_slot(session).await?.ok_or_else(|| anyhow!("the session has no slot"))?;
        let spare = self.store.spare_capabilities().await?.into_iter().find(|(spare, _)| *spare == slot);
        spare.map(|(_, capability)| RouteCapability::from_bytes(capability)).ok_or_else(|| anyhow!("no capability for the slot"))
    }

    /// The session a sender belongs to, from the hash of our capability they used.
    async fn session_via(&self, via: Option<&[u8]>) -> Result<Option<String>> {
        let Some(via) = via else { return Ok(None) };
        let hashes = self.route_capability_hashes().await?;
        let Some(slot) = (1..8).find(|slot| hashes[*slot].as_slice() == via) else { return Ok(None) };
        self.store.session_with_slot(slot as u8).await
    }

    /// Whether what this contact sends must make no noise: they belong to a closed session.
    pub(crate) fn silent(&self, contact: &Contact) -> bool {
        contact.session.as_deref().is_some_and(|session| !self.open_sessions.lock().expect("sessions poisoned").contains_key(session))
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
        for file in theirs {
            if self.store.file(&file.message_id).await?.is_none() {
                let _ = std::fs::remove_file(self.file_path(&file));
            }
        }
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
        if let Some(file) = file {
            let _ = std::fs::remove_file(self.file_path(&file));
        }
        let _ = self.events.send(Event::MessagesChanged { contact: stored.contact });
        Ok(())
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
        let text = text.trim();
        if text.is_empty() {
            bail!("nothing to send");
        }
        self.contact(contact).await?;
        // Answering is always allowed; writing to someone for the first time is starting (§42).
        let started = !self.store.messages(contact, 1).await?.is_empty();
        self.allowed(if started { Doing::Reply } else { Doing::Start }).await?;
        let packet = Packet::new(Body::Message { text: text.to_owned() });
        let message_id = packet.id.to_string();
        self.store
            .insert_message(&Message {
                message_id: message_id.clone(),
                contact: contact.to_owned(),
                outgoing: true,
                body: text.to_owned(),
                sent_at: packet.sent_at as i64,
                state: MessageState::Pending,
            })
            .await?;
        self.store.enqueue(&message_id, contact, now()).await?;
        let _ = self.events.send(Event::MessagesChanged { contact: contact.to_owned() });

        if let Some(entry) = self.store.outbox().await?.into_iter().find(|e| e.message_id == message_id) {
            self.deliver(&entry).await?;
        }
        Ok(message_id)
    }

    /// Retries the outbox entries that are due.
    pub async fn retry_due(&self) -> Result<()> {
        for entry in self.store.due(now()).await? {
            self.deliver(&entry).await?;
        }
        Ok(())
    }

    /// Retries every pending message now (the contact came online, say).
    pub async fn retry_now(&self) -> Result<()> {
        for entry in self.store.outbox().await? {
            self.deliver(&entry).await?;
        }
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

    /// A packet from the network: the DataChannel or the mailbox.
    pub async fn receive(&self, bytes: &[u8]) -> Result<()> {
        let Some((contact, packet, first_contact)) = self.open_sealed(bytes).await? else {
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
                    let name = card.name().map(str::to_owned).unwrap_or_else(|| short_name(&from));
                    self.store
                        .add_contact(&NewContact {
                            device_id: from.to_string(),
                            name,
                            card: card.encode(),
                            mailbox: card.mailbox(),
                            session,
                            receipts: self.receipts_default().await?,
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
            Body::Message { text } => {
                let stored = self
                    .store
                    .insert_message(&Message {
                        message_id: packet.id.to_string(),
                        contact: id.to_owned(),
                        outgoing: false,
                        body: text,
                        sent_at: packet.sent_at as i64,
                        state: MessageState::Delivered,
                    })
                    .await?;
                if stored {
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
            Body::File { name, size, mime, hash, chunk } => {
                self.offered(contact, packet.id, packet.sent_at, name, size, mime, hash, chunk).await?
            }
            Body::FileRequest { file, from, count } => self.serve_chunks(contact, file, from, count).await?,
            Body::FileChunk { file, index, data } => self.take_chunk(contact, file, index, data).await?,
            Body::FileDone { file } => self.file_done(contact, file).await?,
            Body::FileFailed { file } => self.file_failed(contact, file).await?,
            Body::CallOffer { call, sdp, video } => self.call_offered(contact, call, sdp, video).await?,
            Body::CallAnswer { call, sdp } => self.call_answered(contact, call, sdp).await?,
            Body::CallEnd { call, reason } => self.call_ended(contact, call, reason).await?,
            Body::MoveOffer { proof, key, size, hash } => self.move_offered(contact, proof, key, size, hash).await?,
            Body::MoveRequest { from, count } => self.move_requested(contact, from, count).await?,
            Body::MoveChunk { index, data } => self.move_chunk(contact, index, data).await?,
            Body::MoveDone => self.move_finished(contact).await?,
            // Offers and answers travel as signals (see `open_signal`), never as packets.
            Body::Pong | Body::Typing | Body::Block | Body::Offer { .. } | Body::Answer { .. } | Body::Unknown => {}
        }
        Ok(())
    }

    async fn receipt(&self, contact: &str, ids: Vec<MessageId>, state: MessageState) -> Result<()> {
        let ids: Vec<String> = ids.iter().map(ToString::to_string).collect();
        self.store.advance(&ids, state).await?;
        for id in &ids {
            self.store.dequeue(id).await?;
        }
        let _ = self.events.send(Event::MessagesChanged { contact: contact.to_owned() });
        Ok(())
    }

    /// One delivery attempt for an outbox entry.
    async fn deliver(&self, entry: &OutboxEntry) -> Result<()> {
        let Some(message) = self.store.message(&entry.message_id).await? else {
            return self.store.dequeue(&entry.message_id).await;
        };
        let contact = self.contact(&entry.contact).await?;
        if !contact.introduced {
            self.introduce(&contact).await?;
        }
        if let Some(file) = self.store.file(&entry.message_id).await? {
            return self.deliver_file(entry, &contact, &message, &file).await;
        }
        let packet = Packet::resend(MessageId::parse(&message.message_id)?, message.sent_at as u64, Body::Message { text: message.body });
        let attempts = entry.attempts + 1;
        let (next, in_mailbox) = match self.transmit(&contact, &packet).await? {
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

    async fn transmit(&self, contact: &Contact, packet: &Packet) -> Result<Route> {
        let bytes = self.seal_for(contact, packet).await?;
        let card = ContactCard::decode(&contact.card)?;
        let peer = Peer { device_id: contact.device_id.clone(), capability: card.route_capability() };

        if self.transport.send_direct(&peer, bytes.clone()).await.unwrap_or(false) {
            return Ok(Route::Direct);
        }
        if contact.mailbox && self.mailbox().await? {
            self.transport.send_mailbox(&peer, bytes).await.context("the mailbox is not reachable")?;
            return Ok(Route::Mailbox);
        }
        Ok(Route::Unreachable)
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

/// A readable default name for a contact whose card has none.
fn short_name(device_id: &DeviceId) -> String {
    device_id.as_str().chars().take(9).collect()
}

fn now() -> i64 {
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

    #[test]
    fn unreachable_contacts_are_retried_ever_more_slowly_up_to_a_limit() {
        assert!(retry_delay(1) < retry_delay(2));
        assert!(retry_delay(2) < retry_delay(4));
        assert_eq!(retry_delay(50), MAX_RETRY_DELAY);
    }
}
