//! FlickerTalk's native bridge (Plan §5, §82): what the phone's OS only lets Kotlin (and, later,
//! Swift) do. The app's Rust calls it; the WebView never does, so it has no commands.
//!
//! - `open_file`: shows a file of the app in the viewer the user picks (Android's FileProvider
//!   lends it for that viewing only).
//! - `save_to_downloads`: copies a file to the phone's Downloads (MediaStore, Android 10 and up).
//! - `start_ringing` / `stop_ringing`: an incoming call rings with the user's ringtone, vibrates
//!   as the phone is set to (silent, vibrate only or normal) and shows on the screen, over the
//!   lock screen if need be.
//! - Native calls (2026-09-28): `listen_calls` hears answer, hang-up, mute and the audio
//!   session's activation straight from CallKit / the call notification, through a channel made
//!   here (no WebView); `call_started_outgoing`, `call_connected` and `call_ended` tell the OS
//!   about the call; `request_microphone` asks for the microphone first.
//! - `push_token`, `request_notifications`: the FCM token the router wakes this device with, and
//!   Android 13's permission to show the notification a wake-up brings (M4).
//! - `seal_key` / `open_key`: the storage key, sealed by Android Keystore (an AES key that never
//!   leaves it) or kept in the iOS Keychain (this device only), §94.
//! - `pick_files`: the system file picker, copying what was picked into the app's folder.
//! - `share_text`: the system share sheet (WhatsApp, Signal, mail…) with a text, such as the
//!   Contact Card link (§32).
//! - `restart_app`: starts the app again (after moving to a new phone, §60); Tauri's own restart
//!   only exits on Android.

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

/// Arguments of the Kotlin `openFile` command.
#[derive(Serialize)]
struct OpenFile<'a> {
    path: &'a str,
    mime: &'a str,
}

/// Arguments of the Kotlin `pickFiles` command: what kind of file is wanted, if it matters.
#[derive(Serialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct Pick<'a> {
    accept: &'a str,
}

/// Arguments of the Kotlin `saveToDownloads` command.
#[derive(Serialize)]
struct SaveFile<'a> {
    path: &'a str,
    name: &'a str,
    mime: &'a str,
}

/// Arguments of the native `startRinging` command: who is calling, and whether it is video.
#[derive(Serialize)]
struct Ringing<'a> {
    caller: &'a str,
    video: bool,
    /// A muted contact (app#4): the call shows but makes no noise.
    muted: bool,
}

/// Arguments of the native `callAnswering` command: who the answered call is, and whether it is
/// video.
#[derive(Serialize)]
struct Answering<'a> {
    caller: &'a str,
    video: bool,
}

/// What the native side tells the core about a call it owns (CallKit on iOS; the ongoing call
/// notification on Android), with no WebView in between (2026-09-28).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeCallEvent {
    /// A call push arrived (PushKit, or FCM's `t: call` with the process alive): the router
    /// found this phone offline, so its socket is dead; the core reconnects at once to get the
    /// offer, and later the caller's end.
    Incoming,
    /// The user answered (CallKit's answer button, also from the lock screen).
    Answer,
    /// The user hung up or declined, or the system ended the call.
    End,
    /// The user declined an incoming call that had not connected (2026-09-29): the incoming call
    /// notification's decline, or CallKit's end of such a call. Unlike `End`, before the offer
    /// comes it waits for it, and declines it as it arrives.
    Decline,
    /// The user muted (`true`) or unmuted the microphone.
    Mute(bool),
    /// The system activated the audio session: the audio unit may start now (iOS). With the
    /// generation of the CallKit call it belongs to (it grows with each call), so that a late
    /// event of an older call is told apart.
    AudioActivated(u64),
    /// The system took the audio session away: the audio unit stops (iOS).
    AudioDeactivated(u64),
    /// The app came to the screen (`true`) or left it (native video, 2026-09-29): the phone
    /// holds our camera while the app is away (iOS stops it anyway), and gives it back.
    Visible(bool),
    /// The phone turned (native video): iOS `UIDeviceOrientation.rawValue`, Android the display
    /// rotation in degrees. The rotation our frames carry follows it.
    Orientation(i32),
    /// The user asked for video from the phone's own call screen (native video): CallKit's video
    /// button opens the app with a video call intent; the ongoing call notification's camera
    /// action on Android. Our camera turns on.
    VideoRequested,
}

