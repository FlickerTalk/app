use ft_core::online::Online;
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_ft_platform::PlatformExt;

use super::chat::{conversation_views, ConversationView};
use super::circles::{circle_views, CircleView};
use super::{failed, Client};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    id: String,
    conversations: Vec<ConversationView>,
    requests: Vec<ConversationView>,
    circles: Vec<CircleView>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_view_is_its_id_and_its_conversations() {
        let view = serde_json::to_value(SessionView { id: "s1".to_owned(), conversations: vec![], requests: vec![], circles: vec![] }).unwrap();
        assert_eq!(view, serde_json::json!({ "id": "s1", "conversations": [], "requests": [], "circles": [] }));
    }
}
