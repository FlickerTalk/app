//! FlickerTalk's native bridge (Plan §5, §82): what the phone's OS only lets Kotlin (and, later,
//! Swift) do. The app's Rust calls it; the WebView never does, so it has no commands.
//!
//! - `open_file`: shows a file of the app in the viewer the user picks (Android's FileProvider
//!   lends it for that viewing only).
//! - `save_to_downloads`: copies a file to the phone's Downloads (MediaStore, Android 10 and up).
//! - `start_ringing` / `stop_ringing`: an incoming call rings with the user's ringtone, vibrates
//!   as the phone is set to (silent, vibrate only or normal) and shows on the screen, over the
//!   lock screen if need be.
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

/// What Kotlin's `pendingCall` resolves with: what the user pressed on the call notification.
#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct PendingCall {
    action: String,
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

    /// The FCM token of this device.
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

    /// What the user pressed on the call notification ("answer", "decline" or nothing), once.
    pub fn pending_call(&self) -> Result<String> {
        #[cfg(mobile)]
        {
            Ok(self.handle.run_mobile_plugin::<PendingCall>("pendingCall", ())?.action)
        }
        #[cfg(not(mobile))]
        {
            Ok(String::new())
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
        let pending: PendingCall = serde_json::from_value(serde_json::json!({ "action": "answer" })).unwrap();
        assert_eq!(pending.action, "answer");
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
}