/// One event as Swift and Kotlin send it: `{"event": "mute", "muted": true}`.
#[cfg_attr(not(mobile), allow(dead_code))]
fn call_event(body: tauri::ipc::InvokeResponseBody) -> Option<NativeCallEvent> {
    #[derive(Deserialize)]
    struct Wire {
        event: String,
        muted: Option<bool>,
        #[serde(default)]
        generation: u64,
        visible: Option<bool>,
        orientation: Option<i32>,
    }
    let wire: Wire = body.deserialize().ok()?;
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
        _ => None,
    }
}

/// The channel the native side sends call events through: created here, registered by Tauri, and
/// handed to the plugin, so events reach Rust even with no WebView (a locked iPhone).
#[cfg_attr(not(mobile), allow(dead_code))]
fn call_channel(handler: impl Fn(NativeCallEvent) + Send + Sync + 'static) -> tauri::ipc::Channel<serde_json::Value> {
    tauri::ipc::Channel::new(move |body| {
        if let Some(event) = call_event(body) {
            handler(event);
        }
        Ok(())
    })
}

/// Arguments of the native `registerCallEvents` command.
#[derive(Serialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct CallEvents<'a> {
    channel: &'a tauri::ipc::Channel<serde_json::Value>,
}

/// Arguments of the native `setSpeaker` command.
#[derive(Serialize)]
struct Speaker {
    on: bool,
}

/// Arguments of the native `attachVideo` command (native video, 2026-09-29): the call's layers on
/// iOS, as addresses of `CALayer`s the Rust side owns (`ft_media::views::layers`); 0 on Android,
/// where the Kotlin side makes its own `SurfaceView`s and hands their surfaces over JNI.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AttachVideo {
    remote_layer: u64,
    local_layer: u64,
}

/// A rectangle of the WebView, in CSS pixels from its top left corner (`getBoundingClientRect`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VideoRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Where the WebView leaves room for the call's pictures (native video, 2026-09-29): the native
/// views sit under the WebView, which is transparent there. `None` hides a picture.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoLayout {
    pub remote: Option<VideoRect>,
    pub local: Option<VideoRect>,
    /// Our preview as in a mirror (the front camera).
    pub mirror_local: bool,
    /// The corners of our preview, in CSS pixels.
    pub local_radius: f64,
}

/// Arguments of the native `videoShape` command: see `ft_media::RemoteShape`.
#[derive(Serialize)]
struct VideoShape {
    width: u32,
    height: u32,
    rotation: u16,
}

/// Arguments of the native `callVideo` command: whether the call has video now.
#[derive(Serialize)]
struct CallVideo {
    on: bool,
}

/// Arguments of the native `setOpenAppOnAnswer` command (iOS only).
#[derive(Serialize)]
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
struct OpenAppOnAnswer {
    on: bool,
}

/// What the native `requestCamera` command answers.
#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct Camera {
    granted: bool,
}

/// What the native `answerCall` resolves with: whether CallKit took the answer.
#[derive(Deserialize)]
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
struct Answered {
    answered: bool,
}

/// Arguments of the native `diagnose` command (temporary, 2026-09-28): a state name only, never a
/// name or an identifier.
#[derive(Serialize)]
struct Diagnose<'a> {
    what: &'a str,
}

/// Arguments of the native `callStartedOutgoing` command.
#[derive(Serialize)]
struct Outgoing<'a> {
    name: &'a str,
    video: bool,
}

/// Arguments of the native `setOpenSlots` command (app#9): the slots of the open hidden sessions.
#[derive(Serialize)]
struct OpenSlots<'a> {
    slots: &'a [u8],
}

/// Arguments of the native `setQuietHours` command (app#7): the week in the core's compact form.
#[derive(Serialize)]
struct QuietHours<'a> {
    week: &'a str,
}

