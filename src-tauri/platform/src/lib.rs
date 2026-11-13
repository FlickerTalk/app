use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::{Deserialize, Serialize};
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Manager, Runtime};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not available on this platform")]
    Unsupported,
    #[error("the key store gave something that is not a key")]
    Corrupt,
    #[cfg(mobile)]
    #[error(transparent)]
    Invoke(#[from] tauri::plugin::mobile::PluginInvokeError),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Serialize)]
struct OpenFile<'a> {
    path: &'a str,
    mime: &'a str,
}

#[derive(Serialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct Pick<'a> {
    accept: &'a str,
}

#[derive(Serialize)]
struct SaveFile<'a> {
    path: &'a str,
    name: &'a str,
    mime: &'a str,
}

#[derive(Serialize)]
struct Ringing<'a> {
    caller: &'a str,
    video: bool,
    muted: bool,
}

#[derive(Serialize)]
struct Answering<'a> {
    caller: &'a str,
    video: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeCallEvent {
    Incoming,
    Answer,
    End,
    Decline,
    Mute(bool),
    AudioActivated(u64),
    AudioDeactivated(u64),
    Visible(bool),
    /// iOS sends `UIDeviceOrientation.rawValue`, Android the display rotation in degrees
    Orientation(i32),
    VideoRequested,
    Route(CallRoute),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallRoute {
    Receiver,
    Speaker,
    Headset,
}

impl CallRoute {
    fn parse(wire: &str) -> Option<Self> {
        match wire {
            "receiver" => Some(Self::Receiver),
            "speaker" => Some(Self::Speaker),
            "headset" => Some(Self::Headset),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Receiver => "receiver",
            Self::Speaker => "speaker",
            Self::Headset => "headset",
        }
    }
}

#[derive(Deserialize)]
struct CallEventWire {
    event: String,
    muted: Option<bool>,
    #[serde(default)]
    generation: u64,
    visible: Option<bool>,
    orientation: Option<i32>,
    route: Option<String>,
}

#[cfg_attr(not(mobile), allow(dead_code))]
fn call_event(body: tauri::ipc::InvokeResponseBody) -> Option<NativeCallEvent> {
    call_event_of(body.deserialize().ok()?)
}

pub fn parse_call_event(json: &str) -> Option<NativeCallEvent> {
    call_event_of(serde_json::from_str(json).ok()?)
}

fn call_event_of(wire: CallEventWire) -> Option<NativeCallEvent> {
    match (wire.event.as_str(), wire.muted) {
        ("visible", _) => wire.visible.map(NativeCallEvent::Visible),
        ("orientation", _) => wire.orientation.map(NativeCallEvent::Orientation),
        ("video", _) => Some(NativeCallEvent::VideoRequested),
        ("incoming", _) => Some(NativeCallEvent::Incoming),
        ("answer", _) => Some(NativeCallEvent::Answer),
        ("end", _) => Some(NativeCallEvent::End),
        ("decline", _) => Some(NativeCallEvent::Decline),
        ("mute", Some(muted)) => Some(NativeCallEvent::Mute(muted)),
        ("audioActivated", _) => Some(NativeCallEvent::AudioActivated(wire.generation)),
        ("audioDeactivated", _) => Some(NativeCallEvent::AudioDeactivated(wire.generation)),
        ("route", _) => wire.route.as_deref().and_then(CallRoute::parse).map(NativeCallEvent::Route),
        _ => None,
    }
}

// Created on the Rust side so call events still arrive with no WebView, e.g. on a locked iPhone.
#[cfg_attr(not(mobile), allow(dead_code))]
fn call_channel(handler: impl Fn(NativeCallEvent) + Send + Sync + 'static) -> tauri::ipc::Channel<serde_json::Value> {
    tauri::ipc::Channel::new(move |body| {
        if let Some(event) = call_event(body) {
            handler(event);
        }
        Ok(())
    })
}

#[derive(Serialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct CallEvents<'a> {
    channel: &'a tauri::ipc::Channel<serde_json::Value>,
}

#[derive(Serialize)]
struct Speaker {
    on: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AttachVideo {
    remote_layer: u64,
    local_layer: u64,
}

/// CSS px, from getBoundingClientRect()
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VideoRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoLayout {
    pub remote: Option<VideoRect>,
    pub local: Option<VideoRect>,
    pub mirror_local: bool,
    pub local_radius: f64,
}

#[derive(Serialize)]
struct VideoShape {
    width: u32,
    height: u32,
    rotation: u16,
}

#[derive(Serialize)]
struct CallVideo {
    on: bool,
}

#[derive(Serialize)]
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
struct OpenAppOnAnswer {
    on: bool,
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct Camera {
    granted: bool,
}

#[derive(Deserialize)]
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
struct Answered {
    answered: bool,
}

#[derive(Serialize)]
struct Outgoing<'a> {
    name: &'a str,
    video: bool,
}

#[derive(Serialize)]
struct OpenSlots<'a> {
    slots: &'a [u8],
}

#[derive(Serialize)]
struct QuietHours<'a> {
    week: &'a str,
}

#[derive(Serialize)]
struct Reminders<'a> {
    reminders: &'a str,
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct PendingReminder {
    reminder: String,
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct OpenedLinkWire {
    #[serde(default)]
    url: Option<String>,
}

#[cfg_attr(not(mobile), allow(dead_code))]
impl OpenedLinkWire {
    fn url(self) -> Option<String> {
        self.url.filter(|url| !url.is_empty())
    }
}

#[derive(Serialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct LinkEvents<'a> {
    channel: &'a tauri::ipc::Channel<serde_json::Value>,
}

#[cfg_attr(not(mobile), allow(dead_code))]
fn link_opened(body: tauri::ipc::InvokeResponseBody) -> bool {
    #[derive(Deserialize)]
    struct Wire {
        event: String,
    }
    body.deserialize::<Wire>().is_ok_and(|wire| wire.event == "linkOpened")
}

