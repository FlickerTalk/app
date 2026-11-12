//! Views sent to the WebView must never carry keys or the route capability.

pub mod billing;
pub mod call_video;
pub mod calls;
pub mod chat;
pub mod circles;
pub mod contacts;
pub mod files;
pub mod moving;
pub mod plugin_api;
pub mod plugins;
pub mod sessions;
pub mod settings;
pub mod translate;
pub mod vault;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use anyhow::Context;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use ft_core::online::{self, Online};
use ft_core::{CallUpdate, Core, Event};
use ft_storage::{FileRecord, MessageState, Store};
use ft_webrtc::SessionConfig;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_ft_platform::{NativeCallEvent, PlatformExt};

use crate::push_core::{may_start_from_push, Host, PushSide};

use billing::{AppShop, ShopWord};
use call_video::{tell_bridge, video_layers, VideoGlue};
use calls::{
    answered_already, bridge_screen, can_present_now, listen_native_calls, native_call_queue, native_screen, ringing, CallEvent, NativeScreen, Ring,
    CALL_EVENT, CALL_SOCKET_FRESH,
};
use moving::{after_move, MoveEvent, MOVE_EVENT};
use plugin_api::sync_reminders;
use plugins::{look_for_updates, refresh_served_plugins, update_installed_plugins};
use vault::{VaultProgressView, VAULT_EVENT, VAULT_PROGRESS_EVENT};

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

#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct NoteView {
    plugin: String,
    text: String,
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

const DATABASE: &str = "flickertalk.db";
const KEY_FILE: &str = "storage.key";
const MOVE_DIR: &str = "move";
const REPLACED: &str = "replaced";

fn database_files(dir: &Path) -> [PathBuf; 4] {
    ["", "-wal", "-shm", "-journal"].map(|side| dir.join(format!("{DATABASE}{side}")))
}

// The database goes in last, so until then the copy left in move/ makes the next start finish the swap
pub fn apply_move(dir: &Path) -> std::io::Result<bool> {
    let incoming = dir.join(MOVE_DIR);
    let replaced = incoming.join(REPLACED);
    if let Some((database, key)) = ft_core::moving::received_move(&incoming) {
        std::fs::create_dir_all(&replaced)?;
        for file in database_files(dir).into_iter().chain([dir.join(KEY_FILE), dir.join(SEALED_KEY_FILE)]) {
            set_aside(&file, &replaced)?;
        }
        let new_key = incoming.join(KEY_FILE);
        write_private(&new_key, &key)?;
        std::fs::rename(new_key, dir.join(KEY_FILE))?;
        std::fs::rename(database, dir.join(DATABASE))?;
    } else if !replaced.exists() {
        return Ok(false);
    }
    std::fs::remove_dir_all(incoming)?;
    Ok(true)
}

