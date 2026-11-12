use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use ft_core::Core;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_ft_platform::PlatformExt;

use super::files::{file_of, HandedFile, PickedView, PLUGIN_FILE_LIMIT};
use super::{failed, Client};

#[derive(Serialize, Deserialize)]
pub struct ReminderView {
    pub plugin: String,
    pub id: String,
    pub at: i64,
    pub text: String,
}

pub(super) async fn sync_reminders(app: &AppHandle, core: &Arc<Core>) {
    let Ok(reminders) = core.reminders().await else { return };
    let views: Vec<ReminderView> = reminders
        .into_iter()
        .map(|reminder| ReminderView { plugin: reminder.plugin, id: reminder.id, at: reminder.at, text: reminder.text })
        .collect();
    if let Ok(json) = serde_json::to_string(&views) {
        let _ = app.platform().set_reminders(&json);
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
pub async fn core_read_message_file(message: String, client: State<'_, Client>) -> Result<HandedFile, String> {
    let file = file_of(&client, &message).await?;
    if file.size as u64 > PLUGIN_FILE_LIMIT {
        return Err("that file is too big for a plugin".to_owned());
    }
    let bytes = tokio::fs::read(&file.path).await.map_err(|_| "the bytes of that file are no longer here".to_owned())?;
    Ok(HandedFile { name: file.name, mime: file.mime, data: BASE64.encode(bytes) })
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
pub async fn core_exact_alarms(app: AppHandle) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || app.platform().exact_alarms()).await.map_err(failed)?.map_err(failed)
}

#[tauri::command]
pub async fn core_request_exact_alarms(app: AppHandle) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || app.platform().request_exact_alarms()).await.map_err(failed)?.map_err(failed)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_made_file_says_whether_it_was_sent_or_staged() {
        let staged = serde_json::to_value(MadeView { sent: false, staged: Some(PickedView { path: "/p".into(), name: "a.pdf".into(), mime: "application/pdf".into(), size: 3 }) }).unwrap();
        assert_eq!(staged["sent"], false);
        assert_eq!(staged["staged"]["name"], "a.pdf");
        let sent = serde_json::to_value(MadeView { sent: true, staged: None }).unwrap();
        assert_eq!(sent, serde_json::json!({ "sent": true }));
    }

    #[test]
    fn a_tool_that_runs_by_itself_goes_to_the_screen() {
        let hook = ft_core::plugins::ChatHook { plugin: "com.flickertalk.translate".into(), auto_message: true, auto_composer: false, lang: Some("es".into()) };
        let view = serde_json::to_value(ChatHookView::from(hook.clone())).unwrap();
        assert_eq!(view, serde_json::json!({ "plugin": "com.flickertalk.translate", "autoMessage": true, "autoComposer": false, "lang": "es" }));
        let unknown = serde_json::to_value(ChatHookView::from(ft_core::plugins::ChatHook { lang: None, ..hook })).unwrap();
        assert!(unknown.get("lang").is_none(), "no language yet, nothing written: {unknown}");
    }
}
