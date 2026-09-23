//! The app's bridge to ft-core (Plan §82, §106 M3): starts the core online and exposes thin
//! commands to the UI. No business logic here: every command delegates to the core, and what
//! crosses to the WebView are plain views (never keys, never the capability, §54).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use anyhow::Context;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use ft_core::online::{self, Online};
use ft_core::moving::MoveUpdate;
use ft_core::{CallUpdate, Core, Event, TurnGrant};
use ft_storage::{CallOutcome, CallRecord, Conversation, FileRecord, Message, MessageState, Store};
use ft_webrtc::SessionConfig;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_ft_platform::PlatformExt;
use tokio::sync::OnceCell;

/// The router of the cluster, behind the load balancer (Plan §75).
pub const ROUTER: &str = "https://api.flickertalk.com";
/// Sent to the UI whenever contacts or messages change; `contact` says which conversation.
pub const CHANGED_EVENT: &str = "ft://changed";

pub fn state_name(state: MessageState) -> &'static str {
    match state {
        MessageState::Pending => "pending",
        MessageState::Sent => "sent",
        MessageState::Delivered => "delivered",
        MessageState::Read => "read",
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageView {
    id: String,
    outgoing: bool,
    text: String,
    sent_at: i64,
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<FileView>,
}

impl MessageView {
    pub fn new(message: &Message, file: Option<&FileRecord>) -> Self {
        Self {
            id: message.message_id.clone(),
            outgoing: message.outgoing,
            text: message.body.clone(),
            sent_at: message.sent_at,
            state: state_name(message.state),
            file: file.map(FileView::from),
        }
    }
}

/// A file message's transfer (§62–63). `path` is on this device, for showing images.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    name: String,
    size: i64,
    mime: String,
    /// From 0 to 1: chunks sent (outgoing) or received (incoming).
    progress: f64,
    /// `transferring`, `done` (the receiver has it and the hash matches) or `failed`.
    state: &'static str,
    path: String,
}

impl From<&FileRecord> for FileView {
    fn from(file: &FileRecord) -> Self {
        let progress = match file.chunks() {
            _ if file.complete => 1.0,
            0 => 0.0,
            chunks => file.chunks_done as f64 / chunks as f64,
        };
        let state = if file.complete {
            "done"
        } else if file.failed {
            "failed"
        } else {
            "transferring"
        };
        Self { name: file.name.clone(), size: file.size, mime: file.mime.clone(), progress, state, path: file.path.clone() }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationView {
    id: String,
    name: String,
    unread: i64,
    blocked: bool,
    /// Whether a direct connection with the contact is open now.
    connected: bool,
    last: Option<MessageView>,
}

impl ConversationView {
    pub fn new(conversation: &Conversation, connected: bool) -> Self {
        Self {
            id: conversation.contact.device_id.clone(),
            name: conversation.contact.name.clone(),
            unread: conversation.unread,
            blocked: conversation.contact.blocked,
            connected,
            last: conversation.last.as_ref().map(|last| MessageView::new(last, None)),
        }
    }
}

/// A hidden session as the UI sees it: an id and its conversations. No name, nothing to read.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    id: String,
    conversations: Vec<ConversationView>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeView {
    id: String,
    name: String,
    mailbox: bool,
    /// Whether contacts added from now on get receipts (app#6).
    receipts: bool,
    /// Until when (ms) the app is free (§41).
    free_until: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactView {
    id: String,
    name: String,
    fingerprint: String,
    mailbox: bool,
    blocked: bool,
    /// Seconds this phone keeps their messages; 0 forever (issue app#1).
    keep_for: i64,
    /// Seconds a read message stays after being read; 0 never.
    burn_after_read: i64,
    rules: RulesView,
}

/// What this phone takes from a contact and tells them (issues app#4–#6).
#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct RulesView {
    muted: bool,
    accepts_chat: bool,
    accepts_calls: bool,
    receipts: bool,
}

impl From<ft_storage::ContactRules> for RulesView {
    fn from(rules: ft_storage::ContactRules) -> Self {
        Self { muted: rules.muted, accepts_chat: rules.accepts_chat, accepts_calls: rules.accepts_calls, receipts: rules.receipts }
    }
}

impl From<RulesView> for ft_storage::ContactRules {
    fn from(rules: RulesView) -> Self {
        Self { muted: rules.muted, accepts_chat: rules.accepts_chat, accepts_calls: rules.accepts_calls, receipts: rules.receipts }
    }
}

#[derive(Clone, Serialize)]
struct Changed {
    contact: Option<String>,
}

/// Sent to the UI on `ft://call` (§66).
pub const CALL_EVENT: &str = "ft://call";

/// What happened to a call, as the WebView hears it.
#[derive(Clone, Serialize)]
pub struct CallEvent {
    contact: String,
    call: String,
    /// `incoming`, `answered` or `ended`.
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    video: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sdp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    outcome: Option<&'static str>,
}

impl CallEvent {
    pub fn new(contact: &str, call: &str, update: CallUpdate) -> Self {
        let (kind, video, sdp, outcome) = match update {
            CallUpdate::Incoming { video, sdp } => ("incoming", Some(video), Some(sdp), None),
            CallUpdate::Answered { sdp } => ("answered", None, Some(sdp), None),
            CallUpdate::Ended { outcome } => ("ended", None, None, Some(outcome.as_str())),
        };
        Self { contact: contact.to_owned(), call: call.to_owned(), kind, video, sdp, outcome }
    }
}

/// What the phone does about ringing after a call update (§66).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ring {
    /// Ring and show the call on the screen; video calls say so.
    Start { video: bool },
    Stop,
    Nothing,
}

pub fn ringing(update: &CallUpdate) -> Ring {
    match update {
        CallUpdate::Incoming { video, .. } => Ring::Start { video: *video },
        CallUpdate::Ended { .. } => Ring::Stop,
        CallUpdate::Answered { .. } => Ring::Nothing,
    }
}

/// An ICE server as the WebView's `RTCPeerConnection` takes it.
#[derive(Serialize)]
pub struct IceServer {
    urls: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    credential: Option<String>,
}

/// Sent to the UI on `ft://move` (§60).
pub const MOVE_EVENT: &str = "ft://move";

/// How moving to a new phone goes, as the WebView hears it.
#[derive(Clone, Serialize)]
pub struct MoveEvent {
    /// `progress`, `received` (new phone), `sent` (old phone) or `failed`.
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    done: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total: Option<u64>,
}

impl From<MoveUpdate> for MoveEvent {
    fn from(update: MoveUpdate) -> Self {
        let (kind, done, total) = match update {
            MoveUpdate::Progress { done, total } => ("progress", Some(done), Some(total)),
            MoveUpdate::Received => ("received", None, None),
            MoveUpdate::Sent => ("sent", None, None),
            MoveUpdate::Failed => ("failed", None, None),
        };
        Self { kind, done, total }
    }
}

const DATABASE: &str = "flickertalk.db";
const KEY_FILE: &str = "storage.key";
const MOVE_DIR: &str = "move";

/// The database and its SQLite side files.
fn database_files(dir: &Path) -> [PathBuf; 3] {
    [dir.join(DATABASE), dir.join(format!("{DATABASE}-wal")), dir.join(format!("{DATABASE}-shm"))]
}

/// Swaps in the database and key a move brought (§60); `true` if there was one. Runs at start,
/// before the core opens.
pub fn apply_move(dir: &Path) -> std::io::Result<bool> {
    let incoming = dir.join(MOVE_DIR);
    let Some((database, key)) = ft_core::moving::received_move(&incoming) else { return Ok(false) };
    for file in database_files(dir) {
        let _ = std::fs::remove_file(file);
    }
    std::fs::rename(database, dir.join(DATABASE))?;
    // The moved key replaces this phone's own, sealed or not; the next start seals it.
    let _ = std::fs::remove_file(dir.join(SEALED_KEY_FILE));
    std::fs::write(dir.join(KEY_FILE), key)?;
    #[cfg(unix)]
    std::fs::set_permissions(dir.join(KEY_FILE), std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    std::fs::remove_dir_all(incoming)?;
    Ok(true)
}

/// Erases this phone's identity, contacts, history and files: it moved to another phone (§60).
pub fn erase(dir: &Path) -> std::io::Result<()> {
    for file in database_files(dir).into_iter().chain([dir.join(KEY_FILE), dir.join(SEALED_KEY_FILE)]) {
        match std::fs::remove_file(file) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    for folder in [dir.join("files"), dir.join(MOVE_DIR)] {
        match std::fs::remove_dir_all(folder) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    Ok(())
}

/// The router's STUN servers and short-lived TURN user, for the WebView's calls (§16–17).
pub fn ice_servers(stun: Vec<String>, turn: Option<TurnGrant>) -> Vec<IceServer> {
    let mut servers = Vec::new();
    if !stun.is_empty() {
        servers.push(IceServer { urls: stun, username: None, credential: None });
    }
    if let Some(grant) = turn {
        servers.push(IceServer { urls: grant.urls, username: Some(grant.username), credential: Some(grant.credential) });
    }
    servers
}

/// A call in the history.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallView {
    id: String,
    contact: String,
    name: String,
    outgoing: bool,
    video: bool,
    started_at: i64,
    /// How long it was talked, from the answer to the end.
    seconds: i64,
    outcome: Option<&'static str>,
}

impl CallView {
    pub fn new(call: &CallRecord, name: &str) -> Self {
        let seconds = match (call.answered_at, call.ended_at) {
            (Some(answered), Some(ended)) => (ended - answered).max(0) / 1000,
            _ => 0,
        };
        Self {
            id: call.call_id.clone(),
            contact: call.contact.clone(),
            name: name.to_owned(),
            outgoing: call.outgoing,
            video: call.video,
            started_at: call.started_at,
            seconds,
            outcome: call.outcome.map(CallOutcome::as_str),
        }
    }
}

/// The operating system's key store (Android Keystore, iOS Keychain, §94): it seals the storage
/// key so the file alone is useless off this phone.
pub trait KeyVault {
    fn seal(&self, key: &[u8; 32]) -> anyhow::Result<Vec<u8>>;
    fn open(&self, sealed: &[u8]) -> anyhow::Result<[u8; 32]>;
}

const SEALED_KEY_FILE: &str = "storage.key.sealed";

/// Writes a file only this app can read, replacing any before it.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    std::io::Write::write_all(&mut options.open(path)?, bytes)
}

/// The 32-byte key that seals the identity and the Olm sessions at rest. With a `vault` it is kept
/// sealed by the OS key store (`storage.key.sealed`), and a key from before is moved into it;
/// without one (desktop), in a private file. A sealed key that does not open is an error: a new
/// key would lose the identity.
pub fn storage_key(dir: &Path, vault: Option<&dyn KeyVault>) -> anyhow::Result<[u8; 32]> {
    std::fs::create_dir_all(dir)?;
    let (clear, sealed) = (dir.join(KEY_FILE), dir.join(SEALED_KEY_FILE));
    if let Some(vault) = vault {
        if let Ok(bytes) = std::fs::read(&sealed) {
            return vault.open(&bytes).context("the key store cannot open the storage key");
        }
    }
    let key: [u8; 32] = match std::fs::read(&clear) {
        Ok(bytes) => bytes.try_into().map_err(|_| anyhow::anyhow!("the storage key is corrupt"))?,
        Err(_) => {
            let key: [u8; 32] = rand::random();
            if vault.is_none() {
                write_private(&clear, &key)?;
            }
            key
        }
    };
    if let Some(vault) = vault {
        write_private(&sealed, &vault.seal(&key)?)?;
        let _ = std::fs::remove_file(&clear);
    }
    Ok(key)
}

/// A random name for a file being copied in from the WebView.
pub fn new_upload_id() -> String {
    rand::random::<[u8; 16]>().iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Where an upload is written. Only ids made by `new_upload_id` are accepted.
pub fn upload_path(dir: &Path, id: &str) -> Result<PathBuf, String> {
    let valid = id.len() == 32 && id.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c));
    if !valid {
        return Err("invalid upload".to_owned());
    }
    Ok(dir.join("files").join("outgoing").join(id))
}

/// The file behind a message, if it can be opened here: ours always, theirs once it has arrived
/// whole and verified.
pub fn openable(file: Option<FileRecord>, outgoing: bool) -> Result<FileRecord, String> {
    match file {
        Some(file) if outgoing || file.complete => Ok(file),
        Some(_) => Err("the file has not arrived yet".to_owned()),
        None => Err("not a file".to_owned()),
    }
}

/// The running core, started once.
#[derive(Default)]
pub struct Client {
    online: OnceCell<Online>,
    dir: OnceLock<PathBuf>,
    app: OnceLock<AppHandle>,
    /// The OS key store on phones; none on desktop.
    vault: OnceLock<Box<dyn KeyVault + Send + Sync>>,
    /// The web the core uses on someone else's behalf: the catalogue, and what a plugin is
    /// allowed to reach (§55–§56).
    web: ft_core::Web,
}

impl Client {
    pub fn web(&self) -> &ft_core::Web {
        &self.web
    }
}

/// The key store through the native bridge (Android Keystore, iOS Keychain).
struct PlatformVault(AppHandle);

impl KeyVault for PlatformVault {
    fn seal(&self, key: &[u8; 32]) -> anyhow::Result<Vec<u8>> {
        Ok(self.0.platform().seal_key(key)?)
    }