/// Arguments of the native `setReminders` command (2026-09-27): every reminder of every plugin,
/// as JSON `[{plugin, id, at, text}]`. The OS is only the alarm clock; the core keeps the truth.
#[derive(Serialize)]
struct Reminders<'a> {
    reminders: &'a str,
}

/// What the native `pendingReminder` command answers: `plugin\nid` of the reminder the user
/// tapped, or nothing.
#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct PendingReminder {
    reminder: String,
}

/// Arguments of the native `authorize` command (drive, 2026-09-27): the login page to open in
/// the system's browser sheet, and the scheme the provider sends the user back with.
#[derive(Serialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct Authorize<'a> {
    url: &'a str,
    scheme: &'a str,
}

/// What `authorize` answers: the URL the provider sent the user back with, code and all.
#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct Authorized {
    url: String,
}

/// Arguments of the native `shareText` command.
#[derive(Serialize)]
struct ShareText<'a> {
    text: &'a str,
}

/// The key, or its sealed form, as the native side takes and gives it: base64.
#[derive(Serialize, Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct KeyBytes {
    value: String,
}

/// A file the user picked, already copied into the app's folder.
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

/// What the native `requestMicrophone` resolves with.
#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct Microphone {
    granted: bool,
}

/// What Kotlin's `canShowFullScreen` resolves with.
#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct FullScreen {
    allowed: bool,
}

/// What the Store said: until when the subscription runs (ms), 0 when there is none.
#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct Subscription {
    until: i64,
}