#[derive(Serialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct Authorize<'a> {
    url: &'a str,
    scheme: &'a str,
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct Authorized {
    url: String,
}

#[derive(Serialize)]
struct ShareText<'a> {
    text: &'a str,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct KeyBytes {
    value: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
pub struct PickedFile {
    pub path: String,
    pub name: String,
    pub mime: String,
    pub size: u64,
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct Picked {
    files: Vec<PickedFile>,
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct Microphone {
    granted: bool,
}

/// degrees, accuracy in metres, `at` in ms since the epoch
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Location {
    pub lat: f64,
    pub lon: f64,
    pub accuracy: f64,
    pub at: i64,
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct Located {
    found: bool,
    #[serde(default)]
    lat: f64,
    #[serde(default)]
    lon: f64,
    #[serde(default)]
    accuracy: f64,
    #[serde(default)]
    at: f64,
}

#[cfg_attr(not(mobile), allow(dead_code))]
impl Located {
    fn location(self) -> Option<Location> {
        self.found.then_some(Location { lat: self.lat, lon: self.lon, accuracy: self.accuracy, at: self.at as i64 })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TranslateStatus {
    Installed,
    Downloadable,
    Unsupported,
}

#[derive(Serialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct TranslatePair<'a> {
    from: &'a str,
    to: &'a str,
}

#[derive(Serialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct TranslateSource<'a> {
    text: &'a str,
}

#[derive(Serialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct TranslateRequest<'a> {
    text: &'a str,
    from: &'a str,
    to: &'a str,
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct TranslateLanguages {
    languages: Vec<String>,
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct Detected {
    found: bool,
    #[serde(default)]
    tag: Option<String>,
}

#[cfg_attr(not(mobile), allow(dead_code))]
impl Detected {
    fn tag(self) -> Option<String> {
        self.tag.filter(|tag| self.found && !tag.is_empty())
    }
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct PairState {
    status: TranslateStatus,
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct Prepared {
    ready: bool,
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct Removed {
    removed: bool,
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct Translated {
    found: bool,
    #[serde(default)]
    text: Option<String>,
}

#[cfg_attr(not(mobile), allow(dead_code))]
impl Translated {
    fn text(self) -> Option<String> {
        self.text.filter(|_| self.found)
    }
}

#[derive(Deserialize)]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
struct ExactAlarms {
    allowed: bool,
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct FullScreen {
    allowed: bool,
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct Subscription {
    until: i64,
}

#[cfg_attr(not(mobile), allow(dead_code))]
fn entitlement_until(body: tauri::ipc::InvokeResponseBody) -> Option<i64> {
    Some(body.deserialize::<Subscription>().ok()?.until)
}

#[derive(Serialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct EntitlementEvents<'a> {
    channel: &'a tauri::ipc::Channel<serde_json::Value>,
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct SubscriptionPrice {
    #[serde(default)]
    price: Option<String>,
}

#[cfg_attr(not(mobile), allow(dead_code))]
impl SubscriptionPrice {
    fn shown(self) -> Option<String> {
        self.price.map(|price| price.trim().to_owned()).filter(|price| !price.is_empty())
    }
}

#[derive(Serialize)]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
struct SystemBars {
    dark: bool,
}

#[derive(Serialize)]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
struct Appearance<'a> {
    appearance: &'a str,
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct PushToken {
    token: String,
}

pub struct Platform<R: Runtime> {
    #[cfg(mobile)]
    handle: tauri::plugin::PluginHandle<R>,
    #[cfg(not(mobile))]
    _runtime: std::marker::PhantomData<fn() -> R>,
}

impl<R: Runtime> Platform<R> {
    /// Only works for files under `files/`, the FileProvider shares nothing else.
    pub fn open_file(&self, path: &str, mime: &str) -> Result<()> {
        self.run("openFile", OpenFile { path, mime })
    }

    pub fn save_to_downloads(&self, path: &str, name: &str, mime: &str) -> Result<()> {
        self.run("saveToDownloads", SaveFile { path, name, mime })
    }

    pub fn print_file(&self, path: &str, name: &str, mime: &str) -> Result<()> {
        self.run("printFile", SaveFile { path, name, mime })
    }

    /// Returns until when the subscription runs (ms), or 0.
    pub fn subscribe(&self) -> Result<i64> {
        #[cfg(mobile)]
        {
            Ok(self.handle.run_mobile_plugin::<Subscription>("subscribe", ())?.until)
        }
        #[cfg(not(mobile))]
        {
            Err(Error::Unsupported)
        }
    }

    pub fn restore_subscription(&self) -> Result<i64> {
        #[cfg(mobile)]
        {
            Ok(self.handle.run_mobile_plugin::<Subscription>("restoreSubscription", ())?.until)
        }
        #[cfg(not(mobile))]
        {
            Err(Error::Unsupported)
        }
    }

    pub fn subscription(&self) -> Result<i64> {
        #[cfg(mobile)]
        {
            Ok(self.handle.run_mobile_plugin::<Subscription>("subscription", ())?.until)
        }
        #[cfg(not(mobile))]
        {
            Ok(0)
        }
    }

    /// The handler runs on a native thread, keep it short.
    pub fn listen_entitlements(&self, handler: impl Fn(i64) + Send + Sync + 'static) {
        #[cfg(mobile)]
        {
            let channel = tauri::ipc::Channel::new(move |body| {
                if let Some(until) = entitlement_until(body) {
                    handler(until);
                }
                Ok(())
            });
            let _ = self.run("registerEntitlementEvents", EntitlementEvents { channel: &channel });
        }
        #[cfg(not(mobile))]
        let _ = handler;
    }

    /// The Store's own formatted string. Don't convert, round or cache it.
    pub fn subscription_price(&self) -> Result<Option<String>> {
        #[cfg(mobile)]
        {
            Ok(self.handle.run_mobile_plugin::<SubscriptionPrice>("subscriptionPrice", ())?.shown())
        }
        #[cfg(not(mobile))]
        {
            Ok(None)
        }
    }

    pub fn set_system_bars(&self, dark: bool) -> Result<()> {
        #[cfg(target_os = "android")]
        {
            self.run("setSystemBars", SystemBars { dark })
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = dark;
            Ok(())
        }
    }

    pub fn set_appearance(&self, appearance: &str) -> Result<()> {
        #[cfg(target_os = "android")]
        {
            self.run("setAppearance", Appearance { appearance })
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = appearance;
            Ok(())
        }
    }

    pub fn share_text(&self, text: &str) -> Result<()> {
        self.run("shareText", ShareText { text })
    }

    pub fn share_file(&self, path: &str, name: &str, mime: &str) -> Result<()> {
        self.run("shareFile", SaveFile { path, name, mime })
    }

    pub fn start_ringing(&self, caller: &str, video: bool, muted: bool) -> Result<()> {
        self.run("startRinging", Ringing { caller, video, muted })
    }

    pub fn set_open_slots(&self, slots: &[u8]) -> Result<()> {
        self.run("setOpenSlots", OpenSlots { slots })
    }

    pub fn set_quiet_hours(&self, week: &str) -> Result<()> {
        self.run("setQuietHours", QuietHours { week })
    }

    pub fn stop_ringing(&self) -> Result<()> {
        self.run("stopRinging", ())
    }

    pub fn call_refused(&self) -> Result<()> {
        self.call("callRefused", ())
    }

    pub fn call_answering(&self, caller: &str, video: bool) -> Result<()> {
        self.call("callAnswering", Answering { caller, video })
    }

    /// The handler runs on a native background thread, so it must not block for long.
    pub fn listen_calls(&self, handler: impl Fn(NativeCallEvent) + Send + Sync + 'static) {
        #[cfg(mobile)]
        {
            let channel = call_channel(handler);
            let _ = self.run("registerCallEvents", CallEvents { channel: &channel });
        }
        #[cfg(not(mobile))]
        let _ = handler;
    }

    pub fn call_started_outgoing(&self, name: &str, video: bool) -> Result<()> {
        self.call("callStartedOutgoing", Outgoing { name, video })
    }

    pub fn call_connected(&self) -> Result<()> {
        self.call("callConnected", ())
    }

    /// false when the OS has no call to answer, then the core answers by itself
    pub fn answer_call(&self) -> Result<bool> {
        #[cfg(target_os = "ios")]
        {
            Ok(self.handle.run_mobile_plugin::<Answered>("answerCall", ())?.answered)
        }
        #[cfg(not(target_os = "ios"))]
        {
            Ok(false)
        }
    }

    pub fn set_speaker(&self, on: bool) -> Result<()> {
        self.call("setSpeaker", Speaker { on })
    }

    pub fn call_ended(&self) -> Result<()> {
        self.call("callEnded", ())
    }

    /// iOS only opens the app after an answer for video calls, so with `on` every incoming call
    /// is reported to CallKit as video. The call's real media doesn't change.
    pub fn set_open_app_on_answer(&self, on: bool) -> Result<()> {
        #[cfg(target_os = "ios")]
        {
            self.run("setOpenAppOnAnswer", OpenAppOnAnswer { on })
        }
        #[cfg(not(target_os = "ios"))]
        {
            let _ = on;
            Ok(())
        }
    }

    pub fn request_microphone(&self) -> Result<bool> {
        #[cfg(mobile)]
        {
            Ok(self.handle.run_mobile_plugin::<Microphone>("requestMicrophone", ())?.granted)
        }
        #[cfg(not(mobile))]
        {
            Ok(true)
        }
    }

    /// Layer addresses on iOS (`ft_media::views::layers`). Android passes 0 for both and Kotlin
    /// hands the surfaces over JNI instead.
    pub fn attach_video(&self, remote_layer: usize, local_layer: usize) -> Result<()> {
        self.call("attachVideo", AttachVideo { remote_layer: remote_layer as u64, local_layer: local_layer as u64 })
    }

    pub fn video_layout(&self, layout: &VideoLayout) -> Result<()> {
        self.call("videoLayout", layout)
    }

    pub fn video_shape(&self, width: u32, height: u32, rotation: u16) -> Result<()> {
        self.call("videoShape", VideoShape { width, height, rotation })
    }

    /// On iOS this returns once the layers are out of the view hierarchy. Only then may the call's
    /// video devices go.
    pub fn detach_video(&self) -> Result<()> {
        self.call("detachVideo", ())
    }

    pub fn call_video(&self, on: bool) -> Result<()> {
        self.call("callVideo", CallVideo { on })
    }

    pub fn request_camera(&self) -> Result<bool> {
        #[cfg(mobile)]
        {
            Ok(self.handle.run_mobile_plugin::<Camera>("requestCamera", ())?.granted)
        }
        #[cfg(not(mobile))]
        {
            Ok(true)
        }
    }

    fn call(&self, command: &str, args: impl Serialize) -> Result<()> {
        #[cfg(mobile)]
        {
            self.run(command, args)
        }
        #[cfg(not(mobile))]
        {
            let _ = (command, args);
            Ok(())
        }
    }

    /// FCM token on Android, `gateway:bundle:token` for APNs on iOS
    pub fn push_token(&self) -> Result<String> {
        #[cfg(mobile)]
        {
            Ok(self.handle.run_mobile_plugin::<PushToken>("pushToken", ())?.token)
        }
        #[cfg(not(mobile))]
        {
            Err(Error::Unsupported)
        }
    }

    // Used instead of the WebView's file input, which strands the user outside the app until they
    // pick something.
    pub fn pick_files(&self, accept: &str) -> Result<Vec<PickedFile>> {
        #[cfg(mobile)]
        {
            Ok(self.handle.run_mobile_plugin::<Picked>("pickFiles", Pick { accept })?.files)
        }
        #[cfg(not(mobile))]
        let _ = accept;
        #[cfg(not(mobile))]
        {
            Err(Error::Unsupported)
        }
    }

    pub fn take_photo(&self) -> Result<Vec<PickedFile>> {
        #[cfg(mobile)]
        {
            Ok(self.handle.run_mobile_plugin::<Picked>("takePhoto", ())?.files)
        }
        #[cfg(not(mobile))]
        {
            Err(Error::Unsupported)
        }
    }

    pub fn set_reminders(&self, reminders: &str) -> Result<()> {
        self.run("setReminders", Reminders { reminders })
    }

    /// "plugin\nid", empty if none
    pub fn pending_reminder(&self) -> Result<String> {
        #[cfg(mobile)]
        {
            Ok(self.handle.run_mobile_plugin::<PendingReminder>("pendingReminder", ())?.reminder)
        }
        #[cfg(not(mobile))]
        {
            Ok(String::new())
        }
    }

    pub fn opened_link(&self) -> Result<Option<String>> {
        #[cfg(mobile)]
        {
            Ok(self.handle.run_mobile_plugin::<OpenedLinkWire>("openedLink", ())?.url())
        }
        #[cfg(not(mobile))]
        {
            Ok(None)
        }
    }

    pub fn listen_links(&self, handler: impl Fn() + Send + Sync + 'static) {
        #[cfg(mobile)]
        {
            let channel = tauri::ipc::Channel::new(move |body| {
                if link_opened(body) {
                    handler();
                }
                Ok(())
            });
            let _ = self.run("registerLinkEvents", LinkEvents { channel: &channel });
        }
        #[cfg(not(mobile))]
        let _ = handler;
    }

    /// Custom Tabs / ASWebAuthenticationSession, so the WebView never sees the login page or the
    /// tokens.
    pub fn authorize(&self, url: &str, scheme: &str) -> Result<String> {
        #[cfg(mobile)]
        {
            Ok(self.handle.run_mobile_plugin::<Authorized>("authorize", Authorize { url, scheme })?.url)
        }
        #[cfg(not(mobile))]
        {
            let _ = (url, scheme);
            Err(Error::Unsupported)
        }
    }

    /// Blocks until the phone answers (up to ~15 s), don't call it on the main thread.
    pub fn current_location(&self) -> Result<Option<Location>> {
        #[cfg(mobile)]
        {
            Ok(self.handle.run_mobile_plugin::<Located>("currentLocation", ())?.location())
        }
        #[cfg(not(mobile))]
        {
            Ok(None)
        }
    }

    /// Like every translate_* call this blocks until the phone answers, so not on the main thread.
    pub fn translate_languages(&self) -> Result<Vec<String>> {
        #[cfg(mobile)]
        {
            Ok(self.handle.run_mobile_plugin::<TranslateLanguages>("translateLanguages", ())?.languages)
        }
        #[cfg(not(mobile))]
        {
            Ok(vec![])
        }
    }

    pub fn translate_detect(&self, text: &str) -> Result<Option<String>> {
        #[cfg(mobile)]
        {
            Ok(self.handle.run_mobile_plugin::<Detected>("translateDetect", TranslateSource { text })?.tag())
        }
        #[cfg(not(mobile))]
        {
            let _ = text;
            Ok(None)
        }
    }

    pub fn translate_status(&self, from: &str, to: &str) -> Result<TranslateStatus> {
        #[cfg(mobile)]
        {
            Ok(self.handle.run_mobile_plugin::<PairState>("translateStatus", TranslatePair { from, to })?.status)
        }
        #[cfg(not(mobile))]
        {
            let _ = (from, to);
            Ok(TranslateStatus::Unsupported)
        }
    }

    /// Downloads packs from Google or Apple. Only call it once the user agreed.
    pub fn translate_prepare(&self, from: &str, to: &str) -> Result<bool> {
        #[cfg(mobile)]
        {
            Ok(self.handle.run_mobile_plugin::<Prepared>("translatePrepare", TranslatePair { from, to })?.ready)
        }
        #[cfg(not(mobile))]
        {
            let _ = (from, to);
            Ok(false)
        }
    }

    /// English is the pivot and never removed, so `remove(tag, "en")` frees one language.
    pub fn translate_remove(&self, from: &str, to: &str) -> Result<bool> {
        #[cfg(mobile)]
        {
            Ok(self.handle.run_mobile_plugin::<Removed>("translateRemove", TranslatePair { from, to })?.removed)
        }
        #[cfg(not(mobile))]
        {
            let _ = (from, to);
            Ok(false)
        }
    }

    pub fn translate_text(&self, text: &str, from: &str, to: &str) -> Result<Option<String>> {
        #[cfg(mobile)]
        {
            Ok(self.handle.run_mobile_plugin::<Translated>("translateText", TranslateRequest { text, from, to })?.text())
        }
        #[cfg(not(mobile))]
        {
            let _ = (text, from, to);
            Ok(None)
        }
    }

    pub fn can_show_full_screen(&self) -> Result<bool> {
        #[cfg(mobile)]
        {
            Ok(self.handle.run_mobile_plugin::<FullScreen>("canShowFullScreen", ())?.allowed)
        }
        #[cfg(not(mobile))]
        {
            Ok(false)
        }
    }

    pub fn ask_full_screen(&self) -> Result<()> {
        self.run("askFullScreen", ())
    }

    pub fn exact_alarms(&self) -> Result<bool> {
        #[cfg(target_os = "android")]
        {
            Ok(self.handle.run_mobile_plugin::<ExactAlarms>("exactAlarms", ())?.allowed)
        }
        #[cfg(not(target_os = "android"))]
        {
            Ok(true)
        }
    }

    /// The user decides after this returns, so the result is only the current state.
    pub fn request_exact_alarms(&self) -> Result<bool> {
        #[cfg(target_os = "android")]
        {
            Ok(self.handle.run_mobile_plugin::<ExactAlarms>("requestExactAlarms", ())?.allowed)
        }
        #[cfg(not(target_os = "android"))]
        {
            Ok(true)
        }
    }

    pub fn request_notifications(&self) -> Result<()> {
        self.run("requestNotifications", ())
    }

    pub fn seal_key(&self, key: &[u8; 32]) -> Result<Vec<u8>> {
        let sealed = self.ask("sealKey", KeyBytes { value: BASE64.encode(key) })?;
        BASE64.decode(sealed.value).map_err(|_| Error::Corrupt)
    }

    pub fn open_key(&self, sealed: &[u8]) -> Result<[u8; 32]> {
        let key = self.ask("openKey", KeyBytes { value: BASE64.encode(sealed) })?;
        BASE64.decode(key.value).ok().and_then(|bytes| bytes.try_into().ok()).ok_or(Error::Corrupt)
    }

    /// The iOS Keychain item outlives the app, so erasing has to delete it.
    pub fn forget_key(&self) -> Result<()> {
        #[cfg(mobile)]
        {
            self.run("forgetKey", ())
        }
        #[cfg(not(mobile))]
        {
            Ok(())
        }
    }

    #[cfg(mobile)]
    fn ask(&self, command: &str, args: KeyBytes) -> Result<KeyBytes> {
        Ok(self.handle.run_mobile_plugin::<KeyBytes>(command, args)?)
    }

    #[cfg(not(mobile))]
    fn ask(&self, _command: &str, _args: KeyBytes) -> Result<KeyBytes> {
        Err(Error::Unsupported)
    }

    /// Tauri's own restart only exits on Android.
    pub fn restart_app(&self) -> Result<()> {
        self.run("restartApp", ())
    }

    #[cfg(mobile)]
    fn run(&self, command: &str, args: impl Serialize) -> Result<()> {
        self.handle.run_mobile_plugin::<()>(command, args)?;
        Ok(())
    }

    #[cfg(not(mobile))]
    fn run(&self, _command: &str, _args: impl Serialize) -> Result<()> {
        Err(Error::Unsupported)
    }
}

pub trait PlatformExt<R: Runtime> {
    fn platform(&self) -> &Platform<R>;
}

impl<R: Runtime, T: Manager<R>> PlatformExt<R> for T {
    fn platform(&self) -> &Platform<R> {
        self.state::<Platform<R>>().inner()
    }
}

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_ft_platform);

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("ft-platform")
        .setup(|app, _api| {
            #[cfg(target_os = "android")]
            let platform = Platform { handle: _api.register_android_plugin("com.flickertalk.platform", "PlatformPlugin")? };
            #[cfg(target_os = "ios")]
            let platform = Platform { handle: _api.register_ios_plugin(init_plugin_ft_platform)? };
            #[cfg(not(mobile))]
            let platform = Platform::<R> { _runtime: std::marker::PhantomData };
            app.manage(platform);
            Ok(())
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_current_location_comes_back_as_swift_and_kotlin_resolve_it() {
        let found: Located = serde_json::from_value(serde_json::json!({
            "found": true, "lat": 40.41678, "lon": -3.70379, "accuracy": 35.0, "at": 1_790_000_000_000.0
        }))
        .unwrap();
        assert_eq!(found.location(), Some(Location { lat: 40.41678, lon: -3.70379, accuracy: 35.0, at: 1_790_000_000_000 }));
        let nothing: Located = serde_json::from_value(serde_json::json!({ "found": false })).unwrap();
        assert_eq!(nothing.location(), None);
    }

    #[test]
    fn a_translation_travels_as_swift_and_kotlin_read_and_resolve_it() {
        assert_eq!(serde_json::to_value(TranslatePair { from: "es", to: "en" }).unwrap(), serde_json::json!({ "from": "es", "to": "en" }));
        assert_eq!(serde_json::to_value(TranslateSource { text: "hola" }).unwrap(), serde_json::json!({ "text": "hola" }));
        assert_eq!(
            serde_json::to_value(TranslateRequest { text: "hola", from: "es", to: "en" }).unwrap(),
            serde_json::json!({ "text": "hola", "from": "es", "to": "en" })
        );
        let listed: TranslateLanguages = serde_json::from_value(serde_json::json!({ "languages": ["en", "es", "zh-TW"] })).unwrap();
        assert_eq!(listed.languages, ["en", "es", "zh-TW"]);
        for (word, status) in [("installed", TranslateStatus::Installed), ("downloadable", TranslateStatus::Downloadable), ("unsupported", TranslateStatus::Unsupported)] {
            let pair: PairState = serde_json::from_value(serde_json::json!({ "status": word })).unwrap();
            assert_eq!(pair.status, status);
        }
        assert!(serde_json::from_value::<PairState>(serde_json::json!({ "status": "supported" })).is_err());
        let ready: Prepared = serde_json::from_value(serde_json::json!({ "ready": true })).unwrap();
        assert!(ready.ready);
        let removed: Removed = serde_json::from_value(serde_json::json!({ "removed": false })).unwrap();
        assert!(!removed.removed);
    }

    #[test]
    fn a_detected_language_and_a_translation_count_only_when_found() {
        let detected = |json| serde_json::from_value::<Detected>(json).unwrap().tag();
        assert_eq!(detected(serde_json::json!({ "found": true, "tag": "es" })), Some("es".to_owned()));
        assert_eq!(detected(serde_json::json!({ "found": false })), None);
        assert_eq!(detected(serde_json::json!({ "found": false, "tag": "es" })), None);
        assert_eq!(detected(serde_json::json!({ "found": true })), None);
        assert_eq!(detected(serde_json::json!({ "found": true, "tag": "" })), None);
        let translated = |json| serde_json::from_value::<Translated>(json).unwrap().text();
        assert_eq!(translated(serde_json::json!({ "found": true, "text": "hello" })), Some("hello".to_owned()));
        assert_eq!(translated(serde_json::json!({ "found": false })), None);
        assert_eq!(translated(serde_json::json!({ "found": true, "text": null })), None);
    }

    #[cfg(not(mobile))]
    #[test]
    fn a_desktop_cannot_translate() {
        let platform = Platform::<tauri::Wry> { _runtime: std::marker::PhantomData };
        let languages: Result<Vec<String>> = platform.translate_languages();
        assert_eq!(languages.ok(), Some(vec![]));
        let detected: Result<Option<String>> = platform.translate_detect("hola");
        assert_eq!(detected.ok(), Some(None));
        let status: Result<TranslateStatus> = platform.translate_status("es", "en");
        assert_eq!(status.ok(), Some(TranslateStatus::Unsupported));
        let prepared: Result<bool> = platform.translate_prepare("es", "en");
        assert_eq!(prepared.ok(), Some(false));
        let removed: Result<bool> = platform.translate_remove("es", "en");
        assert_eq!(removed.ok(), Some(false));
        let translated: Result<Option<String>> = platform.translate_text("hola", "es", "en");
        assert_eq!(translated.ok(), Some(None));
    }

    #[test]
    fn the_push_token_comes_back_from_kotlin() {
        let answer: PushToken = serde_json::from_value(serde_json::json!({ "token": "fcm-abc" })).unwrap();
        assert_eq!(answer.token, "fcm-abc");
    }

    #[test]
    fn the_kotlin_side_gets_the_names_it_expects() {
        let open = serde_json::to_value(OpenFile { path: "/files/a.jpg", mime: "image/jpeg" }).unwrap();
        assert_eq!(open, serde_json::json!({ "path": "/files/a.jpg", "mime": "image/jpeg" }));
        let save = serde_json::to_value(SaveFile { path: "/files/a.jpg", name: "a.jpg", mime: "image/jpeg" }).unwrap();
        assert_eq!(save, serde_json::json!({ "path": "/files/a.jpg", "name": "a.jpg", "mime": "image/jpeg" }));
        let picked: Picked = serde_json::from_value(serde_json::json!({
            "files": [{ "path": "/data/uploads/1-a.jpg", "name": "a.jpg", "mime": "image/jpeg", "size": 12 }]
        }))
        .unwrap();
        assert_eq!(picked.files[0].name, "a.jpg");
        assert_eq!(picked.files[0].size, 12);
        let reminders = serde_json::to_value(Reminders { reminders: r#"[{"plugin":"p","id":"r1","at":5,"text":""}]"# }).unwrap();
        assert_eq!(reminders["reminders"], r#"[{"plugin":"p","id":"r1","at":5,"text":""}]"#);
        let tapped: PendingReminder = serde_json::from_value(serde_json::json!({ "reminder": "p\nr1" })).unwrap();
        assert_eq!(tapped.reminder, "p\nr1");
        let login = serde_json::to_value(Authorize { url: "https://accounts.example/auth", scheme: "com.flickertalk.app" }).unwrap();
        assert_eq!(login["scheme"], "com.flickertalk.app");
        let back: Authorized = serde_json::from_value(serde_json::json!({ "url": "com.flickertalk.app:/oauth?code=1" })).unwrap();
        assert!(back.url.starts_with("com.flickertalk.app:"));
        let ring = serde_json::to_value(Ringing { caller: "Alice", video: true, muted: true }).unwrap();
        assert_eq!(ring, serde_json::json!({ "caller": "Alice", "video": true, "muted": true }));
        let slots = serde_json::to_value(OpenSlots { slots: &[1, 3] }).unwrap();
        assert_eq!(slots, serde_json::json!({ "slots": [1, 3] }));
        let hours = serde_json::to_value(QuietHours { week: "all;all;all;all;all;none;none" }).unwrap();
        assert_eq!(hours, serde_json::json!({ "week": "all;all;all;all;all;none;none" }));
        let share = serde_json::to_value(ShareText { text: "Add me: https://flickertalk.com/add#card" }).unwrap();
        assert_eq!(share, serde_json::json!({ "text": "Add me: https://flickertalk.com/add#card" }));
        let subscription: Subscription = serde_json::from_value(serde_json::json!({ "until": 1_800_000_000_000i64 })).unwrap();
        assert_eq!(subscription.until, 1_800_000_000_000);
        let pick = serde_json::to_value(Pick { accept: "image/*" }).unwrap();
        assert_eq!(pick, serde_json::json!({ "accept": "image/*" }));
        let print = serde_json::to_value(SaveFile { path: "/files/a.pdf", name: "a.pdf", mime: "application/pdf" }).unwrap();
        assert_eq!(print, serde_json::json!({ "path": "/files/a.pdf", "name": "a.pdf", "mime": "application/pdf" }));
    }

    #[test]
    fn the_store_price_comes_back_as_the_store_formats_it() {
        let said: SubscriptionPrice = serde_json::from_value(serde_json::json!({ "price": "0,99 €" })).unwrap();
        assert_eq!(said.shown().as_deref(), Some("0,99 €"));
        for silent in [serde_json::json!({}), serde_json::json!({ "price": null }), serde_json::json!({ "price": "  " })] {
            let answer: SubscriptionPrice = serde_json::from_value(silent).unwrap();
            assert_eq!(answer.shown(), None);
        }
    }

    #[test]
    fn a_live_entitlement_is_read_as_the_native_side_sends_it() {
        let read = |json: &str| entitlement_until(tauri::ipc::InvokeResponseBody::Json(json.into()));
        assert_eq!(read(r#"{"until":1800000000000}"#), Some(1_800_000_000_000));
        assert_eq!(read(r#"{"until":0}"#), Some(0));
        assert_eq!(read(r#"{"price":"0,99 €"}"#), None);
        assert_eq!(read("not json"), None);
    }

    #[test]
    fn an_opened_link_comes_back_as_swift_and_kotlin_resolve_it() {
        let opened: OpenedLinkWire = serde_json::from_value(serde_json::json!({ "url": "https://flickertalk.com/add#card" })).unwrap();
        assert_eq!(opened.url(), Some("https://flickertalk.com/add#card".to_owned()));
        for none in [serde_json::json!({ "url": "" }), serde_json::json!({}), serde_json::json!({ "url": null })] {
            let opened: OpenedLinkWire = serde_json::from_value(none).unwrap();
            assert_eq!(opened.url(), None);
        }
    }

    #[test]
    fn a_link_opened_event_is_read_as_the_native_side_sends_it() {
        let read = |json: &str| link_opened(tauri::ipc::InvokeResponseBody::Json(json.into()));
        assert!(read(r#"{"event":"linkOpened"}"#));
        assert!(!read(r#"{"event":"answer"}"#));
        assert!(!read("not json"));
    }

    #[cfg(not(mobile))]
    #[test]
    fn a_desktop_has_no_opened_link() {
        let platform = Platform::<tauri::Wry> { _runtime: std::marker::PhantomData };
        assert_eq!(platform.opened_link().ok(), Some(None));
    }

    #[test]
    fn whether_exact_alarms_are_allowed_comes_back_as_kotlin_resolves_it() {
        let allowed: ExactAlarms = serde_json::from_value(serde_json::json!({ "allowed": true })).unwrap();
        assert!(allowed.allowed);
        let refused: ExactAlarms = serde_json::from_value(serde_json::json!({ "allowed": false })).unwrap();
        assert!(!refused.allowed);
    }

    #[cfg(not(mobile))]
    #[test]
    fn a_desktop_rings_reminders_on_time_without_asking() {
        let platform = Platform::<tauri::Wry> { _runtime: std::marker::PhantomData };
        assert_eq!(platform.exact_alarms().ok(), Some(true));
        assert_eq!(platform.request_exact_alarms().ok(), Some(true));
    }

    #[cfg(not(mobile))]
    #[test]
    fn a_desktop_has_no_store_price() {
        let platform = Platform::<tauri::Wry> { _runtime: std::marker::PhantomData };
        assert_eq!(platform.subscription_price().ok(), Some(None));
    }

    #[test]
    fn native_call_events_are_read_from_their_wire_form() {
        let read = |json: &str| call_event(tauri::ipc::InvokeResponseBody::Json(json.into()));
        assert_eq!(read(r#"{"event":"answer"}"#), Some(NativeCallEvent::Answer));
        assert_eq!(read(r#"{"event":"incoming"}"#), Some(NativeCallEvent::Incoming));
        assert_eq!(read(r#"{"event":"end"}"#), Some(NativeCallEvent::End));
        assert_eq!(read(r#"{"event":"mute","muted":true}"#), Some(NativeCallEvent::Mute(true)));
        assert_eq!(read(r#"{"event":"mute","muted":false}"#), Some(NativeCallEvent::Mute(false)));
        assert_eq!(read(r#"{"event":"audioActivated","generation":3}"#), Some(NativeCallEvent::AudioActivated(3)));
        assert_eq!(read(r#"{"event":"audioDeactivated","generation":2}"#), Some(NativeCallEvent::AudioDeactivated(2)));
        assert_eq!(read(r#"{"event":"audioActivated"}"#), Some(NativeCallEvent::AudioActivated(0)));
    }

    #[test]
    fn a_decline_and_an_answered_call_travel_as_the_native_side_says_them() {
        let read = |json: &str| call_event(tauri::ipc::InvokeResponseBody::Json(json.into()));
        assert_eq!(read(r#"{"event":"decline"}"#), Some(NativeCallEvent::Decline));
        let answering = serde_json::to_value(Answering { caller: "Alice", video: true }).unwrap();
        assert_eq!(answering, serde_json::json!({ "caller": "Alice", "video": true }));
    }

    #[test]
    fn a_call_event_is_read_from_the_text_kotlin_hands_over() {
        assert_eq!(parse_call_event(r#"{"event":"decline"}"#), Some(NativeCallEvent::Decline));
        assert_eq!(parse_call_event(r#"{"event":"end"}"#), Some(NativeCallEvent::End));
        assert_eq!(parse_call_event(r#"{"event":"incoming"}"#), Some(NativeCallEvent::Incoming));
        assert_eq!(parse_call_event(r#"{"event":"mute","muted":true}"#), Some(NativeCallEvent::Mute(true)));
        assert_eq!(parse_call_event(r#"{"event":"hold"}"#), None);
        assert_eq!(parse_call_event("not json"), None);
    }

    #[test]
    fn the_call_route_travels_as_the_native_side_says_it() {
        let read = |json: &str| call_event(tauri::ipc::InvokeResponseBody::Json(json.into()));
        assert_eq!(read(r#"{"event":"route","route":"headset"}"#), Some(NativeCallEvent::Route(CallRoute::Headset)));
        assert_eq!(read(r#"{"event":"route","route":"speaker"}"#), Some(NativeCallEvent::Route(CallRoute::Speaker)));
        assert_eq!(read(r#"{"event":"route","route":"receiver"}"#), Some(NativeCallEvent::Route(CallRoute::Receiver)));
        assert_eq!(read(r#"{"event":"route","route":"car"}"#), None);
        assert_eq!(read(r#"{"event":"route"}"#), None);
        assert_eq!(parse_call_event(r#"{"event":"route","route":"headset"}"#), Some(NativeCallEvent::Route(CallRoute::Headset)));
        assert_eq!(CallRoute::Headset.as_str(), "headset");
        assert_eq!(CallRoute::Speaker.as_str(), "speaker");
        assert_eq!(CallRoute::Receiver.as_str(), "receiver");
    }

    #[test]
    fn unknown_or_broken_call_events_are_ignored() {
        let read = |json: &str| call_event(tauri::ipc::InvokeResponseBody::Json(json.into()));
        assert_eq!(read(r#"{"event":"hold"}"#), None);
        assert_eq!(read(r#"{"event":"mute"}"#), None);
        assert_eq!(read("not json"), None);
        assert_eq!(call_event(tauri::ipc::InvokeResponseBody::Raw(vec![1, 2])), None);
    }

    #[test]
    fn the_call_channel_goes_native_and_its_events_reach_the_handler() {
        let heard = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = heard.clone();
        let channel = call_channel(move |event| sink.lock().unwrap().push(event));
        let args = serde_json::to_value(CallEvents { channel: &channel }).unwrap();
        assert_eq!(args, serde_json::json!({ "channel": format!("__CHANNEL__:{}", channel.id()) }));
        channel.send(serde_json::json!({ "event": "answer" })).unwrap();
        channel.send(serde_json::json!({ "event": "mute", "muted": true })).unwrap();
        channel.send(serde_json::json!({ "event": "nonsense" })).unwrap();
        assert_eq!(*heard.lock().unwrap(), vec![NativeCallEvent::Answer, NativeCallEvent::Mute(true)]);
    }

    #[test]
    fn the_speaker_and_the_answer_travel_as_swift_and_kotlin_read_them() {
        assert_eq!(serde_json::to_value(Speaker { on: true }).unwrap(), serde_json::json!({ "on": true }));
        let answered: Answered = serde_json::from_value(serde_json::json!({ "answered": true })).unwrap();
        assert!(answered.answered);
    }

    #[test]
    fn native_video_events_are_read_from_their_wire_form() {
        let read = |json: &str| call_event(tauri::ipc::InvokeResponseBody::Json(json.into()));
        assert_eq!(read(r#"{"event":"visible","visible":true}"#), Some(NativeCallEvent::Visible(true)));
        assert_eq!(read(r#"{"event":"visible","visible":false}"#), Some(NativeCallEvent::Visible(false)));
        assert_eq!(read(r#"{"event":"orientation","orientation":3}"#), Some(NativeCallEvent::Orientation(3)));
        assert_eq!(read(r#"{"event":"video"}"#), Some(NativeCallEvent::VideoRequested));
        assert_eq!(read(r#"{"event":"visible"}"#), None);
        assert_eq!(read(r#"{"event":"orientation"}"#), None);
    }

    #[test]
    fn native_video_travels_as_swift_and_kotlin_read_it() {
        let attach = serde_json::to_value(AttachVideo { remote_layer: 0x1000, local_layer: 0x2000 }).unwrap();
        assert_eq!(attach, serde_json::json!({ "remoteLayer": 4096, "localLayer": 8192 }));
        let layout = VideoLayout {
            remote: Some(VideoRect { x: 0.0, y: 0.0, width: 390.0, height: 844.0 }),
            local: Some(VideoRect { x: 278.0, y: 594.0, width: 96.0, height: 140.0 }),
            mirror_local: true,
            local_radius: 16.0,
        };
        assert_eq!(
            serde_json::to_value(layout).unwrap(),
            serde_json::json!({
                "remote": { "x": 0.0, "y": 0.0, "width": 390.0, "height": 844.0 },
                "local": { "x": 278.0, "y": 594.0, "width": 96.0, "height": 140.0 },
                "mirrorLocal": true,
                "localRadius": 16.0
            })
        );
        let hidden: VideoLayout = serde_json::from_value(serde_json::json!({ "remote": null, "local": null, "mirrorLocal": false, "localRadius": 0 })).unwrap();
        assert_eq!((hidden.remote, hidden.local), (None, None));
        let shape = serde_json::to_value(VideoShape { width: 480, height: 640, rotation: 90 }).unwrap();
        assert_eq!(shape, serde_json::json!({ "width": 480, "height": 640, "rotation": 90 }));
        assert_eq!(serde_json::to_value(CallVideo { on: true }).unwrap(), serde_json::json!({ "on": true }));
        let camera: Camera = serde_json::from_value(serde_json::json!({ "granted": false })).unwrap();
        assert!(!camera.granted);
    }

    #[test]
    fn the_open_app_on_answer_switch_travels_as_swift_reads_it() {
        assert_eq!(serde_json::to_value(OpenAppOnAnswer { on: true }).unwrap(), serde_json::json!({ "on": true }));
        assert_eq!(serde_json::to_value(OpenAppOnAnswer { on: false }).unwrap(), serde_json::json!({ "on": false }));
    }

    #[test]
    fn the_system_bars_travel_as_kotlin_reads_them() {
        assert_eq!(serde_json::to_value(SystemBars { dark: true }).unwrap(), serde_json::json!({ "dark": true }));
        assert_eq!(serde_json::to_value(SystemBars { dark: false }).unwrap(), serde_json::json!({ "dark": false }));
    }

    #[cfg(not(mobile))]
    #[test]
    fn a_desktop_has_no_system_bars_to_tell() {
        let platform = Platform::<tauri::Wry> { _runtime: std::marker::PhantomData };
        assert!(platform.set_system_bars(true).is_ok());
        assert!(platform.set_system_bars(false).is_ok());
    }

    #[test]
    fn the_appearance_travels_as_kotlin_reads_it() {
        for appearance in ["system", "dark", "light"] {
            assert_eq!(serde_json::to_value(Appearance { appearance }).unwrap(), serde_json::json!({ "appearance": appearance }));
        }
    }

    #[cfg(not(mobile))]
    #[test]
    fn a_desktop_keeps_no_appearance() {
        let platform = Platform::<tauri::Wry> { _runtime: std::marker::PhantomData };
        assert!(platform.set_appearance("light").is_ok());
    }

    #[test]
    fn an_outgoing_call_names_the_contact() {
        let outgoing = serde_json::to_value(Outgoing { name: "Alice", video: false }).unwrap();
        assert_eq!(outgoing, serde_json::json!({ "name": "Alice", "video": false }));
    }
}