    fn open(&self, sealed: &[u8]) -> anyhow::Result<[u8; 32]> {
        Ok(self.0.platform().open_key(sealed)?)
    }
}

impl Client {
    pub fn setup(&self, app: &AppHandle) -> anyhow::Result<()> {
        let dir = app.path().app_data_dir().context("no app data directory")?;
        let _ = self.dir.set(dir);
        let _ = self.app.set(app.clone());
        if cfg!(mobile) {
            let _ = self.vault.set(Box::new(PlatformVault(app.clone())));
        }
        Ok(())
    }

    async fn online(&self) -> Result<&Online, String> {
        self.online.get_or_try_init(|| self.start()).await.map_err(|error| error.to_string())
    }

    async fn core(&self) -> Result<Arc<Core>, String> {
        Ok(self.online().await?.core.clone())
    }

    fn dir(&self) -> Result<&Path, String> {
        self.dir.get().map(PathBuf::as_path).ok_or_else(|| "the app is not set up yet".to_owned())
    }

    async fn start(&self) -> anyhow::Result<Online> {
        let dir = self.dir.get().context("the app is not set up yet")?;
        apply_move(dir)?;
        let key = storage_key(dir, self.vault.get().map(|vault| vault.as_ref() as &dyn KeyVault))?;
        let store = Store::open(&dir.join(DATABASE)).await?;
        let online = online::start(store, key, ROUTER, SessionConfig::default()).await?;
        online.core.set_files_dir(dir.join("files"));
        online.core.set_move_dir(dir.join(MOVE_DIR));
        online.core.set_plugins_dir(dir.join("plugins"));
        // A new version of the app brings a fixed tool to whoever had installed it (§53).
        update_installed_plugins(&online.core).await;
        // What the Store says about the subscription, every time the app opens (§45): a renewal
        // shows up on its own, and one that was cancelled stops counting.
        if let Some(app) = self.app.get() {
            if let Ok(until) = app.platform().subscription() {
                let _ = online.core.set_entitlement(until).await;
            }
        }
        if let Some(app) = self.app.get() {
            refresh_served_plugins(app, &online.core, dir).await;
            // The weekly hours live in the core; the native side keeps its own copy (app#7).
            if let Ok(week) = online.core.quiet_week().await {
                let _ = app.platform().set_quiet_hours(&week);
            }
        }

        if let Some(app) = self.app.get().cloned() {
            let mut events = online.core.events();
            let dir_for_events = dir.to_owned();
            let router_for_events = online.router.clone();
            let core_for_events = online.core.clone();
            tauri::async_runtime::spawn(async move {
                while let Ok(event) = events.recv().await {
                    let contact = match event {
                        Event::MessagesChanged { contact } => Some(contact),
                        Event::ContactsChanged | Event::ConnectionChanged { .. } | Event::PluginsChanged => None,
                        Event::Move(update) => {
                            let _ = app.emit(MOVE_EVENT, MoveEvent::from(update.clone()));
                            after_move(&app, &dir_for_events, &router_for_events, update);
                            continue;
                        }
                        Event::Call { contact, call, update } => {
                            match ringing(&update) {
                                Ring::Start { video } => {
                                    // The screen says who is calling, so a call in the background
                                    // is more than a ringtone (§66).
                                    let stored = core_for_events.store().contact(&contact).await.ok().flatten();
                                    let name = stored.as_ref().map(|stored| stored.name.clone()).unwrap_or_default();
                                    // A muted contact shows but makes no noise (app#4).
                                    let muted = stored.is_some_and(|stored| stored.rules.muted);
                                    let _ = app.platform().start_ringing(&name, video, muted);
                                }
                                Ring::Stop => {
                                    let _ = app.platform().stop_ringing();
                                }
                                Ring::Nothing => {}
                            }
                            let _ = app.emit(CALL_EVENT, CallEvent::new(&contact, &call, update));
                            continue;
                        }
                    };
                    let _ = app.emit(CHANGED_EVENT, Changed { contact });
                }
            });
        }
        Ok(online)
    }
}

/// What the WebView may serve of each plugin right now: kept in step with what is installed and
/// what the user granted (§53, §55).
pub async fn refresh_served_plugins(app: &AppHandle, core: &Arc<ft_core::Core>, dir: &Path) {
    let mut served = std::collections::HashMap::new();
    for plugin in core.plugins().await.unwrap_or_default() {
        let Some(component) = plugin.manifest.components.first().cloned() else { continue };
        served.insert(
            plugin.manifest.id.clone(),
            crate::plugins::Served {
                dir: dir.join("plugins").join(&plugin.manifest.id),
                component,
                policy: crate::plugins::policy_for(&plugin.granted),
            },
        );
    }
    app.state::<crate::plugins::Plugins>().set(served);
}

/// After a move (§60): the new phone forgets its temporary identity on the router and starts
/// again with the one it received; the old phone erases itself and starts again empty. The UI
/// gets a moment to say so first.
fn after_move(app: &AppHandle, dir: &Path, router: &Arc<ft_core::RouterClient>, update: MoveUpdate) {
    let (app, dir, router) = (app.clone(), dir.to_owned(), router.clone());
    match update {
        MoveUpdate::Received => {
            tauri::async_runtime::spawn(async move {
                let _ = router.forget().await;
                tokio::time::sleep(RESTART_PAUSE).await;
                restart(&app);
            });
        }
        MoveUpdate::Sent => {
            let _ = erase(&dir);
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(RESTART_PAUSE).await;
                restart(&app);
            });
        }
        MoveUpdate::Progress { .. } | MoveUpdate::Failed => {}
    }
}

