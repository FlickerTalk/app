//! The app's bridge to ft-core (Plan §82, §106 M3): starts the core online and exposes thin
//! commands to the UI. No business logic here: every command delegates to the core, and what
//! crosses to the WebView are plain views (never keys, never the capability, §54).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use anyhow::Context;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use ft_core::online::{self, Online};
use ft_core::moving::MoveUpdate;
use ft_core::{CallPhase, CallUpdate, Core, Event, TurnGrant};
use ft_media::{CallRouting, Facing, Layers, RemoteShape, VideoState};
use ft_storage::{CallOutcome, CallRecord, Conversation, FileRecord, Message, MessageState, Store};
use ft_webrtc::SessionConfig;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_ft_platform::{NativeCallEvent, PlatformExt, VideoLayout};
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
    /// `transferring`, `waiting` (bigger than the user downloads on their own, A4), `done` (the
    /// receiver has it and the hash matches) or `failed`.
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
        } else if file.waiting {
            "waiting"
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

/// A hidden session as the UI sees it: an id, its conversations and the strangers waiting in
/// its requests (A5). No name, nothing to read.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    id: String,
    conversations: Vec<ConversationView>,
    requests: Vec<ConversationView>,
    circles: Vec<CircleView>,
}

/// A circle as the UI sees it (2026-09-27): its card spelled out, and what the list needs.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CircleView {
    id: String,
    name: String,
    members: Vec<CircleMemberView>,
    /// Whether this phone may change it now.
    admin: bool,
    admins_only: bool,
    /// This phone left, or was taken out: what was said stays, read only.
    left: bool,
    unread: i64,
    last: Option<CircleMessageView>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CircleMemberView {
    id: String,
    name: String,
    admin: bool,
    /// Whether it is this phone.
    me: bool,
}

/// Something said or done in a circle. `kind` is `text`, or what happened (`created`, `joined`,
/// `left`, `removed`, `renamed`), with `text` naming who or what.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CircleMessageView {
    id: String,
    outgoing: bool,
    /// The device that said it, and the name this phone has for it.
    sender: String,
    sender_name: String,
    kind: String,
    text: String,
    sent_at: i64,
    state: &'static str,
}

impl CircleMessageView {
    pub fn new(message: &ft_storage::CircleMessage, names: &HashMap<String, String>) -> Self {
        Self {
            id: message.message_id.clone(),
            outgoing: message.outgoing,
            sender: message.sender.clone(),
            sender_name: names.get(&message.sender).cloned().unwrap_or_else(|| message.sender.chars().take(9).collect()),
            kind: message.kind.clone(),
            text: message.body.clone(),
            sent_at: message.sent_at,
            state: state_name(message.state),
        }
    }
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
    /// Files up to this many bytes are downloaded as they arrive; bigger ones wait (A4).
    auto_download: i64,
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
    /// The circle whose conversation changed (2026-09-27); `None` for a contact's, or a list.
    #[serde(skip_serializing_if = "Option::is_none")]
    circle: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    muted: Option<bool>,
    /// `video`: the call's video, a whole state (native video, 2026-09-29).
    #[serde(flatten)]
    view: Option<CallVideoView>,
    /// `incoming`: answered already on the phone's own screen, before its offer came (2026-09-29).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    answered: bool,
}

impl CallEvent {
    pub fn new(contact: &str, call: &str, update: CallUpdate) -> Self {
        let view = match update {
            CallUpdate::Video(state) => Some(CallVideoView::from(state)),
            _ => None,
        };
        let (kind, video, sdp, outcome, muted) = match update {
            CallUpdate::Incoming { video, sdp } => ("incoming", Some(video), Some(sdp), None, None),
            CallUpdate::Answered { sdp } => ("answered", None, Some(sdp), None, None),
            CallUpdate::Answering => ("answering", None, None, None, None),
            CallUpdate::Ended { outcome } => ("ended", None, None, Some(outcome.as_str()), None),
            CallUpdate::Connected => ("connected", None, None, None, None),
            CallUpdate::Muted { muted } => ("muted", None, None, None, Some(muted)),
            // The WebView only logs it: it follows the call it shows.
            CallUpdate::MissedWhileBusy => ("ended", None, None, Some(CallOutcome::Missed.as_str()), None),
            CallUpdate::Video(_) => ("video", None, None, None, None),
            CallUpdate::CameraFailed => ("camera_failed", None, None, None, None),
        };
        Self { contact: contact.to_owned(), call: call.to_owned(), kind, video, sdp, outcome, muted, view, answered: false }
    }

    /// An incoming call the core is answering already: the WebView shows it connecting.
    pub fn answered(self, answered: bool) -> Self {
        Self { answered, ..self }
    }
}

/// What the phone's own call screen (CallKit, the ongoing call notification) is told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeScreen {
    Connected,
    Ended,
    Nothing,
}

pub fn native_screen(update: &CallUpdate) -> NativeScreen {
    match update {
        CallUpdate::Connected => NativeScreen::Connected,
        CallUpdate::Ended { .. } => NativeScreen::Ended,
        CallUpdate::Incoming { .. }
        | CallUpdate::Answering
        | CallUpdate::Answered { .. }
        | CallUpdate::Muted { .. }
        | CallUpdate::MissedWhileBusy
        | CallUpdate::Video(_)
        | CallUpdate::CameraFailed => NativeScreen::Nothing,
    }
}

/// The call going on, for a WebView that comes up after it started (2026-09-28).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentCallView {
    call: String,
    contact: String,
    /// The call's video (native video, 2026-09-29). With no native video yet (it rings, or it
    /// is the WebView's), our camera is wanted in a video call: `camera` says it is one.
    video: CallVideoView,
    outgoing: bool,
    /// `calling`, `ringing`, `connecting` or `active`.
    phase: &'static str,
    /// Their offer while it rings: a WebView call is answered with it.
    #[serde(skip_serializing_if = "Option::is_none")]
    offer: Option<String>,
    native: bool,
    muted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    connected_at: Option<i64>,
}

impl From<ft_core::CurrentCall> for CurrentCallView {
    fn from(current: ft_core::CurrentCall) -> Self {
        let phase = match current.phase {
            CallPhase::Calling => "calling",
            CallPhase::Ringing => "ringing",
            CallPhase::Connecting => "connecting",
            CallPhase::Active => "active",
        };
        Self {
            call: current.call,
            contact: current.contact,
            video: CallVideoView::from(current.video_state.unwrap_or(VideoState { camera: current.video, ..VideoState::default() })),
            outgoing: current.outgoing,
            phase,
            offer: current.offer,
            native: current.native,
            muted: current.muted,
            connected_at: current.connected_at,
        }
    }
}

/// A call's video as the WebView sees it (native video, 2026-09-29): the `kind: "video"` event,
/// `core_call_set_video` and `core_current_call`. The other picture's shape stays with the native
/// views.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallVideoView {
    /// The call has a video line both ways: the camera button is enabled.
    available: bool,
    /// The user wants our camera on.
    camera: bool,
    /// The phone holds our camera (the app away, the call screen left).
    paused: bool,
    /// `front` or `back`.
    facing: &'static str,
    /// The other side's camera is on.
    remote: bool,
    /// The other side's phone holds its camera: its last picture is old.
    remote_paused: bool,
}

impl From<VideoState> for CallVideoView {
    fn from(state: VideoState) -> Self {
        let facing = match state.facing {
            Facing::Front => "front",
            Facing::Back => "back",
        };
        Self {
            available: state.available,
            camera: state.camera,
            paused: state.paused,
            facing,
            remote: state.remote,
            remote_paused: state.remote_paused,
        }
    }
}

/// What the WebView gets when the camera may not be used: it keeps the call as voice.
const CAMERA_DENIED: &str = "camera_denied";

pub fn camera_or_denied(granted: bool) -> Result<(), String> {
    if granted {
        Ok(())
    } else {
        Err(CAMERA_DENIED.to_owned())
    }
}

/// What the native bridge is told about the call's video (docs/video-nativo.md §4).
#[derive(Debug, Clone, PartialEq)]
pub enum BridgeVideo {
    /// Whether the call has video now: CallKit's `hasVideo`, the notification's `setIsVideo`.
    CallVideo(bool),
    /// The call's views, under the WebView (iOS: its layers; Android: zeros).
    Attach(Layers),
    /// Where the WebView left room for the pictures.
    Layout(VideoLayout),
    /// How to lay the other picture out.
    Shape(RemoteShape),
    /// The views go, before the call's devices do.
    Detach,
}

/// The views hidden: the WebView left the call screen.
pub const HIDDEN: VideoLayout = VideoLayout { remote: None, local: None, mirror_local: false, local_radius: 0.0 };

/// What the bridge was last told about the call's video: the bridge hears each thing once, in
/// order (the views before where they go), and nothing about a call that ended.
#[derive(Default)]
pub struct VideoGlue {
    /// The call whose video the bridge follows.
    call: Option<String>,
    /// The call whose views were taken away: its late states change nothing.
    ended: Option<String>,
    has_video: bool,
    attached: bool,
    shape: Option<RemoteShape>,
    /// The WebView's latest layout, kept for when the views come (the call screen may lay them
    /// out before the call has video, or an id).
    layout: Option<VideoLayout>,
    /// The camera ours is on, as the core last said: our preview is mirrored with the front one.
    facing: Facing,
}

impl VideoGlue {
    /// A new video state of `call`. `layers` are the views to attach: on iOS the call's layers,
    /// once its devices exist; zeros on Android.
    pub fn state(&mut self, call: &str, state: &VideoState, layers: Option<Layers>) -> Vec<BridgeVideo> {
        if self.ended.as_deref() == Some(call) {
            return Vec::new();
        }
        if self.call.as_deref() != Some(call) {
            *self = Self { call: Some(call.to_owned()), layout: self.layout, ..Self::default() };
        }
        let mut told = Vec::new();
        if state.any() != self.has_video {
            self.has_video = state.any();
            told.push(BridgeVideo::CallVideo(self.has_video));
        }
        let turned = state.facing != self.facing;
        self.facing = state.facing;
        if !self.attached && state.any() {
            if let Some(layers) = layers {
                self.attached = true;
                told.push(BridgeVideo::Attach(layers));
                told.extend(self.placed().map(BridgeVideo::Layout));
            }
        } else if self.attached && turned {
            // The camera flipped: our preview's mirroring follows it at once.
            told.extend(self.placed().map(BridgeVideo::Layout));
        }
        if self.attached && state.shape != self.shape {
            self.shape = state.shape;
            told.extend(state.shape.map(BridgeVideo::Shape));
        }
        told
    }

    /// Where the WebView left room for the pictures; `None` when it left the call screen.
    pub fn layout(&mut self, layout: Option<VideoLayout>) -> Vec<BridgeVideo> {
        self.layout = layout;
        if self.attached {
            vec![BridgeVideo::Layout(self.placed().unwrap_or(HIDDEN))]
        } else {
            Vec::new()
        }
    }

    /// The WebView's layout with our preview mirrored exactly with the front camera, whatever
    /// facing the WebView last knew.
    fn placed(&self) -> Option<VideoLayout> {
        self.layout.map(|layout| VideoLayout { mirror_local: self.facing == Facing::Front, ..layout })
    }

    /// The call's video devices are about to go: its views go first.
    pub fn end(&mut self) -> Vec<BridgeVideo> {
        let attached = self.attached;
        *self = Self { ended: self.call.take().or(self.ended.take()), layout: self.layout, ..Self::default() };
        if attached {
            vec![BridgeVideo::Detach]
        } else {
            Vec::new()
        }
    }
}

/// The call routing as Settings words it (`direct`, `auto`, `always`, §17).
pub fn call_routing(routing: &str) -> Result<CallRouting, String> {
    routing.parse().map_err(failed)
}

/// What the phone does about ringing after a call update (§66).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ring {
    /// Ring and show the call on the screen; video calls say so.
    Start { video: bool },
    /// Answered already, or being answered (2026-09-29): no ringing, on the phone or in the app;
    /// the phone's own call screen learns who it is.
    Answered,
    Stop,
    Nothing,
}

