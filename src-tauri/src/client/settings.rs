use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_ft_platform::PlatformExt;

use super::{failed, Client};

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

#[tauri::command]
pub async fn core_system_bars(dark: bool, app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || app.platform().set_system_bars(dark)).await.map_err(failed)?.map_err(failed)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_became_of_a_suggestion_reaches_the_screen_as_one_of_three_words() {
        assert_eq!(feedback_word(ft_core::Feedback::Sent), "sent");
        assert_eq!(feedback_word(ft_core::Feedback::TooMany), "tooMany");
        assert_eq!(feedback_word(ft_core::Feedback::Failed), "failed");
    }

    #[test]
    fn the_push_provider_is_the_one_of_the_platform() {
        assert_eq!(push_provider(), if cfg!(target_os = "ios") { "apns" } else { "fcm" });
    }

    #[test]
    fn me_carries_the_free_period() {
        let me = MeView { id: "ft_me".to_owned(), name: "Alice".to_owned(), mailbox: true, receipts: false, free_until: 42, auto_download: 10 };
        assert_eq!(serde_json::to_value(me).unwrap(), serde_json::json!({
            "id": "ft_me", "name": "Alice", "mailbox": true, "receipts": false, "freeUntil": 42, "autoDownload": 10
        }));
    }
}