/// How long the UI shows the end of a move before the app starts again.
const RESTART_PAUSE: std::time::Duration = std::time::Duration::from_millis(2500);

fn restart(app: &AppHandle) {
    if app.platform().restart_app().is_err() {
        app.restart();
    }
}

/// Starts the core as soon as the app is up, so it connects before the first screen asks.
pub fn start_in_background(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = app.state::<Client>().core().await;
    });
}

/// The file with its path on this device (kept relative to the files folder by the core).
fn located(core: &Core, file: FileRecord) -> FileRecord {
    FileRecord { path: core.file_path(&file).to_string_lossy().into_owned(), ..file }
}

fn failed(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[tauri::command]
pub async fn core_me(client: State<'_, Client>) -> Result<MeView, String> {
    let core = client.core().await?;
    Ok(MeView {
        id: core.device_id().to_string(),
        name: core.name().await.map_err(failed)?.unwrap_or_default(),
        mailbox: core.mailbox().await.map_err(failed)?,
        receipts: core.receipts_default().await.map_err(failed)?,
        free_until: core.free_until().await.map_err(failed)?,
    })
}

#[tauri::command]
pub async fn core_set_name(name: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.set_name(&name).await.map_err(failed)
}

/// Stored now; contacts are told in the background.
#[tauri::command]
pub async fn core_set_mailbox(enabled: bool, client: State<'_, Client>) -> Result<(), String> {
    let core = client.core().await?;
    tauri::async_runtime::spawn(async move {
        let _ = core.set_mailbox(enabled).await;
    });
    Ok(())
}

/// This device's Contact Card as a link, for the QR code and for sharing.
#[tauri::command]
/// Our card as a link; from a hidden session, the session's own card (app#9).
pub async fn core_card(session: Option<String>, client: State<'_, Client>) -> Result<String, String> {
    Ok(client.core().await?.my_card_in(session.as_deref()).await.map_err(failed)?.to_link())
}

/// Adds the owner of a scanned or pasted card, to the main list or, given `session`, to that open
/// hidden session; returns their id at once and introduces us in the background.
#[tauri::command]
pub async fn core_add_contact(link: String, session: Option<String>, client: State<'_, Client>) -> Result<String, String> {
    let core = client.core().await?;
    let card = ft_contacts::ContactCard::from_link(&link).map_err(failed)?;
    if &card.device_id() == core.device_id() {
        return Err("that is your own contact card".to_owned());
    }
    let id = card.device_id().to_string();
    tauri::async_runtime::spawn(async move {
        let _ = core.add_contact_in(&link, None, session.as_deref()).await;
    });
    Ok(id)
}

#[tauri::command]
pub async fn core_conversations(client: State<'_, Client>) -> Result<Vec<ConversationView>, String> {
    let online = client.online().await?;
    let conversations = online.core.store().conversations().await.map_err(failed)?;
    conversation_views(online, conversations).await
}

async fn conversation_views(online: &Online, conversations: Vec<Conversation>) -> Result<Vec<ConversationView>, String> {
    let connected = online.network.connected().await;
    let store = online.core.store();
    let mut views = Vec::new();
    for conversation in conversations {
        let mut view = ConversationView::new(&conversation, connected.contains(&conversation.contact.device_id));
        if let Some(last) = view.last.as_mut() {
            last.file = store.file(&last.id).await.map_err(failed)?.map(|file| FileView::from(&located(&online.core, file)));
        }
        views.push(view);
    }
    Ok(views)
}

async fn session_view(online: &Online, session: String) -> Result<SessionView, String> {
    let conversations = online.core.store().session_conversations(&session).await.map_err(failed)?;
    Ok(SessionView { id: session, conversations: conversation_views(online, conversations).await? })
}

/// Six digits open the hidden session that has them, or a new one; the answer never says which.
#[tauri::command]
pub async fn core_session_open(pin: String, app: AppHandle, client: State<'_, Client>) -> Result<SessionView, String> {
    let online = client.online().await?;
    let session = online.core.open_session(&pin).await.map_err(failed)?;
    // Its wake-ups are heard from now on (app#9).
    let _ = app.platform().set_open_slots(&online.core.open_slots());
    session_view(online, session).await
}

#[tauri::command]
pub async fn core_session_close(session: String, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let core = client.core().await?;
    core.close_session(&session);
    let _ = app.platform().set_open_slots(&core.open_slots());
    Ok(())
}

/// The sessions open right now, with their conversations: what the chats list refreshes.
#[tauri::command]
pub async fn core_sessions(client: State<'_, Client>) -> Result<Vec<SessionView>, String> {
    let online = client.online().await?;
    let mut views = Vec::new();
    for session in online.core.open_sessions() {
        views.push(session_view(online, session).await?);
    }
    Ok(views)
}

#[tauri::command]
pub async fn core_messages(contact: String, limit: i64, client: State<'_, Client>) -> Result<Vec<MessageView>, String> {
    let core = client.core().await?;
    let files: HashMap<String, FileRecord> = core
        .store()
        .files(&contact)
        .await
        .map_err(failed)?
        .into_iter()
        .map(|file| (file.message_id.clone(), located(&core, file)))
        .collect();
    let messages = core.store().messages(&contact, limit).await.map_err(failed)?;
    Ok(messages.iter().map(|message| MessageView::new(message, files.get(&message.message_id))).collect())
}

/// Starts copying a file from the WebView into the app; returns the upload's id.
#[tauri::command]
pub async fn core_upload_start(client: State<'_, Client>) -> Result<String, String> {
    let id = new_upload_id();
    let path = upload_path(client.dir()?, &id)?;
    let parent = path.parent().expect("uploads live in a directory");
    tokio::fs::create_dir_all(parent).await.map_err(failed)?;
    tokio::fs::File::create(&path).await.map_err(failed)?;
    Ok(id)
}

/// Appends a slice, in base64 (Android's IPC only carries JSON), to the upload.
#[tauri::command]
pub async fn core_upload_append(upload: String, data: String, client: State<'_, Client>) -> Result<(), String> {
    let bytes = BASE64.decode(data).map_err(failed)?;
    let path = upload_path(client.dir()?, &upload)?;
    let mut file = tokio::fs::OpenOptions::new().append(true).open(&path).await.map_err(failed)?;
    tokio::io::AsyncWriteExt::write_all(&mut file, &bytes).await.map_err(failed)
}

async fn file_of(client: &Client, message: &str) -> Result<FileRecord, String> {
    let core = client.core().await?;
    let stored = core.store().message(message).await.map_err(failed)?.ok_or("unknown message")?;
    let file = core.store().file(message).await.map_err(failed)?.map(|file| located(&core, file));
    openable(file, stored.outgoing)
}

/// Shows the file in the viewer the user picks (Android's FileProvider, §62).
#[tauri::command]
pub async fn core_open_file(message: String, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let file = file_of(&client, &message).await?;
    app.platform().open_file(&file.path, &file.mime).map_err(failed)
}

/// Where this phone stands with the plan (§40–§47). Nothing of this reaches the server.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanView {
    state: String,
    /// When the free year ends, or when the subscription runs out (ms); 0 when neither applies.
    until: i64,
    age: String,
}

#[tauri::command]
pub async fn core_plan(client: State<'_, Client>) -> Result<PlanView, String> {
    let core = client.core().await?;
    let plan = core.plan().await.map_err(failed)?;
    let (state, until) = match ft_billing::Access::of(now_ms(), plan) {
        ft_billing::Access::Trial { until } => ("trial", until),
        ft_billing::Access::Young => ("young", 0),
        ft_billing::Access::Subscribed { until } => ("subscribed", until),
        ft_billing::Access::Limited => ("limited", 0),
    };
    Ok(PlanView { state: state.to_owned(), until, age: plan.age.as_str().to_owned() })
}

/// What the user said about their age. Under 21 is always free (§40); it never leaves the phone.
#[tauri::command]
pub async fn core_set_age(age: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.set_age_class(ft_billing::AgeClass::of(&age)).await.map_err(failed)
}