/// What the ringing does after `update`. `answered`: the core is answering the call already (it
/// was answered on the phone's own screen before its offer came).
pub fn ringing(update: &CallUpdate, answered: bool) -> Ring {
    match update {
        CallUpdate::Incoming { .. } if answered => Ring::Answered,
        CallUpdate::Answering => Ring::Answered,
        CallUpdate::Incoming { video, .. } => Ring::Start { video: *video },
        CallUpdate::Ended { .. } => Ring::Stop,
        CallUpdate::Answered { .. } => Ring::Nothing,
        CallUpdate::Connected | CallUpdate::Muted { .. } | CallUpdate::MissedWhileBusy | CallUpdate::Video(_) | CallUpdate::CameraFailed => {
            Ring::Nothing
        }
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

/// Where the phone's own picker and camera leave what the user chose (`PlatformPlugin.kt`) and
/// where the WebView's uploads go: the only folders a picked path may point into (M1). Anything
/// else the WebView names, however it came to name it, is refused.
pub fn picked_path(dir: &Path, path: &str) -> Result<PathBuf, String> {
    let candidate = Path::new(path);
    let real = candidate.canonicalize().map_err(|_| "that file is no longer there".to_owned())?;
    let allowed = [dir.join("uploads"), dir.join("files").join("uploads"), dir.join("files").join("outgoing"), dir.join("files").join("drive")];
    let inside = allowed.iter().filter_map(|root| root.canonicalize().ok()).any(|root| real.starts_with(&root));
    if inside && real.is_file() {
        Ok(real)
    } else {
        Err("that is not a file the user picked".to_owned())
    }
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
    /// What the native bridge was told about the call's video (native video, 2026-09-29).
    video: Arc<Mutex<VideoGlue>>,
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
        // The user's cloud (plan-drive): what waits to go up lives here; the drive opens in the
        // background from what the phone keeps, and tries what waited.
        online.core.set_vault_dir(dir.join("vault"));
        online.core.set_cloud(Arc::new(ft_core::vault::GoogleCloud));
        let core_for_vault = online.core.clone();
        tauri::async_runtime::spawn(async move {
            if core_for_vault.vault_reopen().await.is_ok() {
                let _ = core_for_vault.vault_run_queue().await;
            }
        });
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
            // The phone's alarm clock is told every reminder again (2026-09-27): the core is
            // the truth, and an alarm lost to a reboot or an update comes back here.
            sync_reminders(app, &online.core).await;
        }

        if let Some(app) = self.app.get().cloned() {
            // The call's video views go before its devices (on iOS the layers are theirs): the
            // core runs this, off the async workers, right before it lets them go.
            let (glue, app_for_views) = (self.video.clone(), app.clone());
            online.core.set_video_detach(Some(Arc::new(move || tell_bridge(&app_for_views, &glue, VideoGlue::end))));
            listen_native_calls(&app, &online);
            let glue_for_events = self.video.clone();
            let mut events = online.core.events();
            let dir_for_events = dir.to_owned();
            let router_for_events = online.router.clone();
            let core_for_events = online.core.clone();
            tauri::async_runtime::spawn(async move {
                while let Ok(event) = events.recv().await {
                    let (contact, circle) = match event {
                        Event::MessagesChanged { contact } => (Some(contact), None),
                        Event::CircleMessagesChanged { circle } => (None, Some(circle)),
                        Event::ContactsChanged | Event::ConnectionChanged { .. } | Event::PluginsChanged | Event::CirclesChanged => (None, None),
                        // A plugin on the other side said something to its twin here (2026-09-27).
                        Event::PluginEvent { plugin, contact, data } => {
                            let _ = app.emit(PLUGIN_EVENT, PluginEventView { plugin, contact, data: BASE64.encode(data) });
                            continue;
                        }
                        Event::RemindersChanged => {
                            sync_reminders(&app, &core_for_events).await;
                            continue;
                        }
                        Event::VaultChanged => {
                            let _ = app.emit(VAULT_EVENT, ());
                            continue;
                        }
                        Event::VaultProgress { done, total } => {
                            let _ = app.emit(VAULT_PROGRESS_EVENT, VaultProgressView { done, total });
                            continue;
                        }
                        Event::Move(update) => {
                            let _ = app.emit(MOVE_EVENT, MoveEvent::from(update.clone()));
                            after_move(&app, &dir_for_events, &router_for_events, update);
                            continue;
                        }
                        Event::Call { contact, call, update } => {
                            // Answered on the phone's own screen before its offer came: the core
                            // is answering it, so it rings nowhere (2026-09-29).
                            let current = match update {
                                CallUpdate::Incoming { .. } | CallUpdate::Answering => core_for_events.current_call().await.ok().flatten(),
                                _ => None,
                            };
                            let answered = matches!(update, CallUpdate::Incoming { .. }) && answered_already(current.as_ref(), &call);
                            match ringing(&update, answered) {
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
                                Ring::Answered => {
                                    let name = core_for_events.store().contact(&contact).await.ok().flatten().map(|stored| stored.name).unwrap_or_default();
                                    let video = current.as_ref().is_some_and(|current| current.video);
                                    let _ = app.platform().call_answering(&name, video);
                                }
                                Ring::Nothing => {}
                            }
                            // CallKit or the ongoing call notification shows the call too.
                            match native_screen(&update) {
                                NativeScreen::Connected => {
                                    let _ = app.platform().call_connected();
                                }
                                NativeScreen::Ended => {
                                    let _ = app.platform().call_ended();
                                }
                                NativeScreen::Nothing => {}
                            }
                            if let CallUpdate::Video(state) = update {
                                // CallKit's `hasVideo`, the views under the WebView, the other
                                // picture's shape (docs/video-nativo.md §4).
                                let (app, glue, call) = (app.clone(), glue_for_events.clone(), call.clone());
                                let _ = tauri::async_runtime::spawn_blocking(move || {
                                    tell_bridge(&app, &glue, |glue| glue.state(&call, &state, video_layers()))
                                })
                                .await;
                            }
                            let name = update_name(&update);
                            let _ = app.emit(CALL_EVENT, CallEvent::new(&contact, &call, update).answered(answered));
                            diagnose_call(&app, &core_for_events, &format!("update {name}")).await;
                            if CALL_DIAGNOSTICS && name == "connected" {
                                diagnose_call(&app, &core_for_events, &timings_line(core_for_events.call_timings())).await;
                                // Whether the voice runs a moment later: on iOS it waits for CallKit.
                                let (app, core) = (app.clone(), core_for_events.clone());
                                tauri::async_runtime::spawn(async move {
                                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                                    diagnose_call(&app, &core, "3 s after connecting").await;
                                    diagnose_call(&app, &core, &timings_line(core.call_timings())).await;
                                });
                            }
                            continue;
                        }
                    };
                    let _ = app.emit(CHANGED_EVENT, Changed { contact, circle });
                }
            });
        }
        Ok(online)
    }
}

/// What the phone's own call screen says (2026-09-28): CallKit on iOS, the ongoing call
/// notification on Android. It works with no WebView at all: a locked iPhone that PushKit woke
/// answers here.
fn listen_native_calls(app: &AppHandle, online: &Online) {
    let (core, router, app_for_events) = (online.core.clone(), online.router.clone(), app.clone());
    // iOS experiment: incoming calls reported as video calls, so answering opens the app.
    let _ = app.platform().set_open_app_on_answer(open_app_on_answer(option_env!("FT_IOS_OPEN_APP_ON_ANSWER")));
    // The handler runs on a native queue: it only hands the event over. They are handled in the
    // order they came (an audio deactivation and activation must not swap); what may take long
    // (answering, hanging up) goes on in the background.
    let (events, mut queue) = tokio::sync::mpsc::unbounded_channel::<NativeCallEvent>();
    app.platform().listen_calls(move |event| {
        let _ = events.send(event);
    });
    tauri::async_runtime::spawn(async move {
        while let Some(event) = queue.recv().await {
            match reconnect_for(event) {
                Reconnect::Now => router.reconnect_now(),
                Reconnect::UnlessFresh => router.reconnect_unless_fresh(CALL_SOCKET_FRESH),
                Reconnect::No => {}
            }
            let (core, app) = (core.clone(), app_for_events.clone());
            if let Some(stage) = native_event_stage(event).filter(|_| CALL_DIAGNOSTICS) {
                core.mark_call_stage(stage);
            }
            match event {
                NativeCallEvent::Incoming => diagnose_call(&app, &core, "event incoming").await,
                NativeCallEvent::Answer => {
                    tauri::async_runtime::spawn(async move {
                        diagnose_call(&app, &core, "event answer").await;
                        if !microphone(&app).await {
                            let failed = fail_current_call(&core).await;
                            diagnose_outcome(&app, &core, "answer without a microphone", &failed).await;
                            return;
                        }
                        // A video call's camera needs its permission: without it the call is voice.
                        let video_call = ringing_video_call(&core).await;
                        let camera_allowed = video_call.is_none() || camera(&app).await;
                        match core.answer_ringing_call().await {
                            Ok(answered) => {
                                if let (true, Some(call), false) = (answered, video_call, camera_allowed) {
                                    let _ = core.set_call_camera(&call, false).await;
                                }
                                diagnose_call(&app, &core, &format!("event answer: {}", answer_state(answered))).await
                            }
                            Err(error) => diagnose_outcome(&app, &core, "answer", &Err(error)).await,
                        }
                    });
                }
                NativeCallEvent::End => {
                    tauri::async_runtime::spawn(async move {
                        let ended = core.end_current_call().await;
                        diagnose_outcome(&app, &core, "end", &ended).await;
                    });
                }
                // Declined before its offer came, the call is declined as it arrives (2026-09-29).
                NativeCallEvent::Decline => {
                    tauri::async_runtime::spawn(async move {
                        let declined = core.decline_ringing_call().await;
                        diagnose_outcome(&app, &core, "decline", &declined).await;
                    });
                }
                NativeCallEvent::Mute(muted) => {
                    let _ = core.mute_current_call(muted).await;
                }
                NativeCallEvent::AudioActivated(generation) => {
                    // A device that will not start is tried again, then fails the call (core).
                    let started = core.set_call_audio_session(true, generation).await;
                    diagnose_outcome(&app, &core, native_event_name(event), &started).await;
                }
                NativeCallEvent::AudioDeactivated(generation) => {
                    let stopped = core.set_call_audio_session(false, generation).await;
                    diagnose_outcome(&app, &core, native_event_name(event), &stopped).await;
                }
                // Native video (docs/video-nativo.md): away from the screen our camera is held.
                NativeCallEvent::Visible(visible) => {
                    let _ = core.set_app_visible(visible).await;
                }
                NativeCallEvent::Orientation(raw) => ft_media::views::set_orientation(raw),
                // CallKit's video button, the notification's camera action: our camera, if allowed.
                NativeCallEvent::VideoRequested => {
                    tauri::async_runtime::spawn(async move {
                        if camera(&app).await {
                            let _ = core.request_call_video().await;
                        }
                    });
                }
            }
        }
    });
}

/// A socket to the router opened this recently is kept when a call event asks for one.
const CALL_SOCKET_FRESH: std::time::Duration = std::time::Duration::from_secs(10);

/// Whether a native call event asks for a fresh socket to the router.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reconnect {
    Now,
    UnlessFresh,
    No,
}

/// A call push means the router found this phone offline (2026-09-28): iOS dropped the socket of
/// the suspended app, whatever the app thinks, and only the WebView (which does not run in the
/// background) used to ask to reconnect. CallKit's answer and the audio activation ask too, in
/// case the push's reconnection did not get through, but keep a socket just opened: dropping it
/// again could lose the call's offer in the gap.
fn reconnect_for(event: NativeCallEvent) -> Reconnect {
    match event {
        NativeCallEvent::Incoming => Reconnect::Now,
        // A decline waits for the offer as an answer does, to decline it.
        NativeCallEvent::Answer | NativeCallEvent::Decline | NativeCallEvent::AudioActivated(_) => Reconnect::UnlessFresh,
        NativeCallEvent::End
        | NativeCallEvent::Mute(_)
        | NativeCallEvent::AudioDeactivated(_)
        | NativeCallEvent::Visible(_)
        | NativeCallEvent::Orientation(_)
        | NativeCallEvent::VideoRequested => Reconnect::No,
    }
}

/// Opening the app when an incoming call is answered (iOS experiment, 2026-09-29): a local debug
/// switch, set at build time with `FT_IOS_OPEN_APP_ON_ANSWER=1`, off by default. With it, the
/// bridge reports every incoming call to CallKit as a video call, so iOS opens the app on answer.
fn open_app_on_answer(flag: Option<&str>) -> bool {
    matches!(flag.map(str::trim), Some("1" | "true"))
}

/// Temporary call diagnostics (2026-09-28), to find why a native call's voice is one-way on the
/// iPhone: state names go to the device log through the bridge (`os_log` subsystem
/// `com.flickertalk.calls` on iOS, `Log` tag `FtCallDiag` on Android). To remove: set this to
/// false, or delete it with `native_event_name`, `update_name`, `diagnose_call`, `device_counts`,
/// `speaker_name`, `CAMERA_SWITCHED`, `diagnose_now_and_later`, `timings_line`,
/// `native_event_stage` (and `ft_core::timings`) and their calls, and the
/// `diagnose` command of the bridge.
const CALL_DIAGNOSTICS: bool = true;

