use std::path::Path;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_ft_platform::PlatformExt;

use super::files::{core_pick_files, file_of, upload_all_picked, PickedView};
use super::{failed, restart, Client, RESTART_PAUSE};

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
    use super::*;

    #[test]
    fn the_drive_s_state_tells_the_page_what_it_needs() {
        use ft_core::vault::{VaultState, VaultStatus};
        let view = VaultStatusView::from(VaultStatus { state: VaultState::Outdated, provider: Some("google".into()), drive: None, problem: None, tries_left: 5, retry_at: None });
        assert_eq!(view.state, "outdated");
        let view = VaultStatusView::from(VaultStatus { state: VaultState::Locked, provider: Some("google".into()), drive: None, problem: None, tries_left: 0, retry_at: Some(42) });
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!((json["state"].as_str(), json["triesLeft"].as_u64(), json["retryAt"].as_i64()), (Some("locked"), Some(0), Some(42)));
    }
}
