use std::collections::HashMap;

use serde::Serialize;
use tauri::State;

use super::{failed, state_name, Client, NoteView};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CircleView {
    id: String,
    name: String,
    members: Vec<CircleMemberView>,
    admin: bool,
    admins_only: bool,
    left: bool,
    unread: i64,
    last: Option<CircleMessageView>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CircleMemberView {
    id: String,
    name: String,
    admin: bool,
    me: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CircleMessageView {
    id: String,
    outgoing: bool,
    sender: String,
    sender_name: String,
    kind: String,
    text: String,
    sent_at: i64,
    state: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    notes: Vec<NoteView>,
}

impl CircleMessageView {
    pub fn new(message: &ft_storage::CircleMessage, names: &HashMap<String, String>) -> Self {
        Self {
            id: message.message_id.clone(),
            outgoing: message.outgoing,
            sender: message.sender.clone(),
            sender_name: names.get(&message.sender).cloned().unwrap_or_else(|| message.sender.chars().take(9).collect()),
            kind: message.kind.clone(),
            text: message.body.clone(),
            sent_at: message.sent_at,
            state: state_name(message.state),
            notes: Vec::new(),
        }
    }

    pub fn noted<'a>(mut self, notes: impl Iterator<Item = &'a ft_storage::MessageNote>) -> Self {
        self.notes = notes.map(|note| NoteView { plugin: note.plugin.clone(), text: note.text.clone() }).collect();
        self
    }
}

async fn circle_names(core: &ft_core::Core, circle: &str) -> Result<HashMap<String, String>, String> {
    let mut names = HashMap::new();
    for member in core.circle_members(circle).await.map_err(failed)? {
        let shown = match core.store().contact(&member.device_id).await.map_err(failed)? {
            Some(contact) if contact.accepted => contact.name,
            _ => member.name,
        };
        names.insert(member.device_id, shown);
    }
    if let Some(name) = core.name().await.map_err(failed)? {
        names.insert(core.device_id().to_string(), name);
    }
    Ok(names)
}

async fn circle_view(core: &ft_core::Core, conversation: ft_storage::CircleConversation) -> Result<CircleView, String> {
    let names = circle_names(core, &conversation.circle.id).await?;
    let me = core.device_id().to_string();
    let members = core
        .circle_members(&conversation.circle.id)
        .await
        .map_err(failed)?
        .into_iter()
        .map(|member| CircleMemberView {
            me: member.device_id == me,
            name: names.get(&member.device_id).cloned().unwrap_or(member.name),
            id: member.device_id,
            admin: member.admin,
        })
        .collect();
    Ok(CircleView {
        admin: core.is_circle_admin(&conversation.circle.id).await.map_err(failed)?,
        id: conversation.circle.id,
        name: conversation.circle.name,
        members,
        admins_only: conversation.circle.admins_only,
        left: conversation.circle.left,
        unread: conversation.unread,
        last: conversation.last.as_ref().map(|last| CircleMessageView::new(last, &names)),
    })
}

pub(super) async fn circle_views(core: &ft_core::Core, session: Option<&str>) -> Result<Vec<CircleView>, String> {
    let mut views = Vec::new();
    for conversation in core.store().circle_conversations(session).await.map_err(failed)? {
        views.push(circle_view(core, conversation).await?);
    }
    Ok(views)
}

#[tauri::command]
pub async fn core_circles(client: State<'_, Client>) -> Result<Vec<CircleView>, String> {
    let core = client.core().await?;
    circle_views(&core, None).await
}

#[tauri::command]
pub async fn core_circle(circle: String, client: State<'_, Client>) -> Result<CircleView, String> {
    let core = client.core().await?;
    let record = core.store().circle(&circle).await.map_err(failed)?.ok_or("unknown circle")?;
    let session = record.session.clone();
    let conversation = core
        .store()
        .circle_conversations(session.as_deref())
        .await
        .map_err(failed)?
        .into_iter()
        .find(|conversation| conversation.circle.id == circle)
        .ok_or("unknown circle")?;
    circle_view(&core, conversation).await
}

#[tauri::command]
pub async fn core_circle_create(name: String, members: Vec<String>, session: Option<String>, client: State<'_, Client>) -> Result<String, String> {
    client.core().await?.create_circle(&name, &members, session.as_deref()).await.map_err(failed)
}

#[tauri::command]
pub async fn core_circle_invite(circle: String, contact: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.invite_to_circle(&circle, &contact).await.map_err(failed)
}

#[tauri::command]
pub async fn core_circle_remove(circle: String, contact: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.remove_from_circle(&circle, &contact).await.map_err(failed)
}

#[tauri::command]
pub async fn core_circle_set_admin(circle: String, contact: String, admin: bool, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.set_circle_admin(&circle, &contact, admin).await.map_err(failed)
}

#[tauri::command]
pub async fn core_circle_rename(circle: String, name: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.rename_circle(&circle, &name).await.map_err(failed)
}

#[tauri::command]
pub async fn core_circle_admins_only(circle: String, admins_only: bool, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.set_circle_admins_only(&circle, admins_only).await.map_err(failed)
}

#[tauri::command]
pub async fn core_circle_leave(circle: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.leave_circle(&circle).await.map_err(failed)
}

#[tauri::command]
pub async fn core_circle_forget(circle: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.forget_circle(&circle).await.map_err(failed)
}

#[tauri::command]
pub async fn core_circle_send(circle: String, text: String, client: State<'_, Client>) -> Result<String, String> {
    if text.trim().is_empty() {
        return Err("nothing to send".to_owned());
    }
    let core = client.core().await?;
    let sent = core.queue_circle_text(&circle, &text).await.map_err(failed)?;
    tauri::async_runtime::spawn(async move {
        core.deliver_circle_queue().await;
    });
    Ok(sent)
}

#[tauri::command]
pub async fn core_circle_messages(circle: String, limit: i64, client: State<'_, Client>) -> Result<Vec<CircleMessageView>, String> {
    let core = client.core().await?;
    let names = circle_names(&core, &circle).await?;
    let messages = core.store().circle_messages(&circle, limit).await.map_err(failed)?;
    let notes = core.store().notes(&circle).await.map_err(failed)?;
    Ok(messages
        .iter()
        .map(|message| CircleMessageView::new(message, &names).noted(notes.iter().filter(|note| note.message_id == message.message_id)))
        .collect())
}

#[tauri::command]
pub async fn core_circle_mark_read(circle: String, client: State<'_, Client>) -> Result<(), String> {
    let core = client.core().await?;
    tauri::async_runtime::spawn(async move {
        let _ = core.mark_circle_read(&circle).await;
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_circle_message_carries_the_notes_tools_left_under_it() {
        let said = ft_storage::CircleMessage {
            message_id: "cm1".into(),
            circle: "circle-1".into(),
            sender: "ft_bob".into(),
            outgoing: false,
            kind: "text".into(),
            body: "salut".into(),
            sent_at: 1,
            received_at: 1,
            state: ft_storage::MessageState::Delivered,
        };
        let notes = [ft_storage::MessageNote { message_id: "cm1".into(), plugin: "com.flickertalk.translate".into(), text: "hola".into() }];
        let view = serde_json::to_value(CircleMessageView::new(&said, &HashMap::new()).noted(notes.iter())).unwrap();
        assert_eq!(view["notes"], serde_json::json!([{ "plugin": "com.flickertalk.translate", "text": "hola" }]));
        assert!(serde_json::to_value(CircleMessageView::new(&said, &HashMap::new())).unwrap().get("notes").is_none());
    }
}