fn native_event_name(event: NativeCallEvent) -> &'static str {
    match event {
        NativeCallEvent::Incoming => "incoming",
        NativeCallEvent::Answer => "answer",
        NativeCallEvent::End => "end",
        NativeCallEvent::Decline => "decline",
        NativeCallEvent::Mute(_) => "mute",
        NativeCallEvent::AudioActivated(_) => "audio activated",
        NativeCallEvent::AudioDeactivated(_) => "audio deactivated",
        NativeCallEvent::Visible(_) => "visible",
        NativeCallEvent::Orientation(_) => "orientation",
        NativeCallEvent::VideoRequested => "video requested",
    }
}

/// The call setup timings, for the device log (diagnostics).
fn timings_line(timings: Option<ft_core::timings::CallTimings>) -> String {
    format!("timings: {}", timings.unwrap_or_default().line())
}

/// The setup stage a native event marks (diagnostics): the push that rang, the user's answer.
fn native_event_stage(event: NativeCallEvent) -> Option<ft_core::timings::CallStage> {
    match event {
        NativeCallEvent::Incoming => Some(ft_core::timings::CallStage::PushReceived),
        NativeCallEvent::Answer => Some(ft_core::timings::CallStage::AnswerTapped),
        _ => None,
    }
}

/// What CallKit's answer did in the core (diagnostics).
fn answer_state(answered: bool) -> &'static str {
    if answered {
        "answered"
    } else {
        "nothing to answer yet (waits for the offer)"
    }
}

/// Where the core's call stands, and whether it is a video call (diagnostics): no identifiers.
fn phase_name(phase: Option<(CallPhase, bool)>) -> String {
    let Some((phase, video)) = phase else { return "no call".to_owned() };
    let name = match phase {
        CallPhase::Calling => "calling",
        CallPhase::Ringing => "ringing",
        CallPhase::Connecting => "connecting",
        CallPhase::Active => "active",
    };
    if video {
        format!("{name} video")
    } else {
        name.to_owned()
    }
}

fn update_name(update: &CallUpdate) -> &'static str {
    match update {
        CallUpdate::Incoming { .. } => "incoming",
        CallUpdate::Answered { .. } => "answered",
        CallUpdate::Answering => "answering",
        CallUpdate::Ended { .. } => "ended",
        CallUpdate::Connected => "connected",
        CallUpdate::Muted { .. } => "muted",
        CallUpdate::MissedWhileBusy => "missed while busy",
        CallUpdate::Video(_) => "video",
        CallUpdate::CameraFailed => "camera failed",
    }
}

/// How a native event went, for the device log. Only an audio device's error is written (an
/// OSStatus, say): others may come from the network and name a device.
async fn diagnose_outcome(app: &AppHandle, core: &Core, what: &str, outcome: &anyhow::Result<()>) {
    let audio = what.starts_with("audio");
    match outcome {
        Ok(()) => diagnose_call(app, core, &format!("event {what} ok")).await,
        Err(error) if audio => diagnose_call(app, core, &format!("event {what} failed: {error}")).await,
        Err(_) => diagnose_call(app, core, &format!("event {what} failed")).await,
    }
}

/// The voice's device counters (diagnostics). Dropped capture and underruns start again at zero
/// each time the streams reopen; the stream errors add up over the call, and on Android each one
/// reopens both streams (a route or device change, for example). The camera has none here.
fn device_counts(stats: Option<ft_media::DeviceStats>) -> String {
    match stats {
        Some(stats) => format!(
            "; audio capture dropped={} playout underruns={} audio stream errors={}",
            stats.capture_dropped, stats.playout_underruns, stats.errors
        ),
        None => String::new(),
    }
}

/// The speaker change, for the device log: it may move the audio route (diagnostics).
fn speaker_name(on: bool) -> &'static str {
    if on {
        "speaker on"
    } else {
        "speaker off"
    }
}

/// The camera flip, for the device log (diagnostics).
const CAMERA_SWITCHED: &str = "camera switched";

/// Writes `what` now and again a second later (diagnostics): AAudio reports a route change on its
/// own thread, a little after the change.
fn diagnose_now_and_later(app: &AppHandle, core: Arc<Core>, what: &'static str) {
    if !CALL_DIAGNOSTICS {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        diagnose_call(&app, &core, what).await;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        diagnose_call(&app, &core, &format!("1 s after {what}")).await;
    });
}

/// Writes a call state and whether the voice's device runs to the device log (diagnostics).
async fn diagnose_call(app: &AppHandle, core: &Core, what: &str) {
    if !CALL_DIAGNOSTICS {
        return;
    }
    let running = core.call_device_running().await;
    let app = app.clone();
    let counts = device_counts(ft_media::device_stats());
    let phase = core.current_call().await.ok().flatten().map(|current| (current.phase, current.video));
    let line = format!("{what}; call {}; device running={running}{counts}", phase_name(phase));
    // Off the async workers: the bridge blocks until the native side answers.
    let _ = tauri::async_runtime::spawn_blocking(move || app.platform().diagnose(&line)).await;
}

/// Whether the app may use the microphone, asking if it has not been asked yet. Off the main
/// thread: the bridge would deadlock there, and the question waits for the user.
async fn microphone(app: &AppHandle) -> bool {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || app.platform().request_microphone()).await.ok().and_then(Result::ok).unwrap_or(false)
}

/// Whether the app may use the camera, asking if it has not been asked yet (native video). Off the
/// main thread, like the microphone.
async fn camera(app: &AppHandle) -> bool {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || app.platform().request_camera()).await.ok().and_then(Result::ok).unwrap_or(false)
}

/// The ringing call, if it is a video call: its camera needs the permission.
async fn ringing_video_call(core: &Core) -> Option<String> {
    let current = core.current_call().await.ok().flatten()?;
    (current.phase == CallPhase::Ringing && current.video).then_some(current.call)
}

/// The views to attach: on iOS the call's layers, once its devices exist; on Android the Kotlin
/// side makes its own views and hands their surfaces over (zeros).
fn video_layers() -> Option<Layers> {
    if cfg!(target_os = "ios") {
        ft_media::views::layers()
    } else {
        Some(Layers { remote: 0, local: 0 })
    }
}

/// Moves the glue a step and tells the bridge what it says, in order, under the glue's lock: the
/// views never come after they went. Blocking: the bridge waits for the native side.
fn tell_bridge(app: &AppHandle, glue: &Mutex<VideoGlue>, step: impl FnOnce(&mut VideoGlue) -> Vec<BridgeVideo>) {
    let mut glue = glue.lock().unwrap_or_else(PoisonError::into_inner);
    for told in step(&mut glue) {
        let platform = app.platform();
        let _ = match told {
            BridgeVideo::CallVideo(on) => platform.call_video(on),
            BridgeVideo::Attach(layers) => platform.attach_video(layers.remote, layers.local),
            BridgeVideo::Layout(layout) => platform.video_layout(&layout),
            BridgeVideo::Shape(shape) => platform.video_shape(shape.width, shape.height, shape.rotation),
            BridgeVideo::Detach => platform.detach_video(),
        };
    }
}

/// Whether the core is answering `call` already, as it says of the call going on: it rings no
/// more (answered on the phone's own screen, maybe before its offer came).
fn answered_already(current: Option<&ft_core::CurrentCall>, call: &str) -> bool {
    current.is_some_and(|current| current.call == call && !current.outgoing && current.phase != CallPhase::Ringing)
}

/// Without a microphone the call cannot go on: it ends as failed, which the other side hears.
async fn fail_current_call(core: &Core) -> anyhow::Result<()> {
    match core.current_call().await? {
        Some(current) => core.end_call(&current.call, true).await,
        None => Ok(()),
    }
}

/// Sent to the UI on `ft://plugin` (2026-09-27): what a plugin on the other side said.
pub const PLUGIN_EVENT: &str = "ft://plugin";

#[derive(Clone, Serialize)]
struct PluginEventView {
    plugin: String,
    contact: String,
    /// The bytes as base64; the app hands them to the plugin's frame as they are.
    data: String,
}

/// One reminder as the phone's alarm clock is told it (2026-09-27).
#[derive(Serialize, Deserialize)]
pub struct ReminderView {
    pub plugin: String,
    pub id: String,
    pub at: i64,
    pub text: String,
}

/// Hands every reminder to the native side, as one list; a phone without the bridge (desktop)
/// keeps them in the core only.
async fn sync_reminders(app: &AppHandle, core: &Arc<Core>) {
    let Ok(reminders) = core.reminders().await else { return };
    let views: Vec<ReminderView> = reminders
        .into_iter()
        .map(|reminder| ReminderView { plugin: reminder.plugin, id: reminder.id, at: reminder.at, text: reminder.text })
        .collect();
    if let Ok(json) = serde_json::to_string(&views) {
        let _ = app.platform().set_reminders(&json);
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
        auto_download: core.auto_download_limit().await.map_err(failed)?,
    })
}

/// Files up to this many bytes are downloaded as they arrive; bigger ones wait for a tap (A4).
#[tauri::command]
pub async fn core_set_auto_download(bytes: i64, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.set_auto_download_limit(bytes).await.map_err(failed)
}

/// The user asks for a file that was waiting (A4).
#[tauri::command]
pub async fn core_accept_file(message: String, client: State<'_, Client>) -> Result<(), String> {
    let core = client.core().await?;
    tauri::async_runtime::spawn(async move {
        let _ = core.accept_file(&message).await;
    });
    Ok(())
}

/// Retires the current link and makes a new one (A5): the router learns the new addresses and
/// the contacts get the new card. Of the main list, or of an open hidden session.
#[tauri::command]
pub async fn core_renew_link(session: Option<String>, client: State<'_, Client>) -> Result<String, String> {
    let online = client.online().await?;
    let hashes = online.core.renew_link(session.as_deref()).await.map_err(failed)?;
    online.router.register(&hashes).await.map_err(failed)?;
    Ok(online.core.my_card_in(session.as_deref()).await.map_err(failed)?.to_link())
}

/// The strangers who wrote first and wait for a yes (A5), of the main list or of a session.
#[tauri::command]
pub async fn core_requests(session: Option<String>, client: State<'_, Client>) -> Result<Vec<ConversationView>, String> {
    let online = client.online().await?;
    let requests = match session.as_deref() {
        None => online.core.requests().await.map_err(failed)?,
        Some(session) => online.core.session_requests(session).await.map_err(failed)?,
    };
    conversation_views(online, requests).await
}

#[tauri::command]
pub async fn core_accept_contact(contact: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.accept_contact(&contact).await.map_err(failed)
}

#[tauri::command]
pub async fn core_decline_contact(contact: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.decline_contact(&contact).await.map_err(failed)
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
    let requests = online.core.session_requests(&session).await.map_err(failed)?;
    let circles = circle_views(&online.core, Some(&session)).await?;
    Ok(SessionView {
        id: session,
        conversations: conversation_views(online, conversations).await?,
        requests: conversation_views(online, requests).await?,
        circles,
    })
}

/// The names this phone shows for the people of a circle: its own name for a contact it chose,
/// what their card says otherwise, and "You" is the UI's to say.
async fn circle_names(core: &ft_core::Core, circle: &str) -> Result<HashMap<String, String>, String> {
    let mut names = HashMap::new();
    for member in core.circle_members(circle).await.map_err(failed)? {
        let shown = match core.store().contact(&member.device_id).await.map_err(failed)? {
            Some(contact) if contact.accepted => contact.name,
            _ => member.name,
        };
        names.insert(member.device_id, shown);
    }
    if let Some(name) = core.name().await.map_err(failed)? {
        names.insert(core.device_id().to_string(), name);
    }
    Ok(names)
}

async fn circle_view(core: &ft_core::Core, conversation: ft_storage::CircleConversation) -> Result<CircleView, String> {
    let names = circle_names(core, &conversation.circle.id).await?;
    let me = core.device_id().to_string();
    let members = core
        .circle_members(&conversation.circle.id)
        .await
        .map_err(failed)?
        .into_iter()
        .map(|member| CircleMemberView {
            me: member.device_id == me,
            name: names.get(&member.device_id).cloned().unwrap_or(member.name),
            id: member.device_id,
            admin: member.admin,
        })
        .collect();
    Ok(CircleView {
        admin: core.is_circle_admin(&conversation.circle.id).await.map_err(failed)?,
        id: conversation.circle.id,
        name: conversation.circle.name,
        members,
        admins_only: conversation.circle.admins_only,
        left: conversation.circle.left,
        unread: conversation.unread,
        last: conversation.last.as_ref().map(|last| CircleMessageView::new(last, &names)),
    })
}