/// What Kotlin's `pushToken` resolves with.
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
    /// Opens a file of the app (under its `files` folder) in another app.
    pub fn open_file(&self, path: &str, mime: &str) -> Result<()> {
        self.run("openFile", OpenFile { path, mime })
    }

    /// Copies a file of the app to the phone's Downloads, where the user finds it.
    pub fn save_to_downloads(&self, path: &str, name: &str, mime: &str) -> Result<()> {
        self.run("saveToDownloads", SaveFile { path, name, mime })
    }

    /// Hands a file to the phone's print service; the user picks the printer (§53).
    pub fn print_file(&self, path: &str, name: &str, mime: &str) -> Result<()> {
        self.run("printFile", SaveFile { path, name, mime })
    }

    /// Asks the Store for the yearly subscription and answers until when it runs (ms), or 0.
    /// The app never handles the payment itself (§47).
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

    /// What the Store already knows about this phone's subscription, without asking to buy.
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

    /// Opens the system share sheet with `text`.
    pub fn share_text(&self, text: &str) -> Result<()> {
        self.run("shareText", ShareText { text })
    }

    /// Opens the system share sheet with a file of the app (§62).
    pub fn share_file(&self, path: &str, name: &str, mime: &str) -> Result<()> {
        self.run("shareFile", SaveFile { path, name, mime })
    }

    /// Rings and shows the incoming call on the screen (§66).
    pub fn start_ringing(&self, caller: &str, video: bool, muted: bool) -> Result<()> {
        self.run("startRinging", Ringing { caller, video, muted })
    }

    /// Tells the native side which hidden sessions are open, so their wake-ups are heard (app#9).
    pub fn set_open_slots(&self, slots: &[u8]) -> Result<()> {
        self.run("setOpenSlots", OpenSlots { slots })
    }

    /// Hands the weekly hours to the native side, which checks them even with the app closed.
    pub fn set_quiet_hours(&self, week: &str) -> Result<()> {
        self.run("setQuietHours", QuietHours { week })
    }

    pub fn stop_ringing(&self) -> Result<()> {
        self.run("stopRinging", ())
    }

    /// The incoming call is being answered (2026-09-29), from any screen, or was answered before
    /// its offer came: it rings no more, and the phone's own call screen learns who it is. On
    /// Android the ringing stops and the app stays over the lock screen; on iOS CallKit, which
    /// answered it, gets the name.
    pub fn call_answering(&self, caller: &str, video: bool) -> Result<()> {
        self.call("callAnswering", Answering { caller, video })
    }

    /// Hears what the native side does with a call (2026-09-28): answer, hang-up and mute from
    /// CallKit or the call notification, and the audio session's activation. Events arrive with
    /// no WebView (PushKit may launch the app in the background of a locked iPhone); those that
    /// come before this is called wait natively and arrive now. A second call replaces the
    /// handler. The handler runs on a native background thread: it must not block for long.
    /// On desktop it does nothing.
    pub fn listen_calls(&self, handler: impl Fn(NativeCallEvent) + Send + Sync + 'static) {
        #[cfg(mobile)]
        {
            let channel = call_channel(handler);
            // Only fails if the plugin is not loaded, and then there is no native call to hear.
            let _ = self.run("registerCallEvents", CallEvents { channel: &channel });
        }
        #[cfg(not(mobile))]
        let _ = handler;
    }

    /// This phone starts a call (2026-09-28): on iOS CallKit takes it (and the audio session); on
    /// Android the audio goes to communication mode and a foreground service keeps the microphone
    /// with the app in the background.
    pub fn call_started_outgoing(&self, name: &str, video: bool) -> Result<()> {
        self.call("callStartedOutgoing", Outgoing { name, video })
    }

    /// The call is connected. On iOS an outgoing call shows as connected; a call answered in the
    /// app's own screen (no CallKit call yet) joins CallKit here, so it gets the audio session. On
    /// Android the call's audio and foreground service start if they had not.
    pub fn call_connected(&self) -> Result<()> {
        self.call("callConnected", ())
    }

    /// The app's answer button (2026-09-28): on iOS, CallKit is asked to answer the ringing call,
    /// so the audio session comes as on the lock screen, and its `Answer` event reaches the core
    /// through `listen_calls`. `false` when the OS has no call to answer (Android, desktop, or
    /// CallKit never had it): the core answers by itself.
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

    /// The call's voice on the speaker (`true`) or the receiver (2026-09-28). A voice call starts
    /// on the receiver, like a phone call.
    pub fn set_speaker(&self, on: bool) -> Result<()> {
        self.call("setSpeaker", Speaker { on })
    }

    /// Temporary call diagnostics (2026-09-28): writes a state name (never a name or an
    /// identifier) to the device log, `os_log` on iOS and `Log` on Android. Nothing on desktop.
    pub fn diagnose(&self, what: &str) {
        let _ = self.call("diagnose", Diagnose { what });
    }

    /// The call is over, whoever ended it: CallKit lets go / the service stops and the audio mode
    /// goes back to what it was.
    pub fn call_ended(&self) -> Result<()> {
        self.call("callEnded", ())
    }

    /// Opening the app when an incoming call is answered (iOS experiment, 2026-09-29): with `on`,
    /// the bridge reports every incoming call to CallKit as a video call (`hasVideo`), whatever
    /// its media, because iOS opens the app after the answer (asking to unlock first on the lock
    /// screen) only for video calls. The call's real media does not change. Remembered by the
    /// bridge, so a call PushKit reports before the core starts follows it. Nothing elsewhere:
    /// Android opens the app on its own.
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

    /// Whether the microphone may be used, asking the user if it was never asked (2026-09-28).
    /// The core calls it before starting or answering a native call: the WebView used to ask
    /// through `getUserMedia`. `false` means denied. On desktop, `true`.
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

    /// Shows the call's video views under the WebView (native video, 2026-09-29), hidden until
    /// `video_layout` places them. iOS: `remote_layer` and `local_layer` are the addresses of the
    /// call's `CALayer`s (`ft_media::views::layers`), added as sublayers on the main thread;
    /// Android: both 0, the Kotlin side makes its `SurfaceView`s and hands their surfaces to
    /// `ft_media::views` over JNI. The WebView turns transparent. Nothing on desktop.
    pub fn attach_video(&self, remote_layer: usize, local_layer: usize) -> Result<()> {
        self.call("attachVideo", AttachVideo { remote_layer: remote_layer as u64, local_layer: local_layer as u64 })
    }

    /// Places the video views where the WebView left room for them.
    pub fn video_layout(&self, layout: &VideoLayout) -> Result<()> {
        self.call("videoLayout", layout)
    }

    /// How to lay the other side's picture out: its upright size (Android, for the aspect ratio)
    /// and the turn its view applies (iOS, bounds swapped for a quarter turn).
    pub fn video_shape(&self, width: u32, height: u32, rotation: u16) -> Result<()> {
        self.call("videoShape", VideoShape { width, height, rotation })
    }

    /// Takes the video views away and makes the WebView opaque again. On iOS it returns once the
    /// layers are out of the view hierarchy: only then may the call's video devices go.
    pub fn detach_video(&self) -> Result<()> {
        self.call("detachVideo", ())
    }

    /// Whether the call has video now (either camera on): CallKit's `hasVideo` on iOS; on
    /// Android the ongoing notification (`CallStyle.setIsVideo`) and the `camera` type of the
    /// call's foreground service, only while our camera may run.
    pub fn call_video(&self, on: bool) -> Result<()> {
        self.call("callVideo", CallVideo { on })
    }

    /// Whether the camera may be used, asking the user if it was never asked (native video,
    /// 2026-09-29): before our camera turns on. `false` means denied. On desktop, `true`.
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

    /// A call command: on desktop there is no native call to tell, so it is not an error.
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

    /// The token the router wakes this device with: FCM's on Android; on iOS, `gateway:bundle:token`
    /// for APNs (2026-09-28).
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

    /// Files picked with the system picker, copied into the app's folder. The WebView's own file
    /// input leaves the user outside the app with no way back unless they pick something. Asking
    /// for `image/*` opens the photo picker, which is a sheet over the app (§62).
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

    /// A photo taken now with the camera app, copied into the app's folder like a picked file;
    /// nothing if the user backed out (or the camera is not allowed yet).
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

    /// Tells the phone's alarm clock every reminder there is (2026-09-27): it cancels what it
    /// had and schedules these. The list is JSON, as the core writes it.
    pub fn set_reminders(&self, reminders: &str) -> Result<()> {
        self.run("setReminders", Reminders { reminders })
    }

    /// The reminder the user tapped to open the app, as `plugin\nid`, once; empty otherwise.
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

    /// Opens a login page in the system's browser sheet (Custom Tabs, `ASWebAuthenticationSession`)
    /// and waits for the provider to send the user back with `scheme`. Returns that URL. The
    /// WebView never sees the page nor the tokens (§54).
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

    /// Whether this phone lets a call take the whole screen (Android 14 asks the user).
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

    /// Opens the system screen where the user allows it.
    pub fn ask_full_screen(&self) -> Result<()> {
        self.run("askFullScreen", ())
    }

    pub fn request_notifications(&self) -> Result<()> {
        self.run("requestNotifications", ())
    }

    /// Seals the storage key with the OS key store; the result is kept in a file.
    pub fn seal_key(&self, key: &[u8; 32]) -> Result<Vec<u8>> {
        let sealed = self.ask("sealKey", KeyBytes { value: BASE64.encode(key) })?;
        BASE64.decode(sealed.value).map_err(|_| Error::Corrupt)
    }

    pub fn open_key(&self, sealed: &[u8]) -> Result<[u8; 32]> {
        let key = self.ask("openKey", KeyBytes { value: BASE64.encode(sealed) })?;
        BASE64.decode(key.value).ok().and_then(|bytes| bytes.try_into().ok()).ok_or(Error::Corrupt)
    }

    #[cfg(mobile)]
    fn ask(&self, command: &str, args: KeyBytes) -> Result<KeyBytes> {
        Ok(self.handle.run_mobile_plugin::<KeyBytes>(command, args)?)
    }

    #[cfg(not(mobile))]
    fn ask(&self, _command: &str, _args: KeyBytes) -> Result<KeyBytes> {
        Err(Error::Unsupported)
    }

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