/// Asks the Store for the subscription and keeps what it answers (§45, §47). The app never sees
/// a card, an address or a name: that is the Store's business.
#[tauri::command]
pub async fn core_subscribe(app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let until = tauri::async_runtime::spawn_blocking(move || app.platform().subscribe())
        .await
        .map_err(failed)?
        .map_err(failed)?;
    client.core().await?.set_entitlement(until).await.map_err(failed)
}

/// The clock of this phone, in ms.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis() as i64)
        .unwrap_or(0)
}

/// Erases one message from this phone (§61). The other side keeps their copy; nothing is sent.
#[tauri::command]
pub async fn core_forget_message(message: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.forget_message(&message).await.map_err(failed)
}

/// Sends a message on to another contact: the same text, or the same file (§61).
#[tauri::command]
pub async fn core_forward(message: String, contact: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.forward(&message, &contact).await.map_err(failed).map(|_| ())
}

/// Hands a message to another app through the phone's share sheet (§62): the text, or the file.
#[tauri::command]
pub async fn core_share_message(message: String, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let core = client.core().await?;
    let stored = core.store().message(&message).await.map_err(failed)?.ok_or("unknown message")?;
    match core.store().file(&message).await.map_err(failed)? {
        Some(file) if file.complete => {
            let path = core.file_path(&file);
            app.platform().share_file(&path.to_string_lossy(), &file.name, &file.mime).map_err(failed)
        }
        Some(_) => Err("that file is not here whole yet".to_owned()),
        None => app.platform().share_text(&stored.body).map_err(failed),
    }
}

/// Erases this phone (§78): the router forgets the device and its mail, everything FlickerTalk
/// keeps here is deleted, and the app starts again at the welcome. The router is best effort: a
/// phone with no network still erases itself.
#[tauri::command]
pub async fn core_erase(app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    if let Ok(online) = client.online().await {
        let _ = online.router.forget().await;
    }
    erase(client.dir()?).map_err(failed)?;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(RESTART_PAUSE).await;
        restart(&app);
    });
    Ok(())
}

/// A plugin as the screen shows it (issue app#3).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginView {
    id: String,
    name: String,
    version: String,
    /// What it asks for, and what it was granted: the screen shows both (§53).
    asks: PermissionsView,
    granted: PermissionsView,
    installed_at: i64,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PermissionsView {
    network: Vec<String>,
    messages: bool,
    send: String,
    #[serde(default)]
    print: bool,
}

impl From<&ft_plugins::Permissions> for PermissionsView {
    fn from(permissions: &ft_plugins::Permissions) -> Self {
        Self {
            network: permissions.network.clone(),
            messages: permissions.reads_given_messages,
            send: match permissions.send {
                ft_plugins::Sending::Nothing => "nothing",
                ft_plugins::Sending::Propose => "propose",
                ft_plugins::Sending::Auto => "auto",
            }
            .to_owned(),
            print: permissions.print,
        }
    }
}

/// The plugins installed on this phone, with what each one asks for and what it may do.
#[tauri::command]
pub async fn core_plugins(client: State<'_, Client>) -> Result<Vec<PluginView>, String> {
    let core = client.core().await?;
    Ok(core
        .plugins()
        .await
        .map_err(failed)?
        .into_iter()
        .map(|plugin| PluginView {
            asks: PermissionsView::from(&plugin.manifest.permissions),
            granted: PermissionsView::from(&plugin.granted),
            id: plugin.manifest.id,
            name: plugin.manifest.name,
            version: plugin.manifest.version,
            installed_at: plugin.installed_at,
        })
        .collect())
}

/// What the user allows a plugin to do, always within what it asked for (§53).
#[tauri::command]
pub async fn core_plugin_grant(
    plugin: String,
    granted: PermissionsView,
    app: AppHandle,
    client: State<'_, Client>,
) -> Result<(), String> {
    let permissions = ft_plugins::Permissions {
        network: granted.network,
        reads_given_messages: granted.messages,
        send: match granted.send.as_str() {
            "propose" => ft_plugins::Sending::Propose,
            "auto" => ft_plugins::Sending::Auto,
            _ => ft_plugins::Sending::Nothing,
        },
        print: granted.print,
    };
    let core = client.core().await?;
    core.grant_plugin(&plugin, permissions).await.map_err(failed)?;
    refresh_served_plugins(&app, &core, client.dir()?).await;
    Ok(())
}

/// Takes a plugin off this phone.
#[tauri::command]
pub async fn core_plugin_remove(plugin: String, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let core = client.core().await?;
    core.remove_plugin(&plugin).await.map_err(failed)?;
    refresh_served_plugins(&app, &core, client.dir()?).await;
    Ok(())
}

/// The tools the app carries: a **seed**, not a store (§52). They weigh almost nothing, so a
/// phone with no network —and an iPhone, where nothing is downloaded in v1— still has them. What
/// is heavy never travels here: it is a download, and only for whoever wants it.
const BUNDLED_PLUGINS: &[&[u8]] = &[
    include_bytes!("../resources/plugins/markdown.ftplugin"),
    include_bytes!("../resources/plugins/images.ftplugin"),
    include_bytes!("../resources/plugins/pdf.ftplugin"),
    include_bytes!("../resources/plugins/redact.ftplugin"),
    include_bytes!("../resources/plugins/sketch.ftplugin"),
];

/// What a seed may weigh, and what all of them may weigh together. Past this, a tool is a
/// download: the app does not grow because the catalogue does. The test is what holds the line,
/// so a heavy tool never reaches a release.
#[cfg(test)]
const SEED_LIMIT: u64 = 32 * 1024;
#[cfg(test)]
const SEEDS_LIMIT: u64 = 128 * 1024;

/// A tool the user may add, from the app itself or from the catalogue (§56).
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OfferedPlugin {
    id: String,
    name: String,
    version: String,
    summary: String,
    size: u64,
    installed: bool,
    /// Whether it is already inside the app; if not, adding it downloads it.
    carried: bool,
}

/// Whether `version` is newer than `than`, both as `1.2.3`.
fn newer(version: &str, than: &str) -> bool {
    let parts = |version: &str| version.split('.').map(|part| part.parse().unwrap_or(0)).collect::<Vec<u32>>();
    parts(version) > parts(than)
}

/// The tools the app carries, read from the packages themselves.
fn seeds() -> Vec<OfferedPlugin> {
    BUNDLED_PLUGINS
        .iter()
        .filter_map(|package| {
            let plugin = ft_plugins::open(package, &ft_plugins::catalogue()).ok()?;
            Some(OfferedPlugin {
                id: plugin.manifest.id,
                name: plugin.manifest.name,
                version: plugin.manifest.version,
                summary: plugin.manifest.summary,
                size: package.len() as u64,
                installed: false,
                carried: true,
            })
        })
        .collect()
}

/// One list for the user: what the app carries and what the catalogue adds, the newer of the two
/// when both have it, and marked with what is already installed here.
fn merged(carried: Vec<OfferedPlugin>, listed: &[ft_plugins::CatalogueEntry], here: &[String]) -> Vec<OfferedPlugin> {
    let mut offered = carried;
    for entry in listed {
        let listed = OfferedPlugin {
            id: entry.id.clone(),
            name: entry.name.clone(),
            version: entry.version.clone(),
            summary: entry.summary.clone(),
            size: entry.size,
            installed: false,
            carried: false,
        };
        match offered.iter_mut().find(|one| one.id == entry.id) {
            Some(seed) if newer(&entry.version, &seed.version) => *seed = listed,
            Some(_) => {}
            None => offered.push(listed),
        }
    }
    for one in &mut offered {
        one.installed = here.contains(&one.id);
    }
    offered.sort_by(|a, b| a.name.cmp(&b.name));
    offered
}

/// The package of a tool the app carries, if it is one of them.
fn seed_package(id: &str) -> Option<&'static [u8]> {
    BUNDLED_PLUGINS
        .iter()
        .find(|package| {
            ft_plugins::open(package, &ft_plugins::catalogue()).is_ok_and(|plugin| plugin.manifest.id == id)
        })
        .copied()
}

/// Whether this phone downloads plugins at all. On iOS the first version of the app carries its
/// tools and asks the catalogue for nothing (App Store 4.7, §52); everywhere else it downloads.
fn downloads() -> bool {
    !cfg!(target_os = "ios")
}

/// The tools the user may add: what the app carries, plus the catalogue where it is read (§56).
/// A catalogue that cannot be reached is not an error: what the app carries is still offered.
#[tauri::command]
pub async fn core_catalogue(client: State<'_, Client>) -> Result<Vec<OfferedPlugin>, String> {
    let core = client.core().await?;
    let here: Vec<String> =
        core.plugins().await.map_err(failed)?.into_iter().map(|plugin| plugin.manifest.id).collect();
    let listed = match downloads() {
        true => core.catalogue(client.web(), &ft_plugins::catalogue()).await.unwrap_or_default(),
        false => Vec::new(),
    };
    Ok(merged(seeds(), &listed, &here))
}