/// The circles of the main list or of a session, newest first.
async fn circle_views(core: &ft_core::Core, session: Option<&str>) -> Result<Vec<CircleView>, String> {
    let mut views = Vec::new();
    for conversation in core.store().circle_conversations(session).await.map_err(failed)? {
        views.push(circle_view(core, conversation).await?);
    }
    Ok(views)
}

#[tauri::command]
pub async fn core_circles(client: State<'_, Client>) -> Result<Vec<CircleView>, String> {
    let core = client.core().await?;
    circle_views(&core, None).await
}

#[tauri::command]
pub async fn core_circle(circle: String, client: State<'_, Client>) -> Result<CircleView, String> {
    let core = client.core().await?;
    let record = core.store().circle(&circle).await.map_err(failed)?.ok_or("unknown circle")?;
    let session = record.session.clone();
    let conversation = core
        .store()
        .circle_conversations(session.as_deref())
        .await
        .map_err(failed)?
        .into_iter()
        .find(|conversation| conversation.circle.id == circle)
        .ok_or("unknown circle")?;
    circle_view(&core, conversation).await
}

/// Makes a circle of these contacts; in a hidden session, of its contacts. Returns its id.
#[tauri::command]
pub async fn core_circle_create(name: String, members: Vec<String>, session: Option<String>, client: State<'_, Client>) -> Result<String, String> {
    client.core().await?.create_circle(&name, &members, session.as_deref()).await.map_err(failed)
}

#[tauri::command]
pub async fn core_circle_invite(circle: String, contact: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.invite_to_circle(&circle, &contact).await.map_err(failed)
}

#[tauri::command]
pub async fn core_circle_remove(circle: String, contact: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.remove_from_circle(&circle, &contact).await.map_err(failed)
}

#[tauri::command]
pub async fn core_circle_set_admin(circle: String, contact: String, admin: bool, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.set_circle_admin(&circle, &contact, admin).await.map_err(failed)
}

#[tauri::command]
pub async fn core_circle_rename(circle: String, name: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.rename_circle(&circle, &name).await.map_err(failed)
}

#[tauri::command]
pub async fn core_circle_admins_only(circle: String, admins_only: bool, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.set_circle_admins_only(&circle, admins_only).await.map_err(failed)
}

#[tauri::command]
pub async fn core_circle_leave(circle: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.leave_circle(&circle).await.map_err(failed)
}

/// Erases a circle this phone is no longer in, with everything said in it.
#[tauri::command]
pub async fn core_circle_forget(circle: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.forget_circle(&circle).await.map_err(failed)
}

#[tauri::command]
pub async fn core_circle_send(circle: String, text: String, client: State<'_, Client>) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("nothing to send".to_owned());
    }
    let core = client.core().await?;
    // What stops a text (admins only, no longer in it) is said at once; delivery runs behind.
    core.circle_writable(&circle).await.map_err(failed)?;
    tauri::async_runtime::spawn(async move {
        let _ = core.send_circle_text(&circle, &text).await;
    });
    Ok(())
}

#[tauri::command]
pub async fn core_circle_messages(circle: String, limit: i64, client: State<'_, Client>) -> Result<Vec<CircleMessageView>, String> {
    let core = client.core().await?;
    let names = circle_names(&core, &circle).await?;
    let messages = core.store().circle_messages(&circle, limit).await.map_err(failed)?;
    Ok(messages.iter().map(|message| CircleMessageView::new(message, &names)).collect())
}

#[tauri::command]
pub async fn core_circle_mark_read(circle: String, client: State<'_, Client>) -> Result<(), String> {
    let core = client.core().await?;
    tauri::async_runtime::spawn(async move {
        let _ = core.mark_circle_read(&circle).await;
    });
    Ok(())
}

/// Six digits open the hidden session that has them or a new empty one (A3): every PIN is
/// valid, and nothing says whether a session existed. `None` only when all seven slots are
/// taken; the screen shows an empty session all the same.
#[tauri::command]
pub async fn core_session_open(pin: String, app: AppHandle, client: State<'_, Client>) -> Result<Option<SessionView>, String> {
    let online = client.online().await?;
    let Some(session) = online.core.open_session(&pin).await.map_err(failed)? else { return Ok(None) };
    // Its wake-ups are heard from now on (app#9).
    let _ = app.platform().set_open_slots(&online.core.open_slots());
    Ok(Some(session_view(online, session).await?))
}

/// Takes an open hidden session away for good, with its contacts and history (A3).
#[tauri::command]
pub async fn core_session_remove(session: String, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let online = client.online().await?;
    online.core.remove_session(&session).await.map_err(failed)?;
    let _ = app.platform().set_open_slots(&online.core.open_slots());
    reregister(online);
    Ok(())
}

/// Leaves a hidden session; one with nobody in it goes for good (A3).
#[tauri::command]
pub async fn core_session_close(session: String, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let online = client.online().await?;
    let gone = online.core.close_session(&session).await.map_err(failed)?;
    let _ = app.platform().set_open_slots(&online.core.open_slots());
    if gone {
        reregister(online);
    }
    Ok(())
}

