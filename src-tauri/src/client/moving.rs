use std::sync::Arc;

use ft_core::moving::MoveUpdate;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use super::{failed, restart, Client, RESTART_PAUSE};

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

pub(super) fn after_move(app: &AppHandle, router: &Arc<ft_core::RouterClient>, update: MoveUpdate) {
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

#[tauri::command]
pub async fn core_move_invite(client: State<'_, Client>) -> Result<String, String> {
    client.core().await?.invite_move().await.map_err(failed)
}

#[tauri::command]
pub async fn core_move_to(link: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.move_to(&link).await.map_err(failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_updates_reach_the_ui() {
        use ft_core::moving::MoveUpdate;
        assert_eq!(serde_json::to_value(MoveEvent::from(MoveUpdate::Progress { done: 3, total: 8 })).unwrap(),
            serde_json::json!({ "kind": "progress", "done": 3, "total": 8 }));
        for (update, kind) in [(MoveUpdate::Received, "received"), (MoveUpdate::Sent, "sent"), (MoveUpdate::Failed, "failed")] {
            assert_eq!(serde_json::to_value(MoveEvent::from(update)).unwrap()["kind"], kind);
        }
    }
}
