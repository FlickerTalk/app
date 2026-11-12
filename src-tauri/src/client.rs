//! Views sent to the WebView must never carry keys or the route capability.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use anyhow::Context;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use ft_core::online::{self, Online};
use ft_core::moving::MoveUpdate;
use ft_core::{CallPhase, CallUpdate, Core, Event, PresentedBy, TurnGrant};
use ft_media::{CallRouting, Facing, Layers, RemoteShape, VideoState};
use ft_storage::{CallOutcome, CallRecord, Conversation, FileRecord, Message, MessageState, Store};
use ft_webrtc::SessionConfig;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_ft_platform::{CallRoute, NativeCallEvent, PlatformExt, VideoLayout};

use crate::push_core::{may_start_from_push, CallScreen, Host, PushSide};

pub const ROUTER: &str = "https://api.flickertalk.com";
pub const CHANGED_EVENT: &str = "ft://changed";
pub const TYPING_EVENT: &str = "ft://typing";
pub const PLUGINS_EVENT: &str = "ft://plugins";
pub const PLAN_EVENT: &str = "ft://plan";

pub fn state_name(state: MessageState) -> &'static str {
    match state {
        MessageState::NotSent => "unsent",
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
    #[serde(skip_serializing_if = "Option::is_none")]
    quote: Option<QuoteView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reactions: Option<ReactionsView>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pinned: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    edited: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    deleted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    scheduled_for: Option<i64>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    update_pending: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    notes: Vec<NoteView>,
}

#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct NoteView {
    plugin: String,
    text: String,
}

#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct ReactionsView {
    #[serde(skip_serializing_if = "Option::is_none")]
    mine: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    theirs: Option<String>,
}

impl From<&ft_storage::Reactions> for ReactionsView {
    fn from(reactions: &ft_storage::Reactions) -> Self {
        Self { mine: reactions.mine.clone(), theirs: reactions.theirs.clone() }
    }
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
            quote: None,
            reactions: None,
            pinned: false,
            edited: false,
            deleted: false,
            scheduled_for: None,
            update_pending: false,
            notes: Vec::new(),
        }
    }

    pub fn noted<'a>(mut self, notes: impl Iterator<Item = &'a ft_storage::MessageNote>) -> Self {
        self.notes = notes.map(|note| NoteView { plugin: note.plugin.clone(), text: note.text.clone() }).collect();
        self
    }

    pub fn for_later(mut self, send_at: Option<i64>) -> Self {
        self.scheduled_for = send_at;
        self
    }

    pub fn pinned(mut self, pinned: bool) -> Self {
        self.pinned = pinned;
        self
    }

    pub fn marked(mut self, edited: bool, deleted: bool) -> Self {
        self.edited = edited;
        self.deleted = deleted;
        self
    }

    pub fn updating(mut self, waiting: bool) -> Self {
        self.update_pending = waiting;
        self
    }

    pub fn answering(mut self, quote: QuoteView) -> Self {
        self.quote = Some(quote);
        self
    }

    pub fn reacted(mut self, reactions: Option<&ft_storage::Reactions>) -> Self {
        self.reactions = reactions.map(ReactionsView::from);
        self
    }
}

#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct QuoteView {
    id: String,
    text: String,
    mine: bool,
    kind: &'static str,
}

impl QuoteView {
    pub fn of(id: &str, quoted: Option<&Message>, file: Option<&FileRecord>) -> Self {
        match (quoted, file) {
            (Some(message), Some(file)) => Self { id: id.to_owned(), text: file.name.clone(), mine: message.outgoing, kind: "file" },
            (Some(message), None) => Self { id: id.to_owned(), text: message.body.clone(), mine: message.outgoing, kind: "text" },
            (None, _) => Self { id: id.to_owned(), text: String::new(), mine: false, kind: "gone" },
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    name: String,
    size: i64,
    mime: String,
    progress: f64,
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    id: String,
    conversations: Vec<ConversationView>,
    requests: Vec<ConversationView>,
    circles: Vec<CircleView>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CircleView {
    id: String,
    name: String,
    members: Vec<CircleMemberView>,
    admin: bool,
    admins_only: bool,
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
    me: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CircleMessageView {
    id: String,
    outgoing: bool,
    sender: String,
    sender_name: String,
    kind: String,
    text: String,
    sent_at: i64,
    state: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    notes: Vec<NoteView>,
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
            notes: Vec::new(),
        }
    }

    pub fn noted<'a>(mut self, notes: impl Iterator<Item = &'a ft_storage::MessageNote>) -> Self {
        self.notes = notes.map(|note| NoteView { plugin: note.plugin.clone(), text: note.text.clone() }).collect();
        self
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeView {
    id: String,
    name: String,
    mailbox: bool,
    receipts: bool,
    free_until: i64,
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
    keep_for: i64,
    burn_after_read: i64,
    rules: RulesView,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct RulesView {
    muted: bool,
    accepts_chat: bool,
    accepts_calls: bool,
    receipts: bool,
    /// Older pages don't send this field.
    #[serde(default = "yes")]
    typing: bool,
}

fn yes() -> bool {
    true
}

impl From<ft_storage::ContactRules> for RulesView {
    fn from(rules: ft_storage::ContactRules) -> Self {
        Self { muted: rules.muted, accepts_chat: rules.accepts_chat, accepts_calls: rules.accepts_calls, receipts: rules.receipts, typing: rules.typing }
    }
}

impl From<RulesView> for ft_storage::ContactRules {
    fn from(rules: RulesView) -> Self {
        Self { muted: rules.muted, accepts_chat: rules.accepts_chat, accepts_calls: rules.accepts_calls, receipts: rules.receipts, typing: rules.typing }
    }
}

#[derive(Clone, Serialize)]
struct TypingView {
    contact: String,
}

#[derive(Clone, Serialize)]
struct Changed {
    contact: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    circle: Option<String>,
}

pub const CALL_EVENT: &str = "ft://call";
#[derive(Clone, Serialize)]
pub struct CallEvent {
    contact: String,
    call: String,
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    video: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sdp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    outcome: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    muted: Option<bool>,
    #[serde(flatten)]
    view: Option<CallVideoView>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    answered: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    plugin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    by: Option<&'static str>,
    #[serde(rename = "canPresent", skip_serializing_if = "Option::is_none")]
    can_present: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    route: Option<&'static str>,
}

impl CallEvent {
    pub fn new(contact: &str, call: &str, update: CallUpdate) -> Self {
        let view = match update {
            CallUpdate::Video(state) => Some(CallVideoView::from(state)),
            _ => None,
        };
        let (plugin, file, by) = match &update {
            CallUpdate::Presenting { plugin, file, by } => (plugin.clone(), file.map(|file| file.to_string()), by.map(presented_by)),
            _ => (None, None, None),
        };
        let (kind, video, sdp, outcome, muted) = match update {
            CallUpdate::Incoming { video, sdp } => ("incoming", Some(video), Some(sdp), None, None),
            CallUpdate::Answered { sdp } => ("answered", None, Some(sdp), None, None),
            CallUpdate::Answering => ("answering", None, None, None, None),
            CallUpdate::Ended { outcome } => ("ended", None, None, Some(outcome.as_str()), None),
            CallUpdate::Connected => ("connected", None, None, None, None),
            CallUpdate::Muted { muted } => ("muted", None, None, None, Some(muted)),
            // The WebView just logs this one, it only follows the call on screen
            CallUpdate::MissedWhileBusy => ("ended", None, None, Some(CallOutcome::Missed.as_str()), None),
            CallUpdate::Video(_) => ("video", None, None, None, None),
            CallUpdate::CameraFailed => ("camera_failed", None, None, None, None),
            CallUpdate::Presenting { .. } => ("presenting", None, None, None, None),
        };
        Self {
            contact: contact.to_owned(),
            call: call.to_owned(),
            kind,
            video,
            sdp,
            outcome,
            muted,
            view,
            answered: false,
            plugin,
            file,
            by,
            can_present: None,
            route: None,
        }
    }

    pub fn route(contact: &str, call: &str, route: CallRoute) -> Self {
        Self {
            contact: contact.to_owned(),
            call: call.to_owned(),
            kind: "route",
            video: None,
            sdp: None,
            outcome: None,
            muted: None,
            view: None,
            answered: false,
            plugin: None,
            file: None,
            by: None,
            can_present: None,
            route: Some(route.as_str()),
        }
    }

    pub fn answered(self, answered: bool) -> Self {
        Self { answered, ..self }
    }

    pub fn can_present(self, can_present: bool) -> Self {
        Self { can_present: Some(can_present), ..self }
    }
}

fn presented_by(by: PresentedBy) -> &'static str {
    match by {
        PresentedBy::Me => "me",
        PresentedBy::Them => "them",
    }
}

fn can_present_now(current: Option<&ft_core::CurrentCall>, call: &str) -> bool {
    current.is_some_and(|current| current.call == call && current.can_present)
}

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
        | CallUpdate::CameraFailed
        | CallUpdate::Presenting { .. } => NativeScreen::Nothing,
    }
}

#[derive(Default)]
pub struct CallRoutes {
    last: Option<(String, CallRoute)>,
}

impl CallRoutes {
    pub fn set(&mut self, call: &str, route: CallRoute) {
        self.last = Some((call.to_owned(), route));
    }

    pub fn of(&self, call: &str) -> Option<CallRoute> {
        self.last.as_ref().filter(|(said, _)| said == call).map(|(_, route)| *route)
    }
}

static CALL_ROUTES: Mutex<CallRoutes> = Mutex::new(CallRoutes { last: None });

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentCallView {
    call: String,
    contact: String,
    video: CallVideoView,
    outgoing: bool,
    phase: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    offer: Option<String>,
    native: bool,
    muted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    connected_at: Option<i64>,
    can_present: bool,
    presenting: Option<PresentingView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    route: Option<&'static str>,
}

impl CurrentCallView {
    pub fn route(self, route: Option<CallRoute>) -> Self {
        Self { route: route.map(CallRoute::as_str), ..self }
    }
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
            can_present: current.can_present,
            presenting: current.presenting.map(PresentingView::from),
            route: None,
        }
    }
}

#[derive(Serialize)]
pub struct PresentingView {
    plugin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<String>,
    by: &'static str,
}