/// A session went and its slot got a new link (A3): the router learns the eight hashes again,
/// in the background, so no command waits for the network. If it cannot be reached, the next
/// start registers them anyway.
fn reregister(online: &Online) {
    let (core, router) = (online.core.clone(), online.router.clone());
    tauri::async_runtime::spawn(async move {
        if let Ok(hashes) = core.route_capability_hashes().await {
            let _ = router.register(&hashes).await;
        }
    });
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
    /// The kinds of file it opens (2026-09-27), for the "open with" of a message.
    #[serde(default)]
    opens: Vec<String>,
    /// The kinds of file it is the viewer of: a tap on such a file opens it here.
    #[serde(default)]
    views: Vec<String>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PermissionsView {
    network: Vec<String>,
    messages: bool,
    send: String,
    #[serde(default)]
    print: bool,
    /// 2026-09-27: the live channel, local reminders, the user's cloud and the room it keeps.
    #[serde(default)]
    live: bool,
    #[serde(default)]
    remind: bool,
    #[serde(default)]
    drive: bool,
    /// `small` or `large`.
    #[serde(default = "small")]
    storage: String,
}

fn small() -> String {
    "small".to_owned()
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
            live: permissions.live,
            remind: permissions.remind,
            drive: permissions.drive,
            storage: match permissions.storage {
                ft_plugins::Storage::Small => "small",
                ft_plugins::Storage::Large => "large",
            }
            .to_owned(),
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
            opens: plugin.manifest.opens,
            views: plugin.manifest.views,
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
        live: granted.live,
        remind: granted.remind,
        drive: granted.drive,
        storage: if granted.storage == "large" { ft_plugins::Storage::Large } else { ft_plugins::Storage::Small },
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

// ---- Records, refs, reminders, the live channel and "open with" (2026-09-27) ----

/// A record of a plugin, as base64: bytes the core never reads.
#[tauri::command]
pub async fn core_plugin_record_get(plugin: String, key: String, client: State<'_, Client>) -> Result<Option<String>, String> {
    let value = client.core().await?.plugin_record(&plugin, &key).await.map_err(failed)?;
    Ok(value.map(|bytes| BASE64.encode(bytes)))
}

#[tauri::command]
pub async fn core_plugin_record_set(plugin: String, key: String, value: String, client: State<'_, Client>) -> Result<(), String> {
    let bytes = BASE64.decode(value.as_bytes()).map_err(|_| "that value is not base64".to_owned())?;
    client.core().await?.plugin_record_set(&plugin, &key, &bytes).await.map_err(failed)
}

#[tauri::command]
pub async fn core_plugin_record_forget(plugin: String, key: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.plugin_record_forget(&plugin, &key).await.map_err(failed)
}

#[tauri::command]
pub async fn core_plugin_record_keys(plugin: String, prefix: String, client: State<'_, Client>) -> Result<Vec<String>, String> {
    client.core().await?.plugin_record_keys(&plugin, &prefix).await.map_err(failed)
}

/// How much of its room a plugin uses and how much it has, in bytes.
#[tauri::command]
pub async fn core_plugin_record_usage(plugin: String, client: State<'_, Client>) -> Result<(u64, u64), String> {
    client.core().await?.plugin_records_usage(&plugin).await.map_err(failed)
}

/// An opaque handle for the message the user hands a plugin (2026-09-27).
#[tauri::command]
pub async fn core_plugin_ref(plugin: String, message: String, client: State<'_, Client>) -> Result<String, String> {
    client.core().await?.plugin_ref(&plugin, &message).await.map_err(failed)
}

/// Where a plugin's ref leads: the conversation to open, or nothing.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefTargetView {
    contact: String,
    message: String,
}

#[tauri::command]
pub async fn core_plugin_open_chat(plugin: String, reference: String, client: State<'_, Client>) -> Result<Option<RefTargetView>, String> {
    let target = client.core().await?.plugin_ref_target(&plugin, &reference).await.map_err(failed)?;
    Ok(target.map(|target| RefTargetView { contact: target.contact, message: target.message_id }))
}

/// A reminder a plugin sets or moves (2026-09-27); the phone's alarm clock is told through the event.
#[tauri::command]
pub async fn core_remind_set(plugin: String, id: String, at: i64, text: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.set_reminder(&plugin, &id, at, &text).await.map_err(failed)
}

#[tauri::command]
pub async fn core_remind_cancel(plugin: String, id: String, client: State<'_, Client>) -> Result<bool, String> {
    client.core().await?.cancel_reminder(&plugin, &id).await.map_err(failed)
}

#[tauri::command]
pub async fn core_remind_list(plugin: String, client: State<'_, Client>) -> Result<Vec<ReminderView>, String> {
    let reminders = client.core().await?.plugin_reminders(&plugin).await.map_err(failed)?;
    Ok(reminders
        .into_iter()
        .map(|reminder| ReminderView { plugin: reminder.plugin, id: reminder.id, at: reminder.at, text: reminder.text })
        .collect())
}

/// The reminder the user tapped to open the app, as `plugin\nid`, once.
#[tauri::command]
pub async fn core_pending_reminder(app: AppHandle) -> Result<String, String> {
    Ok(app.platform().pending_reminder().unwrap_or_default())
}

/// What a plugin says to its twin on the contact's phone (2026-09-27, `ft.live`): only over
/// the direct connection; `false` if the contact cannot be reached now.
#[tauri::command]
pub async fn core_plugin_live_send(plugin: String, contact: String, data: String, client: State<'_, Client>) -> Result<bool, String> {
    let bytes = BASE64.decode(data.as_bytes()).map_err(|_| "that data is not base64".to_owned())?;
    client.core().await?.plugin_live_send(&plugin, &contact, bytes).await.map_err(failed)
}

/// The plugins installed here that open a file of this kind, for "open with".
#[tauri::command]
pub async fn core_plugins_opening(mime: String, client: State<'_, Client>) -> Result<Vec<PluginView>, String> {
    let core = client.core().await?;
    let opening = core.plugins_opening(&mime).await.map_err(failed)?;
    Ok(core
        .plugins()
        .await
        .map_err(failed)?
        .into_iter()
        .filter(|plugin| opening.iter().any(|manifest| manifest.id == plugin.manifest.id))
        .map(|plugin| PluginView {
            asks: PermissionsView::from(&plugin.manifest.permissions),
            granted: PermissionsView::from(&plugin.granted),
            id: plugin.manifest.id,
            name: plugin.manifest.name,
            version: plugin.manifest.version,
            installed_at: plugin.installed_at,
            opens: plugin.manifest.opens,
            views: plugin.manifest.views,
        })
        .collect())
}

/// The file of a message, for a plugin to open it (2026-09-27): only one that is here whole,
/// and no bigger than what a plugin's frame may hold.
#[tauri::command]
pub async fn core_read_message_file(message: String, client: State<'_, Client>) -> Result<HandedFile, String> {
    let file = file_of(&client, &message).await?;
    if file.size as u64 > PLUGIN_FILE_LIMIT {
        return Err("that file is too big for a plugin".to_owned());
    }
    let bytes = tokio::fs::read(&file.path).await.map_err(|_| "the bytes of that file are no longer here".to_owned())?;
    Ok(HandedFile { name: file.name, mime: file.mime, data: BASE64.encode(bytes) })
}

/// A file as a plugin gets it: name, kind and bytes as base64.
#[derive(Serialize)]
pub struct HandedFile {
    pub name: String,
    pub mime: String,
    pub data: String,
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
/// The name is tamed the way the core tames every file name (M11).
fn made_file(client: &State<'_, Client>, name: &str, data: &str, folder: &str) -> Result<(PathBuf, String), String> {
    let bytes = BASE64.decode(data.as_bytes()).map_err(|_| "that is not a file".to_owned())?;
    if bytes.len() as u64 > PLUGIN_FILE_LIMIT {
        return Err("that file is too big".to_owned());
    }
    let safe = ft_core::files::safe_file_name(name);
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

/// Lets the router wake this phone when the app is closed (M4, and APNs on iOS since 2026-09-28): asks to show notifications and
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
    router.set_push(push_provider(), &token).await.map_err(failed)
}

/// Who wakes this phone: APNs on an iPhone (its token names the gateway and the app), FCM
/// elsewhere (2026-09-28).
fn push_provider() -> &'static str {
    if cfg!(target_os = "ios") {
        "apns"
    } else {
        "fcm"
    }
}

/// The app is back on the screen (2026-09-28): the socket to the router is opened again at once,
/// and its welcome fetches the mailbox and retries what waits. iOS cuts the socket of a suspended
/// app, and the phone may take long to notice on its own.
#[tauri::command]
pub async fn core_resume(client: State<'_, Client>) -> Result<(), String> {
    client.online().await?.router.reconnect_now();
    Ok(())
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

/// Whether voice calls run natively on this phone (iOS and Android; 2026-09-28). The desktop
/// keeps the WebView's media.
#[tauri::command]
pub async fn core_native_calls(client: State<'_, Client>) -> Result<bool, String> {
    Ok(client.core().await?.native_calls())
}

/// Places a call whose media runs in Rust; returns its id at once, the offer goes on in the
/// background. The phone's call screen (CallKit) owns the audio session of the call. A `video`
/// call turns our camera on as it connects; without the camera allowed it fails with
/// `camera_denied`, and the WebView places it as voice.
#[tauri::command]
pub async fn core_call_start_native(contact: String, routing: String, video: bool, app: AppHandle, client: State<'_, Client>) -> Result<String, String> {
    let routing = call_routing(&routing)?;
    if !microphone(&app).await {
        return Err("the microphone is not allowed".to_owned());
    }
    if video {
        camera_or_denied(camera(&app).await)?;
    }
    let core = client.core().await?;
    let call = core.start_native_call(&contact, routing, video).await.map_err(failed)?;
    let name = core.store().contact(&contact).await.ok().flatten().map(|stored| stored.name).unwrap_or_default();
    let _ = app.platform().call_started_outgoing(&name, video);
    Ok(call)
}

/// Answers the ringing voice call with our voice in Rust. On iOS CallKit answers it (2026-09-28):
/// its `Answer` event answers in the core, so the in-app button and the lock screen take the same
/// path and the system activates the audio session for both.
#[tauri::command]
///
/// A video call turns our camera on as it connects. Without the camera allowed it is answered all
/// the same, as voice, and the WebView hears `camera_denied`.
pub async fn core_call_answer_native(call: String, routing: String, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let routing = call_routing(&routing)?;
    let core = client.core().await?;
    // The routing the core answers with, whoever answers.
    core.set_call_routing(routing).await.map_err(failed)?;
    // Asked here, with the app on the screen: CallKit's answer asks again and knows by then.
    let video_call = ringing_video_call(&core).await.is_some_and(|ringing| ringing == call);
    let camera_allowed = !video_call || camera(&app).await;
    if app.platform().answer_call().unwrap_or(false) {
        return camera_or_denied(camera_allowed);
    }
    let _ = app.platform().stop_ringing();
    tauri::async_runtime::spawn(async move {
        if !microphone(&app).await {
            let _ = core.end_call(&call, true).await;
            return;
        }
        if core.answer_native_call(&call, routing).await.is_ok() && !camera_allowed {
            let _ = core.set_call_camera(&call, false).await;
        }
    });
    camera_or_denied(camera_allowed)
}

/// Turns our camera on or off in a native call (native video, 2026-09-29): switching between
/// voice and video, with no new offer. On asks for the camera first: `camera_denied` if it may
/// not be used. Before the call connects, it is kept for then.
#[tauri::command]
pub async fn core_call_set_video(call: String, on: bool, app: AppHandle, client: State<'_, Client>) -> Result<CallVideoView, String> {
    if on {
        camera_or_denied(camera(&app).await)?;
    }
    let state = client.core().await?.set_call_camera(&call, on).await.map_err(failed)?;
    Ok(CallVideoView::from(state))
}

/// The other camera: front to back and back again.
#[tauri::command]
pub async fn core_call_switch_camera(call: String, app: AppHandle, client: State<'_, Client>) -> Result<CallVideoView, String> {
    let core = client.core().await?;
    let state = core.switch_call_camera(&call).await.map_err(failed)?;
    diagnose_now_and_later(&app, core, CAMERA_SWITCHED);
    Ok(CallVideoView::from(state))
}

/// Where the call screen leaves room for the pictures, or `null` when it does not show them (it
/// is left): the views hide and our camera is held. It may come before the call has video, or
/// an id: the latest is kept for when the views come.
#[tauri::command]
pub async fn core_call_video_layout(layout: Option<VideoLayout>, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let glue = client.video.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || tell_bridge(&app, &glue, |glue| glue.layout(layout))).await;
    if let Ok(core) = client.core().await {
        let _ = core.set_call_shown(layout.is_some()).await;
    }
    Ok(())
}

/// The call's voice on the speaker or the receiver (2026-09-28).
#[tauri::command]
pub async fn core_call_speaker(on: bool, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    app.platform().set_speaker(on).map_err(|error| error.to_string())?;
    if let Ok(core) = client.core().await {
        diagnose_now_and_later(&app, core, speaker_name(on));
    }
    Ok(())
}

/// Mutes or unmutes our voice in a native call.
#[tauri::command]
pub async fn core_call_mute(call: String, muted: bool, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.mute_call(&call, muted).await.map_err(failed)
}

/// The routing chosen in Settings, kept in the core for calls answered with no WebView (§17).
#[tauri::command]
pub async fn core_set_call_routing(routing: String, client: State<'_, Client>) -> Result<(), String> {
    let routing = call_routing(&routing)?;
    client.core().await?.set_call_routing(routing).await.map_err(failed)
}

/// The ringing or active call, if any: the core may have heard it, or answered it from CallKit,
/// before the WebView was there.
#[tauri::command]
pub async fn core_current_call(client: State<'_, Client>) -> Result<Option<CurrentCallView>, String> {
    Ok(client.core().await?.current_call().await.map_err(failed)?.map(CurrentCallView::from))
}

/// Hangs up, declines or gives up; `failed` when the media could not connect. A native call's
/// voice stops with it.
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
/// the system picker (so only from the picker's own folder, M1), and only up to the limit.
#[tauri::command]
pub async fn core_read_picked(path: String, client: State<'_, Client>) -> Result<String, String> {
    let path = picked_path(client.dir()?, &path)?;
    let size = std::fs::metadata(&path).map_err(failed)?.len();
    if size > PLUGIN_FILE_LIMIT {
        return Err("that file is too big to hand over".to_owned());
    }
    let bytes = std::fs::read(&path).map_err(failed)?;
    Ok(BASE64.encode(bytes))
}

/// What became of a file a plugin made (A2): sent by itself (`auto`), or left in the composer
/// for the user to send (`propose`), as a picked file.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MadeView {
    sent: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    staged: Option<PickedView>,
}

/// A plugin hands the app a file (A2). What happens is what the user granted the plugin: with
/// `auto` the core sends it, with `propose` it is put in the composer for the user to send, and
/// with nothing the file goes nowhere. The bytes are written to the app's folder either way, like
/// any other sent file, and stay until their message is erased.
#[tauri::command]
pub async fn core_plugin_made(
    plugin: String,
    contact: String,
    name: String,
    mime: String,
    data: String,
    client: State<'_, Client>,
) -> Result<MadeView, String> {
    let core = client.core().await?;
    let sending = core.plugin_sending(&plugin).await.map_err(failed)?;
    if sending == ft_plugins::Sending::Nothing {
        return Err("that plugin may not write in the chat".to_owned());
    }
    let bytes = BASE64.decode(data.as_bytes()).map_err(|_| "that is not a file".to_owned())?;
    if bytes.len() as u64 > PLUGIN_FILE_LIMIT {
        return Err("that file is too big to send".to_owned());
    }
    let safe = ft_core::files::safe_file_name(&name);
    let dir = client.dir()?.join("files").join("outgoing");
    std::fs::create_dir_all(&dir).map_err(failed)?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|since| since.as_millis()).unwrap_or(0);
    let path = dir.join(format!("{stamp}-{safe}"));
    std::fs::write(&path, &bytes).map_err(failed)?;

    if sending == ft_plugins::Sending::Auto {
        // `send_file` returns once the offer is out, not once the bytes are pulled.
        let sent = path.clone();
        tauri::async_runtime::spawn(async move {
            let _ = core.plugin_send_file(&plugin, &contact, &sent, &safe, &mime).await;
        });
        return Ok(MadeView { sent: true, staged: None });
    }
    let staged = PickedView { path: path.to_string_lossy().into_owned(), name: safe, mime, size: bytes.len() as u64 };
    Ok(MadeView { sent: false, staged: Some(staged) })
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

/// A photo taken right now with the camera app; it waits in the app's folder like a picked file.
#[tauri::command]
pub async fn core_take_photo(app: AppHandle) -> Result<Vec<PickedView>, String> {
    let taken = tauri::async_runtime::spawn_blocking(move || app.platform().take_photo()).await.map_err(failed)?.map_err(failed)?;
    Ok(taken.into_iter().map(|file| PickedView { path: file.path, name: file.name, mime: file.mime, size: file.size }).collect())
}

/// A file the user picked, waiting in the app's folder.
#[derive(Serialize, Deserialize, Clone)]
pub struct PickedView {
    pub path: String,
    pub name: String,
    pub mime: String,
    pub size: u64,
}

/// Sends a file the user picked; the bytes never go through the WebView. The copy the picker
/// made stays in the app: the contact pulls its chunks after `send_file` has returned, and the
/// message shows it afterwards. It goes when the message is erased, like a voice note.
#[tauri::command]
pub async fn core_send_picked(contact: String, file: PickedView, client: State<'_, Client>) -> Result<(), String> {
    // Only what the picker or a plugin left in the app's own folders (M1).
    let path = picked_path(client.dir()?, &file.path)?;
    let core = client.core().await?;
    tauri::async_runtime::spawn(async move {
        let _ = core.send_file(&contact, &path, &file.name, &file.mime).await;
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


// ---------------------------------------------------------------------------------------------
// The user's cloud (plan-drive, 2026-09-27): the login through the system browser, the drive and
// the backup. Tokens, the vault key and the recovery code stay in the core; the WebView sees
// names, sizes and states.
// ---------------------------------------------------------------------------------------------

/// Sent to the UI on `ft://vault` when the drive changes, and on `ft://vault-progress` while
/// bytes move.
pub const VAULT_EVENT: &str = "ft://vault";
pub const VAULT_PROGRESS_EVENT: &str = "ft://vault-progress";

#[derive(Serialize, Clone)]
pub struct VaultProgressView {
    pub done: u64,
    pub total: u64,
}

/// The system browser, for a login: the platform bridge opens the address and brings back the
/// redirect to the app's scheme.
struct PlatformAuthorizer(AppHandle);

#[async_trait::async_trait]
impl ft_core::vault::Authorizer for PlatformAuthorizer {
    async fn authorize(&self, url: &str, scheme: &str) -> anyhow::Result<String> {
        let (app, url, scheme) = (self.0.clone(), url.to_owned(), scheme.to_owned());
        tauri::async_runtime::spawn_blocking(move || app.platform().authorize(&url, &scheme).map_err(anyhow::Error::from)).await?
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QuotaView {
    pub used: u64,
    pub total: u64,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DriveStatusView {
    pub files: usize,
    pub folders: usize,
    pub used: u64,
    pub pending: usize,
    pub quota: Option<QuotaView>,
    pub backup_at: Option<i64>,
}

/// Where the drive stands: `none`, `empty` (logged in, no drive yet), `locked` (a drive from
/// another phone: needs the phrase), `outdated` (a drive of the first version: made again) or
/// `ready`. `triesLeft` and `retryAt` say how the recovery's tries stand (2026-09-28).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatusView {
    pub state: String,
    pub provider: Option<String>,
    pub drive: Option<DriveStatusView>,
    pub problem: Option<String>,
    pub tries_left: u32,
    pub retry_at: Option<i64>,
}

impl From<ft_core::vault::VaultStatus> for VaultStatusView {
    fn from(status: ft_core::vault::VaultStatus) -> Self {
        use ft_core::vault::VaultState;
        Self {
            state: match status.state {
                VaultState::None => "none",
                VaultState::Empty => "empty",
                VaultState::Locked => "locked",
                VaultState::Outdated => "outdated",
                VaultState::Ready => "ready",
            }
            .to_owned(),
            provider: status.provider,
            drive: status.drive.map(|drive| DriveStatusView {
                files: drive.files,
                folders: drive.folders,
                used: drive.used,
                pending: drive.pending,
                quota: drive.quota.map(|quota| QuotaView { used: quota.used, total: quota.total }),
                backup_at: drive.backup_at,
            }),
            problem: status.problem,
            tries_left: status.tries_left,
            retry_at: status.retry_at,
        }
    }
}

#[derive(Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DriveFolderView {
    pub id: String,
    pub name: String,
    pub parent: Option<String>,
    pub modified: i64,
}

#[derive(Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DriveFileView {
    pub id: String,
    pub name: String,
    pub parent: Option<String>,
    pub size: u64,
    pub mime: String,
    pub modified: i64,
}

#[derive(Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DrivePendingView {
    pub blob: String,
    pub name: String,
    pub parent: Option<String>,
    pub size: u64,
    pub mime: String,
    pub error: String,
}

#[derive(Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DriveListingView {
    pub folders: Vec<DriveFolderView>,
    pub files: Vec<DriveFileView>,
    pub pending: Vec<DrivePendingView>,
}

impl From<ft_vault::Listing> for DriveListingView {
    fn from(listing: ft_vault::Listing) -> Self {
        Self {
            folders: listing.folders.into_iter().map(|one| DriveFolderView { id: one.id, name: one.name, parent: one.parent, modified: one.modified }).collect(),
            files: listing
                .files
                .into_iter()
                .map(|one| DriveFileView { id: one.id, name: one.name, parent: one.parent, size: one.size, mime: one.mime, modified: one.modified })
                .collect(),
            pending: listing
                .pending
                .into_iter()
                .map(|one| DrivePendingView { blob: one.blob, name: one.name, parent: one.parent, size: one.size, mime: one.mime, error: one.error })
                .collect(),
        }
    }
}

#[derive(Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackupView {
    pub at: i64,
    pub files: usize,
    pub db_size: u64,
}