/// Access to the bridge from anything that manages Tauri state.
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

    // What Kotlin's `pushToken` resolves with.
    #[test]
    fn the_push_token_comes_back_from_kotlin() {
        let answer: PushToken = serde_json::from_value(serde_json::json!({ "token": "fcm-abc" })).unwrap();
        assert_eq!(answer.token, "fcm-abc");
    }

    // The Kotlin side reads these names (`OpenFileArgs`, `SaveFileArgs` in PlatformPlugin.kt).
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
        // 2026-09-27: reminders travel as one JSON list, and a login comes back as a URL.
        let reminders = serde_json::to_value(Reminders { reminders: r#"[{"plugin":"p","id":"r1","at":5,"text":""}]"# }).unwrap();
        assert_eq!(reminders["reminders"], r#"[{"plugin":"p","id":"r1","at":5,"text":""}]"#);
        let tapped: PendingReminder = serde_json::from_value(serde_json::json!({ "reminder": "p\nr1" })).unwrap();
        assert_eq!(tapped.reminder, "p\nr1");
        let login = serde_json::to_value(Authorize { url: "https://accounts.example/auth", scheme: "com.flickertalk.app" }).unwrap();
        assert_eq!(login["scheme"], "com.flickertalk.app");
        let back: Authorized = serde_json::from_value(serde_json::json!({ "url": "com.flickertalk.app:/oauth?code=1" })).unwrap();
        assert!(back.url.starts_with("com.flickertalk.app:"));
        let ring = serde_json::to_value(Ringing { caller: "Ioan", video: true, muted: true }).unwrap();
        assert_eq!(ring, serde_json::json!({ "caller": "Ioan", "video": true, "muted": true }));
        let slots = serde_json::to_value(OpenSlots { slots: &[1, 3] }).unwrap();
        assert_eq!(slots, serde_json::json!({ "slots": [1, 3] }));
        let hours = serde_json::to_value(QuietHours { week: "all;all;all;all;all;none;none" }).unwrap();
        assert_eq!(hours, serde_json::json!({ "week": "all;all;all;all;all;none;none" }));
        let share = serde_json::to_value(ShareText { text: "Add me: https://flickertalk.com/add#card" }).unwrap();
        assert_eq!(share, serde_json::json!({ "text": "Add me: https://flickertalk.com/add#card" }));
        // Printing is the phone's: the app hands it a file and the user picks the printer (§53).
        let subscription: Subscription = serde_json::from_value(serde_json::json!({ "until": 1_800_000_000_000i64 })).unwrap();
        assert_eq!(subscription.until, 1_800_000_000_000);
        let pick = serde_json::to_value(Pick { accept: "image/*" }).unwrap();
        assert_eq!(pick, serde_json::json!({ "accept": "image/*" }));
        let print = serde_json::to_value(SaveFile { path: "/files/a.pdf", name: "a.pdf", mime: "application/pdf" }).unwrap();
        assert_eq!(print, serde_json::json!({ "path": "/files/a.pdf", "name": "a.pdf", "mime": "application/pdf" }));
    }

    // Native calls (2026-09-28): what Swift and Kotlin send through the channel.
    #[test]
    fn native_call_events_are_read_from_their_wire_form() {
        let read = |json: &str| call_event(tauri::ipc::InvokeResponseBody::Json(json.into()));
        assert_eq!(read(r#"{"event":"answer"}"#), Some(NativeCallEvent::Answer));
        assert_eq!(read(r#"{"event":"incoming"}"#), Some(NativeCallEvent::Incoming));
        assert_eq!(read(r#"{"event":"end"}"#), Some(NativeCallEvent::End));
        assert_eq!(read(r#"{"event":"mute","muted":true}"#), Some(NativeCallEvent::Mute(true)));
        assert_eq!(read(r#"{"event":"mute","muted":false}"#), Some(NativeCallEvent::Mute(false)));
        // Each CallKit call has its generation: a late event of an older call is told apart.
        assert_eq!(read(r#"{"event":"audioActivated","generation":3}"#), Some(NativeCallEvent::AudioActivated(3)));
        assert_eq!(read(r#"{"event":"audioDeactivated","generation":2}"#), Some(NativeCallEvent::AudioDeactivated(2)));
        assert_eq!(read(r#"{"event":"audioActivated"}"#), Some(NativeCallEvent::AudioActivated(0)));
    }

    // 2026-09-29: the incoming call notification's decline (and CallKit's, of a call that never
    // connected) is a decline, not a hang-up: before the offer comes, the core declines the offer
    // when it arrives. And a call answered already only tells the native side who it is.
    #[test]
    fn a_decline_and_an_answered_call_travel_as_the_native_side_says_them() {
        let read = |json: &str| call_event(tauri::ipc::InvokeResponseBody::Json(json.into()));
        assert_eq!(read(r#"{"event":"decline"}"#), Some(NativeCallEvent::Decline));
        let answering = serde_json::to_value(Answering { caller: "Ioan", video: true }).unwrap();
        assert_eq!(answering, serde_json::json!({ "caller": "Ioan", "video": true }));
    }

    // Anything else is ignored, never a panic on the phone's main thread.
    #[test]
    fn unknown_or_broken_call_events_are_ignored() {
        let read = |json: &str| call_event(tauri::ipc::InvokeResponseBody::Json(json.into()));
        assert_eq!(read(r#"{"event":"hold"}"#), None);
        assert_eq!(read(r#"{"event":"mute"}"#), None);
        assert_eq!(read("not json"), None);
        assert_eq!(call_event(tauri::ipc::InvokeResponseBody::Raw(vec![1, 2])), None);
    }

    // The channel reaches the native side as Tauri's reference, which Swift's and Kotlin's
    // `Channel` decode; what they send comes back to the handler.
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

    // Speaker or receiver, and CallKit answering for the app's button (2026-09-28).
    #[test]
    fn the_speaker_and_the_answer_travel_as_swift_and_kotlin_read_them() {
        assert_eq!(serde_json::to_value(Speaker { on: true }).unwrap(), serde_json::json!({ "on": true }));
        let answered: Answered = serde_json::from_value(serde_json::json!({ "answered": true })).unwrap();
        assert!(answered.answered);
    }

    // Temporary call diagnostics (2026-09-28): a state name, as Swift's and Kotlin's `diagnose` read it.
    #[test]
    fn a_diagnostic_is_a_state_name() {
        let args = serde_json::to_value(Diagnose { what: "audio activated; device running" }).unwrap();
        assert_eq!(args, serde_json::json!({ "what": "audio activated; device running" }));
    }

    // Native video (2026-09-29, docs/video-nativo.md): what Swift and Kotlin add to the channel.
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

    // Native video: the views, where the WebView draws them, the remote picture's shape, the
    // call's video for CallKit and the ongoing notification, and the camera permission.
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
        // The WebView sends the same shape: it reads back what it wrote.
        let hidden: VideoLayout = serde_json::from_value(serde_json::json!({ "remote": null, "local": null, "mirrorLocal": false, "localRadius": 0 })).unwrap();
        assert_eq!((hidden.remote, hidden.local), (None, None));
        let shape = serde_json::to_value(VideoShape { width: 480, height: 640, rotation: 90 }).unwrap();
        assert_eq!(shape, serde_json::json!({ "width": 480, "height": 640, "rotation": 90 }));
        assert_eq!(serde_json::to_value(CallVideo { on: true }).unwrap(), serde_json::json!({ "on": true }));
        let camera: Camera = serde_json::from_value(serde_json::json!({ "granted": false })).unwrap();
        assert!(!camera.granted);
    }

    // Opening the app when an incoming call is answered (iOS experiment, 2026-09-29): what
    // Swift's `setOpenAppOnAnswer` reads (`OpenAppOnAnswerArgs`).
    #[test]
    fn the_open_app_on_answer_switch_travels_as_swift_reads_it() {
        assert_eq!(serde_json::to_value(OpenAppOnAnswer { on: true }).unwrap(), serde_json::json!({ "on": true }));
        assert_eq!(serde_json::to_value(OpenAppOnAnswer { on: false }).unwrap(), serde_json::json!({ "on": false }));
    }

    #[test]
    fn an_outgoing_call_names_the_contact() {
        let outgoing = serde_json::to_value(Outgoing { name: "Ioan", video: false }).unwrap();
        assert_eq!(outgoing, serde_json::json!({ "name": "Ioan", "video": false }));
    }
}