impl From<ft_core::Presenting> for PresentingView {
    fn from(presenting: ft_core::Presenting) -> Self {
        Self { plugin: presenting.plugin, file: presenting.file.map(|file| file.to_string()), by: presented_by(presenting.by) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallVideoView {
    available: bool,
    camera: bool,
    paused: bool,
    facing: &'static str,
    remote: bool,
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

const CAMERA_DENIED: &str = "camera_denied";

pub fn camera_or_denied(granted: bool) -> Result<(), String> {
    if granted {
        Ok(())
    } else {
        Err(CAMERA_DENIED.to_owned())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BridgeVideo {
    CallVideo(bool),
    Attach(Layers),
    Layout(VideoLayout),
    Shape(RemoteShape),
    Detach,
}

pub const HIDDEN: VideoLayout = VideoLayout { remote: None, local: None, mirror_local: false, local_radius: 0.0 };

#[derive(Default)]
pub struct VideoGlue {
    call: Option<String>,
    ended: Option<String>,
    has_video: bool,
    attached: bool,
    shape: Option<RemoteShape>,
    layout: Option<VideoLayout>,
    facing: Facing,
}

impl VideoGlue {
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
            told.extend(self.placed().map(BridgeVideo::Layout));
        }
        if self.attached && state.shape != self.shape {
            self.shape = state.shape;
            told.extend(state.shape.map(BridgeVideo::Shape));
        }
        told
    }

    pub fn layout(&mut self, layout: Option<VideoLayout>) -> Vec<BridgeVideo> {
        self.layout = layout;
        if self.attached {
            vec![BridgeVideo::Layout(self.placed().unwrap_or(HIDDEN))]
        } else {
            Vec::new()
        }
    }

    fn placed(&self) -> Option<VideoLayout> {
        self.layout.map(|layout| VideoLayout { mirror_local: self.facing == Facing::Front, ..layout })
    }

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

pub fn call_routing(routing: &str) -> Result<CallRouting, String> {
    routing.parse().map_err(failed)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ring {
    Start { video: bool },
    Answered,
    Stop,
    Nothing,
}

pub fn ringing(update: &CallUpdate, answered: bool) -> Ring {
    match update {
        CallUpdate::Incoming { .. } if answered => Ring::Answered,
        CallUpdate::Answering => Ring::Answered,
        CallUpdate::Incoming { video, .. } => Ring::Start { video: *video },
        CallUpdate::Ended { .. } => Ring::Stop,
        CallUpdate::Answered { .. } => Ring::Nothing,
        CallUpdate::Connected
        | CallUpdate::Muted { .. }
        | CallUpdate::MissedWhileBusy
        | CallUpdate::Video(_)
        | CallUpdate::CameraFailed
        | CallUpdate::Presenting { .. } => Ring::Nothing,
    }
}

#[derive(Serialize)]
pub struct IceServer {
    urls: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    credential: Option<String>,
}

pub const MOVE_EVENT: &str = "ft://move";

#[derive(Clone, Serialize)]
pub struct MoveEvent {
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

fn database_files(dir: &Path) -> [PathBuf; 3] {
    [dir.join(DATABASE), dir.join(format!("{DATABASE}-wal")), dir.join(format!("{DATABASE}-shm"))]
}

pub fn apply_move(dir: &Path) -> std::io::Result<bool> {
    let incoming = dir.join(MOVE_DIR);
    let Some((database, key)) = ft_core::moving::received_move(&incoming) else { return Ok(false) };
    for file in database_files(dir) {
        let _ = std::fs::remove_file(file);
    }
    std::fs::rename(database, dir.join(DATABASE))?;
    // Drop the old sealed key, the moved one replaces it and gets sealed on the next start
    let _ = std::fs::remove_file(dir.join(SEALED_KEY_FILE));
    std::fs::write(dir.join(KEY_FILE), key)?;
    #[cfg(unix)]
    std::fs::set_permissions(dir.join(KEY_FILE), std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    std::fs::remove_dir_all(incoming)?;
    Ok(true)
}

pub fn erase(dir: &Path) -> std::io::Result<()> {
    for file in database_files(dir).into_iter().chain([dir.join(KEY_FILE), dir.join(SEALED_KEY_FILE)]) {
        match std::fs::remove_file(file) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    for folder in ["files", MOVE_DIR, "plugins", "vault", "uploads"].map(|name| dir.join(name)) {
        match std::fs::remove_dir_all(folder) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    Ok(())
}

pub fn wipe(dir: &Path, vault: Option<&dyn KeyVault>) -> anyhow::Result<()> {
    erase(dir)?;
    if let Some(vault) = vault {
        vault.forget()?;
    }
    Ok(())
}

fn first_page(mut url: tauri::Url) -> tauri::Url {
    url.set_path("/");
    url.set_query(None);
    url.set_fragment(None);
    url
}

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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallView {
    id: String,
    contact: String,
    name: String,
    outgoing: bool,
    video: bool,
    started_at: i64,
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

pub trait KeyVault {
    fn seal(&self, key: &[u8; 32]) -> anyhow::Result<Vec<u8>>;
    fn open(&self, sealed: &[u8]) -> anyhow::Result<[u8; 32]>;
    fn forget(&self) -> anyhow::Result<()>;
}

const SEALED_KEY_FILE: &str = "storage.key.sealed";

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    std::io::Write::write_all(&mut options.open(path)?, bytes)
}

/// If the sealed key won't open, fail. Making a new key would lose the identity.
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

/// The WebView can only name files in these folders, anything else is refused.
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

pub fn new_upload_id() -> String {
    rand::random::<[u8; 16]>().iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn upload_path(dir: &Path, id: &str) -> Result<PathBuf, String> {
    let valid = id.len() == 32 && id.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c));
    if !valid {
        return Err("invalid upload".to_owned());
    }
    Ok(dir.join("files").join("outgoing").join(id))
}

pub fn openable(file: Option<FileRecord>, outgoing: bool) -> Result<FileRecord, String> {
    match file {
        Some(file) if outgoing || file.complete => Ok(file),
        Some(_) => Err("the file has not arrived yet".to_owned()),
        None => Err("not a file".to_owned()),
    }
}

#[derive(Default)]
pub struct Client {
    dir: OnceLock<PathBuf>,
    app: OnceLock<AppHandle>,
    vault: OnceLock<Box<dyn KeyVault + Send + Sync>>,
    web: ft_core::Web,
    video: Arc<Mutex<VideoGlue>>,
}

impl Client {
    pub fn web(&self) -> &ft_core::Web {
        &self.web
    }
}

struct Running {
    online: Option<Arc<Online>>,
    host: Option<Arc<Host<AppHandle>>>,
    stopped: bool,
}

static RUNNING: tokio::sync::Mutex<Running> = tokio::sync::Mutex::const_new(Running { online: None, host: None, stopped: false });

struct PlatformVault(AppHandle);

impl KeyVault for PlatformVault {
    fn seal(&self, key: &[u8; 32]) -> anyhow::Result<Vec<u8>> {
        Ok(self.0.platform().seal_key(key)?)
    }

    fn open(&self, sealed: &[u8]) -> anyhow::Result<[u8; 32]> {
        Ok(self.0.platform().open_key(sealed)?)
    }

    fn forget(&self) -> anyhow::Result<()> {
        Ok(self.0.platform().forget_key()?)
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

    async fn online(&self) -> Result<Arc<Online>, String> {
        let mut running = RUNNING.lock().await;
        if running.stopped {
            return Err("the app is starting again".to_owned());
        }
        if let Some(online) = running.online.clone() {
            if let (Some(host), Some(app)) = (running.host.clone(), self.app.get()) {
                if host.adopt(app.clone()) {
                    if let Some(push) = host.push_side() {
                        push.adopted();
                    }
                    forget_push_events();
                    self.attach(app, &online, &host, false).await;
                }
            }
            return Ok(online);
        }
        let dir = self.dir.get().ok_or_else(|| "the app is not set up yet".to_owned())?;
        let key = opened_key(dir, self.vault.get().map(|vault| vault.as_ref() as &dyn KeyVault)).map_err(|error| error.to_string())?;
        let online = Arc::new(boot(dir, key).await.map_err(|error| error.to_string())?);
        if let Some(app) = self.app.get() {
            let host = Arc::new(Host::for_app(app.clone()));
            self.attach(app, &online, &host, true).await;
            running.host = Some(host);
        }
        running.online = Some(online.clone());
        Ok(online)
    }

    async fn stop(&self) {
        let stopped = {
            let mut running = RUNNING.lock().await;
            running.stopped = true;
            running.host = None;
            running.online.take()
        };
        if let Some(online) = stopped {
            online.shutdown().await;
        }
    }

    fn wipe(&self) -> Result<(), String> {
        wipe(self.dir()?, self.vault.get().map(|vault| vault.as_ref() as &dyn KeyVault)).map_err(failed)
    }

    async fn core(&self) -> Result<Arc<Core>, String> {
        Ok(self.online().await?.core.clone())
    }

    fn dir(&self) -> Result<&Path, String> {
        self.dir.get().map(PathBuf::as_path).ok_or_else(|| "the app is not set up yet".to_owned())
    }

    async fn attach(&self, app: &AppHandle, online: &Online, host: &Arc<Host<AppHandle>>, follow: bool) {
        let Ok(dir) = self.dir() else { return };
        let core_for_vault = online.core.clone();
        tauri::async_runtime::spawn(async move {
            if core_for_vault.vault_reopen().await.is_ok() {
                let _ = core_for_vault.vault_run_queue().await;
            }
        });
        update_installed_plugins(&online.core).await;
        if let Ok(until) = app.platform().subscription() {
            let _ = online.core.set_entitlement(until).await;
        }
        let (said, mut store_said) = tokio::sync::mpsc::unbounded_channel::<i64>();
        app.platform().listen_entitlements(move |until| {
            let _ = said.send(until);
        });
        let core_for_store = online.core.clone();
        tauri::async_runtime::spawn(async move {
            while let Some(until) = store_said.recv().await {
                let _ = core_for_store.set_entitlement(until).await;
            }
        });
        tauri::async_runtime::spawn(ft_core::Core::watch_plan(
            Arc::downgrade(&online.core),
            Arc::new(ShopWord(Arc::new(AppShop(app.clone())))),
        ));
        refresh_served_plugins(app, &online.core, dir).await;
        look_for_updates(app, online.core.clone(), dir);
        if let Ok(week) = online.core.quiet_week().await {
            let _ = app.platform().set_quiet_hours(&week);
        }
        let _ = app.platform().set_open_slots(&online.core.open_slots());
        // Alarms can get lost on a reboot or an update, so resend them all on every start
        sync_reminders(app, &online.core).await;

        // The views have to go before the call's video devices (on iOS the layers belong to them).
        // The core calls this right before releasing them.
        let (glue, app_for_views) = (self.video.clone(), app.clone());
        online.core.set_video_detach(Some(Arc::new(move || tell_bridge(&app_for_views, &glue, VideoGlue::end))));
        listen_native_calls(app, online);
        if follow {
            follow_events(online, host.clone());
        }
    }
}

fn opened_key(dir: &Path, vault: Option<&dyn KeyVault>) -> anyhow::Result<[u8; 32]> {
    apply_move(dir)?;
    storage_key(dir, vault)
}

// plus the pickers' old folder outside files/
fn transit_folders(dir: &Path) -> Vec<PathBuf> {
    let files = dir.join("files");
    ft_core::files::TRANSIT_FOLDERS.iter().map(|name| files.join(name)).chain([dir.join("uploads")]).collect()
}

static SWEPT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Sweep only on the first start in this process. On later starts the WebView may still hold a
/// staged or just-picked file.
async fn open_store(dir: &Path, swept: &std::sync::atomic::AtomicBool) -> anyhow::Result<Store> {
    let sweeps = !swept.swap(true, std::sync::atomic::Ordering::SeqCst);
    let store = Store::open(&dir.join(DATABASE)).await?;
    if sweeps {
        let _ = ft_core::files::sweep_orphans(&store, &dir.join("files"), transit_folders(dir)).await;
    }
    Ok(store)
}

async fn boot(dir: &Path, key: [u8; 32]) -> anyhow::Result<Online> {
    let store = open_store(dir, &SWEPT).await?;
    let online = online::start(store, key, ROUTER, SessionConfig::default()).await?;
    online.core.set_files_dir(dir.join("files"));
    online.core.set_move_dir(dir.join(MOVE_DIR));
    online.core.set_plugins_dir(dir.join("plugins"));
    online.core.set_vault_dir(dir.join("vault"));
    online.core.set_cloud(Arc::new(ft_core::vault::GoogleCloud));
    Ok(online)
}

struct BridgeScreen(AppHandle);

impl CallScreen for BridgeScreen {
    fn ring(&self, caller: &str, video: bool, muted: bool) {
        let _ = self.0.platform().start_ringing(caller, video, muted);
    }

    fn stop_ringing(&self) {
        let _ = self.0.platform().stop_ringing();
    }

    fn refused(&self) {
        let _ = self.0.platform().call_refused();
    }

    fn answering(&self, caller: &str, video: bool) {
        let _ = self.0.platform().call_answering(caller, video);
    }

    fn connected(&self) {
        let _ = self.0.platform().call_connected();
    }

    fn ended(&self) {
        let _ = self.0.platform().call_ended();
    }
}

fn bridge_screen(app: &AppHandle) -> Arc<dyn CallScreen> {
    Arc::new(BridgeScreen(app.clone()))
}

fn follow_events(online: &Online, host: Arc<Host<AppHandle>>) {
    let mut events = online.core.events();
    let stopped = online.stopped();
    let router_for_events = online.router.clone();
    let core_for_events = online.core.clone();
    tauri::async_runtime::spawn(async move {
        use crate::core_events::{self, EverythingChanged, Next};
        tokio::pin!(stopped);
        loop {
            let event = match core_events::next(&mut events, &mut stopped).await {
                Next::Event(event) => event,
                Next::Lost => {
                    if let Some(app) = host.app().cloned() {
                        let _ = app.emit(CHANGED_EVENT, EverythingChanged::default());
                        let _ = app.emit(VAULT_EVENT, ());
                        let _ = app.emit(PLUGINS_EVENT, ());
                        sync_reminders(&app, &core_for_events).await;
                    }
                    continue;
                }
                Next::End => break,
            };
            host.saw(&event, std::time::Instant::now());
            let app = host.app().cloned();
            let plugins = matches!(event, Event::PluginsChanged);
            let (contact, circle) = match event {
                Event::MessagesChanged { contact } => (Some(contact), None),
                Event::CircleMessagesChanged { circle } => (None, Some(circle)),
                Event::ContactsChanged | Event::ConnectionChanged { .. } | Event::PluginsChanged | Event::CirclesChanged => (None, None),
                Event::CallRefused => {
                    if let Some(screen) = host.screen(bridge_screen) {
                        screen.refused();
                    }
                    continue;
                }
                Event::Call { contact, call, update } => {
                    let current = match update {
                        CallUpdate::Incoming { .. } | CallUpdate::Answering | CallUpdate::Connected => core_for_events.current_call().await.ok().flatten(),
                        _ => None,
                    };
                    let answered = matches!(update, CallUpdate::Incoming { .. }) && answered_already(current.as_ref(), &call);
                    if let Some(screen) = host.screen(bridge_screen) {
                        match ringing(&update, answered) {
                            Ring::Start { video } => {
                                let stored = core_for_events.store().contact(&contact).await.ok().flatten();
                                let name = stored.as_ref().map(|stored| stored.name.clone()).unwrap_or_default();
                                let muted = stored.is_some_and(|stored| stored.rules.muted);
                                screen.ring(&name, video, muted);
                            }
                            Ring::Stop => screen.stop_ringing(),
                            Ring::Answered => {
                                let name = core_for_events.store().contact(&contact).await.ok().flatten().map(|stored| stored.name).unwrap_or_default();
                                let video = current.as_ref().is_some_and(|current| current.video);
                                screen.answering(&name, video);
                            }
                            Ring::Nothing => {}
                        }
                        match native_screen(&update) {
                            NativeScreen::Connected => screen.connected(),
                            NativeScreen::Ended => screen.ended(),
                            NativeScreen::Nothing => {}
                        }
                    }
                    let Some(app) = app else { continue };
                    if let CallUpdate::Video(state) = update.clone() {
                        let (app, glue, call) = (app.clone(), app.state::<Client>().video.clone(), call.clone());
                        let _ = tauri::async_runtime::spawn_blocking(move || {
                            tell_bridge(&app, &glue, |glue| glue.state(&call, &state, video_layers()))
                        })
                        .await;
                    }
                    let connected = matches!(update, CallUpdate::Connected);
                    let mut event = CallEvent::new(&contact, &call, update).answered(answered);
                    if connected {
                        event = event.can_present(can_present_now(current.as_ref(), &call));
                    }
                    let _ = app.emit(CALL_EVENT, event);
                    continue;
                }
                other => {
                    let Some(app) = app else { continue };
                    match other {
                        Event::Typing { contact } => {
                            let _ = app.emit(TYPING_EVENT, TypingView { contact });
                        }
                        Event::PluginEvent { plugin, contact, data } => {
                            let _ = app.emit(PLUGIN_EVENT, PluginEventView { plugin, contact, data: BASE64.encode(data) });
                        }
                        Event::RemindersChanged => sync_reminders(&app, &core_for_events).await,
                        Event::VaultChanged => {
                            let _ = app.emit(VAULT_EVENT, ());
                        }
                        Event::PlanChanged => {
                            if let Ok(dir) = app.state::<Client>().dir() {
                                refresh_served_plugins(&app, &core_for_events, dir).await;
                            }
                            let _ = app.emit(PLAN_EVENT, ());
                        }
                        Event::VaultProgress { done, total } => {
                            let _ = app.emit(VAULT_PROGRESS_EVENT, VaultProgressView { done, total });
                        }
                        Event::Move(update) => {
                            let _ = app.emit(MOVE_EVENT, MoveEvent::from(update.clone()));
                            after_move(&app, &router_for_events, update);
                        }
                        _ => {}
                    }
                    continue;
                }
            };
            if let Some(app) = app {
                let _ = app.emit(CHANGED_EVENT, Changed { contact, circle });
                if plugins {
                    let _ = app.emit(PLUGINS_EVENT, ());
                }
            }
        }
    });
}

fn listen_native_calls(app: &AppHandle, online: &Online) {
    let _ = app.platform().set_open_app_on_answer(open_app_on_answer(option_env!("FT_IOS_OPEN_APP_ON_ANSWER")));
    let events = native_call_queue(online, Some(app.clone()));
    app.platform().listen_calls(move |event| {
        let _ = events.send(event);
    });
}

/// Events are handled in order because an audio deactivation and activation must not swap. Slow
/// work (answering, hanging up) is spawned.
fn native_call_queue(online: &Online, app: Option<AppHandle>) -> tokio::sync::mpsc::UnboundedSender<NativeCallEvent> {
    let (core, router, lifecycle) = (online.core.clone(), online.router.clone(), online.lifecycle());
    let (events, mut queue) = tokio::sync::mpsc::unbounded_channel::<NativeCallEvent>();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = queue.recv().await {
            if app.is_none() && !push_core_handles(event) {
                continue;
            }
            // await it, iOS may suspend the app moments after it leaves
            if let Some(foreground) = foreground_of(event) {
                lifecycle.set_foreground(foreground).await;
            }
            let reconnect = reconnect_for(event);
            if reconnect != Reconnect::No && !lifecycle.woken().await {
                match reconnect {
                    Reconnect::Now => router.reconnect_now(),
                    Reconnect::UnlessFresh => router.reconnect_unless_fresh(CALL_SOCKET_FRESH),
                    Reconnect::No => {}
                }
            }
            let core = core.clone();
            match event {
                // reconnecting above is all a push needs, the core rings when the offer comes
                NativeCallEvent::Incoming => {}
                NativeCallEvent::Answer => {
                    let Some(app) = app.clone() else { continue };
                    tauri::async_runtime::spawn(async move {
                        if !microphone(&app).await {
                            let _ = fail_current_call(&core).await;
                            return;
                        }
                        let video_call = ringing_video_call(&core).await;
                        let camera_allowed = video_call.is_none() || camera(&app).await;
                        if let (Ok(true), Some(call), false) = (core.answer_ringing_call().await, video_call, camera_allowed) {
                            let _ = core.set_call_camera(&call, false).await;
                        }
                    });
                }
                NativeCallEvent::End => {
                    tauri::async_runtime::spawn(async move {
                        let _ = core.end_current_call().await;
                    });
                }
                NativeCallEvent::Decline => {
                    tauri::async_runtime::spawn(async move {
                        let _ = core.decline_ringing_call().await;
                    });
                }
                NativeCallEvent::Mute(muted) => {
                    let _ = core.mute_current_call(muted).await;
                }
                NativeCallEvent::AudioActivated(generation) => {
                    let _ = core.set_call_audio_session(true, generation).await;
                }
                NativeCallEvent::AudioDeactivated(generation) => {
                    let _ = core.set_call_audio_session(false, generation).await;
                }
                NativeCallEvent::Visible(visible) => {
                    let _ = core.set_app_visible(visible).await;
                }
                NativeCallEvent::Orientation(raw) => ft_media::views::set_orientation(raw),
                NativeCallEvent::Route(route) => {
                    let Some(app) = app.clone() else { continue };
                    let Ok(Some(current)) = core.current_call().await else { continue };
                    CALL_ROUTES.lock().unwrap_or_else(PoisonError::into_inner).set(&current.call, route);
                    let _ = app.emit(CALL_EVENT, CallEvent::route(&current.contact, &current.call, route));
                }
                NativeCallEvent::VideoRequested => {
                    let Some(app) = app.clone() else { continue };
                    tauri::async_runtime::spawn(async move {
                        if camera(&app).await {
                            let _ = core.request_call_video().await;
                        }
                    });
                }
            }
        }
    });
    events
}

fn push_core_handles(event: NativeCallEvent) -> bool {
    matches!(event, NativeCallEvent::Incoming | NativeCallEvent::Decline | NativeCallEvent::End)
}

static PUSH_EVENTS: Mutex<Option<tokio::sync::mpsc::UnboundedSender<NativeCallEvent>>> = Mutex::new(None);

fn forget_push_events() {
    PUSH_EVENTS.lock().unwrap_or_else(PoisonError::into_inner).take();
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn push_event(event: NativeCallEvent) -> bool {
    PUSH_EVENTS.lock().unwrap_or_else(PoisonError::into_inner).as_ref().is_some_and(|events| events.send(event).is_ok())
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
const PUSH_CORE_LOOK: std::time::Duration = std::time::Duration::from_secs(1);

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub async fn start_for_push(dir: PathBuf, vault: Box<dyn KeyVault + Send + Sync>, side: Arc<dyn PushSide>) {
    let mut running = RUNNING.lock().await;
    if running.stopped {
        side.stopped("stopped");
        return;
    }
    if running.online.is_some() {
        if let Some(host) = &running.host {
            host.pushed(std::time::Instant::now());
        }
        return;
    }
    if !may_start_from_push(&dir, SEALED_KEY_FILE, DATABASE) {
        side.stopped("no identity");
        return;
    }
    let Ok(key) = opened_key(&dir, Some(vault.as_ref())) else {
        side.stopped("failed");
        return;
    };
    let online = match boot(&dir, key).await {
        Ok(online) => Arc::new(online),
        Err(_) => {
            side.stopped("failed");
            return;
        }
    };
    let host = Arc::new(Host::for_push(side.clone(), std::time::Instant::now()));
    follow_events(&online, host.clone());
    *PUSH_EVENTS.lock().unwrap_or_else(PoisonError::into_inner) = Some(native_call_queue(&online, None));
    running.online = Some(online.clone());
    running.host = Some(host.clone());
    drop(running);
    side.up();
    tauri::async_runtime::spawn(watch_push_core(online, host));
}

/// Runs under the lock so an app opening meanwhile waits, then starts its own core.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
async fn watch_push_core(online: Arc<Online>, host: Arc<Host<AppHandle>>) {
    loop {
        tokio::time::sleep(PUSH_CORE_LOOK).await;
        let mut running = RUNNING.lock().await;
        let ours = running.online.as_ref().is_some_and(|running| Arc::ptr_eq(running, &online));
        if !ours || host.app().is_some() {
            return;
        }
        let going_on = online.core.current_call().await.ok().flatten().is_some();
        let Some(stop) = host.stops(std::time::Instant::now(), going_on) else { continue };
        running.online = None;
        running.host = None;
        forget_push_events();
        online.shutdown().await;
        drop(running);
        if let Some(side) = host.push_side() {
            side.stopped(stop.word());
        }
        return;
    }
}

const CALL_SOCKET_FRESH: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reconnect {
    Now,
    UnlessFresh,
    No,
}

/// A call push means iOS dropped the suspended app's socket. Answer and audio activation keep a
/// socket that was just opened, dropping it again could lose the call's offer.
fn reconnect_for(event: NativeCallEvent) -> Reconnect {
    match event {
        NativeCallEvent::Incoming => Reconnect::Now,
        NativeCallEvent::Answer | NativeCallEvent::Decline | NativeCallEvent::AudioActivated(_) => Reconnect::UnlessFresh,
        NativeCallEvent::End
        | NativeCallEvent::Mute(_)
        | NativeCallEvent::AudioDeactivated(_)
        | NativeCallEvent::Visible(_)
        | NativeCallEvent::Orientation(_)
        | NativeCallEvent::VideoRequested
        | NativeCallEvent::Route(_) => Reconnect::No,
    }
}

/// The WebView's visibilitychange can come too late, or never, on a phone about to suspend the app.
fn foreground_of(event: NativeCallEvent) -> Option<bool> {
    match event {
        NativeCallEvent::Visible(visible) => Some(visible),
        _ => None,
    }
}

fn open_app_on_answer(flag: Option<&str>) -> bool {
    matches!(flag.map(str::trim), Some("1" | "true"))
}

/// Not on the main thread, the bridge would deadlock there.
async fn microphone(app: &AppHandle) -> bool {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || app.platform().request_microphone()).await.ok().and_then(Result::ok).unwrap_or(false)
}

async fn camera(app: &AppHandle) -> bool {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || app.platform().request_camera()).await.ok().and_then(Result::ok).unwrap_or(false)
}

async fn ringing_video_call(core: &Core) -> Option<String> {
    let current = core.current_call().await.ok().flatten()?;
    (current.phase == CallPhase::Ringing && current.video).then_some(current.call)
}

fn video_layers() -> Option<Layers> {
    if cfg!(target_os = "ios") {
        ft_media::views::layers()
    } else {
        Some(Layers { remote: 0, local: 0 })
    }
}

/// Holds the glue's lock while the bridge is told, so the views never come back after they went.
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

fn answered_already(current: Option<&ft_core::CurrentCall>, call: &str) -> bool {
    current.is_some_and(|current| current.call == call && !current.outgoing && current.phase != CallPhase::Ringing)
}

async fn fail_current_call(core: &Core) -> anyhow::Result<()> {
    match core.current_call().await? {
        Some(current) => core.end_call(&current.call, true).await,
        None => Ok(()),
    }
}

pub const PLUGIN_EVENT: &str = "ft://plugin";

#[derive(Clone, Serialize)]
struct PluginEventView {
    plugin: String,
    contact: String,
    data: String,
}

#[derive(Serialize, Deserialize)]
pub struct ReminderView {
    pub plugin: String,
    pub id: String,
    pub at: i64,
    pub text: String,
}

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

pub async fn refresh_served_plugins(app: &AppHandle, core: &Arc<ft_core::Core>, dir: &Path) {
    let mut plugins = Vec::new();
    for plugin in core.plugins().await.unwrap_or_default() {
        let open = core.may_use_plugin(&plugin.manifest.id, plugin.manifest.kind).await.unwrap_or(false);
        plugins.push((plugin, open));
    }
    app.state::<crate::plugins::Plugins>().set(served_of(plugins, dir));
}

fn served_of(
    plugins: Vec<(ft_core::plugins::InstalledPlugin, bool)>,
    dir: &Path,
) -> std::collections::HashMap<String, crate::plugins::Served> {
    let mut served = std::collections::HashMap::new();
    for (plugin, open) in plugins {
        if !open {
            continue;
        }
        let Some(component) = plugin.manifest.components.first().cloned() else { continue };
        served.insert(
            plugin.manifest.id.clone(),
            crate::plugins::Served {
                dir: dir.join("plugins").join(&plugin.manifest.id),
                component,
                policy: crate::plugins::policy_for(&plugin.granted),
                version: plugin.manifest.version.clone(),
            },
        );
    }
    served
}

fn after_move(app: &AppHandle, router: &Arc<ft_core::RouterClient>, update: MoveUpdate) {
    let (app, router) = (app.clone(), router.clone());
    match update {
        MoveUpdate::Received => {
            tauri::async_runtime::spawn(async move {
                let _ = router.forget().await;
                tokio::time::sleep(RESTART_PAUSE).await;
                restart(&app).await;
            });
        }
        MoveUpdate::Sent => {
            tauri::async_runtime::spawn(async move {
                let client = app.state::<Client>();
                client.stop().await;
                let _ = client.wipe();
                tokio::time::sleep(RESTART_PAUSE).await;
                restart(&app).await;
            });
        }
        MoveUpdate::Progress { .. } | MoveUpdate::Failed => {}
    }
}

const RESTART_PAUSE: std::time::Duration = std::time::Duration::from_millis(2500);

/// iOS can't relaunch (or quit) an app, so there the core restarts in place and the WebView goes
/// back to its first page.
async fn restart(app: &AppHandle) {
    if cfg!(target_os = "android") && app.platform().restart_app().is_ok() {
        return;
    }
    let client = app.state::<Client>();
    client.stop().await;
    RUNNING.lock().await.stopped = false;
    start_in_background(app);
    if let Some(window) = app.get_webview_window("main") {
        if let Ok(url) = window.url() {
            let _ = window.navigate(first_page(url));
        }
    }
}

pub fn start_in_background(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = app.state::<Client>().core().await;
    });
}

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

#[tauri::command]
pub async fn core_set_auto_download(bytes: i64, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.set_auto_download_limit(bytes).await.map_err(failed)
}

#[tauri::command]
pub async fn core_accept_file(message: String, client: State<'_, Client>) -> Result<(), String> {
    let core = client.core().await?;
    tauri::async_runtime::spawn(async move {
        let _ = core.accept_file(&message).await;
    });
    Ok(())
}

#[tauri::command]
pub async fn core_renew_link(session: Option<String>, client: State<'_, Client>) -> Result<String, String> {
    let online = client.online().await?;
    online.core.renew_link(session.as_deref()).await.map_err(failed)?;
    Ok(online.core.my_card_in(session.as_deref()).await.map_err(failed)?.to_link())
}

#[tauri::command]
pub async fn core_requests(session: Option<String>, client: State<'_, Client>) -> Result<Vec<ConversationView>, String> {
    let online = client.online().await?;
    let requests = match session.as_deref() {
        None => online.core.requests().await.map_err(failed)?,
        Some(session) => online.core.session_requests(session).await.map_err(failed)?,
    };
    conversation_views(&online, requests).await
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

#[tauri::command]
pub async fn core_set_mailbox(enabled: bool, client: State<'_, Client>) -> Result<(), String> {
    let core = client.core().await?;
    tauri::async_runtime::spawn(async move {
        let _ = core.set_mailbox(enabled).await;
    });
    Ok(())
}

#[tauri::command]
pub async fn core_card(session: Option<String>, client: State<'_, Client>) -> Result<String, String> {
    Ok(client.core().await?.my_card_in(session.as_deref()).await.map_err(failed)?.to_link())
}

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
pub async fn core_add_support_contact(client: State<'_, Client>) -> Result<(), String> {
    let core = client.core().await?;
    core.add_bundled_contact(crate::support::support_link(), Some(crate::support::SUPPORT_NAME.to_owned())).await.map_err(failed)?;
    Ok(())
}

#[tauri::command]
pub fn core_support_id() -> Option<String> {
    crate::support::support_id()
}

#[tauri::command]
pub async fn core_conversations(client: State<'_, Client>) -> Result<Vec<ConversationView>, String> {
    let online = client.online().await?;
    let conversations = online.core.store().conversations().await.map_err(failed)?;
    conversation_views(&online, conversations).await
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

#[tauri::command]
pub async fn core_circle_forget(circle: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.forget_circle(&circle).await.map_err(failed)
}

#[tauri::command]
pub async fn core_circle_send(circle: String, text: String, client: State<'_, Client>) -> Result<String, String> {
    if text.trim().is_empty() {
        return Err("nothing to send".to_owned());
    }
    let core = client.core().await?;
    let sent = core.queue_circle_text(&circle, &text).await.map_err(failed)?;
    tauri::async_runtime::spawn(async move {
        core.deliver_circle_queue().await;
    });
    Ok(sent)
}

#[tauri::command]
pub async fn core_circle_messages(circle: String, limit: i64, client: State<'_, Client>) -> Result<Vec<CircleMessageView>, String> {
    let core = client.core().await?;
    let names = circle_names(&core, &circle).await?;
    let messages = core.store().circle_messages(&circle, limit).await.map_err(failed)?;
    let notes = core.store().notes(&circle).await.map_err(failed)?;
    Ok(messages
        .iter()
        .map(|message| CircleMessageView::new(message, &names).noted(notes.iter().filter(|note| note.message_id == message.message_id)))
        .collect())
}

#[tauri::command]
pub async fn core_circle_mark_read(circle: String, client: State<'_, Client>) -> Result<(), String> {
    let core = client.core().await?;
    tauri::async_runtime::spawn(async move {
        let _ = core.mark_circle_read(&circle).await;
    });
    Ok(())
}

/// Any PIN opens a session, existing or new, so nobody can tell whether one existed.
#[tauri::command]
pub async fn core_session_open(pin: String, app: AppHandle, client: State<'_, Client>) -> Result<Option<SessionView>, String> {
    let online = client.online().await?;
    let Some(session) = online.core.open_session(&pin).await.map_err(failed)? else { return Ok(None) };
    let _ = app.platform().set_open_slots(&online.core.open_slots());
    Ok(Some(session_view(&online, session).await?))
}

#[tauri::command]
pub async fn core_session_remove(session: String, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let online = client.online().await?;
    online.core.remove_session(&session).await.map_err(failed)?;
    let _ = app.platform().set_open_slots(&online.core.open_slots());
    Ok(())
}

#[tauri::command]
pub async fn core_session_close(session: String, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let online = client.online().await?;
    online.core.close_session(&session).await.map_err(failed)?;
    let _ = app.platform().set_open_slots(&online.core.open_slots());
    Ok(())
}

#[tauri::command]
pub async fn core_sessions(client: State<'_, Client>) -> Result<Vec<SessionView>, String> {
    let online = client.online().await?;
    let mut views = Vec::new();
    for session in online.core.open_sessions() {
        views.push(session_view(&online, session).await?);
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
    let replies = core.store().replies(&contact).await.map_err(failed)?;
    let mut quoted: HashMap<String, Message> = messages.iter().map(|message| (message.message_id.clone(), message.clone())).collect();
    for quoted_id in replies.values() {
        if !quoted.contains_key(quoted_id) {
            if let Some(message) = core.store().message(quoted_id).await.map_err(failed)?.filter(|message| message.contact == contact) {
                quoted.insert(quoted_id.clone(), message);
            }
        }
    }
    let reactions = core.store().reactions(&contact).await.map_err(failed)?;
    let pinned: HashSet<String> = core.store().pinned(&contact).await.map_err(failed)?.into_iter().collect();
    let edited = core.store().edited(&contact).await.map_err(failed)?;
    let deleted = core.store().deleted(&contact).await.map_err(failed)?;
    let later = core.store().scheduled(&contact).await.map_err(failed)?;
    let changing = core.updates_waiting(&contact).await.map_err(failed)?;
    let notes = core.store().notes(&contact).await.map_err(failed)?;
    let moment = ft_core::now();
    Ok(messages
        .iter()
        .map(|message| {
            let view = MessageView::new(message, files.get(&message.message_id))
                .reacted(reactions.get(&message.message_id))
                .pinned(pinned.contains(&message.message_id))
                .marked(edited.contains(&message.message_id), deleted.contains(&message.message_id))
                .for_later(later.get(&message.message_id).copied().filter(|send_at| *send_at > moment))
                .updating(changing.contains(&message.message_id))
                .noted(notes.iter().filter(|note| note.message_id == message.message_id));
            match replies.get(&message.message_id) {
                Some(quoted_id) => view.answering(QuoteView::of(quoted_id, quoted.get(quoted_id), files.get(quoted_id))),
                None => view,
            }
        })
        .collect())
}

#[tauri::command]
pub async fn core_upload_start(client: State<'_, Client>) -> Result<String, String> {
    let id = new_upload_id();
    let path = upload_path(client.dir()?, &id)?;
    let parent = path.parent().expect("uploads live in a directory");
    tokio::fs::create_dir_all(parent).await.map_err(failed)?;
    tokio::fs::File::create(&path).await.map_err(failed)?;
    Ok(id)
}

/// base64 because Android's IPC only carries JSON
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

#[tauri::command]
pub async fn core_open_file(message: String, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let file = file_of(&client, &message).await?;
    app.platform().open_file(&file.path, &file.mime).map_err(failed)
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanView {
    state: String,
    until: i64,
    /// Google Play never gives the phone an expiry date, so `until` is only how long the last check
    /// holds. The screen shows "renews automatically" instead.
    #[serde(default)]
    renews: bool,
}

fn plan_view(access: ft_billing::Access, play: bool) -> PlanView {
    let renews = play && matches!(access, ft_billing::Access::Subscribed { .. });
    let (state, until) = match access {
        ft_billing::Access::Trial { until } => ("trial", until),
        ft_billing::Access::Subscribed { until } => ("subscribed", until),
        ft_billing::Access::Limited => ("limited", 0),
    };
    PlanView { state: state.to_owned(), until, renews }
}

#[tauri::command]
pub async fn core_plan(client: State<'_, Client>) -> Result<PlanView, String> {
    let core = client.core().await?;
    let plan = core.plan().await.map_err(failed)?;
    Ok(plan_view(ft_billing::Access::of(now_ms(), plan), cfg!(target_os = "android")))
}

#[derive(Serialize)]
pub struct PriceView {
    price: Option<String>,
}

fn price_view(answer: Result<Option<String>, String>) -> PriceView {
    PriceView { price: answer.ok().flatten() }
}

/// Never cache the price, it could end up shown from another store or country.
#[tauri::command]
pub async fn core_subscription_price(app: AppHandle) -> Result<PriceView, String> {
    let answer = tauri::async_runtime::spawn_blocking(move || app.platform().subscription_price().map_err(failed))
        .await
        .map_err(failed)
        .and_then(|answer| answer);
    Ok(price_view(answer))
}

/// Nothing of the suggestion is stored or logged on the phone.
#[tauri::command]
pub async fn core_send_feedback(text: String, app: AppHandle, client: State<'_, Client>) -> Result<&'static str, String> {
    let version = app.package_info().version.to_string();
    Ok(feedback_word(client.online().await?.send_feedback(&text, &version).await))
}

fn feedback_word(outcome: ft_core::Feedback) -> &'static str {
    match outcome {
        ft_core::Feedback::Sent => "sent",
        ft_core::Feedback::TooMany => "tooMany",
        ft_core::Feedback::Failed => "failed",
    }
}

#[tauri::command]
pub async fn core_subscribe(app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let until = tauri::async_runtime::spawn_blocking(move || buy_from(app.platform(), now_ms()))
        .await
        .map_err(failed)??;
    client.core().await?.set_entitlement(until).await.map_err(failed)
}

/// Paid-up-until in ms (0 = not paid), or a `plan.trouble.*` key the screen translates.
pub(crate) trait Shop {
    fn subscribe(&self) -> Result<i64, String>;
    fn restore(&self) -> Result<i64, String>;
    fn current(&self) -> Result<i64, String>;
}

impl<R: tauri::Runtime> Shop for tauri_plugin_ft_platform::Platform<R> {
    fn subscribe(&self) -> Result<i64, String> {
        tauri_plugin_ft_platform::Platform::subscribe(self).map_err(failed)
    }

    fn restore(&self) -> Result<i64, String> {
        self.restore_subscription().map_err(failed)
    }

    fn current(&self) -> Result<i64, String> {
        self.subscription().map_err(failed)
    }
}

struct AppShop(AppHandle);

impl Shop for AppShop {
    fn subscribe(&self) -> Result<i64, String> {
        Shop::subscribe(self.0.platform())
    }

    fn restore(&self) -> Result<i64, String> {
        Shop::restore(self.0.platform())
    }

    fn current(&self) -> Result<i64, String> {
        Shop::current(self.0.platform())
    }
}

/// The bridge calls block, so the plan watch runs them off the async runtime.
struct ShopWord<S>(Arc<S>);

#[async_trait::async_trait]
impl<S: Shop + Send + Sync + 'static> ft_core::Entitlements for ShopWord<S> {
    async fn until(&self) -> anyhow::Result<i64> {
        let shop = self.0.clone();
        tauri::async_runtime::spawn_blocking(move || shop.current()).await?.map_err(anyhow::Error::msg)
    }
}

fn buy_from(shop: &impl Shop, now: i64) -> Result<i64, String> {
    let until = shop.subscribe()?;
    if until > now {
        Ok(until)
    } else {
        Err("payment_failed".to_owned())
    }
}

fn restore_from(shop: &impl Shop, now: i64) -> Result<(i64, &'static str), String> {
    let until = shop.restore()?;
    Ok((until, if until > now { "restored" } else { "nothing" }))
}

#[tauri::command]
pub async fn core_restore_subscription(app: AppHandle, client: State<'_, Client>) -> Result<&'static str, String> {
    let (until, word) = tauri::async_runtime::spawn_blocking(move || restore_from(app.platform(), now_ms()))
        .await
        .map_err(failed)??;
    client.core().await?.set_entitlement(until).await.map_err(failed)?;
    Ok(word)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis() as i64)
        .unwrap_or(0)
}

#[tauri::command]
pub async fn core_forget_message(message: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.forget_message(&message).await.map_err(failed)
}

#[tauri::command]
pub async fn core_schedule(contact: String, text: String, send_at: i64, reply_to: Option<String>, client: State<'_, Client>) -> Result<String, String> {
    client.core().await?.schedule_text(&contact, &text, send_at, reply_to.as_deref()).await.map_err(failed)
}

#[tauri::command]
pub async fn core_search(contact: String, query: String, client: State<'_, Client>) -> Result<Vec<MessageView>, String> {
    let core = client.core().await?;
    let found = core.search(&contact, &query).await.map_err(failed)?;
    let mut views = Vec::with_capacity(found.len());
    for message in &found {
        let file = core.store().file(&message.message_id).await.map_err(failed)?.map(|file| located(&core, file));
        views.push(MessageView::new(message, file.as_ref()));
    }
    Ok(views)
}

#[tauri::command]
pub async fn core_edit(message: String, text: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.edit_message(&message, &text).await.map_err(failed)
}

#[tauri::command]
pub async fn core_delete_everyone(message: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.delete_for_everyone(&message).await.map_err(failed)
}

#[tauri::command]
pub async fn core_pin(message: String, pinned: bool, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.pin_message(&message, pinned).await.map_err(failed)
}

#[tauri::command]
pub async fn core_react(contact: String, message: String, emoji: Option<String>, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.react(&contact, &message, emoji.as_deref()).await.map_err(failed)
}

#[tauri::command]
pub async fn core_forward(message: String, contact: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.forward(&message, &contact).await.map_err(failed).map(|_| ())
}

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

/// Router is best effort, an offline phone still has to be able to erase itself.
#[tauri::command]
pub async fn core_erase(app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    if let Ok(online) = client.online().await {
        let _ = online.router.forget().await;
    }
    client.stop().await;
    let wiped = client.wipe();
    let _ = app.platform().set_open_slots(&[]);
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(RESTART_PAUSE).await;
        restart(&app).await;
    });
    wiped
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginView {
    id: String,
    name: String,
    version: String,
    asks: PermissionsView,
    granted: PermissionsView,
    installed_at: i64,
    #[serde(default)]
    opens: Vec<String>,
    #[serde(default)]
    views: Vec<String>,
    kind: ft_plugins::Kind,
    locales: BTreeMap<String, ft_plugins::Localized>,
    #[serde(skip_serializing_if = "String::is_empty")]
    icon: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    image: String,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    hooks: BTreeMap<ft_plugins::HookName, ft_plugins::HookDeclaration>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PermissionsView {
    network: Vec<String>,
    messages: bool,
    send: String,
    #[serde(default)]
    print: bool,
    #[serde(default)]
    live: bool,
    #[serde(default)]
    remind: bool,
    #[serde(default)]
    drive: bool,
    #[serde(default = "small")]
    storage: String,
    #[serde(default)]
    location: bool,
    #[serde(default)]
    translate: bool,
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
            location: permissions.location,
            translate: permissions.translate,
        }
    }
}

impl From<PermissionsView> for ft_plugins::Permissions {
    fn from(granted: PermissionsView) -> Self {
        Self {
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
            location: granted.location,
            translate: granted.translate,
        }
    }
}

fn plugin_view(plugin: ft_core::plugins::InstalledPlugin) -> PluginView {
    PluginView {
        asks: PermissionsView::from(&plugin.manifest.permissions),
        granted: PermissionsView::from(&plugin.granted),
        id: plugin.manifest.id,
        name: plugin.manifest.name,
        version: plugin.manifest.version,
        installed_at: plugin.installed_at,
        opens: plugin.manifest.opens,
        views: plugin.manifest.views,
        kind: plugin.manifest.kind,
        locales: plugin.manifest.locales,
        icon: plugin.manifest.icon,
        image: plugin.image,
        hooks: plugin.manifest.hooks,
    }
}

#[tauri::command]
pub async fn core_plugins(client: State<'_, Client>) -> Result<Vec<PluginView>, String> {
    let core = client.core().await?;
    Ok(core
        .plugins()
        .await
        .map_err(failed)?
        .into_iter()
        .map(plugin_view)
        .collect())
}

#[tauri::command]
pub async fn core_plugin_grant(
    plugin: String,
    granted: PermissionsView,
    app: AppHandle,
    client: State<'_, Client>,
) -> Result<(), String> {
    let permissions = ft_plugins::Permissions::from(granted);
    let core = client.core().await?;
    core.grant_plugin(&plugin, permissions).await.map_err(failed)?;
    refresh_served_plugins(&app, &core, client.dir()?).await;
    Ok(())
}

#[tauri::command]
pub async fn core_plugin_remove(plugin: String, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let core = client.core().await?;
    core.remove_plugin(&plugin).await.map_err(failed)?;
    refresh_served_plugins(&app, &core, client.dir()?).await;
    Ok(())
}

const BUNDLED_PLUGINS: &[&[u8]] = &[
    include_bytes!("../resources/plugins/markdown.ftplugin"),
    include_bytes!("../resources/plugins/images.ftplugin"),
    include_bytes!("../resources/plugins/pdf.ftplugin"),
    include_bytes!("../resources/plugins/redact.ftplugin"),
    include_bytes!("../resources/plugins/sketch.ftplugin"),
    include_bytes!("../resources/plugins/scanner.ftplugin"),
    include_bytes!("../resources/plugins/countdown.ftplugin"),
    include_bytes!("../resources/plugins/board.ftplugin"),
    include_bytes!("../resources/plugins/clean.ftplugin"),
    include_bytes!("../resources/plugins/drive.ftplugin"),
    include_bytes!("../resources/plugins/list.ftplugin"),
    include_bytes!("../resources/plugins/location.ftplugin"),
    include_bytes!("../resources/plugins/notes.ftplugin"),
    include_bytes!("../resources/plugins/poll.ftplugin"),
    include_bytes!("../resources/plugins/sign.ftplugin"),
    include_bytes!("../resources/plugins/split.ftplugin"),
    include_bytes!("../resources/plugins/pdfviewer.ftplugin"),
    include_bytes!("../resources/plugins/translate.ftplugin"),
    include_bytes!("../resources/plugins/remind.ftplugin"),
    include_bytes!("../resources/plugins/game.tictactoe.ftplugin"),
    include_bytes!("../resources/plugins/game.fourinarow.ftplugin"),
    include_bytes!("../resources/plugins/game.chess.ftplugin"),
    include_bytes!("../resources/plugins/game.backgammon.ftplugin"),
    include_bytes!("../resources/plugins/game.checkers.ftplugin"),
    include_bytes!("../resources/plugins/game.dotsandboxes.ftplugin"),
    include_bytes!("../resources/plugins/game.eights.ftplugin"),
    include_bytes!("../resources/plugins/game.gomoku.ftplugin"),
    include_bytes!("../resources/plugins/game.mancala.ftplugin"),
    include_bytes!("../resources/plugins/game.reversi.ftplugin"),
    include_bytes!("../resources/plugins/game.seabattle.ftplugin"),
    include_bytes!("../resources/plugins/game.wordduel.ftplugin"),
    include_bytes!("../resources/plugins/game.wordgrid.ftplugin"),
];

#[cfg(test)]
const SEED_LIMIT: u64 = 4 * 1024 * 1024;
#[cfg(test)]
const SEEDS_LIMIT: u64 = 8 * 1024 * 1024;

#[cfg(test)]
fn seeds_weight_problem(weights: impl IntoIterator<Item = u64>) -> Option<String> {
    let mut total = 0;
    for weight in weights {
        if weight > SEED_LIMIT {
            return Some(format!("a plugin of {weight} bytes is a download, not a seed"));
        }
        total += weight;
    }
    (total > SEEDS_LIMIT).then(|| format!("the seeds weigh {total} bytes, which is no longer little"))
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OfferedPlugin {
    id: String,
    name: String,
    version: String,
    summary: String,
    size: u64,
    installed: bool,
    carried: bool,
    kind: ft_plugins::Kind,
    locales: BTreeMap<String, ft_plugins::Localized>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    icon: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    image: String,
}

fn newer(version: &str, than: &str) -> bool {
    let parts = |version: &str| version.split('.').map(|part| part.parse().unwrap_or(0)).collect::<Vec<u32>>();
    parts(version) > parts(than)
}

fn seeds() -> Vec<OfferedPlugin> {
    BUNDLED_PLUGINS
        .iter()
        .filter_map(|package| {
            let plugin = ft_plugins::open(package, &ft_plugins::catalogue()).ok()?;
            let image = plugin.image();
            Some(offered_from(plugin.manifest, image, package.len() as u64))
        })
        .collect()
}

fn offered_from(manifest: ft_plugins::Manifest, image: String, size: u64) -> OfferedPlugin {
    OfferedPlugin {
        id: manifest.id,
        name: manifest.name,
        version: manifest.version,
        summary: manifest.summary,
        size,
        installed: false,
        carried: true,
        kind: manifest.kind,
        locales: manifest.locales,
        icon: manifest.icon,
        image,
    }
}

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
            kind: entry.kind,
            locales: entry.locales.clone(),
            icon: entry.icon.clone(),
            image: entry.image.clone(),
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

fn seed_package(id: &str) -> Option<&'static [u8]> {
    BUNDLED_PLUGINS
        .iter()
        .find(|package| {
            ft_plugins::open(package, &ft_plugins::catalogue()).is_ok_and(|plugin| plugin.manifest.id == id)
        })
        .copied()
}

/// App Store rule 4.7, iOS downloads nothing.
fn downloads() -> bool {
    !cfg!(target_os = "ios")
}

#[tauri::command]
pub async fn core_catalogue(app: AppHandle, client: State<'_, Client>) -> Result<Vec<OfferedPlugin>, String> {
    let core = client.core().await?;
    let here: Vec<String> =
        core.plugins().await.map_err(failed)?.into_iter().map(|plugin| plugin.manifest.id).collect();
    let listed = match downloads() {
        true => core.catalogue(client.web(), &ft_plugins::catalogue()).await.unwrap_or_default(),
        false => Vec::new(),
    };
    if !listed.is_empty() {
        update_downloaded(&app, core.clone(), listed.clone());
    }
    Ok(merged(seeds(), &listed, &here))
}

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

#[derive(Default)]
pub struct OpenPlugins(std::sync::Mutex<HashMap<String, u32>>);

impl OpenPlugins {
    pub fn set(&self, id: &str, open: bool) {
        let mut frames = self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let count = frames.entry(id.to_owned()).or_default();
        *count = if open { *count + 1 } else { count.saturating_sub(1) };
        if *count == 0 {
            frames.remove(id);
        }
    }

    pub fn ids(&self) -> std::collections::HashSet<String> {
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).keys().cloned().collect()
    }
}

#[tauri::command]
pub async fn core_plugin_open(plugin: String, open: bool, frames: State<'_, OpenPlugins>, client: State<'_, Client>) -> Result<(), String> {
    if open {
        client.core().await?.open_plugin(&plugin).await.map_err(failed)?;
    }
    frames.set(&plugin, open);
    Ok(())
}

const UPDATE_EVERY_MS: i64 = 12 * 3_600_000;
const UPDATE_LOOK: &str = "plugin-updates.at";

fn updates_due(last: Option<i64>, now: i64) -> bool {
    last.is_none_or(|last| now - last >= UPDATE_EVERY_MS || last > now)
}

fn last_update_look(dir: &Path) -> Option<i64> {
    std::fs::read_to_string(dir.join(UPDATE_LOOK)).ok()?.trim().parse().ok()
}

fn looked_for_updates(dir: &Path, now: i64) {
    let _ = std::fs::write(dir.join(UPDATE_LOOK), now.to_string());
}

fn update_downloaded(app: &AppHandle, core: Arc<ft_core::Core>, listed: Vec<ft_plugins::CatalogueEntry>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let client = app.state::<Client>();
        let open = app.state::<OpenPlugins>().ids();
        let updated = core.update_plugins(&listed, client.web(), &ft_plugins::catalogue(), &open).await;
        if !updated.is_empty() {
            if let Ok(dir) = client.dir() {
                refresh_served_plugins(&app, &core, dir).await;
            }
        }
    });
}

fn look_for_updates(app: &AppHandle, core: Arc<ft_core::Core>, dir: &Path) {
    if !downloads() || !updates_due(last_update_look(dir), now_ms()) {
        return;
    }
    let (app, dir) = (app.clone(), dir.to_path_buf());
    tauri::async_runtime::spawn(async move {
        let listed = {
            let client = app.state::<Client>();
            core.catalogue(client.web(), &ft_plugins::catalogue()).await
        };
        let Ok(listed) = listed else { return };
        looked_for_updates(&dir, now_ms());
        update_downloaded(&app, core, listed);
    });
}

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

#[tauri::command]
pub async fn core_plugin_read(plugin: String, key: String, session: Option<String>, client: State<'_, Client>) -> Result<Option<String>, String> {
    client.core().await?.plugin_remembers(&plugin, session.as_deref(), &key).await.map_err(failed)
}

#[tauri::command]
pub async fn core_plugin_write(
    plugin: String,
    key: String,
    value: String,
    session: Option<String>,
    client: State<'_, Client>,
) -> Result<(), String> {
    client.core().await?.plugin_remember(&plugin, session.as_deref(), &key, &value).await.map_err(failed)
}

#[tauri::command]
pub async fn core_plugin_forget(plugin: String, key: String, session: Option<String>, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.plugin_forget(&plugin, session.as_deref(), &key).await.map_err(failed)
}

#[tauri::command]
pub async fn core_plugin_record_get(plugin: String, key: String, session: Option<String>, client: State<'_, Client>) -> Result<Option<String>, String> {
    let value = client.core().await?.plugin_record(&plugin, session.as_deref(), &key).await.map_err(failed)?;
    Ok(value.map(|bytes| BASE64.encode(bytes)))
}

#[tauri::command]
pub async fn core_plugin_record_set(
    plugin: String,
    key: String,
    value: String,
    session: Option<String>,
    client: State<'_, Client>,
) -> Result<(), String> {
    let bytes = BASE64.decode(value.as_bytes()).map_err(|_| "that value is not base64".to_owned())?;
    client.core().await?.plugin_record_set(&plugin, session.as_deref(), &key, &bytes).await.map_err(failed)
}

#[tauri::command]
pub async fn core_plugin_record_forget(plugin: String, key: String, session: Option<String>, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.plugin_record_forget(&plugin, session.as_deref(), &key).await.map_err(failed)
}

#[tauri::command]
pub async fn core_plugin_record_keys(plugin: String, prefix: String, session: Option<String>, client: State<'_, Client>) -> Result<Vec<String>, String> {
    client.core().await?.plugin_record_keys(&plugin, session.as_deref(), &prefix).await.map_err(failed)
}

#[tauri::command]
pub async fn core_plugin_record_usage(plugin: String, session: Option<String>, client: State<'_, Client>) -> Result<(u64, u64), String> {
    client.core().await?.plugin_records_usage(&plugin, session.as_deref()).await.map_err(failed)
}

#[tauri::command]
pub async fn core_plugin_ref(plugin: String, message: String, client: State<'_, Client>) -> Result<String, String> {
    client.core().await?.plugin_ref(&plugin, &message).await.map_err(failed)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefTargetView {
    contact: String,
    message: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    circle: bool,
}

#[tauri::command]
pub async fn core_plugin_open_chat(
    plugin: String,
    reference: String,
    session: Option<String>,
    client: State<'_, Client>,
) -> Result<Option<RefTargetView>, String> {
    let core = client.core().await?;
    let Some(target) = core.plugin_ref_target(&plugin, session.as_deref(), &reference).await.map_err(failed)? else { return Ok(None) };
    let circle = core.store().circle(&target.contact).await.map_err(failed)?.is_some();
    Ok(Some(RefTargetView { contact: target.contact, message: target.message_id, circle }))
}

#[tauri::command]
pub async fn core_remind_set(
    plugin: String,
    id: String,
    at: i64,
    text: String,
    session: Option<String>,
    client: State<'_, Client>,
) -> Result<(), String> {
    client.core().await?.set_reminder(&plugin, session.as_deref(), &id, at, &text).await.map_err(failed)
}

#[tauri::command]
pub async fn core_remind_cancel(plugin: String, id: String, session: Option<String>, client: State<'_, Client>) -> Result<bool, String> {
    client.core().await?.cancel_reminder(&plugin, session.as_deref(), &id).await.map_err(failed)
}

#[tauri::command]
pub async fn core_remind_list(plugin: String, session: Option<String>, client: State<'_, Client>) -> Result<Vec<ReminderView>, String> {
    let reminders = client.core().await?.plugin_reminders(&plugin, session.as_deref()).await.map_err(failed)?;
    Ok(reminders
        .into_iter()
        .map(|reminder| ReminderView { plugin: reminder.plugin, id: reminder.id, at: reminder.at, text: reminder.text })
        .collect())
}

#[derive(Serialize)]
pub struct TappedReminderView {
    plugin: String,
    id: String,
    session: Option<String>,
}

#[tauri::command]
pub async fn core_pending_reminder(app: AppHandle, client: State<'_, Client>) -> Result<Option<TappedReminderView>, String> {
    let tapped = app.platform().pending_reminder().unwrap_or_default();
    let Some((plugin, id)) = tapped.split_once('\n') else { return Ok(None) };
    // The tap is already consumed, so without a core we still open the plugin, in the main list
    let ringing = match client.core().await {
        Ok(core) => core.ringing_reminder(plugin, id).await.ok().flatten(),
        Err(_) => None,
    };
    Ok(Some(TappedReminderView { plugin: plugin.to_owned(), id: id.to_owned(), session: ringing.and_then(|reminder| reminder.session) }))
}

#[tauri::command]
pub async fn core_plugin_live_send(plugin: String, contact: String, data: String, client: State<'_, Client>) -> Result<bool, String> {
    let bytes = BASE64.decode(data.as_bytes()).map_err(|_| "that data is not base64".to_owned())?;
    client.core().await?.plugin_live_send(&plugin, &contact, bytes).await.map_err(failed)
}

/// Opaque, and a different id for each plugin.
#[tauri::command]
pub async fn core_plugin_chat(plugin: String, contact: String, client: State<'_, Client>) -> Result<String, String> {
    client.core().await?.plugin_chat(&plugin, &contact).await.map_err(failed)
}

// Hooks get the same opaque chat id as core_plugin_chat, so a tool finds its per-chat data from
// both.

#[tauri::command]
pub async fn core_message_note_set(message: String, plugin: String, text: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.set_message_note(&message, &plugin, &text).await.map_err(failed)
}

#[tauri::command]
pub async fn core_message_note_clear(message: String, plugin: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.clear_message_note(&message, &plugin).await.map_err(failed)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatHookView {
    plugin: String,
    auto_message: bool,
    auto_composer: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    lang: Option<String>,
}

impl From<ft_core::plugins::ChatHook> for ChatHookView {
    fn from(hook: ft_core::plugins::ChatHook) -> Self {
        Self { plugin: hook.plugin, auto_message: hook.auto_message, auto_composer: hook.auto_composer, lang: hook.lang }
    }
}

#[tauri::command]
pub async fn core_chat_hooks(conversation: String, client: State<'_, Client>) -> Result<Vec<ChatHookView>, String> {
    let hooks = client.core().await?.chat_hooks(&conversation).await.map_err(failed)?;
    Ok(hooks.into_iter().map(ChatHookView::from).collect())
}

#[tauri::command]
pub async fn core_chat_hooks_set(
    conversation: String,
    plugin: String,
    auto_message: bool,
    auto_composer: bool,
    lang: Option<String>,
    client: State<'_, Client>,
) -> Result<(), String> {
    let lang = lang.filter(|lang| !lang.is_empty());
    client.core().await?.set_chat_hooks(&conversation, &plugin, auto_message, auto_composer, lang.as_deref()).await.map_err(failed)
}

#[tauri::command]
pub async fn core_hook_sample(plugin: String, conversation: String, client: State<'_, Client>) -> Result<String, String> {
    client.core().await?.hook_sample(&plugin, &conversation).await.map_err(failed)
}

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
        .map(plugin_view)
        .collect())
}

#[tauri::command]
pub async fn core_read_message_file(message: String, client: State<'_, Client>) -> Result<HandedFile, String> {
    let file = file_of(&client, &message).await?;
    if file.size as u64 > PLUGIN_FILE_LIMIT {
        return Err("that file is too big for a plugin".to_owned());
    }
    let bytes = tokio::fs::read(&file.path).await.map_err(|_| "the bytes of that file are no longer here".to_owned())?;
    Ok(HandedFile { name: file.name, mime: file.mime, data: BASE64.encode(bytes) })
}

#[derive(Serialize)]
pub struct HandedFile {
    pub name: String,
    pub mime: String,
    pub data: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchView {
    status: u16,
    body: String,
}

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
    // The print service reads the file later, so it can't be deleted yet. Clean up older prints
    // instead.
    let (path, safe) = made_file(&client, &name, &data, "printing")?;
    forget_old_prints(path.parent().unwrap_or(&path), &path);
    app.platform().print_file(&path.to_string_lossy(), &safe, &mime).map_err(failed)
}

#[derive(Serialize)]
pub struct LocationView {
    lat: f64,
    lon: f64,
    accuracy: f64,
    at: i64,
}

struct PlatformLocator(AppHandle);

#[async_trait::async_trait]
impl ft_core::plugins::Locator for PlatformLocator {
    async fn locate(&self) -> anyhow::Result<Option<ft_core::plugins::Fix>> {
        let app = self.0.clone();
        let found = tauri::async_runtime::spawn_blocking(move || app.platform().current_location()).await??;
        Ok(found.map(|fix| ft_core::plugins::Fix { lat: fix.lat, lon: fix.lon, accuracy: fix.accuracy, at: fix.at }))
    }
}

#[tauri::command]
pub async fn core_plugin_location(plugin: String, app: AppHandle, client: State<'_, Client>) -> Result<Option<LocationView>, String> {
    let core = client.core().await?;
    let fix = core.plugin_location(&plugin, &PlatformLocator(app)).await.map_err(failed)?;
    Ok(fix.map(|fix| LocationView { lat: fix.lat, lon: fix.lon, accuracy: fix.accuracy, at: fix.at }))
}

struct PlatformTranslator(AppHandle);

impl PlatformTranslator {
    async fn ask<T: Send + 'static>(
        &self,
        call: impl FnOnce(&AppHandle) -> tauri_plugin_ft_platform::Result<T> + Send + 'static,
    ) -> anyhow::Result<T> {
        let app = self.0.clone();
        Ok(tauri::async_runtime::spawn_blocking(move || call(&app)).await??)
    }
}

fn pair_status(status: tauri_plugin_ft_platform::TranslateStatus) -> ft_core::plugins::PairStatus {
    match status {
        tauri_plugin_ft_platform::TranslateStatus::Installed => ft_core::plugins::PairStatus::Installed,
        tauri_plugin_ft_platform::TranslateStatus::Downloadable => ft_core::plugins::PairStatus::Downloadable,
        tauri_plugin_ft_platform::TranslateStatus::Unsupported => ft_core::plugins::PairStatus::Unsupported,
    }
}

#[async_trait::async_trait]
impl ft_core::plugins::Translator for PlatformTranslator {
    async fn languages(&self) -> anyhow::Result<Vec<String>> {
        self.ask(|app| app.platform().translate_languages()).await
    }

    async fn detect(&self, text: &str) -> anyhow::Result<Option<String>> {
        let text = text.to_owned();
        self.ask(move |app| app.platform().translate_detect(&text)).await
    }

    async fn status(&self, from: &str, to: &str) -> anyhow::Result<ft_core::plugins::PairStatus> {
        let (from, to) = (from.to_owned(), to.to_owned());
        Ok(pair_status(self.ask(move |app| app.platform().translate_status(&from, &to)).await?))
    }

    async fn prepare(&self, from: &str, to: &str) -> anyhow::Result<bool> {
        let (from, to) = (from.to_owned(), to.to_owned());
        self.ask(move |app| app.platform().translate_prepare(&from, &to)).await
    }

    async fn remove(&self, from: &str, to: &str) -> anyhow::Result<bool> {
        let (from, to) = (from.to_owned(), to.to_owned());
        self.ask(move |app| app.platform().translate_remove(&from, &to)).await
    }

    async fn translate(&self, text: &str, from: &str, to: &str) -> anyhow::Result<Option<String>> {
        let (text, from, to) = (text.to_owned(), from.to_owned(), to.to_owned());
        self.ask(move |app| app.platform().translate_text(&text, &from, &to)).await
    }
}

#[tauri::command]
pub async fn core_translate_languages(plugin: String, app: AppHandle, client: State<'_, Client>) -> Result<Vec<String>, String> {
    let core = client.core().await?;
    core.plugin_translate_languages(&plugin, &PlatformTranslator(app)).await.map_err(failed)
}

#[tauri::command]
pub async fn core_translate_detect(plugin: String, text: String, app: AppHandle, client: State<'_, Client>) -> Result<Option<String>, String> {
    let core = client.core().await?;
    core.plugin_translate_detect(&plugin, &PlatformTranslator(app), &text).await.map_err(failed)
}

#[tauri::command]
pub async fn core_translate_status(
    plugin: String,
    from: String,
    to: String,
    app: AppHandle,
    client: State<'_, Client>,
) -> Result<ft_core::plugins::PairStatus, String> {
    let core = client.core().await?;
    core.plugin_translate_status(&plugin, &PlatformTranslator(app), &from, &to).await.map_err(failed)
}

#[tauri::command]
pub async fn core_translate_prepare(plugin: String, from: String, to: String, app: AppHandle, client: State<'_, Client>) -> Result<bool, String> {
    let core = client.core().await?;
    core.plugin_translate_prepare(&plugin, &PlatformTranslator(app), &from, &to).await.map_err(failed)
}

#[tauri::command]
pub async fn core_translate_remove(plugin: String, from: String, to: String, app: AppHandle, client: State<'_, Client>) -> Result<bool, String> {
    let core = client.core().await?;
    core.plugin_translate_remove(&plugin, &PlatformTranslator(app), &from, &to).await.map_err(failed)
}

/// Don't keep or log the text, it never leaves the phone.
#[tauri::command]
pub async fn core_translate_text(
    plugin: String,
    text: String,
    from: String,
    to: String,
    app: AppHandle,
    client: State<'_, Client>,
) -> Result<Option<String>, String> {
    let core = client.core().await?;
    core.plugin_translate_text(&plugin, &PlatformTranslator(app), &text, &from, &to).await.map_err(failed)
}

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

#[tauri::command]
pub async fn core_share(text: String, app: AppHandle) -> Result<(), String> {
    app.platform().share_text(&text).map_err(failed)
}

#[tauri::command]
pub async fn core_save_file(message: String, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let file = file_of(&client, &message).await?;
    app.platform().save_to_downloads(&file.path, &file.name, &file.mime).map_err(failed)
}

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

fn push_provider() -> &'static str {
    if cfg!(target_os = "ios") {
        "apns"
    } else {
        "fcm"
    }
}

/// iOS may have cut the socket while the app was suspended, so a stale one is reopened.
#[tauri::command]
pub async fn core_resume(client: State<'_, Client>) -> Result<(), String> {
    let online = client.online().await?;
    online.core.look_at_plan_again();
    if !online.set_foreground(true).await {
        online.router.reconnect_unless_fresh(CALL_SOCKET_FRESH);
    }
    Ok(())
}

#[tauri::command]
pub async fn core_move_invite(client: State<'_, Client>) -> Result<String, String> {
    client.core().await?.invite_move().await.map_err(failed)
}

#[tauri::command]
pub async fn core_move_to(link: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.move_to(&link).await.map_err(failed)
}

#[tauri::command]
pub async fn core_call_ice(client: State<'_, Client>) -> Result<Vec<IceServer>, String> {
    let (stun, turn) = client.online().await?.network.fresh_ice().await;
    Ok(ice_servers(stun, turn))
}

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

#[tauri::command]
pub async fn core_native_calls(client: State<'_, Client>) -> Result<bool, String> {
    Ok(client.core().await?.native_calls())
}

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

#[tauri::command]
pub async fn core_call_answer_native(call: String, routing: String, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let routing = call_routing(&routing)?;
    let core = client.core().await?;
    core.set_call_routing(routing).await.map_err(failed)?;
    // Ask now while the app is on screen. CallKit's answer asks again and knows by then.
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

#[tauri::command]
pub async fn core_call_set_video(call: String, on: bool, app: AppHandle, client: State<'_, Client>) -> Result<CallVideoView, String> {
    if on {
        camera_or_denied(camera(&app).await)?;
    }
    let state = client.core().await?.set_call_camera(&call, on).await.map_err(failed)?;
    Ok(CallVideoView::from(state))
}

#[tauri::command]
pub async fn core_call_switch_camera(call: String, client: State<'_, Client>) -> Result<CallVideoView, String> {
    let state = client.core().await?.switch_call_camera(&call).await.map_err(failed)?;
    Ok(CallVideoView::from(state))
}

#[tauri::command]
pub async fn core_call_video_layout(layout: Option<VideoLayout>, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let glue = client.video.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || tell_bridge(&app, &glue, |glue| glue.layout(layout))).await;
    if let Ok(core) = client.core().await {
        let _ = core.set_call_shown(layout.is_some()).await;
    }
    Ok(())
}

#[tauri::command]
pub async fn core_system_bars(dark: bool, app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || app.platform().set_system_bars(dark)).await.map_err(failed)?.map_err(failed)
}

#[tauri::command]
pub async fn core_exact_alarms(app: AppHandle) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || app.platform().exact_alarms()).await.map_err(failed)?.map_err(failed)
}