impl From<ft_vault::Backup> for BackupView {
    fn from(backup: ft_vault::Backup) -> Self {
        Self { at: backup.at, files: backup.files.len(), db_size: backup.db_size }
    }
}

/// Where the drive stands; the plugin and Settings ask this first.
#[tauri::command]
pub async fn core_vault_status(client: State<'_, Client>) -> Result<VaultStatusView, String> {
    Ok(client.core().await?.vault_status().await.map_err(failed)?.into())
}

/// Logs in to a cloud through the system browser; the WebView never sees the tokens.
#[tauri::command]
pub async fn core_vault_connect(provider: String, app: AppHandle, client: State<'_, Client>) -> Result<VaultStatusView, String> {
    let core = client.core().await?;
    Ok(core.vault_connect(&provider, &PlatformAuthorizer(app)).await.map_err(failed)?.into())
}

/// Makes the drive, its key sealed with the phrase the user chose (kept by nobody but them).
#[tauri::command]
pub async fn core_vault_setup(phrase: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.vault_setup(&phrase).await.map_err(failed)
}

#[tauri::command]
pub async fn core_vault_unlock(phrase: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.vault_unlock(&phrase).await.map_err(failed)
}

/// A strong phrase, for whoever wants the app to suggest one.
#[tauri::command]
pub async fn core_vault_suggest_phrase(client: State<'_, Client>) -> Result<String, String> {
    Ok(client.core().await?.vault_suggest_phrase())
}

/// Seals the drive's key with a new phrase; the old one stops opening it.
#[tauri::command]
pub async fn core_vault_change_phrase(phrase: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.vault_change_phrase(&phrase).await.map_err(failed)
}

#[tauri::command]
pub async fn core_vault_disconnect(client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.vault_disconnect().await.map_err(failed)
}

/// A development build without a client id baked in sets one here.
#[tauri::command]
pub async fn core_vault_set_client_id(id: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.set_google_client_id(&id).await.map_err(failed)
}

#[tauri::command]
pub async fn core_vault_list(parent: Option<String>, client: State<'_, Client>) -> Result<DriveListingView, String> {
    Ok(client.core().await?.vault_list(parent.as_deref()).await.map_err(failed)?.into())
}

#[tauri::command]
pub async fn core_vault_mkdir(name: String, parent: Option<String>, client: State<'_, Client>) -> Result<String, String> {
    client.core().await?.vault_mkdir(&name, parent.as_deref()).await.map_err(failed)
}

#[tauri::command]
pub async fn core_vault_rename(id: String, name: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.vault_rename(&id, &name).await.map_err(failed)
}

#[tauri::command]
pub async fn core_vault_move(id: String, parent: Option<String>, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.vault_move(&id, parent.as_deref()).await.map_err(failed)
}

#[tauri::command]
pub async fn core_vault_remove(id: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.vault_remove(&id).await.map_err(failed)
}

/// Puts a file the user picked in the drive; `null` when it waits for the network.
#[tauri::command]
pub async fn core_vault_upload(file: PickedView, parent: Option<String>, client: State<'_, Client>) -> Result<Option<String>, String> {
    let core = client.core().await?;
    let path = picked_path(client.dir()?, &file.path)?;
    core.vault_upload(&path, &file.name, &file.mime, parent.as_deref()).await.map_err(failed)
}

/// Puts the file of a message in the drive: what "keep in my drive" does from a bubble.
#[tauri::command]
pub async fn core_vault_upload_message(message: String, parent: Option<String>, client: State<'_, Client>) -> Result<Option<String>, String> {
    let file = file_of(&client, &message).await?;
    let core = client.core().await?;
    core.vault_upload(Path::new(&file.path), &file.name, &file.mime, parent.as_deref()).await.map_err(failed)
}

/// Tries again what waits; how many still wait.
#[tauri::command]
pub async fn core_vault_retry(client: State<'_, Client>) -> Result<usize, String> {
    client.core().await?.vault_run_queue().await.map_err(failed)
}

#[tauri::command]
pub async fn core_vault_cancel_pending(blob: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.vault_cancel_pending(&blob).await.map_err(failed)
}

/// Brings a file down, opened, into the app's folder; what the composer or a viewer needs.
#[tauri::command]
pub async fn core_vault_download(id: String, client: State<'_, Client>) -> Result<PickedView, String> {
    let (path, file) = client.core().await?.vault_download(&id).await.map_err(failed)?;
    Ok(PickedView { path: path.to_string_lossy().into_owned(), name: file.name, mime: file.mime, size: file.size })
}

/// Opens a file of the drive in the viewer the user picks.
#[tauri::command]
pub async fn core_vault_open(id: String, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let (path, file) = client.core().await?.vault_download(&id).await.map_err(failed)?;
    app.platform().open_file(&path.to_string_lossy(), &file.mime).map_err(failed)
}

/// Copies a file of the drive to the phone's Downloads.
#[tauri::command]
pub async fn core_vault_save(id: String, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let (path, file) = client.core().await?.vault_download(&id).await.map_err(failed)?;
    app.platform().save_to_downloads(&path.to_string_lossy(), &file.name, &file.mime).map_err(failed)
}

/// Sends a file of the drive to a contact, as any file (§62).
#[tauri::command]
pub async fn core_vault_send(id: String, contact: String, client: State<'_, Client>) -> Result<String, String> {
    client.core().await?.vault_send(&id, &contact).await.map_err(failed)
}

#[tauri::command]
pub async fn core_vault_backup(client: State<'_, Client>) -> Result<BackupView, String> {
    Ok(client.core().await?.vault_backup().await.map_err(failed)?.into())
}

#[tauri::command]
pub async fn core_vault_backup_info(client: State<'_, Client>) -> Result<Option<BackupView>, String> {
    Ok(client.core().await?.vault_backup_info().await.map_err(failed)?.map(Into::into))
}

/// Brings the backup down and restarts the app, which swaps it in as after a move (§60).
#[tauri::command]
pub async fn core_vault_restore(app: AppHandle, client: State<'_, Client>) -> Result<BackupView, String> {
    let core = client.core().await?;
    let backup = core.vault_restore().await.map_err(failed)?;
    if let Ok(online) = client.online().await {
        let _ = online.router.forget().await;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(RESTART_PAUSE).await;
        restart(&app);
    });
    Ok(backup.into())
}

/// Whether a plugin was granted the drive; the sheet asks before answering any `ft.drive.*`.
#[tauri::command]
pub async fn core_plugin_may_use_drive(plugin: String, client: State<'_, Client>) -> Result<bool, String> {
    client.core().await?.plugin_may_use_drive(&plugin).await.map_err(failed)
}

#[cfg(test)]
mod tests {
    use ft_storage::{Contact, Conversation, Message, MessageState};

    use ft_media::{Facing, Layers, RemoteShape, VideoState};
    use tauri_plugin_ft_platform::{VideoLayout, VideoRect};

    use super::*;

    // Opening the app when an incoming call is answered (iOS experiment, 2026-09-29): off unless
    // the build set FT_IOS_OPEN_APP_ON_ANSWER to 1 (or true).
    #[test]
    fn the_open_app_on_answer_switch_is_off_unless_the_build_turns_it_on() {
        assert!(!open_app_on_answer(None));
        assert!(!open_app_on_answer(Some("")));
        assert!(!open_app_on_answer(Some("0")));
        assert!(!open_app_on_answer(Some("false")));
        assert!(open_app_on_answer(Some("1")));
        assert!(open_app_on_answer(Some("true")));
        assert!(open_app_on_answer(Some(" 1\n")));
    }

    // iPhones are woken through APNs, the rest through FCM (2026-09-28): the router is told which.
    #[test]
    fn the_push_provider_is_the_one_of_the_platform() {
        assert_eq!(push_provider(), if cfg!(target_os = "ios") { "apns" } else { "fcm" });
    }