/// Adds a tool: the one the app carries, or the one the catalogue lists when it is newer.
/// Installing grants nothing (§53).
#[tauri::command]
pub async fn core_plugin_add(plugin: String, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let core = client.core().await?;
    let seed = seed_package(&plugin);
    let listed = match downloads() {
        true => core.catalogue(client.web(), &ft_plugins::catalogue()).await.unwrap_or_default(),
        false => Vec::new(),
    };
    let wanted = listed.iter().find(|entry| entry.id == plugin);
    let carried_version = seed.and_then(|package| ft_plugins::open(package, &ft_plugins::catalogue()).ok());

    match (wanted, seed) {
        // The catalogue has it, and the app either does not carry it or carries an older one.
        (Some(entry), _)
            if carried_version.as_ref().is_none_or(|carried| newer(&entry.version, &carried.manifest.version)) =>
        {
            core.add_plugin(entry, client.web(), &ft_plugins::catalogue()).await.map_err(failed)?;
        }
        (_, Some(package)) => {
            core.install_plugin(package, &ft_plugins::catalogue(), ft_plugins::Permissions::default())
                .await
                .map_err(failed)?;
        }
        _ => return Err("that tool is not offered here".to_owned()),
    }
    refresh_served_plugins(&app, &core, client.dir()?).await;
    Ok(())
}

/// Keeps what the user installed in step with what the app now carries: a new version of the app
/// brings a fixed tool even where nothing is downloaded. It never installs one by itself (§53).
async fn update_installed_plugins(core: &Arc<ft_core::Core>) {
    let installed = core.plugins().await.unwrap_or_default();
    for package in BUNDLED_PLUGINS {
        let Ok(plugin) = ft_plugins::open(package, &ft_plugins::catalogue()) else { continue };
        let Some(known) = installed.iter().find(|one| one.manifest.id == plugin.manifest.id) else { continue };
        if !newer(&plugin.manifest.version, &known.manifest.version) {
            continue;
        }
        let _ = core.install_plugin(package, &ft_plugins::catalogue(), known.granted.clone()).await;
    }
}

/// What a plugin remembers between two openings; its frame has no storage of its own (§53).
#[tauri::command]
pub async fn core_plugin_read(plugin: String, key: String, client: State<'_, Client>) -> Result<Option<String>, String> {
    client.core().await?.plugin_remembers(&plugin, &key).await.map_err(failed)
}

#[tauri::command]
pub async fn core_plugin_write(
    plugin: String,
    key: String,
    value: String,
    client: State<'_, Client>,
) -> Result<(), String> {
    client.core().await?.plugin_remember(&plugin, &key, &value).await.map_err(failed)
}

#[tauri::command]
pub async fn core_plugin_forget(plugin: String, key: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.plugin_forget(&plugin, &key).await.map_err(failed)
}

/// What came back from a call the core made for a plugin.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchView {
    status: u16,
    /// The body as base64: the core never looks inside it.
    body: String,
}

/// A call a plugin asked for, made by the core and only to a host the user granted it (§55).
#[tauri::command]
pub async fn core_plugin_fetch(
    plugin: String,
    url: String,
    method: String,
    headers: Vec<(String, String)>,
    body: Option<String>,
    client: State<'_, Client>,
) -> Result<FetchView, String> {
    let body = match body {
        Some(body) => Some(BASE64.decode(body.as_bytes()).map_err(|_| "that body is not base64".to_owned())?),
        None => None,
    };
    let request = ft_core::WebRequest { url, method, headers, body };
    let answer = client.core().await?.plugin_fetch(&plugin, request, client.web()).await.map_err(failed)?;
    Ok(FetchView { status: answer.status, body: BASE64.encode(answer.body) })
}

/// Writes what a plugin made where the phone keeps it for a moment, ready to be saved or printed.
fn made_file(client: &State<'_, Client>, name: &str, data: &str, folder: &str) -> Result<(PathBuf, String), String> {
    let bytes = BASE64.decode(data.as_bytes()).map_err(|_| "that is not a file".to_owned())?;
    if bytes.len() as u64 > PLUGIN_FILE_LIMIT {
        return Err("that file is too big".to_owned());
    }
    let safe = name.replace(['/', '\\'], "_");
    let dir = client.dir()?.join("files").join(folder);
    std::fs::create_dir_all(&dir).map_err(failed)?;
    let path = dir.join(&safe);
    std::fs::write(&path, bytes).map_err(failed)?;
    Ok((path, safe))
}

/// Saves what a plugin made to the phone (§62). The user picks where, in the system's own sheet.
#[tauri::command]
pub async fn core_plugin_save(
    name: String,
    mime: String,
    data: String,
    app: AppHandle,
    client: State<'_, Client>,
) -> Result<(), String> {
    let (path, safe) = made_file(&client, &name, &data, "outgoing")?;
    let saved = app.platform().save_to_downloads(&path.to_string_lossy(), &safe, &mime).map_err(failed);
    let _ = std::fs::remove_file(&path);
    saved
}

/// Prints what a plugin made, if the user granted it printing (§53). The printer is the phone's.
#[tauri::command]
pub async fn core_plugin_print(
    plugin: String,
    name: String,
    mime: String,
    data: String,
    app: AppHandle,
    client: State<'_, Client>,
) -> Result<(), String> {
    let core = client.core().await?;
    let granted = core.plugins().await.map_err(failed)?;
    let may = granted.iter().find(|one| one.manifest.id == plugin).is_some_and(|one| one.granted.print);
    if !may {
        return Err("that plugin was not allowed to print".to_owned());
    }
    // The print service reads the file later, on its own time: it cannot be deleted right away.
    // What was printed before is cleaned up instead.
    let (path, safe) = made_file(&client, &name, &data, "printing")?;
    forget_old_prints(path.parent().unwrap_or(&path), &path);
    app.platform().print_file(&path.to_string_lossy(), &safe, &mime).map_err(failed)
}