#[tauri::command]
pub async fn core_request_exact_alarms(app: AppHandle) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || app.platform().request_exact_alarms()).await.map_err(failed)?.map_err(failed)
}

#[tauri::command]
pub async fn core_call_speaker(on: bool, app: AppHandle) -> Result<(), String> {
    app.platform().set_speaker(on).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn core_call_mute(call: String, muted: bool, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.mute_call(&call, muted).await.map_err(failed)
}

#[tauri::command]
pub async fn core_call_present(call: String, plugin: String, file: Option<String>, client: State<'_, Client>) -> Result<(), String> {
    let file = presented_file(file)?;
    client.core().await?.present_in_call(&call, &plugin, file).await.map_err(failed)
}

#[tauri::command]
pub async fn core_call_present_stop(call: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.stop_presenting(&call).await.map_err(failed)
}

fn presented_file(file: Option<String>) -> Result<Option<ft_core::MessageId>, String> {
    file.map(|file| ft_core::MessageId::parse(&file).map_err(failed)).transpose()
}

#[tauri::command]
pub async fn core_file_belongs_to(file: String, contact: String, client: State<'_, Client>) -> Result<bool, String> {
    let Some(file) = presented_message(&file) else { return Ok(false) };
    client.core().await?.file_belongs_to(&file, &contact).await.map_err(failed)
}

fn presented_message(file: &str) -> Option<ft_core::MessageId> {
    ft_core::MessageId::parse(file).ok()
}

#[tauri::command]
pub async fn core_set_call_routing(routing: String, client: State<'_, Client>) -> Result<(), String> {
    let routing = call_routing(&routing)?;
    client.core().await?.set_call_routing(routing).await.map_err(failed)
}

#[tauri::command]
pub async fn core_current_call(client: State<'_, Client>) -> Result<Option<CurrentCallView>, String> {
    let current = client.core().await?.current_call().await.map_err(failed)?;
    Ok(current.map(|current| {
        let route = CALL_ROUTES.lock().unwrap_or_else(PoisonError::into_inner).of(&current.call);
        CurrentCallView::from(current).route(route)
    }))
}

#[tauri::command]
pub async fn core_call_end(call: String, failed: bool, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let _ = app.platform().stop_ringing();
    let core = client.core().await?;
    tauri::async_runtime::spawn(async move {
        let _ = core.end_call(&call, failed).await;
    });
    Ok(())
}

#[tauri::command]
pub async fn core_calls(client: State<'_, Client>) -> Result<Vec<CallView>, String> {
    let core = client.core().await?;
    let mut contacts = core.store().contacts().await.map_err(failed)?;
    for session in core.open_sessions() {
        contacts.extend(core.store().session_contacts(&session).await.map_err(failed)?);
    }
    let names: HashMap<String, String> = contacts.into_iter().map(|contact| (contact.device_id, contact.name)).collect();
    let calls = core.visible_calls(100).await.map_err(failed)?;
    Ok(calls.iter().map(|call| CallView::new(call, names.get(&call.contact).map_or("", String::as_str))).collect())
}

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

pub const PLUGIN_FILE_LIMIT: u64 = 32 * 1024 * 1024;

#[tauri::command]
pub async fn core_pick_for_plugin(accept: Option<String>, app: AppHandle, client: State<'_, Client>) -> Result<Option<HandedFile>, String> {
    let dir = client.dir()?.to_owned();
    let picked = core_pick_files(accept, app).await?;
    hand_over_picked(&dir, &picked)
}

#[tauri::command]
pub async fn core_take_photo_for_plugin(app: AppHandle, client: State<'_, Client>) -> Result<Option<HandedFile>, String> {
    let dir = client.dir()?.to_owned();
    let taken = core_take_photo(app).await?;
    hand_over_picked(&dir, &taken)
}

/// Every picker copy is deleted, handed over or not, so the original (with the place Clean would
/// strip from a photo) doesn't stay in the app.
pub fn hand_over_picked(dir: &Path, picked: &[PickedView]) -> Result<Option<HandedFile>, String> {
    let mut files = picked.iter();
    let first = files.next().map(|file| {
        let bytes = take_picked(dir, &file.path)?;
        Ok(HandedFile { name: file.name.clone(), mime: file.mime.clone(), data: BASE64.encode(bytes) })
    });
    for file in files {
        forget_picked(dir, &file.path);
    }
    first.transpose()
}

/// Picker folders only. Those copies are fresh and no message points to them.
fn picker_copy(dir: &Path, path: &str) -> Result<PathBuf, String> {
    let path = picked_path(dir, path)?;
    let pickers = [dir.join("uploads"), dir.join("files").join("uploads")];
    if pickers.iter().filter_map(|root| root.canonicalize().ok()).any(|root| path.starts_with(&root)) {
        Ok(path)
    } else {
        Err("that is not a file the user picked".to_owned())
    }
}

fn forget_picked(dir: &Path, path: &str) {
    if let Ok(path) = picker_copy(dir, path) {
        let _ = std::fs::remove_file(path);
    }
}

fn take_picked(dir: &Path, path: &str) -> Result<Vec<u8>, String> {
    let path = picker_copy(dir, path)?;
    let read = match std::fs::metadata(&path) {
        Ok(meta) if meta.len() > PLUGIN_FILE_LIMIT => Err("that file is too big to hand over".to_owned()),
        Ok(_) => std::fs::read(&path).map_err(failed),
        Err(error) => Err(failed(error)),
    };
    let _ = std::fs::remove_file(&path);
    read
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MadeView {
    sent: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    staged: Option<PickedView>,
}

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

#[tauri::command]
pub async fn core_take_photo(app: AppHandle) -> Result<Vec<PickedView>, String> {
    let taken = tauri::async_runtime::spawn_blocking(move || app.platform().take_photo()).await.map_err(failed)?.map_err(failed)?;
    Ok(taken.into_iter().map(|file| PickedView { path: file.path, name: file.name, mime: file.mime, size: file.size }).collect())
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PickedView {
    pub path: String,
    pub name: String,
    pub mime: String,
    pub size: u64,
}

/// Keeps the picker's copy, the contact pulls the chunks after `send_file` returns.
#[tauri::command]
pub async fn core_send_picked(contact: String, file: PickedView, client: State<'_, Client>) -> Result<(), String> {
    let path = picked_path(client.dir()?, &file.path)?;
    let core = client.core().await?;
    tauri::async_runtime::spawn(async move {
        let _ = core.send_file(&contact, &path, &file.name, &file.mime).await;
    });
    Ok(())
}

#[tauri::command]
pub async fn core_send(contact: String, text: String, reply_to: Option<String>, client: State<'_, Client>) -> Result<String, String> {
    if text.trim().is_empty() {
        return Err("nothing to send".to_owned());
    }
    let core = client.core().await?;
    let sent = core.queue_text(&contact, &text, reply_to.as_deref()).await.map_err(failed)?;
    let queued = sent.clone();
    tauri::async_runtime::spawn(async move {
        core.deliver_queued(&queued).await;
    });
    Ok(sent)
}

#[tauri::command]
pub async fn core_resend(message: String, client: State<'_, Client>) -> Result<(), String> {
    let core = client.core().await?;
    tauri::async_runtime::spawn(async move {
        let _ = core.resend(&message).await;
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

#[tauri::command]
pub async fn core_typing(contact: String, client: State<'_, Client>) -> Result<bool, String> {
    client.core().await?.typing(&contact).await.map_err(failed)
}

#[tauri::command]
pub async fn core_set_receipts(enabled: bool, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.set_receipts_default(enabled).await.map_err(failed)
}

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

/// seconds, 0 = forever / never
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

#[tauri::command]
pub async fn core_remove_contact(contact: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.remove_contact(&contact).await.map_err(failed)
}


// Drive tokens, the vault key and the recovery code never go to the WebView.

pub const VAULT_EVENT: &str = "ft://vault";
pub const VAULT_PROGRESS_EVENT: &str = "ft://vault-progress";

#[derive(Serialize, Clone)]
pub struct VaultProgressView {
    pub done: u64,
    pub total: u64,
}

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

#[tauri::command]
pub async fn core_vault_status(client: State<'_, Client>) -> Result<VaultStatusView, String> {
    Ok(client.core().await?.vault_status().await.map_err(failed)?.into())
}

#[tauri::command]
pub async fn core_vault_connect(provider: String, app: AppHandle, client: State<'_, Client>) -> Result<VaultStatusView, String> {
    let core = client.core().await?;
    Ok(core.vault_connect(&provider, &PlatformAuthorizer(app)).await.map_err(failed)?.into())
}

#[tauri::command]
pub async fn core_vault_setup(phrase: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.vault_setup(&phrase).await.map_err(failed)
}

#[tauri::command]
pub async fn core_vault_unlock(phrase: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.vault_unlock(&phrase).await.map_err(failed)
}

#[tauri::command]
pub async fn core_vault_suggest_phrase(client: State<'_, Client>) -> Result<String, String> {
    Ok(client.core().await?.vault_suggest_phrase())
}

#[tauri::command]
pub async fn core_vault_change_phrase(phrase: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.vault_change_phrase(&phrase).await.map_err(failed)
}

#[tauri::command]
pub async fn core_vault_disconnect(client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.vault_disconnect().await.map_err(failed)
}

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

#[tauri::command]
pub async fn core_vault_upload_picked(parent: Option<String>, app: AppHandle, client: State<'_, Client>) -> Result<usize, String> {
    let dir = client.dir()?.to_owned();
    let core = client.core().await?;
    let picked = core_pick_files(None, app).await?;
    upload_all_picked(&dir, &picked, |file, path| {
        let (core, parent) = (core.clone(), parent.clone());
        async move { core.vault_upload(&path, &file.name, &file.mime, parent.as_deref()).await }
    })
    .await
}

/// Every picker copy gets deleted, uploaded or not. The drive queue keeps a sealed copy, never the
/// path.
pub async fn upload_all_picked<Upload, Uploading>(dir: &Path, picked: &[PickedView], mut upload: Upload) -> Result<usize, String>
where
    Upload: FnMut(PickedView, PathBuf) -> Uploading,
    Uploading: std::future::Future<Output = anyhow::Result<Option<String>>>,
{
    let mut went = Ok(0);
    for file in picked {
        if let Ok(count) = went {
            went = match picker_copy(dir, &file.path) {
                Ok(path) => upload(file.clone(), path).await.map(|_| count + 1).map_err(failed),
                Err(error) => Err(error),
            };
        }
        forget_picked(dir, &file.path);
    }
    went
}

#[tauri::command]
pub async fn core_vault_upload_message(message: String, parent: Option<String>, client: State<'_, Client>) -> Result<Option<String>, String> {
    let file = file_of(&client, &message).await?;
    let core = client.core().await?;
    core.vault_upload(Path::new(&file.path), &file.name, &file.mime, parent.as_deref()).await.map_err(failed)
}

#[tauri::command]
pub async fn core_vault_retry(client: State<'_, Client>) -> Result<usize, String> {
    client.core().await?.vault_run_queue().await.map_err(failed)
}

#[tauri::command]
pub async fn core_vault_cancel_pending(blob: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.vault_cancel_pending(&blob).await.map_err(failed)
}

#[tauri::command]
pub async fn core_vault_download(id: String, client: State<'_, Client>) -> Result<PickedView, String> {
    let (path, file) = client.core().await?.vault_download(&id).await.map_err(failed)?;
    Ok(PickedView { path: path.to_string_lossy().into_owned(), name: file.name, mime: file.mime, size: file.size })
}

#[tauri::command]
pub async fn core_vault_open(id: String, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let (path, file) = client.core().await?.vault_download(&id).await.map_err(failed)?;
    app.platform().open_file(&path.to_string_lossy(), &file.mime).map_err(failed)
}

#[tauri::command]
pub async fn core_vault_save(id: String, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let (path, file) = client.core().await?.vault_download(&id).await.map_err(failed)?;
    app.platform().save_to_downloads(&path.to_string_lossy(), &file.name, &file.mime).map_err(failed)
}

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

#[tauri::command]
pub async fn core_vault_restore(app: AppHandle, client: State<'_, Client>) -> Result<BackupView, String> {
    let core = client.core().await?;
    let backup = core.vault_restore().await.map_err(failed)?;
    if let Ok(online) = client.online().await {
        let _ = online.router.forget().await;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(RESTART_PAUSE).await;
        restart(&app).await;
    });
    Ok(backup.into())
}

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

    struct FakeShop {
        bought: Result<i64, String>,
        restored: Result<i64, String>,
        current: Result<i64, String>,
    }

    impl FakeShop {
        fn restoring(restored: Result<i64, String>) -> Self {
            Self { bought: Err("not asked".into()), restored, current: Err("not asked".into()) }
        }

        fn selling(bought: Result<i64, String>) -> Self {
            Self { bought, restored: Err("not asked".into()), current: Err("not asked".into()) }
        }

        fn saying(current: Result<i64, String>) -> Self {
            Self { bought: Err("not asked".into()), restored: Err("not asked".into()), current }
        }
    }

    impl Shop for FakeShop {
        fn subscribe(&self) -> Result<i64, String> {
            self.bought.clone()
        }

        fn restore(&self) -> Result<i64, String> {
            self.restored.clone()
        }

        fn current(&self) -> Result<i64, String> {
            self.current.clone()
        }
    }

    #[tokio::test]
    async fn the_plans_watch_hears_what_the_store_says_now() {
        use ft_core::Entitlements;
        assert_eq!(ShopWord(Arc::new(FakeShop::saying(Ok(42)))).until().await.unwrap(), 42);
        assert_eq!(ShopWord(Arc::new(FakeShop::saying(Ok(0)))).until().await.unwrap(), 0);
        assert!(ShopWord(Arc::new(FakeShop::saying(Err("store_unavailable".into())))).until().await.is_err());
    }

    const DAY_MS: i64 = 24 * 60 * 60 * 1000;

    #[test]
    fn restoring_says_whether_the_store_found_a_subscription() {
        let now = 1_800_000_000_000;
        assert_eq!(restore_from(&FakeShop::restoring(Ok(now + 300 * DAY_MS)), now), Ok((now + 300 * DAY_MS, "restored")));
        assert_eq!(restore_from(&FakeShop::restoring(Ok(0)), now), Ok((0, "nothing")));
        assert_eq!(restore_from(&FakeShop::restoring(Ok(now - DAY_MS)), now), Ok((now - DAY_MS, "nothing")));
        assert_eq!(restore_from(&FakeShop::restoring(Err("store_unavailable".into())), now), Err("store_unavailable".to_owned()));
    }

    #[test]
    fn a_purchase_answered_with_a_refunded_transaction_is_a_failed_payment() {
        let now = 1_800_000_000_000;
        assert_eq!(buy_from(&FakeShop::selling(Ok(0)), now), Err("payment_failed".to_owned()));
        assert_eq!(buy_from(&FakeShop::selling(Ok(now - DAY_MS)), now), Err("payment_failed".to_owned()));
        assert_eq!(buy_from(&FakeShop::selling(Ok(now + 365 * DAY_MS)), now), Ok(now + 365 * DAY_MS));
        assert_eq!(buy_from(&FakeShop::selling(Err("cancelled".into())), now), Err("cancelled".to_owned()));
        assert_eq!(buy_from(&FakeShop::selling(Err("pending_approval".into())), now), Err("pending_approval".to_owned()));
    }

    #[test]
    fn a_play_subscription_is_shown_as_renewing_and_an_apple_one_with_its_expiry() {
        let until = 1_830_000_000_000;
        let play = plan_view(ft_billing::Access::Subscribed { until }, true);
        assert_eq!(serde_json::to_value(&play).unwrap(), serde_json::json!({ "state": "subscribed", "until": until, "renews": true }));
        let apple = plan_view(ft_billing::Access::Subscribed { until }, false);
        assert_eq!(serde_json::to_value(&apple).unwrap()["renews"], serde_json::json!(false));
        for access in [ft_billing::Access::Trial { until }, ft_billing::Access::Limited] {
            assert_eq!(serde_json::to_value(plan_view(access, true)).unwrap()["renews"], serde_json::json!(false));
        }
    }

    #[test]
    fn the_plan_has_three_states_and_no_age() {
        let states: Vec<serde_json::Value> = [ft_billing::Access::Trial { until: 1 }, ft_billing::Access::Subscribed { until: 1 }, ft_billing::Access::Limited]
            .into_iter()
            .map(|access| serde_json::to_value(plan_view(access, false)).unwrap())
            .collect();
        assert_eq!(states.iter().map(|view| view["state"].as_str().unwrap()).collect::<Vec<_>>(), ["trial", "subscribed", "limited"]);
        assert!(states.iter().all(|view| view.get("age").is_none()));
    }

    #[test]
    fn a_closed_tool_is_not_served_and_a_game_is() {
        let plugin = |id: &str, kind: &str| ft_core::plugins::InstalledPlugin {
            manifest: serde_json::from_str(&format!(
                r#"{{"id":"{id}","name":"X","version":"1.0.0","minCoreVersion":"0.1.0","components":["ft-x"],"kind":"{kind}"}}"#
            ))
            .unwrap(),
            granted: ft_plugins::Permissions::default(),
            installed_at: 0,
            image: String::new(),
        };
        let dir = Path::new("/tmp/ft");
        let served = served_of(vec![(plugin("com.example.code", "tool"), false), (plugin("game.example.chess", "game"), true)], dir);
        assert_eq!(served.keys().collect::<Vec<_>>(), ["game.example.chess"]);
        assert_eq!(served["game.example.chess"].dir, dir.join("plugins").join("game.example.chess"));
        let served = served_of(vec![(plugin("com.example.code", "tool"), true)], dir);
        assert!(served.contains_key("com.example.code"), "open in the free year or with the subscription");
    }

    #[test]
    fn what_became_of_a_suggestion_reaches_the_screen_as_one_of_three_words() {
        assert_eq!(feedback_word(ft_core::Feedback::Sent), "sent");
        assert_eq!(feedback_word(ft_core::Feedback::TooMany), "tooMany");
        assert_eq!(feedback_word(ft_core::Feedback::Failed), "failed");
    }

    #[test]
    fn the_location_permission_goes_to_the_screen_and_back() {
        let asked = ft_plugins::Permissions { location: true, ..ft_plugins::Permissions::default() };
        let shown = serde_json::to_value(PermissionsView::from(&asked)).unwrap();
        assert_eq!(shown["location"], serde_json::json!(true), "{shown}");
        let switched: PermissionsView = serde_json::from_value(serde_json::json!({
            "network": [], "messages": false, "send": "nothing", "location": true
        }))
        .unwrap();
        assert!(ft_plugins::Permissions::from(switched).location);
        let silent: PermissionsView = serde_json::from_value(serde_json::json!({ "network": [], "messages": false, "send": "nothing" })).unwrap();
        assert!(!ft_plugins::Permissions::from(silent).location);
    }

    #[test]
    fn the_translate_permission_goes_to_the_screen_and_back() {
        let asked = ft_plugins::Permissions { translate: true, ..ft_plugins::Permissions::default() };
        let shown = serde_json::to_value(PermissionsView::from(&asked)).unwrap();
        assert_eq!(shown["translate"], serde_json::json!(true), "{shown}");
        let switched: PermissionsView = serde_json::from_value(serde_json::json!({
            "network": [], "messages": false, "send": "nothing", "translate": true
        }))
        .unwrap();
        assert!(ft_plugins::Permissions::from(switched).translate);
        let silent: PermissionsView = serde_json::from_value(serde_json::json!({ "network": [], "messages": false, "send": "nothing" })).unwrap();
        assert!(!ft_plugins::Permissions::from(silent).translate);
    }

    #[test]
    fn a_pair_reaches_the_screen_as_one_of_three_words() {
        use tauri_plugin_ft_platform::TranslateStatus;
        for (bridge, word) in [
            (TranslateStatus::Installed, "installed"),
            (TranslateStatus::Downloadable, "downloadable"),
            (TranslateStatus::Unsupported, "unsupported"),
        ] {
            assert_eq!(serde_json::to_value(pair_status(bridge)).unwrap(), serde_json::json!(word), "{bridge:?}");
        }
    }

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

    #[test]
    fn the_versions_go_together_and_play_takes_the_next_one() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let version = conf["version"].as_str().unwrap();
        assert_eq!(version, ft_core::plugins::CORE_VERSION);
        assert_eq!(version, env!("CARGO_PKG_VERSION"));
        let parts: Vec<u64> = version.split('.').map(|part| part.parse().unwrap()).collect();
        let version_code = parts[0] * 1_000_000 + parts[1] * 1_000 + parts[2];
        assert!(version_code > 1_002_001, "versionCode {version_code} is not above 1002001");
    }

    #[test]
    fn the_ios_app_icons_have_no_alpha_channel() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("gen/apple/Assets.xcassets/AppIcon.appiconset");
        let mut icons = 0;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("png") {
                continue;
            }
            let bytes = std::fs::read(&path).unwrap();
            assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "{} is not a PNG", path.display());
            let mut chunks = Vec::new();
            let mut at = 8;
            while at + 8 <= bytes.len() {
                let length = u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize;
                chunks.push((bytes[at + 4..at + 8].to_vec(), at + 8));
                at += 12 + length;
            }
            let (kind, header) = &chunks[0];
            assert_eq!(kind.as_slice(), b"IHDR");
            let colour = bytes[header + 9];
            assert!(![4, 6].contains(&colour), "{} has an alpha channel (colour type {colour})", path.display());
            assert!(!chunks.iter().any(|(kind, _)| kind.as_slice() == b"tRNS"), "{} has transparency", path.display());
            icons += 1;
        }
        assert!(icons >= 1, "no icons in {}", dir.display());
    }

    #[test]
    fn the_push_provider_is_the_one_of_the_platform() {
        assert_eq!(push_provider(), if cfg!(target_os = "ios") { "apns" } else { "fcm" });
    }

    #[test]
    fn the_drive_s_state_tells_the_page_what_it_needs() {
        use ft_core::vault::{VaultState, VaultStatus};
        let view = VaultStatusView::from(VaultStatus { state: VaultState::Outdated, provider: Some("google".into()), drive: None, problem: None, tries_left: 5, retry_at: None });
        assert_eq!(view.state, "outdated");
        let view = VaultStatusView::from(VaultStatus { state: VaultState::Locked, provider: Some("google".into()), drive: None, problem: None, tries_left: 0, retry_at: Some(42) });
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!((json["state"].as_str(), json["triesLeft"].as_u64(), json["retryAt"].as_i64()), (Some("locked"), Some(0), Some(42)));
    }

    #[test]
    fn what_travels_inside_the_app_stays_tiny() {
        for package in BUNDLED_PLUGINS {
            let plugin = ft_plugins::open(package, &ft_plugins::catalogue()).expect("a seed is not signed for us");
            assert!(!plugin.manifest.components.is_empty(), "{} shows nothing", plugin.manifest.id);
            assert!(plugin.file("dist/index.js").is_some(), "{} has no code", plugin.manifest.id);
        }
        let weights = BUNDLED_PLUGINS.iter().map(|package| package.len() as u64);
        assert_eq!(seeds_weight_problem(weights), None);
    }

    #[test]
    fn every_seed_runs_on_this_core() {
        for package in BUNDLED_PLUGINS {
            let manifest = ft_plugins::open(package, &ft_plugins::catalogue()).expect("a seed is signed for us").manifest;
            assert!(
                ft_plugins::version_at_least(ft_core::plugins::CORE_VERSION, &manifest.min_core_version),
                "{} needs FlickerTalk {}",
                manifest.id,
                manifest.min_core_version
            );
        }
    }

    #[test]
    fn a_heavy_seed_is_a_download() {
        assert!(seeds_weight_problem([5 * 1024 * 1024]).is_some(), "a seed of 5 MiB is refused");
        assert_eq!(seeds_weight_problem([3 * 1024 * 1024 + 512 * 1024]), None, "the PDF viewer travels");
        assert!(seeds_weight_problem([SEED_LIMIT + 1]).is_some(), "a seed just over the limit is refused");
        assert_eq!(seeds_weight_problem([SEED_LIMIT]), None, "a seed at the limit travels");
        let many = vec![SEED_LIMIT; (SEEDS_LIMIT / SEED_LIMIT) as usize + 1];
        assert!(seeds_weight_problem(many).is_some(), "seeds that are light one by one but heavy together are refused");
    }

    #[test]
    #[ignore = "needs the web repo's catalogue on disk"]
    fn every_seed_is_the_package_the_catalogue_serves() {
        let served = std::env::var("FT_CATALOGUE_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/site/plugins"));
        for package in BUNDLED_PLUGINS {
            let manifest = ft_plugins::open(package, &ft_plugins::catalogue()).expect("a seed is signed for us").manifest;
            let path = served.join(&manifest.id).join(format!("{}.ftplugin", manifest.version));
            let listed = std::fs::read(&path).unwrap_or_else(|_| panic!("{} {} is not in the catalogue ({})", manifest.id, manifest.version, path.display()));
            assert!(listed == *package, "the seed of {} {} is not the package the catalogue serves", manifest.id, manifest.version);
        }
    }

    #[test]
    fn a_seed_that_writes_in_the_chat_asks_for_it() {
        for package in BUNDLED_PLUGINS {
            let plugin = ft_plugins::open(package, &ft_plugins::catalogue()).expect("a seed is signed for us");
            let code = String::from_utf8_lossy(plugin.file("dist/index.js").unwrap_or_default());
            let writes = ["ft.send(", "ft?.send(", "ft.say(", "ft?.say("].iter().any(|call| code.contains(call));
            if writes {
                assert_ne!(plugin.manifest.permissions.send, ft_plugins::Sending::Nothing, "{} writes in the chat without asking", plugin.manifest.id);
            }
        }
    }

    #[test]
    fn the_app_carries_its_tools_and_games() {
        const TOOLS: [&str; 19] = [
            "markdown", "images", "pdf", "redact", "sketch", "scanner", "countdown", "board", "clean", "drive", "list", "location",
            "notes", "poll", "sign", "split", "pdfviewer", "translate", "remind",
        ];
        const GAMES: [&str; 13] = [
            "tictactoe", "fourinarow", "chess", "backgammon", "checkers", "dotsandboxes", "eights", "gomoku", "mancala", "reversi",
            "seabattle", "wordduel", "wordgrid",
        ];
        let offered = seeds();
        let wanted = TOOLS
            .iter()
            .map(|tool| (format!("com.flickertalk.{tool}"), ft_plugins::Kind::Tool))
            .chain(GAMES.iter().map(|game| (format!("com.flickertalk.game.{game}"), ft_plugins::Kind::Game)));
        for (id, kind) in wanted {
            let seed = offered.iter().find(|one| one.id == id).unwrap_or_else(|| panic!("{id} is not carried"));
            assert_eq!(seed.kind, kind, "{id} is a {kind:?}");
            assert!(seed.carried, "{id} is inside the app");
        }
        assert_eq!(offered.len(), TOOLS.len() + GAMES.len(), "every package the app carries is one it means to carry");
    }

    #[test]
    fn every_seed_speaks_the_app_s_languages() {
        const LANGUAGES: [&str; 20] =
            ["es", "pt", "fr", "de", "it", "ro", "ru", "uk", "pl", "tr", "ar", "hi", "bn", "id", "vi", "th", "ja", "ko", "zh-CN", "zh-TW"];
        const KEEP_THEIR_NAME: [&str; 2] = ["com.flickertalk.markdown", "com.flickertalk.pdf"];
        for package in BUNDLED_PLUGINS {
            let manifest = ft_plugins::open(package, &ft_plugins::catalogue()).expect("a seed is signed for us").manifest;
            let mut codes: Vec<&str> = manifest.locales.keys().map(String::as_str).collect();
            let mut wanted = LANGUAGES.to_vec();
            codes.sort_unstable();
            wanted.sort_unstable();
            assert_eq!(codes, wanted, "{} speaks the app's languages", manifest.id);
            for code in LANGUAGES {
                let said = &manifest.locales[code];
                assert!(said.summary.as_deref().is_some_and(|summary| !summary.trim().is_empty()), "{} has no summary in {code}", manifest.id);
                let named = said.name.as_deref().is_some_and(|name| !name.trim().is_empty());
                assert_eq!(named, !KEEP_THEIR_NAME.contains(&manifest.id.as_str()), "{}: its name in {code}", manifest.id);
            }
        }
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
            kind: ft_plugins::Kind::Tool,
            locales: Default::default(),
            icon: String::new(),
            image: String::new(),
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
            kind: ft_plugins::Kind::Tool,
            locales: Default::default(),
            icon: String::new(),
            image: String::new(),
        }
    }

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
    fn the_list_keeps_whether_each_one_is_a_tool_or_a_game() {
        let seeds = vec![carried("com.flickertalk.sketch", "1.0.0")];
        let chess = ft_plugins::CatalogueEntry { kind: ft_plugins::Kind::Game, ..entry("com.flickertalk.game.chess", "1.0.0") };
        let offered = serde_json::to_value(merged(seeds, &[chess], &[])).unwrap();
        let kinds: Vec<(&str, &str)> = offered
            .as_array()
            .unwrap()
            .iter()
            .map(|one| (one["id"].as_str().unwrap(), one["kind"].as_str().unwrap_or("missing")))
            .collect();
        assert!(kinds.contains(&("com.flickertalk.game.chess", "game")), "{kinds:?}");
        assert!(kinds.contains(&("com.flickertalk.sketch", "tool")), "{kinds:?}");
    }

    #[test]
    fn an_installed_plugin_says_whether_it_is_a_game() {
        let manifest: ft_plugins::Manifest = serde_json::from_str(
            r#"{"id":"com.flickertalk.game.chess","name":"Chess","version":"1.0.0","minCoreVersion":"1.3.0","components":["ft-chess"],"kind":"game","permissions":{"live":true}}"#,
        )
        .unwrap();
        let view = plugin_view(ft_core::plugins::InstalledPlugin { manifest, granted: Default::default(), installed_at: 7, image: String::new() });
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(json["kind"], "game");
        assert_eq!((json["id"].as_str(), json["installedAt"].as_i64()), (Some("com.flickertalk.game.chess"), Some(7)));
        assert_eq!((json["asks"]["live"].as_bool(), json["granted"]["live"].as_bool()), (Some(true), Some(false)));
    }

    #[test]
    fn every_view_of_a_plugin_keeps_its_translations() {
        let manifest: ft_plugins::Manifest = serde_json::from_str(
            r#"{"id":"com.flickertalk.list","name":"List","version":"1.0.0","minCoreVersion":"1.3.0","components":["ft-list"],"summary":"A list.","locales":{"es":{"name":"Listas","summary":"Una lista."}}}"#,
        )
        .unwrap();
        let view = plugin_view(ft_core::plugins::InstalledPlugin { manifest: manifest.clone(), granted: Default::default(), installed_at: 7, image: String::new() });
        assert_eq!(serde_json::to_value(&view).unwrap()["locales"]["es"]["name"], "Listas");

        let seed = serde_json::to_value(offered_from(manifest, String::new(), 4096)).unwrap();
        assert_eq!((seed["locales"]["es"]["summary"].as_str(), seed["carried"].as_bool()), (Some("Una lista."), Some(true)));

        let spanish = |name: &str| {
            [("es".to_owned(), ft_plugins::Localized { name: Some(name.to_owned()), ..Default::default() })].into_iter().collect()
        };
        let seeds = vec![
            OfferedPlugin { locales: spanish("Dibujo"), ..carried("com.flickertalk.sketch", "1.0.0") },
            OfferedPlugin { locales: spanish("Imagen"), ..carried("com.flickertalk.images", "2.0.0") },
        ];
        let listed = vec![
            ft_plugins::CatalogueEntry { locales: spanish("Boceto"), ..entry("com.flickertalk.sketch", "1.1.0") },
            ft_plugins::CatalogueEntry { locales: spanish("Foto"), ..entry("com.flickertalk.images", "1.0.0") },
            ft_plugins::CatalogueEntry { locales: spanish("Ajedrez"), ..entry("com.flickertalk.game.chess", "1.0.0") },
        ];
        let offered = serde_json::to_value(merged(seeds, &listed, &[])).unwrap();
        let spanish_of = |id: &str| {
            let one = offered.as_array().unwrap().iter().find(|one| one["id"] == id).unwrap();
            one["locales"]["es"]["name"].as_str().unwrap_or("missing").to_owned()
        };
        assert_eq!(spanish_of("com.flickertalk.sketch"), "Boceto", "the catalogue's is newer");
        assert_eq!(spanish_of("com.flickertalk.images"), "Imagen", "the app's is newer");
        assert_eq!(spanish_of("com.flickertalk.game.chess"), "Ajedrez", "only in the catalogue");
    }

    #[test]
    fn an_installed_tool_says_which_hooks_it_binds_to() {
        let manifest: ft_plugins::Manifest = serde_json::from_str(
            r#"{"id":"com.flickertalk.translate","name":"Translator","version":"1.1.0","minCoreVersion":"1.7.0","components":["ft-translate"],"permissions":{"messages":"given","send":"propose"},"hooks":{"message":{"label":"Translate","icon":"language-outline"},"settings":{"label":"Translation","icon":"language-outline","fields":[{"key":"lang","type":"select","label":"Language"},{"key":"receive","type":"toggle","label":"Translate what I receive"}],"note":"Packs are downloaded once."}},"locales":{"es":{"name":"Traductor","hooks":{"message":"Traducir","settings":{"label":"Traducción","fields":{"lang":"Idioma"}}}}}}"#,
        )
        .unwrap();
        let plain = ft_plugins::Manifest { hooks: Default::default(), ..manifest.clone() };
        let view = |manifest: ft_plugins::Manifest| {
            serde_json::to_value(plugin_view(ft_core::plugins::InstalledPlugin { manifest, granted: Default::default(), installed_at: 7, image: String::new() })).unwrap()
        };
        let json = view(manifest);
        assert_eq!(
            json["hooks"],
            serde_json::json!({
                "message": { "label": "Translate", "icon": "language-outline" },
                "settings": {
                    "label": "Translation",
                    "icon": "language-outline",
                    "fields": [
                        { "key": "lang", "type": "select", "label": "Language" },
                        { "key": "receive", "type": "toggle", "label": "Translate what I receive" }
                    ],
                    "note": "Packs are downloaded once."
                }
            })
        );
        assert_eq!(json["locales"]["es"]["hooks"]["settings"]["fields"]["lang"], "Idioma");
        assert!(view(plain).get("hooks").is_none(), "bound to none, nothing written");
    }

    #[test]
    fn every_view_of_a_plugin_keeps_its_icon() {
        let drawn: ft_plugins::Manifest = serde_json::from_str(
            r#"{"id":"com.flickertalk.list","name":"List","version":"1.0.0","minCoreVersion":"1.3.0","components":["ft-list"],"icon":"list-outline"}"#,
        )
        .unwrap();
        let plain = ft_plugins::Manifest { icon: String::new(), ..drawn.clone() };
        let installed = |manifest: &ft_plugins::Manifest| {
            serde_json::to_value(plugin_view(ft_core::plugins::InstalledPlugin { manifest: manifest.clone(), granted: Default::default(), installed_at: 7, image: String::new() }))
                .unwrap()
        };
        assert_eq!(installed(&drawn)["icon"], "list-outline");
        assert!(installed(&plain).get("icon").is_none(), "no icon, nothing written");

        let seed = serde_json::to_value(offered_from(drawn, String::new(), 4096)).unwrap();
        assert_eq!(seed["icon"], "list-outline");
        assert!(serde_json::to_value(offered_from(plain, String::new(), 4096)).unwrap().get("icon").is_none());

        let listed = vec![
            ft_plugins::CatalogueEntry { icon: "brush-outline".to_owned(), ..entry("com.flickertalk.sketch", "1.1.0") },
            entry("com.flickertalk.pdf", "1.0.0"),
        ];
        let offered = serde_json::to_value(merged(vec![], &listed, &[])).unwrap();
        let icon_of = |id: &str| offered.as_array().unwrap().iter().find(|one| one["id"] == id).unwrap().get("icon").cloned();
        assert_eq!(icon_of("com.flickertalk.sketch"), Some(serde_json::json!("brush-outline")));
        assert_eq!(icon_of("com.flickertalk.pdf"), None);
    }

    #[test]
    fn every_view_of_a_plugin_keeps_its_image() {
        const IMAGE: &str = r#"<svg viewBox="0 0 64 64"><rect width="64" height="64"/></svg>"#;
        let manifest: ft_plugins::Manifest = serde_json::from_str(
            r#"{"id":"com.flickertalk.list","name":"List","version":"1.0.0","minCoreVersion":"1.3.0","components":["ft-list"]}"#,
        )
        .unwrap();
        let installed = |image: &str| {
            serde_json::to_value(plugin_view(ft_core::plugins::InstalledPlugin {
                manifest: manifest.clone(),
                granted: Default::default(),
                installed_at: 7,
                image: image.to_owned(),
            }))
            .unwrap()
        };
        assert_eq!(installed(IMAGE)["image"], IMAGE);
        assert!(installed("").get("image").is_none(), "no image, nothing written");
        assert_eq!(serde_json::to_value(offered_from(manifest.clone(), IMAGE.to_owned(), 4096)).unwrap()["image"], IMAGE);
        let listed = vec![
            ft_plugins::CatalogueEntry { image: IMAGE.to_owned(), ..entry("com.flickertalk.sketch", "1.1.0") },
            entry("com.flickertalk.pdf", "1.0.0"),
        ];
        let offered = serde_json::to_value(merged(vec![], &listed, &[])).unwrap();
        let image_of = |id: &str| offered.as_array().unwrap().iter().find(|one| one["id"] == id).unwrap().get("image").cloned();
        assert_eq!(image_of("com.flickertalk.sketch"), Some(serde_json::json!(IMAGE)));
        assert_eq!(image_of("com.flickertalk.pdf"), None);
    }

    #[test]
    fn a_tool_already_here_shows_as_installed() {
        let here = ["com.flickertalk.sketch".to_owned()];
        let offered = merged(vec![carried("com.flickertalk.sketch", "1.0.0")], &[], &here);
        assert!(offered[0].installed);
    }

    #[test]
    fn a_plugin_is_open_until_every_frame_of_it_has_closed() {
        let open = OpenPlugins::default();
        let ids = |open: &OpenPlugins| {
            let mut ids: Vec<String> = open.ids().into_iter().collect();
            ids.sort();
            ids
        };
        open.set("com.example.a", true);
        open.set("com.example.a", true);
        open.set("com.example.b", true);
        open.set("com.example.a", false);
        assert_eq!(ids(&open), ["com.example.a", "com.example.b"], "one frame of it is still there");
        open.set("com.example.a", false);
        open.set("com.example.a", false);
        assert_eq!(ids(&open), ["com.example.b"]);
        open.set("com.example.a", true);
        assert_eq!(ids(&open), ["com.example.a", "com.example.b"]);
    }

    #[test]
    fn updates_are_looked_for_at_most_every_twelve_hours() {
        let now = 1_800_000_000_000_i64;
        let hour = 3_600_000;
        assert!(updates_due(None, now), "never looked");
        assert!(!updates_due(Some(now - 11 * hour), now));
        assert!(updates_due(Some(now - 12 * hour), now));
        assert!(updates_due(Some(now + hour), now), "the clock went back");

        let dir = std::env::temp_dir().join(format!("ft-updates-{now}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(last_update_look(&dir), None);
        looked_for_updates(&dir, now);
        assert_eq!(last_update_look(&dir), Some(now));
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
        let names: Vec<_> = [MessageState::NotSent, MessageState::Pending, MessageState::Sent, MessageState::Delivered, MessageState::Read]
            .into_iter()
            .map(state_name)
            .collect();
        assert_eq!(names, ["unsent", "pending", "sent", "delivered", "read"]);
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
            unblocked_at: None,
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

    #[test]
    fn only_whole_files_can_be_opened() {
        assert!(openable(Some(record(4, true, false)), false).is_ok());
        assert!(openable(Some(record(2, false, false)), false).is_err());
        assert!(openable(Some(record(0, false, true)), false).is_err());
        assert!(openable(Some(record(1, false, false)), true).is_ok());
        assert!(openable(None, true).is_err());
    }

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
        assert_eq!(serde_json::to_value(CallEvent::new("ft_bob", "c1", CallUpdate::Connected)).unwrap(), serde_json::json!({
            "contact": "ft_bob", "call": "c1", "kind": "connected"
        }));
        assert_eq!(serde_json::to_value(CallEvent::new("ft_bob", "c1", CallUpdate::Muted { muted: true })).unwrap(), serde_json::json!({
            "contact": "ft_bob", "call": "c1", "kind": "muted", "muted": true
        }));
    }

    #[test]
    fn what_is_presented_reaches_the_webview() {
        let file = ft_core::MessageId::new();
        let viewer = CallUpdate::Presenting { plugin: Some("com.flickertalk.pdfviewer".to_owned()), file: Some(file), by: Some(PresentedBy::Them) };
        assert_eq!(serde_json::to_value(CallEvent::new("ft_bob", "c1", viewer.clone())).unwrap(), serde_json::json!({
            "contact": "ft_bob", "call": "c1", "kind": "presenting", "plugin": "com.flickertalk.pdfviewer", "file": file.to_string(), "by": "them"
        }));
        let board = CallUpdate::Presenting { plugin: Some("com.flickertalk.board".to_owned()), file: None, by: Some(PresentedBy::Me) };
        assert_eq!(serde_json::to_value(CallEvent::new("ft_bob", "c1", board)).unwrap(), serde_json::json!({
            "contact": "ft_bob", "call": "c1", "kind": "presenting", "plugin": "com.flickertalk.board", "by": "me"
        }));
        let stopped = CallUpdate::Presenting { plugin: None, file: None, by: None };
        assert_eq!(serde_json::to_value(CallEvent::new("ft_bob", "c1", stopped)).unwrap(), serde_json::json!({
            "contact": "ft_bob", "call": "c1", "kind": "presenting"
        }));
        assert_eq!(native_screen(&viewer), NativeScreen::Nothing);
        assert_eq!(ringing(&viewer, false), Ring::Nothing);
    }

    #[test]
    fn connected_says_whether_the_call_can_present() {
        let connected = CallEvent::new("ft_bob", "c1", CallUpdate::Connected).can_present(true);
        assert_eq!(serde_json::to_value(connected).unwrap(), serde_json::json!({
            "contact": "ft_bob", "call": "c1", "kind": "connected", "canPresent": true
        }));
        let current = |can_present| ft_core::CurrentCall {
            call: "c1".to_owned(),
            contact: "ft_bob".to_owned(),
            video: false,
            outgoing: true,
            phase: ft_core::CallPhase::Active,
            offer: None,
            native: true,
            muted: false,
            connected_at: Some(7),
            video_state: None,
            presenting: None,
            can_present,
        };
        assert!(can_present_now(Some(&current(true)), "c1"));
        assert!(!can_present_now(Some(&current(false)), "c1"));
        assert!(!can_present_now(Some(&current(true)), "c2"), "another call");
        assert!(!can_present_now(None, "c1"));
    }

    #[test]
    fn the_current_call_carries_what_is_presented() {
        let file = ft_core::MessageId::new();
        let active = ft_core::CurrentCall {
            call: "c1".to_owned(),
            contact: "ft_bob".to_owned(),
            video: false,
            outgoing: true,
            phase: ft_core::CallPhase::Active,
            offer: None,
            native: true,
            muted: false,
            connected_at: Some(7),
            video_state: None,
            presenting: Some(ft_core::Presenting { plugin: "com.flickertalk.pdfviewer".to_owned(), file: Some(file), by: PresentedBy::Them }),
            can_present: true,
        };
        let view = serde_json::to_value(CurrentCallView::from(active.clone())).unwrap();
        assert_eq!(view["canPresent"], true);
        assert_eq!(view["presenting"], serde_json::json!({ "plugin": "com.flickertalk.pdfviewer", "file": file.to_string(), "by": "them" }));
        let board = ft_core::CurrentCall {
            presenting: Some(ft_core::Presenting { plugin: "com.flickertalk.board".to_owned(), file: None, by: PresentedBy::Me }),
            ..active
        };
        assert_eq!(serde_json::to_value(CurrentCallView::from(board)).unwrap()["presenting"], serde_json::json!({ "plugin": "com.flickertalk.board", "by": "me" }));
    }

    #[test]
    fn a_presented_file_is_a_message_id() {
        assert_eq!(presented_file(None), Ok(None));
        let file = ft_core::MessageId::new();
        assert_eq!(presented_file(Some(file.to_string())), Ok(Some(file)));
        assert!(presented_file(Some("not an id".to_owned())).is_err());
    }

    #[test]
    fn a_presented_file_that_is_not_a_message_id_belongs_to_no_chat() {
        let file = ft_core::MessageId::new();
        assert_eq!(presented_message(&file.to_string()), Some(file));
        assert_eq!(presented_message("not an id"), None);
    }

    #[test]
    fn the_native_call_screen_hears_when_a_call_connects_and_ends() {
        assert_eq!(native_screen(&CallUpdate::Connected), NativeScreen::Connected);
        assert_eq!(native_screen(&CallUpdate::Ended { outcome: CallOutcome::Answered }), NativeScreen::Ended);
        assert_eq!(native_screen(&CallUpdate::Answered { sdp: String::new() }), NativeScreen::Nothing);
        assert_eq!(native_screen(&CallUpdate::Muted { muted: true }), NativeScreen::Nothing);
        assert_eq!(native_screen(&CallUpdate::Incoming { video: false, sdp: String::new() }), NativeScreen::Nothing);
    }

    #[test]
    fn a_call_refused_as_busy_leaves_the_phone_s_call_screen_alone() {
        assert_eq!(native_screen(&CallUpdate::MissedWhileBusy), NativeScreen::Nothing);
        assert_eq!(ringing(&CallUpdate::MissedWhileBusy, false), Ring::Nothing);
        let event = serde_json::to_value(CallEvent::new("ft_carol", "c2", CallUpdate::MissedWhileBusy)).unwrap();
        assert_eq!(event, serde_json::json!({ "contact": "ft_carol", "call": "c2", "kind": "ended", "outcome": "missed" }));
    }

    #[test]
    fn a_call_push_the_answer_and_the_audio_activation_reconnect_to_the_router() {
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

    #[test]
    fn the_call_route_reaches_the_call_screen() {
        use tauri_plugin_ft_platform::CallRoute;
        assert_eq!(
            serde_json::to_value(CallEvent::route("ft_bob", "c1", CallRoute::Headset)).unwrap(),
            serde_json::json!({ "contact": "ft_bob", "call": "c1", "kind": "route", "route": "headset" })
        );
        assert_eq!(reconnect_for(NativeCallEvent::Route(CallRoute::Speaker)), Reconnect::No);
        let mut routes = CallRoutes::default();
        assert_eq!(routes.of("c1"), None);
        routes.set("c1", CallRoute::Headset);
        assert_eq!(routes.of("c1"), Some(CallRoute::Headset));
        assert_eq!(routes.of("c2"), None, "another call's route is not this one's");
        routes.set("c2", CallRoute::Speaker);
        assert_eq!(routes.of("c1"), None);
        assert_eq!(routes.of("c2"), Some(CallRoute::Speaker));
        let active = ft_core::CurrentCall {
            call: "c2".to_owned(),
            contact: "ft_bob".to_owned(),
            video: false,
            outgoing: true,
            phase: ft_core::CallPhase::Active,
            offer: None,
            native: true,
            muted: false,
            connected_at: Some(7),
            video_state: None,
            presenting: None,
            can_present: false,
        };
        let view = serde_json::to_value(CurrentCallView::from(active.clone()).route(routes.of("c2"))).unwrap();
        assert_eq!(view["route"], "speaker");
        let unknown = serde_json::to_value(CurrentCallView::from(active).route(None)).unwrap();
        assert!(unknown.get("route").is_none(), "no route heard yet");
    }

    #[test]
    fn the_platform_says_whether_the_app_is_in_the_foreground() {
        assert_eq!(foreground_of(NativeCallEvent::Visible(false)), Some(false));
        assert_eq!(foreground_of(NativeCallEvent::Visible(true)), Some(true));
        for other in [
            NativeCallEvent::Incoming,
            NativeCallEvent::Answer,
            NativeCallEvent::End,
            NativeCallEvent::Decline,
            NativeCallEvent::Mute(false),
            NativeCallEvent::AudioActivated(1),
            NativeCallEvent::AudioDeactivated(1),
            NativeCallEvent::Orientation(1),
            NativeCallEvent::VideoRequested,
        ] {
            assert_eq!(foreground_of(other), None, "{other:?}");
        }
    }

    #[test]
    fn an_answered_call_never_rings() {
        assert_eq!(ringing(&CallUpdate::Incoming { video: true, sdp: String::new() }, true), Ring::Answered);
        assert_eq!(ringing(&CallUpdate::Answering, false), Ring::Answered);
        assert_eq!(native_screen(&CallUpdate::Answering), NativeScreen::Nothing);
    }

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
            presenting: None,
            can_present: false,
        };
        assert!(answered_already(Some(&current(ft_core::CallPhase::Connecting)), "c1"));
        assert!(!answered_already(Some(&current(ft_core::CallPhase::Ringing)), "c1"));
        assert!(!answered_already(Some(&current(ft_core::CallPhase::Connecting)), "c2"), "another call");
        assert!(!answered_already(None, "c1"));
    }

    #[test]
    fn a_decline_asks_for_the_router() {
        assert_eq!(reconnect_for(NativeCallEvent::Decline), Reconnect::UnlessFresh);
    }

    #[test]
    fn a_core_a_push_started_acts_only_on_the_push_a_decline_and_a_hang_up() {
        assert!(push_core_handles(NativeCallEvent::Incoming));
        assert!(push_core_handles(NativeCallEvent::Decline));
        assert!(push_core_handles(NativeCallEvent::End));
        for waits in [
            NativeCallEvent::Answer,
            NativeCallEvent::Mute(true),
            NativeCallEvent::AudioActivated(1),
            NativeCallEvent::AudioDeactivated(1),
            NativeCallEvent::Visible(true),
            NativeCallEvent::Orientation(90),
            NativeCallEvent::VideoRequested,
        ] {
            assert!(!push_core_handles(waits), "{waits:?} is the app's");
        }
    }

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
            presenting: None,
            can_present: false,
        };
        let no_video = serde_json::json!({
            "available": false, "camera": false, "paused": false, "facing": "front", "remote": false, "remotePaused": false
        });
        assert_eq!(serde_json::to_value(CurrentCallView::from(ringing.clone())).unwrap(), serde_json::json!({
            "call": "c1", "contact": "ft_bob", "video": no_video, "outgoing": false, "phase": "ringing",
            "offer": "offer", "native": false, "muted": false, "canPresent": false, "presenting": null
        }));
        let active = ft_core::CurrentCall { phase: ft_core::CallPhase::Active, offer: None, native: true, muted: true, connected_at: Some(7), ..ringing.clone() };
        let view = serde_json::to_value(CurrentCallView::from(active)).unwrap();
        assert_eq!((view["phase"].as_str(), view["connectedAt"].as_i64(), view["muted"].as_bool()), (Some("active"), Some(7), Some(true)));
        assert!(view.get("offer").is_none());
    }

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
            presenting: None,
            can_present: false,
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
    }

    #[test]
    fn a_camera_that_could_not_start_reaches_the_webview() {
        assert_eq!(serde_json::to_value(CallEvent::new("ft_bob", "c1", CallUpdate::CameraFailed)).unwrap(), serde_json::json!({
            "contact": "ft_bob", "call": "c1", "kind": "camera_failed"
        }));
        assert_eq!(native_screen(&CallUpdate::CameraFailed), NativeScreen::Nothing);
        assert_eq!(ringing(&CallUpdate::CameraFailed, false), Ring::Nothing);
    }

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

    #[test]
    fn the_views_wait_for_the_call_s_layers() {
        let mut glue = VideoGlue::default();
        let theirs = VideoState { remote: true, ..VideoState::default() };
        assert_eq!(glue.state("c1", &theirs, None), vec![BridgeVideo::CallVideo(true)]);
        assert_eq!(glue.state("c1", &theirs, Some(LAYERS)), vec![BridgeVideo::Attach(LAYERS)], "nothing laid out yet");
        assert_eq!(glue.end(), vec![BridgeVideo::Detach]);
        assert_eq!(glue.state("c2", &theirs, Some(LAYERS)), vec![BridgeVideo::CallVideo(true), BridgeVideo::Attach(LAYERS)]);
    }

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

    #[test]
    fn me_carries_the_free_period() {
        let me = MeView { id: "ft_me".to_owned(), name: "Alice".to_owned(), mailbox: true, receipts: false, free_until: 42, auto_download: 10 };
        assert_eq!(serde_json::to_value(me).unwrap(), serde_json::json!({
            "id": "ft_me", "name": "Alice", "mailbox": true, "receipts": false, "freeUntil": 42, "autoDownload": 10
        }));
    }

    #[test]
    fn the_plan_screen_gets_the_store_price_or_none() {
        let price = |answer| serde_json::to_value(price_view(answer)).unwrap();
        assert_eq!(price(Ok(Some("0,99 €".to_owned()))), serde_json::json!({ "price": "0,99 €" }));
        assert_eq!(price(Ok(None)), serde_json::json!({ "price": null }));
        assert_eq!(price(Err("store_unavailable".to_owned())), serde_json::json!({ "price": null }));
    }

    #[test]
    fn only_incoming_calls_ring() {
        assert_eq!(ringing(&CallUpdate::Incoming { video: true, sdp: String::new() }, false), Ring::Start { video: true });
        assert_eq!(ringing(&CallUpdate::Incoming { video: false, sdp: String::new() }, false), Ring::Start { video: false });
        assert_eq!(ringing(&CallUpdate::Ended { outcome: CallOutcome::Missed }, false), Ring::Stop);
        assert_eq!(ringing(&CallUpdate::Answered { sdp: String::new() }, false), Ring::Nothing);
        assert_eq!(ringing(&CallUpdate::Connected, false), Ring::Nothing);
        assert_eq!(ringing(&CallUpdate::Muted { muted: false }, false), Ring::Nothing);
    }

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

    #[test]
    fn a_picked_copy_is_gone_once_a_plugin_has_it() {
        let dir = scratch("taken");
        for folder in [dir.join("files").join("uploads"), dir.join("uploads")] {
            std::fs::create_dir_all(&folder).unwrap();
            let copy = folder.join("1759400000000-photo.jpg");
            std::fs::write(&copy, b"jpg with gps").unwrap();

            assert_eq!(take_picked(&dir, &copy.to_string_lossy()).unwrap(), b"jpg with gps");
            assert!(!copy.exists(), "{}", copy.display());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_pickers_copies_can_be_taken() {
        let dir = scratch("not-taken");
        std::fs::create_dir_all(dir.join("files").join("outgoing")).unwrap();
        std::fs::create_dir_all(dir.join("files").join("drive").join("f1")).unwrap();
        std::fs::create_dir_all(dir.join("files").join("uploads")).unwrap();
        let kept = [
            dir.join("files").join("outgoing").join("1-made.pdf"),
            dir.join("files").join("drive").join("f1").join("doc.pdf"),
            dir.join("flickertalk.db"),
        ];
        for file in &kept {
            std::fs::write(file, b"keep").unwrap();
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&kept[0], dir.join("files").join("uploads").join("link.pdf")).unwrap();

        for refused in kept.iter().cloned().chain([dir.join("files").join("uploads").join("link.pdf")]) {
            assert!(take_picked(&dir, &refused.to_string_lossy()).is_err(), "{}", refused.display());
        }
        for file in &kept {
            assert_eq!(std::fs::read(file).unwrap(), b"keep", "{}", file.display());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn picked(path: &Path, name: &str) -> PickedView {
        PickedView { path: path.to_string_lossy().into_owned(), name: name.to_owned(), mime: "image/jpeg".to_owned(), size: 1 }
    }

    #[test]
    fn a_plugin_gets_the_first_picked_file_and_no_copy_stays() {
        let dir = scratch("hand-over");
        let uploads = dir.join("files").join("uploads");
        std::fs::create_dir_all(&uploads).unwrap();
        let (first, second) = (uploads.join("1759400000000-a.jpg"), uploads.join("1759400000000-b.jpg"));
        std::fs::write(&first, b"first with gps").unwrap();
        std::fs::write(&second, b"second with gps").unwrap();

        let handed = hand_over_picked(&dir, &[picked(&first, "a.jpg"), picked(&second, "b.jpg")]).unwrap().expect("a file");
        assert_eq!((handed.name.as_str(), handed.mime.as_str()), ("a.jpg", "image/jpeg"));
        assert_eq!(BASE64.decode(&handed.data).unwrap(), b"first with gps");
        assert!(!first.exists() && !second.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_first_pick_too_big_for_a_plugin_leaves_no_copy_either() {
        let dir = scratch("hand-over-big");
        let uploads = dir.join("files").join("uploads");
        std::fs::create_dir_all(&uploads).unwrap();
        let (first, second) = (uploads.join("1759400000000-video.mp4"), uploads.join("1759400000000-b.jpg"));
        std::fs::File::create(&first).unwrap().set_len(PLUGIN_FILE_LIMIT + 1).unwrap();
        std::fs::write(&second, b"second").unwrap();

        assert!(hand_over_picked(&dir, &[picked(&first, "video.mp4"), picked(&second, "b.jpg")]).is_err());
        assert!(!first.exists() && !second.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn handing_over_deletes_only_what_the_pick_returned_in_the_pickers_folder() {
        let dir = scratch("hand-over-none");
        let uploads = dir.join("files").join("uploads");
        std::fs::create_dir_all(&uploads).unwrap();
        std::fs::create_dir_all(dir.join("files").join("outgoing")).unwrap();
        let sent = uploads.join("1759300000000-sent.jpg");
        let made = dir.join("files").join("outgoing").join("1-made.pdf");
        std::fs::write(&sent, b"a sent message's file").unwrap();
        std::fs::write(&made, b"made").unwrap();

        assert!(hand_over_picked(&dir, &[]).unwrap().is_none());
        assert!(sent.exists());

        let first = uploads.join("1759400000000-a.jpg");
        std::fs::write(&first, b"a").unwrap();
        hand_over_picked(&dir, &[picked(&first, "a.jpg"), picked(&made, "made.pdf")]).unwrap();
        assert!(!first.exists());
        assert_eq!(std::fs::read(&made).unwrap(), b"made");
        assert!(sent.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_copy_a_drive_upload_picked_is_gone() {
        let dir = scratch("drive-up");
        let uploads = dir.join("files").join("uploads");
        std::fs::create_dir_all(&uploads).unwrap();
        let copies: Vec<_> = (0..3).map(|index| uploads.join(format!("1759400000000-{index}-a.jpg"))).collect();
        for copy in &copies {
            std::fs::write(copy, b"plain with gps").unwrap();
        }
        let picks: Vec<_> = copies.iter().map(|copy| picked(copy, "a.jpg")).collect();
        let mut answers = vec![Ok(Some("x1".to_owned())), Ok(None), Ok(Some("x3".to_owned()))].into_iter();

        let went = tauri::async_runtime::block_on(upload_all_picked(&dir, &picks, |file, path| {
            assert_eq!(file.name, "a.jpg");
            let answer = answers.next().unwrap();
            async move {
                assert_eq!(std::fs::read(path).unwrap(), b"plain with gps", "sealed from the copy, before it goes");
                answer
            }
        }));
        assert_eq!(went, Ok(3));
        assert!(copies.iter().all(|copy| !copy.exists()));
        assert_eq!(tauri::async_runtime::block_on(upload_all_picked(&dir, &[], |_, _| async { panic!("nothing to upload") })), Ok(0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_drive_upload_leaves_no_copy_either() {
        let dir = scratch("drive-fails");
        let uploads = dir.join("files").join("uploads");
        std::fs::create_dir_all(&uploads).unwrap();
        let copies: Vec<_> = (0..3).map(|index| uploads.join(format!("1759400000000-{index}-a.jpg"))).collect();
        for copy in &copies {
            std::fs::write(copy, b"plain").unwrap();
        }
        let picks: Vec<_> = copies.iter().map(|copy| picked(copy, "a.jpg")).collect();
        let mut tries = 0;

        let went = tauri::async_runtime::block_on(upload_all_picked(&dir, &picks, |_, _| {
            tries += 1;
            let answer = if tries == 1 { Ok(Some("x1".to_owned())) } else { Err(anyhow::anyhow!("no such folder")) };
            async move { answer }
        }));
        assert!(went.is_err());
        assert_eq!(tries, 2, "the third is not tried");
        assert!(copies.iter().all(|copy| !copy.exists()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_drive_upload_takes_only_the_pickers_copies() {
        let dir = scratch("drive-keep");
        std::fs::create_dir_all(dir.join("files").join("outgoing")).unwrap();
        let made = dir.join("files").join("outgoing").join("1-made.pdf");
        std::fs::write(&made, b"keep").unwrap();

        let went = tauri::async_runtime::block_on(upload_all_picked(&dir, &[picked(&made, "made.pdf")], |_, _| async { panic!("never uploaded") }));
        assert!(went.is_err());
        assert_eq!(std::fs::read(&made).unwrap(), b"keep");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_first_start_in_a_process_sweeps() {
        let dir = scratch("first-start");
        let uploads = dir.join("files").join("uploads");
        std::fs::create_dir_all(&uploads).unwrap();
        let (left, staged) = (uploads.join("1-left.jpg"), uploads.join("2-staged.jpg"));
        std::fs::write(&left, b"left by an older run").unwrap();
        let swept = std::sync::atomic::AtomicBool::new(false);

        tauri::async_runtime::block_on(open_store(&dir, &swept)).unwrap();
        assert!(!left.exists(), "the first start sweeps");
        std::fs::write(&staged, b"picked while the app is up").unwrap();
        tauri::async_runtime::block_on(open_store(&dir, &swept)).unwrap();
        assert!(staged.exists(), "a second start in the same process does not");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_first_start_that_fails_uses_up_the_sweep() {
        let dir = scratch("failed-start");
        let uploads = dir.join("files").join("uploads");
        std::fs::create_dir_all(&uploads).unwrap();
        std::fs::create_dir_all(dir.join(DATABASE)).unwrap();
        let swept = std::sync::atomic::AtomicBool::new(false);
        assert!(tauri::async_runtime::block_on(open_store(&dir, &swept)).is_err());

        std::fs::remove_dir_all(dir.join(DATABASE)).unwrap();
        let staged = uploads.join("2-staged.jpg");
        std::fs::write(&staged, b"picked while the app is up").unwrap();
        tauri::async_runtime::block_on(open_store(&dir, &swept)).unwrap();
        assert!(staged.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_start_sweeps_only_where_files_wait_on_their_way() {
        let dir = Path::new("/data");
        let files = dir.join("files");
        assert_eq!(
            transit_folders(dir),
            vec![files.join("uploads"), files.join("outgoing"), files.join("drive"), dir.join("uploads")]
        );
    }

    #[test]
    fn a_picked_copy_too_big_for_a_plugin_is_gone_too() {
        let dir = scratch("too-big");
        std::fs::create_dir_all(dir.join("files").join("uploads")).unwrap();
        let copy = dir.join("files").join("uploads").join("1759400000000-video.mp4");
        std::fs::File::create(&copy).unwrap().set_len(PLUGIN_FILE_LIMIT + 1).unwrap();

        assert!(take_picked(&dir, &copy.to_string_lossy()).is_err());
        assert!(!copy.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_made_file_says_whether_it_was_sent_or_staged() {
        let staged = serde_json::to_value(MadeView { sent: false, staged: Some(PickedView { path: "/p".into(), name: "a.pdf".into(), mime: "application/pdf".into(), size: 3 }) }).unwrap();
        assert_eq!(staged["sent"], false);
        assert_eq!(staged["staged"]["name"], "a.pdf");
        let sent = serde_json::to_value(MadeView { sent: true, staged: None }).unwrap();
        assert_eq!(sent, serde_json::json!({ "sent": true }));
    }

    #[test]
    fn upload_ids_cannot_point_elsewhere() {
        let dir = Path::new("/data");
        let id = new_upload_id();
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
    fn an_erased_phone_keeps_nothing_of_the_user() {
        let dir = scratch("erase-all");
        std::fs::write(dir.join("flickertalk.db"), b"x").unwrap();
        for folder in ["plugins/com.flickertalk.notes", "vault/queue", "uploads", "files/uploads"] {
            std::fs::create_dir_all(dir.join(folder)).unwrap();
        }
        erase(&dir).unwrap();
        for name in ["flickertalk.db", "plugins", "vault", "uploads", "files"] {
            assert!(!dir.join(name).exists(), "{name} is gone");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wiping_forgets_the_storage_key_in_the_key_store() {
        let dir = scratch("wipe-key");
        let vault = FakeVault::default();
        storage_key(&dir, Some(&vault)).expect("creates");
        wipe(&dir, Some(&vault)).expect("wipes");
        assert!(!dir.join("storage.key.sealed").exists());
        assert!(vault.forgotten.load(std::sync::atomic::Ordering::SeqCst), "the key store forgot the key");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn starting_again_goes_back_to_the_first_page() {
        let first = |url: &str| first_page(url.parse().unwrap()).to_string();
        assert_eq!(first("tauri://localhost/tabs/settings"), "tauri://localhost/");
        assert_eq!(first("http://tauri.localhost/chat/ft_x?y=1#z"), "http://tauri.localhost/");
        assert_eq!(first("http://localhost:1420/"), "http://localhost:1420/");
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

    #[derive(Default)]
    struct FakeVault {
        broken: bool,
        forgotten: std::sync::atomic::AtomicBool,
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

        fn forget(&self) -> anyhow::Result<()> {
            self.forgotten.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn the_storage_key_is_created_once_and_kept() {
        let dir = scratch("key");
        let first = storage_key(&dir, None).expect("creates");
        assert_eq!(storage_key(&dir, None).expect("reads"), first);
        assert_ne!(first, [0; 32]);
        let _ = std::fs::remove_dir_all(&dir);
    }

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

    #[test]
    fn a_key_the_keystore_cannot_open_is_an_error_never_replaced() {
        let dir = scratch("broken-key");
        storage_key(&dir, Some(&FakeVault::default())).expect("creates");
        let sealed = std::fs::read(dir.join("storage.key.sealed")).unwrap();
        assert!(storage_key(&dir, Some(&FakeVault { broken: true, ..Default::default() })).is_err());
        assert_eq!(std::fs::read(dir.join("storage.key.sealed")).unwrap(), sealed, "left as it was");
        let _ = std::fs::remove_dir_all(&dir);
    }

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

    #[test]
    fn a_session_view_is_its_id_and_its_conversations() {
        let view = serde_json::to_value(SessionView { id: "s1".to_owned(), conversations: vec![], requests: vec![], circles: vec![] }).unwrap();
        assert_eq!(view, serde_json::json!({ "id": "s1", "conversations": [], "requests": [], "circles": [] }));
    }

    #[test]
    fn a_contact_view_carries_its_rules() {
        let rules = ft_storage::ContactRules { muted: true, accepts_chat: false, accepts_calls: true, receipts: false, typing: false };
        let view = serde_json::to_value(RulesView::from(rules)).unwrap();
        assert_eq!(view, serde_json::json!({ "muted": true, "acceptsChat": false, "acceptsCalls": true, "receipts": false, "typing": false }));
    }

    #[test]
    fn a_quote_is_the_text_a_files_name_or_gone() {
        let message = Message { message_id: "m1".into(), contact: "ft_bob".into(), outgoing: false, body: "dinner?".into(), sent_at: 1, received_at: 1, state: ft_storage::MessageState::Read };
        let text = serde_json::to_value(QuoteView::of("m1", Some(&message), None)).unwrap();
        assert_eq!(text, serde_json::json!({ "id": "m1", "text": "dinner?", "mine": false, "kind": "text" }));
        let gone = serde_json::to_value(QuoteView::of("m0", None, None)).unwrap();
        assert_eq!(gone, serde_json::json!({ "id": "m0", "text": "", "mine": false, "kind": "gone" }));
        let view = serde_json::to_value(MessageView::new(&message, None).answering(QuoteView::of("m0", None, None))).unwrap();
        assert_eq!(view["quote"]["kind"], "gone");
        assert!(serde_json::to_value(MessageView::new(&message, None)).unwrap().get("quote").is_none(), "no quote, no field");
    }

    #[test]
    fn a_message_for_later_carries_its_time() {
        let message = Message { message_id: "m1".into(), contact: "ft_bob".into(), outgoing: true, body: "hi".into(), sent_at: 9, received_at: 1, state: ft_storage::MessageState::Pending };
        assert_eq!(serde_json::to_value(MessageView::new(&message, None).for_later(Some(9))).unwrap()["scheduledFor"], 9);
        assert!(serde_json::to_value(MessageView::new(&message, None).for_later(None)).unwrap().get("scheduledFor").is_none());
    }

    #[test]
    fn a_message_says_whether_it_was_edited_or_taken_back() {
        let message = Message { message_id: "m1".into(), contact: "ft_bob".into(), outgoing: true, body: String::new(), sent_at: 1, received_at: 1, state: ft_storage::MessageState::Read };
        let view = serde_json::to_value(MessageView::new(&message, None).marked(true, false)).unwrap();
        assert_eq!((view["edited"].clone(), view.get("deleted").cloned()), (serde_json::json!(true), None));
        let view = serde_json::to_value(MessageView::new(&message, None).marked(false, true)).unwrap();
        assert_eq!((view.get("edited").cloned(), view["deleted"].clone()), (None, serde_json::json!(true)));
    }

    #[test]
    fn a_message_says_whether_a_change_of_it_still_waits() {
        let message = Message { message_id: "m1".into(), contact: "ft_bob".into(), outgoing: true, body: "hi".into(), sent_at: 1, received_at: 1, state: ft_storage::MessageState::Delivered };
        let view = serde_json::to_value(MessageView::new(&message, None).updating(true)).unwrap();
        assert_eq!((view["updatePending"].clone(), view["state"].clone()), (serde_json::json!(true), serde_json::json!("delivered")));
        assert!(serde_json::to_value(MessageView::new(&message, None).updating(false)).unwrap().get("updatePending").is_none());
    }

    #[test]
    fn a_pinned_message_says_so() {
        let message = Message { message_id: "m1".into(), contact: "ft_bob".into(), outgoing: false, body: "hi".into(), sent_at: 1, received_at: 1, state: ft_storage::MessageState::Read };
        assert_eq!(serde_json::to_value(MessageView::new(&message, None).pinned(true)).unwrap()["pinned"], true);
        assert!(serde_json::to_value(MessageView::new(&message, None)).unwrap().get("pinned").is_none());
    }

    #[test]
    fn a_message_carries_the_notes_tools_left_under_it() {
        let message = Message { message_id: "m1".into(), contact: "ft_bob".into(), outgoing: false, body: "see you at six".into(), sent_at: 1, received_at: 1, state: ft_storage::MessageState::Read };
        let notes = [ft_storage::MessageNote { message_id: "m1".into(), plugin: "com.flickertalk.translate".into(), text: "nos vemos a las seis".into() }];
        let view = serde_json::to_value(MessageView::new(&message, None).noted(notes.iter())).unwrap();
        assert_eq!(view["notes"], serde_json::json!([{ "plugin": "com.flickertalk.translate", "text": "nos vemos a las seis" }]));
        assert!(serde_json::to_value(MessageView::new(&message, None).noted([].iter())).unwrap().get("notes").is_none());
    }

    #[test]
    fn a_circle_message_carries_the_notes_tools_left_under_it() {
        let said = ft_storage::CircleMessage {
            message_id: "cm1".into(),
            circle: "circle-1".into(),
            sender: "ft_bob".into(),
            outgoing: false,
            kind: "text".into(),
            body: "salut".into(),
            sent_at: 1,
            received_at: 1,
            state: ft_storage::MessageState::Delivered,
        };
        let notes = [ft_storage::MessageNote { message_id: "cm1".into(), plugin: "com.flickertalk.translate".into(), text: "hola".into() }];
        let view = serde_json::to_value(CircleMessageView::new(&said, &HashMap::new()).noted(notes.iter())).unwrap();
        assert_eq!(view["notes"], serde_json::json!([{ "plugin": "com.flickertalk.translate", "text": "hola" }]));
        assert!(serde_json::to_value(CircleMessageView::new(&said, &HashMap::new())).unwrap().get("notes").is_none());
    }

    #[test]
    fn a_tool_that_runs_by_itself_goes_to_the_screen() {
        let hook = ft_core::plugins::ChatHook { plugin: "com.flickertalk.translate".into(), auto_message: true, auto_composer: false, lang: Some("es".into()) };
        let view = serde_json::to_value(ChatHookView::from(hook.clone())).unwrap();
        assert_eq!(view, serde_json::json!({ "plugin": "com.flickertalk.translate", "autoMessage": true, "autoComposer": false, "lang": "es" }));
        let unknown = serde_json::to_value(ChatHookView::from(ft_core::plugins::ChatHook { lang: None, ..hook })).unwrap();
        assert!(unknown.get("lang").is_none(), "no language yet, nothing written: {unknown}");
    }

    #[test]
    fn a_message_carries_the_reactions_of_each_side() {
        let message = Message { message_id: "m1".into(), contact: "ft_bob".into(), outgoing: false, body: "hi".into(), sent_at: 1, received_at: 1, state: ft_storage::MessageState::Read };
        let both = ft_storage::Reactions { mine: Some("👍".into()), theirs: Some("❤️".into()) };
        let view = serde_json::to_value(MessageView::new(&message, None).reacted(Some(&both))).unwrap();
        assert_eq!(view["reactions"], serde_json::json!({ "mine": "👍", "theirs": "❤️" }));
        let theirs = ft_storage::Reactions { mine: None, theirs: Some("❤️".into()) };
        let view = serde_json::to_value(MessageView::new(&message, None).reacted(Some(&theirs))).unwrap();
        assert_eq!(view["reactions"], serde_json::json!({ "theirs": "❤️" }));
        assert!(serde_json::to_value(MessageView::new(&message, None).reacted(None)).unwrap().get("reactions").is_none());
    }

    #[test]
    fn rules_without_the_typing_switch_keep_it_on() {
        let view: RulesView = serde_json::from_value(serde_json::json!({ "muted": false, "acceptsChat": true, "acceptsCalls": true, "receipts": true })).unwrap();
        assert!(ft_storage::ContactRules::from(view).typing);
        let view: RulesView = serde_json::from_value(serde_json::json!({ "muted": false, "acceptsChat": true, "acceptsCalls": true, "receipts": true, "typing": false })).unwrap();
        assert!(!ft_storage::ContactRules::from(view).typing);
    }
}