    // The recovery phrase (2026-09-28): the page learns a drive of the first version, how many
    // tries are left and until when the recovery is locked.
    #[test]
    fn the_drive_s_state_tells_the_page_what_it_needs() {
        use ft_core::vault::{VaultState, VaultStatus};
        let view = VaultStatusView::from(VaultStatus { state: VaultState::Outdated, provider: Some("google".into()), drive: None, problem: None, tries_left: 5, retry_at: None });
        assert_eq!(view.state, "outdated");
        let view = VaultStatusView::from(VaultStatus { state: VaultState::Locked, provider: Some("google".into()), drive: None, problem: None, tries_left: 0, retry_at: Some(42) });
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!((json["state"].as_str(), json["triesLeft"].as_u64(), json["retryAt"].as_i64()), (Some("locked"), Some(0), Some(42)));
    }

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
        Message { message_id: "m1".to_owned(), contact: "ft_bob".to_owned(), outgoing, body: "hi".to_owned(), sent_at: 42, received_at: 42, state }
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
            accepted: true,
            via_circle: false,
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
            waiting: false,
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
        let waiting = ft_storage::FileRecord { waiting: true, ..record(0, false, false) };
        assert_eq!(serde_json::to_value(FileView::from(&waiting)).unwrap()["state"], "waiting", "A4: it waits for a tap");
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
        // A native call (2026-09-28): the core says when its media connected and when it was muted.
        assert_eq!(serde_json::to_value(CallEvent::new("ft_bob", "c1", CallUpdate::Connected)).unwrap(), serde_json::json!({
            "contact": "ft_bob", "call": "c1", "kind": "connected"
        }));
        assert_eq!(serde_json::to_value(CallEvent::new("ft_bob", "c1", CallUpdate::Muted { muted: true })).unwrap(), serde_json::json!({
            "contact": "ft_bob", "call": "c1", "kind": "muted", "muted": true
        }));
    }

    // The phone's own call screen (CallKit, the ongoing call notification) follows the call.
    #[test]
    fn the_native_call_screen_hears_when_a_call_connects_and_ends() {
        assert_eq!(native_screen(&CallUpdate::Connected), NativeScreen::Connected);
        assert_eq!(native_screen(&CallUpdate::Ended { outcome: CallOutcome::Answered }), NativeScreen::Ended);
        assert_eq!(native_screen(&CallUpdate::Answered { sdp: String::new() }), NativeScreen::Nothing);
        assert_eq!(native_screen(&CallUpdate::Muted { muted: true }), NativeScreen::Nothing);
        assert_eq!(native_screen(&CallUpdate::Incoming { video: false, sdp: String::new() }), NativeScreen::Nothing);
    }

    // Bug of 2026-09-29: a contact who called during another call was refused as busy, and its
    // end also ended the call going on in CallKit (and its audio) and stopped its ringing. Only the
    // history changes now; the WebView hears an end of another call, which it only logs.
    #[test]
    fn a_call_refused_as_busy_leaves_the_phone_s_call_screen_alone() {
        assert_eq!(native_screen(&CallUpdate::MissedWhileBusy), NativeScreen::Nothing);
        assert_eq!(ringing(&CallUpdate::MissedWhileBusy, false), Ring::Nothing);
        let event = serde_json::to_value(CallEvent::new("ft_carol", "c2", CallUpdate::MissedWhileBusy)).unwrap();
        assert_eq!(event, serde_json::json!({ "contact": "ft_carol", "call": "c2", "kind": "ended", "outcome": "missed" }));
        assert_eq!(update_name(&CallUpdate::MissedWhileBusy), "missed while busy");
    }

    // Bug of 2026-09-28: a suspended iPhone rang through PushKit and was answered, but its socket
    // to the router was dead and only the WebView (not running) asked to reconnect: the offer and
    // the caller's end never came. The push, the answer and the audio activation reconnect now.
    #[test]
    fn a_call_push_the_answer_and_the_audio_activation_reconnect_to_the_router() {
        // The push means the router has no live socket for this phone: whatever this phone
        // thinks, its socket is dead (iOS may suspend the app right after opening it).
        assert_eq!(reconnect_for(NativeCallEvent::Incoming), Reconnect::Now);
        assert_eq!(reconnect_for(NativeCallEvent::Answer), Reconnect::UnlessFresh);
        assert_eq!(reconnect_for(NativeCallEvent::AudioActivated(1)), Reconnect::UnlessFresh);
        assert_eq!(reconnect_for(NativeCallEvent::End), Reconnect::No);
        assert_eq!(reconnect_for(NativeCallEvent::Mute(true)), Reconnect::No);
        assert_eq!(reconnect_for(NativeCallEvent::AudioDeactivated(1)), Reconnect::No);
        assert_eq!(reconnect_for(NativeCallEvent::Visible(true)), Reconnect::No);
        assert_eq!(reconnect_for(NativeCallEvent::Orientation(3)), Reconnect::No);
        assert_eq!(reconnect_for(NativeCallEvent::VideoRequested), Reconnect::No);
        assert!(CALL_SOCKET_FRESH <= std::time::Duration::from_secs(15), "a socket from before the sleep is never fresh");
    }

    // Bug of 2026-09-29 (Android with the app closed, the iPhone with it open): a call answered on
    // the phone's own screen rang again, on the phone and in the app, and asked for a second
    // answer. A call the core answered before its offer came does not ring when the offer
    // arrives, and a ringing call stops ringing the moment it is answered, not when it connects.
    #[test]
    fn an_answered_call_never_rings() {
        assert_eq!(ringing(&CallUpdate::Incoming { video: true, sdp: String::new() }, true), Ring::Answered);
        assert_eq!(ringing(&CallUpdate::Answering, false), Ring::Answered);
        assert_eq!(native_screen(&CallUpdate::Answering), NativeScreen::Nothing);
        assert_eq!(update_name(&CallUpdate::Answering), "answering");
    }

    // The WebView hears it too, wherever it is: `answering` for a ringing call, and an incoming
    // call answered already says so, so that it never shows its ringing screen.
    #[test]
    fn the_webview_hears_that_a_call_is_answered_the_moment_it_is() {
        assert_eq!(serde_json::to_value(CallEvent::new("ft_bob", "c1", CallUpdate::Answering)).unwrap(), serde_json::json!({
            "contact": "ft_bob", "call": "c1", "kind": "answering"
        }));
        let early = CallEvent::new("ft_bob", "c1", CallUpdate::Incoming { video: false, sdp: "offer".to_owned() }).answered(true);
        assert_eq!(serde_json::to_value(early).unwrap(), serde_json::json!({
            "contact": "ft_bob", "call": "c1", "kind": "incoming", "video": false, "sdp": "offer", "answered": true
        }));
    }

    // What the core says of the call whose offer just arrived: answered already when it is not
    // ringing (it is being answered).
    #[test]
    fn a_call_is_answered_already_when_the_core_is_answering_it() {
        let current = |phase| ft_core::CurrentCall {
            call: "c1".to_owned(),
            contact: "ft_bob".to_owned(),
            video: false,
            outgoing: false,
            phase,
            offer: None,
            native: false,
            muted: false,
            connected_at: None,
            video_state: None,
        };
        assert!(answered_already(Some(&current(ft_core::CallPhase::Connecting)), "c1"));
        assert!(!answered_already(Some(&current(ft_core::CallPhase::Ringing)), "c1"));
        assert!(!answered_already(Some(&current(ft_core::CallPhase::Connecting)), "c2"), "another call");
        assert!(!answered_already(None, "c1"));
    }

    // A decline needs the offer to come, to decline it: like an answer, it asks for a fresh socket.
    #[test]
    fn a_decline_asks_for_the_router_and_is_named() {
        assert_eq!(reconnect_for(NativeCallEvent::Decline), Reconnect::UnlessFresh);
        assert_eq!(native_event_name(NativeCallEvent::Decline), "decline");
    }

    // Temporary call diagnostics (2026-09-28): state names only, nothing about who.
    #[test]
    fn call_diagnostics_name_states_only() {
        assert_eq!(native_event_name(NativeCallEvent::Incoming), "incoming");
        assert_eq!(native_event_name(NativeCallEvent::Answer), "answer");
        assert_eq!(native_event_name(NativeCallEvent::End), "end");
        assert_eq!(native_event_name(NativeCallEvent::Mute(true)), "mute");
        assert_eq!(native_event_name(NativeCallEvent::AudioActivated(1)), "audio activated");
        assert_eq!(native_event_name(NativeCallEvent::AudioDeactivated(1)), "audio deactivated");
        assert_eq!(update_name(&CallUpdate::Connected), "connected");
        assert_eq!(update_name(&CallUpdate::Ended { outcome: CallOutcome::Failed }), "ended");
        assert_eq!(update_name(&CallUpdate::Incoming { video: false, sdp: "v=0 secret".to_owned() }), "incoming");
        // 2026-09-29: what CallKit's answer did, and where the call stood, for the answers that
        // sometimes did nothing.
        assert_eq!(answer_state(true), "answered");
        assert_eq!(answer_state(false), "nothing to answer yet (waits for the offer)");
        assert_eq!(phase_name(None), "no call");
        assert_eq!(phase_name(Some((ft_core::CallPhase::Ringing, false))), "ringing");
        assert_eq!(phase_name(Some((ft_core::CallPhase::Active, true))), "active video");
    }

    // A WebView that comes up after the call started (PushKit launched the app, CallKit answered)
    // finds it in the core.
    #[test]
    fn the_current_call_reaches_a_late_webview() {
        let ringing = ft_core::CurrentCall {
            call: "c1".to_owned(),
            contact: "ft_bob".to_owned(),
            video: false,
            outgoing: false,
            phase: ft_core::CallPhase::Ringing,
            offer: Some("offer".to_owned()),
            native: false,
            muted: false,
            connected_at: None,
            video_state: None,
        };
        let no_video = serde_json::json!({
            "available": false, "camera": false, "paused": false, "facing": "front", "remote": false, "remotePaused": false
        });
        assert_eq!(serde_json::to_value(CurrentCallView::from(ringing.clone())).unwrap(), serde_json::json!({
            "call": "c1", "contact": "ft_bob", "video": no_video, "outgoing": false, "phase": "ringing",
            "offer": "offer", "native": false, "muted": false
        }));
        let active = ft_core::CurrentCall { phase: ft_core::CallPhase::Active, offer: None, native: true, muted: true, connected_at: Some(7), ..ringing.clone() };
        let view = serde_json::to_value(CurrentCallView::from(active)).unwrap();
        assert_eq!((view["phase"].as_str(), view["connectedAt"].as_i64(), view["muted"].as_bool()), (Some("active"), Some(7), Some(true)));
        assert!(view.get("offer").is_none());
    }

    // Native video (2026-09-29): `video` is the call's video as `kind: "video"` says it. A call
    // with no native video yet (it rings, or it is the WebView's) wants our camera when it is a
    // video call, so the WebView still tells a video call from a voice call.
    #[test]
    fn the_current_call_carries_its_video() {
        let ringing = ft_core::CurrentCall {
            call: "c1".to_owned(),
            contact: "ft_bob".to_owned(),
            video: true,
            outgoing: false,
            phase: ft_core::CallPhase::Ringing,
            offer: Some("offer".to_owned()),
            native: false,
            muted: false,
            connected_at: None,
            video_state: None,
        };
        let view = serde_json::to_value(CurrentCallView::from(ringing.clone())).unwrap();
        assert_eq!(view["video"]["camera"], true, "a video call wants our camera: {view}");
        assert_eq!(view["video"]["available"], false);
        let state = VideoState { available: true, camera: false, remote: true, remote_paused: true, ..VideoState::default() };
        let active = ft_core::CurrentCall { phase: ft_core::CallPhase::Active, native: true, video_state: Some(state), ..ringing };
        assert_eq!(serde_json::to_value(CurrentCallView::from(active)).unwrap()["video"], serde_json::json!({
            "available": true, "camera": false, "paused": false, "facing": "front", "remote": true, "remotePaused": true
        }));
    }

    // Native video (docs/video-nativo.md §4): the call's video reaches the WebView as a whole
    // state, never a change; the other picture's shape is only the native views' business.
    #[test]
    fn the_call_s_video_reaches_the_webview_as_a_whole_state() {
        let state = VideoState {
            available: true,
            camera: true,
            paused: false,
            facing: Facing::Back,
            remote: true,
            remote_paused: true,
            shape: Some(RemoteShape { width: 640, height: 480, rotation: 90 }),
        };
        assert_eq!(serde_json::to_value(CallEvent::new("ft_bob", "c1", CallUpdate::Video(state))).unwrap(), serde_json::json!({
            "contact": "ft_bob", "call": "c1", "kind": "video", "available": true, "camera": true, "paused": false,
            "facing": "back", "remote": true, "remotePaused": true
        }));
        assert_eq!(serde_json::to_value(CallVideoView::from(VideoState::default())).unwrap(), serde_json::json!({
            "available": false, "camera": false, "paused": false, "facing": "front", "remote": false, "remotePaused": false
        }));
        assert_eq!(native_screen(&CallUpdate::Video(state)), NativeScreen::Nothing);
        assert_eq!(ringing(&CallUpdate::Video(state), false), Ring::Nothing);
        assert_eq!(update_name(&CallUpdate::Video(state)), "video");
    }

    // A camera that could not start as the call connected (2026-09-29): the WebView says so, and
    // the phone's own call screen and ringing are not touched.
    #[test]
    fn a_camera_that_could_not_start_reaches_the_webview() {
        assert_eq!(serde_json::to_value(CallEvent::new("ft_bob", "c1", CallUpdate::CameraFailed)).unwrap(), serde_json::json!({
            "contact": "ft_bob", "call": "c1", "kind": "camera_failed"
        }));
        assert_eq!(native_screen(&CallUpdate::CameraFailed), NativeScreen::Nothing);
        assert_eq!(ringing(&CallUpdate::CameraFailed, false), Ring::Nothing);
    }

    // Call setup timings (2026-09-29): the device log gets the stages and their milliseconds.
    #[test]
    fn the_call_setup_timings_go_to_the_device_log_as_one_line() {
        use ft_core::timings::{CallStage, CallTimings};
        let timings = CallTimings { stages: vec![(CallStage::AnswerTapped, 0), (CallStage::Connected, 700)], candidates: None, link: None };
        assert_eq!(timings_line(Some(timings)), "timings: answer tapped 0 ms, connected 700 ms");
        assert_eq!(timings_line(None), "timings: no call timings");
        assert_eq!(native_event_stage(NativeCallEvent::Incoming), Some(CallStage::PushReceived));
        assert_eq!(native_event_stage(NativeCallEvent::Answer), Some(CallStage::AnswerTapped));
        assert_eq!(native_event_stage(NativeCallEvent::End), None);
        assert_eq!(native_event_stage(NativeCallEvent::AudioActivated(1)), None);
    }

    // Temporary call diagnostics: the counters say they are the audio device's (an AAudio or
    // VoiceProcessingIO stream error, which reopens the streams), not the camera's.
    #[test]
    fn the_diagnostic_counts_name_the_audio_streams() {
        let stats = ft_media::DeviceStats { capture_dropped: 2, playout_underruns: 3, errors: 1 };
        assert_eq!(device_counts(Some(stats)), "; audio capture dropped=2 playout underruns=3 audio stream errors=1");
        assert_eq!(device_counts(None), "");
    }

    // A speaker change or a camera flip may move the audio route: the log marks both, so a
    // stream error falls between two named lines.
    #[test]
    fn the_diagnostics_name_the_speaker_and_the_flip() {
        assert_eq!(speaker_name(true), "speaker on");
        assert_eq!(speaker_name(false), "speaker off");
        assert_eq!(CAMERA_SWITCHED, "camera switched");
    }

    // The WebView tells a denied camera by this word (`core_call_set_video`, and a video call's
    // start or answer).
    #[test]
    fn a_denied_camera_says_camera_denied() {
        assert_eq!(camera_or_denied(true), Ok(()));
        assert_eq!(camera_or_denied(false), Err("camera_denied".to_owned()));
    }

    fn laid_out() -> VideoLayout {
        VideoLayout {
            remote: Some(VideoRect { x: 0.0, y: 0.0, width: 390.0, height: 844.0 }),
            local: Some(VideoRect { x: 278.0, y: 594.0, width: 96.0, height: 140.0 }),
            mirror_local: true,
            local_radius: 16.0,
        }
    }

    // The layout as the WebView sends it to `core_call_video_layout` (`src/calls.ts`,
    // `VideoLayout`): CSS pixels, a picture it does not show as `null`, and `null` for the whole
    // layout when it leaves the call screen.
    #[test]
    fn the_webview_s_layout_is_read_as_it_sends_it() {
        let sent = serde_json::json!({
            "remote": { "x": 0, "y": 0, "width": 390, "height": 844 },
            "local": { "x": 278, "y": 594, "width": 96, "height": 140 },
            "mirrorLocal": true,
            "localRadius": 16
        });
        assert_eq!(serde_json::from_value::<Option<VideoLayout>>(sent).unwrap(), Some(laid_out()));
        let voice = serde_json::json!({ "remote": null, "local": null, "mirrorLocal": false, "localRadius": 0 });
        assert_eq!(serde_json::from_value::<Option<VideoLayout>>(voice).unwrap(), Some(HIDDEN));
        assert_eq!(serde_json::from_value::<Option<VideoLayout>>(serde_json::Value::Null).unwrap(), None, "left the call screen");
    }

    const LAYERS: Layers = Layers { remote: 0x10, local: 0x20 };

    // Native video (docs/video-nativo.md §4): the bridge hears when the call has video (CallKit's
    // `hasVideo`), gets the views once, where the WebView left room for them, and the other
    // picture's shape; the views go when the call ends.
    #[test]
    fn the_bridge_attaches_the_views_once_the_call_has_video_and_follows_it() {
        let mut glue = VideoGlue::default();
        let voice = VideoState { available: true, ..VideoState::default() };
        assert_eq!(glue.state("c1", &voice, Some(LAYERS)), vec![], "a voice call shows nothing");
        assert_eq!(glue.layout(Some(laid_out())), vec![], "laid out before there is video: kept for then");
        let theirs = VideoState { remote: true, ..voice };
        assert_eq!(
            glue.state("c1", &theirs, Some(LAYERS)),
            vec![BridgeVideo::CallVideo(true), BridgeVideo::Attach(LAYERS), BridgeVideo::Layout(laid_out())]
        );
        assert_eq!(glue.state("c1", &VideoState { camera: true, ..theirs }, Some(LAYERS)), vec![], "attached once, and CallKit knows");
        let shape = RemoteShape { width: 640, height: 480, rotation: 90 };
        let shaped = VideoState { shape: Some(shape), ..theirs };
        assert_eq!(glue.state("c1", &shaped, Some(LAYERS)), vec![BridgeVideo::Shape(shape)]);
        assert_eq!(glue.state("c1", &shaped, Some(LAYERS)), vec![], "the same shape again");
        let moved = VideoLayout { local: None, ..laid_out() };
        assert_eq!(glue.layout(Some(moved)), vec![BridgeVideo::Layout(moved)]);
        assert_eq!(glue.layout(None), vec![BridgeVideo::Layout(HIDDEN)], "off the call screen the views hide");
        let voice_again = VideoState { shape: Some(shape), ..voice };
        assert_eq!(glue.state("c1", &voice_again, Some(LAYERS)), vec![BridgeVideo::CallVideo(false)], "the views stay until the end");
        assert_eq!(glue.end(), vec![BridgeVideo::Detach]);
        assert_eq!(glue.state("c1", &theirs, Some(LAYERS)), vec![], "a late state of the call that ended");
        assert_eq!(glue.end(), vec![], "taken away once");
    }

    // iOS: the layers exist once the call's devices do; the views wait for them. The next call
    // starts afresh.
    #[test]
    fn the_views_wait_for_the_call_s_layers() {
        let mut glue = VideoGlue::default();
        let theirs = VideoState { remote: true, ..VideoState::default() };
        assert_eq!(glue.state("c1", &theirs, None), vec![BridgeVideo::CallVideo(true)]);
        assert_eq!(glue.state("c1", &theirs, Some(LAYERS)), vec![BridgeVideo::Attach(LAYERS)], "nothing laid out yet");
        assert_eq!(glue.end(), vec![BridgeVideo::Detach]);
        assert_eq!(glue.state("c2", &theirs, Some(LAYERS)), vec![BridgeVideo::CallVideo(true), BridgeVideo::Attach(LAYERS)]);
    }

    // The camera flip (docs/video-nativo.md §3): our preview is mirrored exactly with the front
    // camera. The glue takes the facing from the core's state, not from the WebView's last word,
    // and places the views again as soon as the camera turns (iOS mirrors only by `mirrorLocal`).
    #[test]
    fn our_preview_is_mirrored_only_with_the_front_camera() {
        let mut glue = VideoGlue::default();
        let front = VideoState { available: true, camera: true, ..VideoState::default() };
        let unmirrored = VideoLayout { mirror_local: false, ..laid_out() };
        assert_eq!(glue.layout(Some(unmirrored)), vec![]);
        assert_eq!(
            glue.state("c1", &front, Some(LAYERS)),
            vec![BridgeVideo::CallVideo(true), BridgeVideo::Attach(LAYERS), BridgeVideo::Layout(laid_out())],
            "the front camera is mirrored"
        );
        let back = VideoState { facing: Facing::Back, ..front };
        assert_eq!(glue.state("c1", &back, Some(LAYERS)), vec![BridgeVideo::Layout(unmirrored)], "flipped to the back: not mirrored, at once");
        assert_eq!(glue.layout(Some(laid_out())), vec![BridgeVideo::Layout(unmirrored)], "a late layout from before the flip");
        assert_eq!(glue.state("c1", &front, Some(LAYERS)), vec![BridgeVideo::Layout(laid_out())], "and back to the front");
        assert_eq!(glue.layout(None), vec![BridgeVideo::Layout(HIDDEN)]);
        assert_eq!(glue.state("c1", &back, Some(LAYERS)), vec![], "hidden: nothing to place");
        assert_eq!(glue.layout(Some(laid_out())), vec![BridgeVideo::Layout(unmirrored)], "shown again with the back camera");
    }

    // A call that never had video has no views to take away.
    #[test]
    fn a_call_without_video_has_no_views_to_take_away() {
        let mut glue = VideoGlue::default();
        assert_eq!(glue.state("c1", &VideoState { available: true, ..VideoState::default() }, Some(LAYERS)), vec![]);
        assert_eq!(glue.end(), vec![]);
    }

    #[test]
    fn the_call_routing_is_one_of_the_settings_words() {
        assert_eq!(call_routing("always"), Ok(ft_media::CallRouting::Always));
        assert_eq!(call_routing("direct"), Ok(ft_media::CallRouting::Direct));
        assert!(call_routing("sometimes").is_err());
    }

    // §41: the free year shows in Settings, counted on this phone.
    #[test]
    fn me_carries_the_free_period() {
        let me = MeView { id: "ft_me".to_owned(), name: "Ioan".to_owned(), mailbox: true, receipts: false, free_until: 42, auto_download: 10 };
        assert_eq!(serde_json::to_value(me).unwrap(), serde_json::json!({
            "id": "ft_me", "name": "Ioan", "mailbox": true, "receipts": false, "freeUntil": 42, "autoDownload": 10
        }));
    }

    // An incoming call rings until it is answered, declined or given up (§66); our own calls
    // being answered never ring.
    #[test]
    fn only_incoming_calls_ring() {
        assert_eq!(ringing(&CallUpdate::Incoming { video: true, sdp: String::new() }, false), Ring::Start { video: true });
        assert_eq!(ringing(&CallUpdate::Incoming { video: false, sdp: String::new() }, false), Ring::Start { video: false });
        assert_eq!(ringing(&CallUpdate::Ended { outcome: CallOutcome::Missed }, false), Ring::Stop);
        assert_eq!(ringing(&CallUpdate::Answered { sdp: String::new() }, false), Ring::Nothing);
        assert_eq!(ringing(&CallUpdate::Connected, false), Ring::Nothing);
        assert_eq!(ringing(&CallUpdate::Muted { muted: false }, false), Ring::Nothing);
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

    // M1: what the WebView names as "picked" must be in the picker's folder, nothing else.
    #[test]
    fn picked_paths_stay_in_the_pickers_folders() {
        let dir = scratch("picked");
        std::fs::create_dir_all(dir.join("uploads")).unwrap();
        std::fs::create_dir_all(dir.join("files").join("outgoing")).unwrap();
        std::fs::write(dir.join("uploads").join("photo.jpg"), b"jpg").unwrap();
        std::fs::write(dir.join("files").join("outgoing").join("made.pdf"), b"pdf").unwrap();
        std::fs::write(dir.join("flickertalk.db"), b"secret").unwrap();
        std::fs::write(dir.join("storage.key.sealed"), b"secret").unwrap();

        assert!(picked_path(&dir, &dir.join("uploads").join("photo.jpg").to_string_lossy()).is_ok());
        assert!(picked_path(&dir, &dir.join("files").join("outgoing").join("made.pdf").to_string_lossy()).is_ok());
        // What came down from the drive (2026-09-27) can be sent too.
        std::fs::create_dir_all(dir.join("files").join("drive").join("f1")).unwrap();
        std::fs::write(dir.join("files").join("drive").join("f1").join("doc.pdf"), b"pdf").unwrap();
        assert!(picked_path(&dir, &dir.join("files").join("drive").join("f1").join("doc.pdf").to_string_lossy()).is_ok());
        for refused in [
            dir.join("flickertalk.db"),
            dir.join("storage.key.sealed"),
            dir.join("uploads").join("..").join("flickertalk.db"),
            dir.join("uploads"),
            PathBuf::from("/etc/passwd"),
            dir.join("uploads").join("missing.jpg"),
        ] {
            assert!(picked_path(&dir, &refused.to_string_lossy()).is_err(), "{}", refused.display());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    // A2: what a plugin made goes out only as far as the user let it.
    #[test]
    fn a_made_file_says_whether_it_was_sent_or_staged() {
        let staged = serde_json::to_value(MadeView { sent: false, staged: Some(PickedView { path: "/p".into(), name: "a.pdf".into(), mime: "application/pdf".into(), size: 3 }) }).unwrap();
        assert_eq!(staged["sent"], false);
        assert_eq!(staged["staged"]["name"], "a.pdf");
        let sent = serde_json::to_value(MadeView { sent: true, staged: None }).unwrap();
        assert_eq!(sent, serde_json::json!({ "sent": true }));
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
        let view = serde_json::to_value(SessionView { id: "s1".to_owned(), conversations: vec![], requests: vec![], circles: vec![] }).unwrap();
        assert_eq!(view, serde_json::json!({ "id": "s1", "conversations": [], "requests": [], "circles": [] }));
    }

    // Issues app#4–#6: the contact's page shows what this phone takes from them.
    #[test]
    fn a_contact_view_carries_its_rules() {
        let rules = ft_storage::ContactRules { muted: true, accepts_chat: false, accepts_calls: true, receipts: false };
        let view = serde_json::to_value(RulesView::from(rules)).unwrap();
        assert_eq!(view, serde_json::json!({ "muted": true, "acceptsChat": false, "acceptsCalls": true, "receipts": false }));
    }
}