/// Deletes what was left for the printer before, an hour old or more; never the one going now.
fn forget_old_prints(dir: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == keep {
            continue;
        }
        let old = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .map(|when| when.elapsed().unwrap_or_default() > std::time::Duration::from_secs(3600))
            .unwrap_or(false);
        if old {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// What the user pressed on the incoming call notification: "answer", "decline" or nothing (§66).
#[tauri::command]
pub async fn core_pending_call(app: AppHandle) -> Result<String, String> {
    Ok(app.platform().pending_call().unwrap_or_default())
}

/// Opens the phone's share sheet with a text, such as the Contact Card link (§32). Fails where
/// there is none (desktop): the UI copies the link instead.
#[tauri::command]
pub async fn core_share(text: String, app: AppHandle) -> Result<(), String> {
    app.platform().share_text(&text).map_err(failed)
}

/// Copies the file to the phone's Downloads.
#[tauri::command]
pub async fn core_save_file(message: String, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let file = file_of(&client, &message).await?;
    app.platform().save_to_downloads(&file.path, &file.name, &file.mime).map_err(failed)
}

/// Lets the router wake this phone when the app is closed (M4): asks to show notifications and
/// hands the FCM token over. Where there is no push (iOS for now, desktop) it says so.
#[tauri::command]
pub async fn core_enable_push(app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let router = client.online().await?.router.clone();
    let platform = app.clone();
    let token = tauri::async_runtime::spawn_blocking(move || {
        let _ = platform.platform().request_notifications();
        platform.platform().push_token()
    })
    .await
    .map_err(failed)?
    .map_err(failed)?;
    router.set_push("fcm", &token).await.map_err(failed)
}

/// New phone: the invite to show as a QR code (§60).
#[tauri::command]
pub async fn core_move_invite(client: State<'_, Client>) -> Result<String, String> {
    client.core().await?.invite_move().await.map_err(failed)
}

/// Old phone: hands everything to the phone whose invite was scanned; the transfer goes on in the
/// background and is told on `ft://move`.
#[tauri::command]
pub async fn core_move_to(link: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.move_to(&link).await.map_err(failed)
}

/// STUN and TURN for the WebView's calls.
#[tauri::command]
pub async fn core_call_ice(client: State<'_, Client>) -> Result<Vec<IceServer>, String> {
    let (stun, turn) = client.online().await?.network.ice();
    Ok(ice_servers(stun, turn))
}

/// Places a call with our offer; returns its id at once, the offer goes in the background.
#[tauri::command]
pub async fn core_call_start(contact: String, video: bool, sdp: String, client: State<'_, Client>) -> Result<String, String> {
    let core = client.core().await?;
    let call = core.place_call(&contact, video).await.map_err(failed)?;
    let offered = call.clone();
    tauri::async_runtime::spawn(async move {
        let _ = core.offer_call(&offered, &sdp).await;
    });
    Ok(call)
}

#[tauri::command]
pub async fn core_call_answer(call: String, sdp: String, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let _ = app.platform().stop_ringing();
    let core = client.core().await?;
    tauri::async_runtime::spawn(async move {
        let _ = core.answer_call(&call, &sdp).await;
    });
    Ok(())
}

/// Hangs up, declines or gives up; `failed` when the media could not connect.
#[tauri::command]
pub async fn core_call_end(call: String, failed: bool, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let _ = app.platform().stop_ringing();
    let core = client.core().await?;
    tauri::async_runtime::spawn(async move {
        let _ = core.end_call(&call, failed).await;
    });
    Ok(())
}

/// The call history, newest first.
#[tauri::command]
pub async fn core_calls(client: State<'_, Client>) -> Result<Vec<CallView>, String> {
    let core = client.core().await?;
    let mut contacts = core.store().contacts().await.map_err(failed)?;
    for session in core.open_sessions() {
        contacts.extend(core.store().session_contacts(&session).await.map_err(failed)?);
    }
    let names: HashMap<String, String> = contacts.into_iter().map(|contact| (contact.device_id, contact.name)).collect();
    // A closed hidden session's calls stay out of sight (Plan §108).
    let calls = core.visible_calls(100).await.map_err(failed)?;
    Ok(calls.iter().map(|call| CallView::new(call, names.get(&call.contact).map_or("", String::as_str))).collect())
}

/// Offers the uploaded file to the contact; the file stays in the app until it is delivered.
#[tauri::command]
pub async fn core_send_file(contact: String, upload: String, name: String, mime: String, client: State<'_, Client>) -> Result<(), String> {
    let path = upload_path(client.dir()?, &upload)?;
    if !path.exists() {
        return Err("unknown upload".to_owned());
    }
    let core = client.core().await?;
    tauri::async_runtime::spawn(async move {
        let _ = core.send_file(&contact, &path, &name, &mime).await;
    });
    Ok(())
}

/// What a plugin may be handed, and what it may hand back: a file, never more than this. A plugin
/// that could ask for anything of any size would be a way to drain the phone (issue app#3, §53).
pub const PLUGIN_FILE_LIMIT: u64 = 32 * 1024 * 1024;

/// Reads a file the user picked, for a plugin that asked for one. Only a file the user chose in
/// the system picker, and only up to the limit.
#[tauri::command]
pub async fn core_read_picked(path: String) -> Result<String, String> {
    let path = PathBuf::from(path);
    let size = std::fs::metadata(&path).map_err(failed)?.len();
    if size > PLUGIN_FILE_LIMIT {
        return Err("that file is too big to hand over".to_owned());
    }
    let bytes = std::fs::read(&path).map_err(failed)?;
    Ok(BASE64.encode(bytes))
}

/// Sends what a plugin made: the bytes are written to the app's folder and sent as a file (§62).
#[tauri::command]
pub async fn core_send_made(
    contact: String,
    name: String,
    mime: String,
    data: String,
    client: State<'_, Client>,
) -> Result<(), String> {
    let bytes = BASE64.decode(data.as_bytes()).map_err(|_| "that is not a file".to_owned())?;
    if bytes.len() as u64 > PLUGIN_FILE_LIMIT {
        return Err("that file is too big to send".to_owned());
    }
    let safe = name.replace(['/', '\\'], "_");
    let dir = client.dir()?.join("files").join("outgoing");
    std::fs::create_dir_all(&dir).map_err(failed)?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|since| since.as_millis()).unwrap_or(0);
    let path = dir.join(format!("{stamp}-{safe}"));
    std::fs::write(&path, bytes).map_err(failed)?;

    let core = client.core().await?;
    tauri::async_runtime::spawn(async move {
        let _ = core.send_file(&contact, &path, &safe, &mime).await;
        let _ = std::fs::remove_file(&path);
    });
    Ok(())
}

/// The system file picker. What it gives back is already in the app's folder, so sending it is
/// only a matter of naming it (§62). The WebView's own file input leaves the user outside the app.
#[tauri::command]
pub async fn core_pick_files(accept: Option<String>, app: AppHandle) -> Result<Vec<PickedView>, String> {
    let accept = accept.unwrap_or_default();
    let picked = tauri::async_runtime::spawn_blocking(move || app.platform().pick_files(&accept))
        .await
        .map_err(failed)?
        .map_err(failed)?;
    Ok(picked
        .into_iter()
        .map(|file| PickedView { path: file.path, name: file.name, mime: file.mime, size: file.size })
        .collect())
}

/// A file the user picked, waiting in the app's folder.
#[derive(Serialize, Deserialize, Clone)]
pub struct PickedView {
    pub path: String,
    pub name: String,
    pub mime: String,
    pub size: u64,
}

/// Sends a file the user picked; the bytes never go through the WebView.
#[tauri::command]
pub async fn core_send_picked(contact: String, file: PickedView, client: State<'_, Client>) -> Result<(), String> {
    let path = PathBuf::from(&file.path);
    if !path.exists() {
        return Err("that file is no longer there".to_owned());
    }
    let core = client.core().await?;
    tauri::async_runtime::spawn(async move {
        let _ = core.send_file(&contact, &path, &file.name, &file.mime).await;
        let _ = std::fs::remove_file(&path);
    });
    Ok(())
}

/// Stored at once (the UI hears about it); delivered in the background.
#[tauri::command]
pub async fn core_send(contact: String, text: String, client: State<'_, Client>) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("nothing to send".to_owned());
    }
    let core = client.core().await?;
    tauri::async_runtime::spawn(async move {
        let _ = core.send_text(&contact, &text).await;
    });
    Ok(())
}

#[tauri::command]
pub async fn core_mark_read(contact: String, client: State<'_, Client>) -> Result<(), String> {
    let core = client.core().await?;
    tauri::async_runtime::spawn(async move {
        let _ = core.mark_read(&contact).await;
    });
    Ok(())
}

#[tauri::command]
pub async fn core_contact(contact: String, client: State<'_, Client>) -> Result<ContactView, String> {
    let core = client.core().await?;
    let stored = core.store().contact(&contact).await.map_err(failed)?.ok_or("unknown contact")?;
    Ok(ContactView {
        fingerprint: core.fingerprint(&contact).await.map_err(failed)?,
        id: stored.device_id,
        name: stored.name,
        mailbox: stored.mailbox,
        blocked: stored.blocked,
        keep_for: stored.keep_for,
        burn_after_read: stored.burn_after_read,
        rules: stored.rules.into(),
    })
}

#[tauri::command]
pub async fn core_set_rules(contact: String, rules: RulesView, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.set_rules(&contact, rules.into()).await.map_err(failed)
}

/// Whether contacts added from now on are told their messages arrived and were read (app#6).
#[tauri::command]
pub async fn core_set_receipts(enabled: bool, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.set_receipts_default(enabled).await.map_err(failed)
}

/// The weekly hours as JSON (app#7); `None` when off.
#[tauri::command]
pub async fn core_quiet_hours(client: State<'_, Client>) -> Result<Option<String>, String> {
    client.core().await?.quiet_hours().await.map_err(failed)
}

#[tauri::command]
pub async fn core_set_quiet_hours(hours: Option<String>, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let core = client.core().await?;
    core.set_quiet_hours(hours.as_deref()).await.map_err(failed)?;
    let week = core.quiet_week().await.map_err(failed)?;
    app.platform().set_quiet_hours(&week).map_err(failed)
}

/// How long this phone keeps the conversation with a contact, and how long a read message stays
/// (issue app#1). Both in seconds; 0 means forever and never.
#[tauri::command]
pub async fn core_set_history(contact: String, keep_for: i64, burn_after_read: i64, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.set_history(&contact, keep_for, burn_after_read).await.map_err(failed)
}

#[tauri::command]
pub async fn core_block(contact: String, blocked: bool, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.block(&contact, blocked).await.map_err(failed)
}

#[tauri::command]
pub async fn core_rename(contact: String, name: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.rename_contact(&contact, &name).await.map_err(failed)
}

#[cfg(test)]
mod tests {
    use ft_storage::{Contact, Conversation, Message, MessageState};

    use super::*;

    /// The tools the app carries are a seed, not a store: what is heavy is a download and only
    /// for whoever wants it. The app stays small, whatever the catalogue grows to (§52).
    #[test]
    fn what_travels_inside_the_app_stays_tiny() {
        let mut total = 0;
        for package in BUNDLED_PLUGINS {
            assert!(
                package.len() as u64 <= SEED_LIMIT,
                "a tool of {} bytes is a download, not a seed",
                package.len()
            );
            let plugin = ft_plugins::open(package, &ft_plugins::catalogue()).expect("a seed is not signed for us");
            assert!(!plugin.manifest.components.is_empty(), "{} shows nothing", plugin.manifest.id);
            assert!(plugin.file("dist/index.js").is_some(), "{} has no code", plugin.manifest.id);
            total += package.len() as u64;
        }
        assert!(total <= SEEDS_LIMIT, "the seeds weigh {total} bytes, which is no longer little");
    }