// Never overwrite what an earlier start already set aside
fn set_aside(file: &Path, aside: &Path) -> std::io::Result<()> {
    let to = aside.join(file.file_name().unwrap_or_default());
    if to.try_exists()? {
        return Ok(());
    }
    match std::fs::rename(file, to) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
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

fn read_if_there(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Never replace a key that cannot be read or opened, or make one next to an existing database, because the identity would be lost.
pub fn storage_key(dir: &Path, vault: Option<&dyn KeyVault>) -> anyhow::Result<[u8; 32]> {
    std::fs::create_dir_all(dir)?;
    let (clear, sealed) = (dir.join(KEY_FILE), dir.join(SEALED_KEY_FILE));
    if let Some(vault) = vault {
        if let Some(bytes) = read_if_there(&sealed)? {
            return vault.open(&bytes).context("the key store cannot open the storage key");
        }
    }
    let key: [u8; 32] = match read_if_there(&clear)? {
        Some(bytes) => bytes.try_into().map_err(|_| anyhow::anyhow!("the storage key is corrupt"))?,
        None => {
            anyhow::ensure!(!dir.join(DATABASE).try_exists()?, "the storage key is missing");
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

pub const PLUGIN_EVENT: &str = "ft://plugin";

#[derive(Clone, Serialize)]
struct PluginEventView {
    plugin: String,
    contact: String,
    data: String,
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

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis() as i64)
        .unwrap_or(0)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_versions_go_together_and_play_takes_the_next_one() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
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
    fn message_states_have_the_names_the_ui_uses() {
        let names: Vec<_> = [MessageState::NotSent, MessageState::Pending, MessageState::Sent, MessageState::Delivered, MessageState::Read]
            .into_iter()
            .map(state_name)
            .collect();
        assert_eq!(names, ["unsent", "pending", "sent", "delivered", "read"]);
    }

    pub(super) fn record(chunks_done: i64, complete: bool, failed: bool) -> ft_storage::FileRecord {
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

    pub(super) fn scratch(name: &str) -> std::path::PathBuf {
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
        for name in ["flickertalk.db", "flickertalk.db-shm", "flickertalk.db-journal", "storage.key", "storage.key.sealed"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        std::fs::create_dir_all(dir.join("files").join("m1")).unwrap();
        std::fs::create_dir_all(dir.join("move")).unwrap();
        erase(&dir).unwrap();
        for name in ["flickertalk.db", "flickertalk.db-shm", "flickertalk.db-journal", "storage.key", "storage.key.sealed", "files", "move"] {
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
    fn a_missing_key_next_to_a_database_is_an_error_never_replaced() {
        let dir = scratch("missing-key");
        std::fs::write(dir.join("flickertalk.db"), b"history").unwrap();
        assert!(storage_key(&dir, None).is_err());
        assert!(storage_key(&dir, Some(&FakeVault::default())).is_err());
        assert!(!dir.join("storage.key").exists());
        assert!(!dir.join("storage.key.sealed").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_key_that_cannot_be_read_is_an_error_never_replaced() {
        let dir = scratch("unreadable-key");
        std::fs::create_dir_all(dir.join("storage.key")).unwrap();
        assert!(storage_key(&dir, Some(&FakeVault::default())).is_err());
        assert!(storage_key(&dir, None).is_err());
        assert!(dir.join("storage.key").is_dir());
        assert!(!dir.join("storage.key.sealed").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_sealed_key_that_cannot_be_read_is_an_error_never_replaced() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("unreadable-sealed-key");
        let vault = FakeVault::default();
        storage_key(&dir, Some(&vault)).expect("creates");
        let path = dir.join("storage.key.sealed");
        let sealed = std::fs::read(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o200)).unwrap();
        let opened = storage_key(&dir, Some(&vault));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(opened.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), sealed, "left as it was");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_install_and_an_erased_phone_start_with_a_new_key() {
        let root = scratch("first-key");
        let dir = root.join("app-data");
        let vault = FakeVault::default();
        let first = opened_key(&dir, Some(&vault)).expect("a new install");
        std::fs::write(dir.join("flickertalk.db"), b"history").unwrap();
        erase(&dir).unwrap();
        assert_ne!(opened_key(&dir, Some(&vault)).expect("an erased phone"), first);
        let _ = std::fs::remove_dir_all(&root);
    }

    fn a_phone_with_a_received_move(name: &str) -> PathBuf {
        let dir = scratch(name);
        std::fs::write(dir.join("flickertalk.db"), b"temporary").unwrap();
        std::fs::write(dir.join("flickertalk.db-wal"), b"old wal").unwrap();
        std::fs::write(dir.join("storage.key.sealed"), FakeVault::default().seal(&[1; 32]).unwrap()).unwrap();
        std::fs::create_dir_all(dir.join("move")).unwrap();
        std::fs::write(dir.join("move").join(ft_core::moving::MOVE_DB), b"moved").unwrap();
        std::fs::write(dir.join("move").join(ft_core::moving::MOVE_KEY), [2; 32]).unwrap();
        dir
    }

    fn assert_starts_with_the_moved_pair(dir: &Path) {
        assert_eq!(opened_key(dir, Some(&FakeVault::default())).expect("starts"), [2; 32]);
        assert_eq!(std::fs::read(dir.join("flickertalk.db")).unwrap(), b"moved");
        for name in ["flickertalk.db-wal", "flickertalk.db-shm", "flickertalk.db-journal", "move"] {
            assert!(!dir.join(name).exists(), "{name} is gone");
        }
        assert!(!holds(dir, b"temporary"), "the old database is gone once the new one is in");
    }

    fn holds(dir: &Path, needle: &[u8]) -> bool {
        std::fs::read_dir(dir).into_iter().flatten().flatten().any(|entry| {
            let path = entry.path();
            if path.is_dir() {
                holds(&path, needle)
            } else {
                std::fs::read(&path).is_ok_and(|bytes| bytes.windows(needle.len()).any(|window| window == needle))
            }
        })
    }

    fn set_aside(dir: &Path, names: &[&str]) {
        let aside = dir.join("move").join("replaced");
        std::fs::create_dir_all(&aside).unwrap();
        for name in names {
            std::fs::rename(dir.join(name), aside.join(name)).unwrap();
        }
    }

    #[test]
    fn a_stale_journal_of_the_old_database_never_meets_the_moved_one() {
        let dir = a_phone_with_a_received_move("apply-journal");
        std::fs::write(dir.join("flickertalk.db-journal"), b"old journal").unwrap();
        assert_starts_with_the_moved_pair(&dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_key_that_cannot_be_written_in_place_never_loses_the_moved_pair() {
        let dir = a_phone_with_a_received_move("apply-key-in-the-way");
        std::fs::create_dir_all(dir.join("storage.key").join("in the way")).unwrap();
        let _ = opened_key(&dir, Some(&FakeVault::default()));
        let _ = std::fs::remove_dir_all(dir.join("storage.key"));
        assert_starts_with_the_moved_pair(&dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_swap_that_fails_halfway_keeps_the_old_pair_until_a_start_can_finish_it() {
        let dir = a_phone_with_a_received_move("apply-fails");
        let in_the_way = dir.join("move").join("storage.key");
        std::fs::create_dir_all(in_the_way.join("in the way")).unwrap();
        let vault = FakeVault::default();
        assert!(opened_key(&dir, Some(&vault)).is_err());
        assert!(opened_key(&dir, Some(&vault)).is_err(), "and again on the next start");
        assert!(holds(&dir, b"temporary"), "the old database is kept");
        assert!(holds(&dir, &vault.seal(&[1; 32]).unwrap()), "with its key");
        std::fs::remove_dir_all(&in_the_way).unwrap();
        assert_starts_with_the_moved_pair(&dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_swap_cut_while_setting_the_old_pair_aside_is_finished_on_the_next_start() {
        let dir = a_phone_with_a_received_move("apply-cut-aside");
        set_aside(&dir, &["flickertalk.db"]);
        assert_starts_with_the_moved_pair(&dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_swap_cut_while_writing_the_new_key_is_finished_on_the_next_start() {
        let dir = a_phone_with_a_received_move("apply-cut-writing-key");
        set_aside(&dir, &["flickertalk.db", "flickertalk.db-wal", "storage.key.sealed"]);
        std::fs::write(dir.join("move").join("storage.key"), [2; 16]).unwrap();
        assert_starts_with_the_moved_pair(&dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_swap_cut_after_the_new_key_went_in_is_finished_on_the_next_start() {
        let dir = a_phone_with_a_received_move("apply-cut-key");
        set_aside(&dir, &["flickertalk.db", "flickertalk.db-wal", "storage.key.sealed"]);
        std::fs::write(dir.join("storage.key"), [2; 32]).unwrap();
        assert_starts_with_the_moved_pair(&dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_swap_cut_after_the_new_database_went_in_only_clears_the_old_pair_away() {
        let dir = a_phone_with_a_received_move("apply-cut-database");
        set_aside(&dir, &["flickertalk.db", "flickertalk.db-wal", "storage.key.sealed"]);
        std::fs::write(dir.join("storage.key"), [2; 32]).unwrap();
        std::fs::rename(dir.join("move").join(ft_core::moving::MOVE_DB), dir.join("flickertalk.db")).unwrap();
        assert_starts_with_the_moved_pair(&dir);
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
}
