use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use ft_core::online::Online;
use ft_core::{CallPhase, CallUpdate, Core, PresentedBy, TurnGrant};
use ft_media::{CallRouting, Facing, VideoState};
use ft_storage::{CallOutcome, CallRecord};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_ft_platform::{CallRoute, NativeCallEvent, PlatformExt};

use crate::push_core::CallScreen;

use super::{failed, Client};

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

pub(super) fn can_present_now(current: Option<&ft_core::CurrentCall>, call: &str) -> bool {
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

pub(super) fn bridge_screen(app: &AppHandle) -> Arc<dyn CallScreen> {
    Arc::new(BridgeScreen(app.clone()))
}

pub(super) fn listen_native_calls(app: &AppHandle, online: &Online) {
    let _ = app.platform().set_open_app_on_answer(open_app_on_answer(option_env!("FT_IOS_OPEN_APP_ON_ANSWER")));
    let events = native_call_queue(online, Some(app.clone()));
    app.platform().listen_calls(move |event| {
        let _ = events.send(event);
    });
}

/// Events are handled in order because an audio deactivation and activation must not swap. Slow
/// work (answering, hanging up) is spawned.
pub(super) fn native_call_queue(online: &Online, app: Option<AppHandle>) -> tokio::sync::mpsc::UnboundedSender<NativeCallEvent> {
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

pub(super) const CALL_SOCKET_FRESH: std::time::Duration = std::time::Duration::from_secs(10);

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

pub(super) fn answered_already(current: Option<&ft_core::CurrentCall>, call: &str) -> bool {
    current.is_some_and(|current| current.call == call && !current.outgoing && current.phase != CallPhase::Ringing)
}

async fn fail_current_call(core: &Core) -> anyhow::Result<()> {
    match core.current_call().await? {
        Some(current) => core.end_call(&current.call, true).await,
        None => Ok(()),
    }
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

#[cfg(test)]
mod tests {
    use ft_media::RemoteShape;

    use super::*;

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

    #[test]
    fn the_call_routing_is_one_of_the_settings_words() {
        assert_eq!(call_routing("always"), Ok(ft_media::CallRouting::Always));
        assert_eq!(call_routing("direct"), Ok(ft_media::CallRouting::Direct));
        assert!(call_routing("sometimes").is_err());
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
}