    fn entry(id: &str, version: &str) -> ft_plugins::CatalogueEntry {
        ft_plugins::CatalogueEntry {
            id: id.to_owned(),
            name: "Tool".to_owned(),
            version: version.to_owned(),
            min_core_version: "0.1.0".to_owned(),
            size: 10,
            hash: "ab".repeat(32),
            url: format!("{}/{id}/{version}.ftplugin", ft_core::CATALOGUE_HOME),
            summary: "Does a thing.".to_owned(),
        }
    }

    fn carried(id: &str, version: &str) -> OfferedPlugin {
        OfferedPlugin {
            id: id.to_owned(),
            name: "Tool".to_owned(),
            version: version.to_owned(),
            summary: "Does a thing.".to_owned(),
            size: 4096,
            installed: false,
            carried: true,
        }
    }

    // What the user sees is one list: what the app already carries and what the catalogue adds.
    // When both have the same tool, the newer one wins, so an app that sat in a store for months
    // still installs the version that was fixed since.
    #[test]
    fn the_list_joins_what_is_carried_and_what_is_offered_keeping_the_newer() {
        let seeds = vec![carried("com.flickertalk.sketch", "1.0.0"), carried("com.flickertalk.pdf", "2.0.0")];
        let listed = vec![entry("com.flickertalk.sketch", "1.1.0"), entry("com.flickertalk.ocr", "1.0.0")];

        let offered = merged(seeds, &listed, &[]);
        let by_id = |id: &str| offered.iter().find(|one| one.id == id).expect("listed").clone();

        assert_eq!(by_id("com.flickertalk.sketch").version, "1.1.0", "the catalogue has a newer one");
        assert!(!by_id("com.flickertalk.sketch").carried, "so it is a download");
        assert_eq!(by_id("com.flickertalk.pdf").version, "2.0.0", "nothing newer is offered");
        assert!(by_id("com.flickertalk.pdf").carried);
        assert_eq!(by_id("com.flickertalk.ocr").size, 10, "what is only in the catalogue is a download");
        assert!(!by_id("com.flickertalk.ocr").carried);
        assert_eq!(offered.len(), 3);
    }

    #[test]
    fn a_tool_already_here_shows_as_installed() {
        let here = ["com.flickertalk.sketch".to_owned()];
        let offered = merged(vec![carried("com.flickertalk.sketch", "1.0.0")], &[], &here);
        assert!(offered[0].installed);
    }

    #[test]
    fn a_version_is_newer_only_when_it_really_is() {
        assert!(newer("1.1.0", "1.0.9"));
        assert!(newer("1.0.10", "1.0.9"));
        assert!(newer("2.0.0", "1.9.9"));
        assert!(!newer("1.0.0", "1.0.0"));
        assert!(!newer("1.0.0", "1.0.1"));
    }

    fn message(state: MessageState, outgoing: bool) -> Message {
        Message { message_id: "m1".to_owned(), contact: "ft_bob".to_owned(), outgoing, body: "hi".to_owned(), sent_at: 42, state }
    }

    #[test]
    fn message_states_have_the_names_the_ui_uses() {
        let names: Vec<_> = [MessageState::Pending, MessageState::Sent, MessageState::Delivered, MessageState::Read]
            .into_iter()
            .map(state_name)
            .collect();
        assert_eq!(names, ["pending", "sent", "delivered", "read"]);
    }

    #[test]
    fn messages_become_plain_views() {
        let view = MessageView::new(&message(MessageState::Delivered, true), None);
        assert_eq!(serde_json::to_value(&view).unwrap(), serde_json::json!({
            "id": "m1", "outgoing": true, "text": "hi", "sentAt": 42, "state": "delivered"
        }));
    }

    #[test]
    fn conversations_carry_the_last_message_and_the_unread_count() {
        let contact = Contact {
            device_id: "ft_bob".to_owned(),
            name: "Bob".to_owned(),
            card: vec![1],
            mailbox: true,
            blocked: false,
            introduced: true,
            added_at: 1,
            keep_for: 0,
            burn_after_read: 0,
            session: None,
            rules: Default::default(),
        };
        let conversation = Conversation { contact, last: Some(message(MessageState::Pending, true)), unread: 2 };
        let view = serde_json::to_value(ConversationView::new(&conversation, true)).unwrap();
        assert_eq!(view["id"], "ft_bob");
        assert_eq!(view["connected"], true, "a direct connection is open");
        assert_eq!(view["name"], "Bob");
        assert_eq!(view["unread"], 2);
        assert_eq!(view["last"]["state"], "pending");
        assert!(view.get("card").is_none(), "the card, with the route capability, stays in Rust");
    }

    fn record(chunks_done: i64, complete: bool, failed: bool) -> ft_storage::FileRecord {
        ft_storage::FileRecord {
            message_id: "m1".to_owned(),
            name: "photo.jpg".to_owned(),
            size: 100,
            mime: "image/jpeg".to_owned(),
            hash: [0; 32],
            chunk: 25,
            path: "/data/files/m1/photo.jpg".to_owned(),
            chunks_done,
            complete,
            failed,
        }
    }

    // A file message shows what is known about the transfer, never more (§84).
    #[test]
    fn file_messages_carry_their_transfer() {
        let view = MessageView::new(&message(MessageState::Delivered, false), Some(&record(2, false, false)));
        assert_eq!(serde_json::to_value(&view).unwrap()["file"], serde_json::json!({
            "name": "photo.jpg", "size": 100, "mime": "image/jpeg", "progress": 0.5,
            "state": "transferring", "path": "/data/files/m1/photo.jpg"
        }));
        let done = serde_json::to_value(FileView::from(&record(4, true, false))).unwrap();
        assert_eq!((done["state"].as_str(), done["progress"].as_f64()), (Some("done"), Some(1.0)));
        assert_eq!(serde_json::to_value(FileView::from(&record(0, false, true))).unwrap()["state"], "failed");
        let empty = ft_storage::FileRecord { size: 0, ..record(0, true, false) };
        assert_eq!(serde_json::to_value(FileView::from(&empty)).unwrap()["progress"], 1.0);
    }

    #[test]
    fn a_text_has_no_file() {
        let view = serde_json::to_value(MessageView::new(&message(MessageState::Sent, true), None)).unwrap();
        assert!(view.get("file").is_none());
    }

    // Ours can always be opened; theirs only once it has arrived whole and verified.
    #[test]
    fn only_whole_files_can_be_opened() {
        assert!(openable(Some(record(4, true, false)), false).is_ok());
        assert!(openable(Some(record(2, false, false)), false).is_err());
        assert!(openable(Some(record(0, false, true)), false).is_err());
        assert!(openable(Some(record(1, false, false)), true).is_ok());
        assert!(openable(None, true).is_err());
    }

    // §66: what the WebView hears about a call, and what it needs to answer or show it.
    #[test]
    fn call_updates_reach_the_ui_as_plain_events() {
        let incoming = CallEvent::new("ft_bob", "c1", CallUpdate::Incoming { video: true, sdp: "offer".to_owned() });
        assert_eq!(serde_json::to_value(&incoming).unwrap(), serde_json::json!({
            "contact": "ft_bob", "call": "c1", "kind": "incoming", "video": true, "sdp": "offer"
        }));
        let answered = serde_json::to_value(CallEvent::new("ft_bob", "c1", CallUpdate::Answered { sdp: "answer".to_owned() })).unwrap();
        assert_eq!((answered["kind"].as_str(), answered["sdp"].as_str()), (Some("answered"), Some("answer")));
        let ended = serde_json::to_value(CallEvent::new("ft_bob", "c1", CallUpdate::Ended { outcome: CallOutcome::Busy })).unwrap();
        assert_eq!((ended["kind"].as_str(), ended["outcome"].as_str()), (Some("ended"), Some("busy")));
        assert!(ended.get("sdp").is_none());
    }

    // §41: the free year shows in Settings, counted on this phone.
    #[test]
    fn me_carries_the_free_period() {
        let me = MeView { id: "ft_me".to_owned(), name: "Ioan".to_owned(), mailbox: true, receipts: false, free_until: 42 };
        assert_eq!(serde_json::to_value(me).unwrap(), serde_json::json!({
            "id": "ft_me", "name": "Ioan", "mailbox": true, "receipts": false, "freeUntil": 42
        }));
    }

    // An incoming call rings until it is answered, declined or given up (§66); our own calls
    // being answered never ring.
    #[test]
    fn only_incoming_calls_ring() {
        assert_eq!(ringing(&CallUpdate::Incoming { video: true, sdp: String::new() }), Ring::Start { video: true });
        assert_eq!(ringing(&CallUpdate::Incoming { video: false, sdp: String::new() }), Ring::Start { video: false });
        assert_eq!(ringing(&CallUpdate::Ended { outcome: CallOutcome::Missed }), Ring::Stop);
        assert_eq!(ringing(&CallUpdate::Answered { sdp: String::new() }), Ring::Nothing);
    }

    // The WebView's WebRTC uses the cluster's STUN and a short-lived TURN user (§16–17).
    #[test]
    fn the_webview_gets_the_routers_ice_servers() {
        let turn = ft_core::TurnGrant { urls: vec!["turn:t:3478".to_owned()], username: "u".to_owned(), credential: "c".to_owned() };
        let servers = serde_json::to_value(ice_servers(vec!["stun:s:3478".to_owned()], Some(turn))).unwrap();
        assert_eq!(servers, serde_json::json!([
            { "urls": ["stun:s:3478"] },
            { "urls": ["turn:t:3478"], "username": "u", "credential": "c" }
        ]));
        assert_eq!(serde_json::to_value(ice_servers(vec![], None)).unwrap(), serde_json::json!([]));
    }

    #[test]
    fn the_call_history_shows_who_how_and_how_long() {
        let call = ft_storage::CallRecord {
            call_id: "c1".to_owned(),
            contact: "ft_bob".to_owned(),
            outgoing: false,
            video: true,
            started_at: 1_000,
            answered_at: Some(3_000),
            ended_at: Some(65_500),
            outcome: Some(CallOutcome::Answered),
        };
        assert_eq!(serde_json::to_value(CallView::new(&call, "Bob")).unwrap(), serde_json::json!({
            "id": "c1", "contact": "ft_bob", "name": "Bob", "outgoing": false, "video": true,
            "startedAt": 1_000, "seconds": 62, "outcome": "answered"
        }));
        let missed = ft_storage::CallRecord { answered_at: None, outcome: Some(CallOutcome::Missed), ..call };
        assert_eq!(serde_json::to_value(CallView::new(&missed, "Bob")).unwrap()["seconds"], 0);
    }

    // Uploads are named by the app, never by the WebView: no way out of their directory.
    #[test]
    fn upload_ids_cannot_point_elsewhere() {
        let dir = Path::new("/data");
        let id = new_upload_id();
        // Under `files/`, the one folder Android's FileProvider shares when a file is opened.
        assert_eq!(upload_path(dir, &id).unwrap(), dir.join("files").join("outgoing").join(&id));
        assert_ne!(new_upload_id(), id);
        for bad in ["../identity", "", "abc", "/etc/passwd", "0123456789abcdef0123456789abcdeg"] {
            assert!(upload_path(dir, bad).is_err(), "{bad}");
        }
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ft-client-{name}-{}", rand::random::<u64>()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // §60: the new phone swaps in the database and key it received, once, at start.
    #[test]
    fn a_received_move_replaces_the_database_and_key() {
        let dir = scratch("apply");
        std::fs::write(dir.join("flickertalk.db"), b"temporary").unwrap();
        std::fs::write(dir.join("flickertalk.db-wal"), b"wal").unwrap();
        std::fs::write(dir.join("storage.key"), [1; 32]).unwrap();
        assert!(!apply_move(&dir).unwrap(), "nothing to apply");

        std::fs::create_dir_all(dir.join("move")).unwrap();
        std::fs::write(dir.join("move").join(ft_core::moving::MOVE_DB), b"moved").unwrap();
        std::fs::write(dir.join("move").join(ft_core::moving::MOVE_KEY), [2; 32]).unwrap();
        assert!(apply_move(&dir).unwrap());
        assert_eq!(std::fs::read(dir.join("flickertalk.db")).unwrap(), b"moved");
        assert_eq!(std::fs::read(dir.join("storage.key")).unwrap(), [2; 32]);
        assert!(!dir.join("flickertalk.db-wal").exists(), "the old write-ahead log goes");
        assert!(!dir.join("move").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // §60: once the new phone has everything, the old one keeps nothing of the identity.
    #[test]
    fn a_moved_phone_is_erased() {
        let dir = scratch("wipe");
        for name in ["flickertalk.db", "flickertalk.db-shm", "storage.key", "storage.key.sealed"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        std::fs::create_dir_all(dir.join("files").join("m1")).unwrap();
        std::fs::create_dir_all(dir.join("move")).unwrap();
        erase(&dir).unwrap();
        for name in ["flickertalk.db", "flickertalk.db-shm", "storage.key", "storage.key.sealed", "files", "move"] {
            assert!(!dir.join(name).exists(), "{name} is gone");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn move_updates_reach_the_ui() {
        use ft_core::moving::MoveUpdate;
        assert_eq!(serde_json::to_value(MoveEvent::from(MoveUpdate::Progress { done: 3, total: 8 })).unwrap(),
            serde_json::json!({ "kind": "progress", "done": 3, "total": 8 }));
        for (update, kind) in [(MoveUpdate::Received, "received"), (MoveUpdate::Sent, "sent"), (MoveUpdate::Failed, "failed")] {
            assert_eq!(serde_json::to_value(MoveEvent::from(update)).unwrap()["kind"], kind);
        }
    }

    /// Stands in for Android Keystore / iOS Keychain: seals by reversing and tagging; can be told
    /// to fail, like a keystore that lost its key.
    #[derive(Default)]
    struct FakeVault {
        broken: bool,
    }

    impl KeyVault for FakeVault {
        fn seal(&self, key: &[u8; 32]) -> anyhow::Result<Vec<u8>> {
            let mut sealed = b"sealed:".to_vec();
            sealed.extend(key.iter().rev());
            Ok(sealed)
        }

        fn open(&self, sealed: &[u8]) -> anyhow::Result<[u8; 32]> {
            anyhow::ensure!(!self.broken, "the keystore lost its key");
            let mut key: [u8; 32] = sealed.strip_prefix(b"sealed:").ok_or_else(|| anyhow::anyhow!("not sealed"))?.try_into()?;
            key.reverse();
            Ok(key)
        }
    }

    // The key that seals the identity is created once and kept.
    #[test]
    fn the_storage_key_is_created_once_and_kept() {
        let dir = scratch("key");
        let first = storage_key(&dir, None).expect("creates");
        assert_eq!(storage_key(&dir, None).expect("reads"), first);
        assert_ne!(first, [0; 32]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // §94: with the OS keystore, the file only holds the key sealed by it.
    #[test]
    fn the_storage_key_is_sealed_by_the_os_keystore() {
        let dir = scratch("sealed-key");
        let vault = FakeVault::default();
        let key = storage_key(&dir, Some(&vault)).expect("creates");
        assert!(!dir.join("storage.key").exists(), "no key in the clear");
        let sealed = std::fs::read(dir.join("storage.key.sealed")).unwrap();
        assert!(sealed.starts_with(b"sealed:"));
        assert_eq!(storage_key(&dir, Some(&vault)).expect("opens"), key);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Installs from before the keystore keep their identity: the key moves into it.
    #[test]
    fn a_key_in_the_clear_moves_into_the_keystore() {
        let dir = scratch("migrate-key");
        let old = storage_key(&dir, None).expect("the old way");
        let vault = FakeVault::default();
        assert_eq!(storage_key(&dir, Some(&vault)).expect("migrates"), old);
        assert!(!dir.join("storage.key").exists());
        assert!(dir.join("storage.key.sealed").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // A keystore that cannot open the key is an error: making a new key would lose the identity.
    #[test]
    fn a_key_the_keystore_cannot_open_is_an_error_never_replaced() {
        let dir = scratch("broken-key");
        storage_key(&dir, Some(&FakeVault::default())).expect("creates");
        let sealed = std::fs::read(dir.join("storage.key.sealed")).unwrap();
        assert!(storage_key(&dir, Some(&FakeVault { broken: true })).is_err());
        assert_eq!(std::fs::read(dir.join("storage.key.sealed")).unwrap(), sealed, "left as it was");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // §60: after a move, the old key (sealed or not) gives way to the one that came with the copy.
    #[test]
    fn a_received_move_replaces_a_sealed_key_too() {
        let dir = scratch("apply-sealed");
        std::fs::write(dir.join("storage.key.sealed"), b"sealed:temporary").unwrap();
        std::fs::create_dir_all(dir.join("move")).unwrap();
        std::fs::write(dir.join("move").join(ft_core::moving::MOVE_DB), b"moved").unwrap();
        std::fs::write(dir.join("move").join(ft_core::moving::MOVE_KEY), [2; 32]).unwrap();
        assert!(apply_move(&dir).unwrap());
        assert!(!dir.join("storage.key.sealed").exists());
        assert_eq!(storage_key(&dir, Some(&FakeVault::default())).unwrap(), [2; 32]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Hidden sessions: the UI gets an id and the conversations, and nothing else to show.
    #[test]
    fn a_session_view_is_its_id_and_its_conversations() {
        let view = serde_json::to_value(SessionView { id: "s1".to_owned(), conversations: vec![] }).unwrap();
        assert_eq!(view, serde_json::json!({ "id": "s1", "conversations": [] }));
    }

    // Issues app#4–#6: the contact's page shows what this phone takes from them.
    #[test]
    fn a_contact_view_carries_its_rules() {
        let rules = ft_storage::ContactRules { muted: true, accepts_chat: false, accepts_calls: true, receipts: false };
        let view = serde_json::to_value(RulesView::from(rules)).unwrap();
        assert_eq!(view, serde_json::json!({ "muted": true, "acceptsChat": false, "acceptsCalls": true, "receipts": false }));
    }
}
