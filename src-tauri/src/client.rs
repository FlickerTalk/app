//! The app's bridge to ft-core (Plan §82, §106 M3): starts the core online and exposes thin
//! commands to the UI. No business logic here: every command delegates to the core, and what
//! crosses to the WebView are plain views (never keys, never the capability, §54).

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use anyhow::Context;
use ft_core::online::{self, Online};
use ft_core::{Core, Event};
use ft_storage::{Conversation, Message, MessageState, Store};
use ft_webrtc::SessionConfig;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::OnceCell;

/// The router of the cluster, behind the load balancer (Plan §75).
pub const ROUTER: &str = "https://api.flickertalk.com";
/// Sent to the UI whenever contacts or messages change; `contact` says which conversation.
pub const CHANGED_EVENT: &str = "ft://changed";

pub fn state_name(state: MessageState) -> &'static str {
    match state {
        MessageState::Pending => "pending",
        MessageState::Sent => "sent",
        MessageState::Delivered => "delivered",
        MessageState::Read => "read",
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageView {
    id: String,
    outgoing: bool,
    text: String,
    sent_at: i64,
    state: &'static str,
}

impl From<&Message> for MessageView {
    fn from(message: &Message) -> Self {
        Self {
            id: message.message_id.clone(),
            outgoing: message.outgoing,
            text: message.body.clone(),
            sent_at: message.sent_at,
            state: state_name(message.state),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationView {
    id: String,
    name: String,
    unread: i64,
    blocked: bool,
    last: Option<MessageView>,
}

impl From<&Conversation> for ConversationView {
    fn from(conversation: &Conversation) -> Self {
        Self {
            id: conversation.contact.device_id.clone(),
            name: conversation.contact.name.clone(),
            unread: conversation.unread,
            blocked: conversation.contact.blocked,
            last: conversation.last.as_ref().map(MessageView::from),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeView {
    id: String,
    name: String,
    mailbox: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactView {
    id: String,
    name: String,
    fingerprint: String,
    mailbox: bool,
    blocked: bool,
}

#[derive(Clone, Serialize)]
struct Changed {
    contact: Option<String>,
}

/// The 32-byte key that seals the identity and the Olm sessions at rest, in the app's private
/// storage. TODO(§94): keep it in Android Keystore / Keychain through a native bridge.
pub fn storage_key(dir: &Path) -> anyhow::Result<[u8; 32]> {
    let path = dir.join("storage.key");
    if let Ok(bytes) = std::fs::read(&path) {
        return bytes.try_into().map_err(|_| anyhow::anyhow!("the storage key is corrupt"));
    }
    std::fs::create_dir_all(dir)?;
    let key: [u8; 32] = rand::random();
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    std::io::Write::write_all(&mut options.open(&path)?, &key)?;
    Ok(key)
}

/// The running core, started once.
#[derive(Default)]
pub struct Client {
    online: OnceCell<Online>,
    dir: OnceLock<PathBuf>,
    app: OnceLock<AppHandle>,
}

impl Client {
    pub fn setup(&self, app: &AppHandle) -> anyhow::Result<()> {
        let dir = app.path().app_data_dir().context("no app data directory")?;
        let _ = self.dir.set(dir);
        let _ = self.app.set(app.clone());
        Ok(())
    }

    async fn core(&self) -> Result<Arc<Core>, String> {
        let online = self.online.get_or_try_init(|| self.start()).await.map_err(|error| error.to_string())?;
        Ok(online.core.clone())
    }

    async fn start(&self) -> anyhow::Result<Online> {
        let dir = self.dir.get().context("the app is not set up yet")?;
        let key = storage_key(dir)?;
        let store = Store::open(&dir.join("flickertalk.db")).await?;
        let online = online::start(store, key, ROUTER, SessionConfig::default()).await?;

        if let Some(app) = self.app.get().cloned() {
            let mut events = online.core.events();
            tauri::async_runtime::spawn(async move {
                while let Ok(event) = events.recv().await {
                    let contact = match event {
                        Event::MessagesChanged { contact } => Some(contact),
                        Event::ContactsChanged => None,
                    };
                    let _ = app.emit(CHANGED_EVENT, Changed { contact });
                }
            });
        }
        Ok(online)
    }
}

/// Starts the core as soon as the app is up, so it connects before the first screen asks.
pub fn start_in_background(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = app.state::<Client>().core().await;
    });
}

fn failed(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[tauri::command]
pub async fn core_me(client: State<'_, Client>) -> Result<MeView, String> {
    let core = client.core().await?;
    Ok(MeView {
        id: core.device_id().to_string(),
        name: core.name().await.map_err(failed)?.unwrap_or_default(),
        mailbox: core.mailbox().await.map_err(failed)?,
    })
}

#[tauri::command]
pub async fn core_set_name(name: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.set_name(&name).await.map_err(failed)
}

/// Stored now; contacts are told in the background.
#[tauri::command]
pub async fn core_set_mailbox(enabled: bool, client: State<'_, Client>) -> Result<(), String> {
    let core = client.core().await?;
    tauri::async_runtime::spawn(async move {
        let _ = core.set_mailbox(enabled).await;
    });
    Ok(())
}

/// This device's Contact Card as a link, for the QR code and for sharing.
#[tauri::command]
pub async fn core_card(client: State<'_, Client>) -> Result<String, String> {
    Ok(client.core().await?.my_card().await.map_err(failed)?.to_link())
}

/// Adds the owner of a scanned or pasted card; returns their id at once and introduces us in the
/// background.
#[tauri::command]
pub async fn core_add_contact(link: String, client: State<'_, Client>) -> Result<String, String> {
    let core = client.core().await?;
    let card = ft_contacts::ContactCard::from_link(&link).map_err(failed)?;
    if &card.device_id() == core.device_id() {
        return Err("that is your own contact card".to_owned());
    }
    let id = card.device_id().to_string();
    tauri::async_runtime::spawn(async move {
        let _ = core.add_contact(&link, None).await;
    });
    Ok(id)
}

#[tauri::command]
pub async fn core_conversations(client: State<'_, Client>) -> Result<Vec<ConversationView>, String> {
    let core = client.core().await?;
    Ok(core.store().conversations().await.map_err(failed)?.iter().map(ConversationView::from).collect())
}

#[tauri::command]
pub async fn core_messages(contact: String, limit: i64, client: State<'_, Client>) -> Result<Vec<MessageView>, String> {
    let core = client.core().await?;
    Ok(core.store().messages(&contact, limit).await.map_err(failed)?.iter().map(MessageView::from).collect())
}

/// Stored at once (the UI hears about it); delivered in the background.
#[tauri::command]
pub async fn core_send(contact: String, text: String, client: State<'_, Client>) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("nothing to send".to_owned());
    }
    let core = client.core().await?;
    tauri::async_runtime::spawn(async move {
        let _ = core.send_text(&contact, &text).await;
    });
    Ok(())
}

#[tauri::command]
pub async fn core_mark_read(contact: String, client: State<'_, Client>) -> Result<(), String> {
    let core = client.core().await?;
    tauri::async_runtime::spawn(async move {
        let _ = core.mark_read(&contact).await;
    });
    Ok(())
}

#[tauri::command]
pub async fn core_contact(contact: String, client: State<'_, Client>) -> Result<ContactView, String> {
    let core = client.core().await?;
    let stored = core.store().contact(&contact).await.map_err(failed)?.ok_or("unknown contact")?;
    Ok(ContactView {
        fingerprint: core.fingerprint(&contact).await.map_err(failed)?,
        id: stored.device_id,
        name: stored.name,
        mailbox: stored.mailbox,
        blocked: stored.blocked,
    })
}

#[tauri::command]
pub async fn core_block(contact: String, blocked: bool, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.block(&contact, blocked).await.map_err(failed)
}

#[tauri::command]
pub async fn core_rename(contact: String, name: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.rename_contact(&contact, &name).await.map_err(failed)
}

#[cfg(test)]
mod tests {
    use ft_storage::{Contact, Conversation, Message, MessageState};

    use super::*;

    fn message(state: MessageState, outgoing: bool) -> Message {
        Message { message_id: "m1".to_owned(), contact: "ft_bob".to_owned(), outgoing, body: "hi".to_owned(), sent_at: 42, state }
    }

    #[test]
    fn message_states_have_the_names_the_ui_uses() {
        let names: Vec<_> = [MessageState::Pending, MessageState::Sent, MessageState::Delivered, MessageState::Read]
            .into_iter()
            .map(state_name)
            .collect();
        assert_eq!(names, ["pending", "sent", "delivered", "read"]);
    }

    #[test]
    fn messages_become_plain_views() {
        let view = MessageView::from(&message(MessageState::Delivered, true));
        assert_eq!(serde_json::to_value(&view).unwrap(), serde_json::json!({
            "id": "m1", "outgoing": true, "text": "hi", "sentAt": 42, "state": "delivered"
        }));
    }

    #[test]
    fn conversations_carry_the_last_message_and_the_unread_count() {
        let contact = Contact {
            device_id: "ft_bob".to_owned(),
            name: "Bob".to_owned(),
            card: vec![1],
            mailbox: true,
            blocked: false,
            introduced: true,
            added_at: 1,
        };
        let conversation = Conversation { contact, last: Some(message(MessageState::Pending, true)), unread: 2 };
        let view = serde_json::to_value(ConversationView::from(&conversation)).unwrap();
        assert_eq!(view["id"], "ft_bob");
        assert_eq!(view["name"], "Bob");
        assert_eq!(view["unread"], 2);
        assert_eq!(view["last"]["state"], "pending");
        assert!(view.get("card").is_none(), "the card, with the route capability, stays in Rust");
    }

    // The key that seals the identity is created once and kept (Plan §106: Keystore later, §94).
    #[test]
    fn the_storage_key_is_created_once_and_kept() {
        let dir = std::env::temp_dir().join(format!("ft-key-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let first = storage_key(&dir).expect("creates");
        assert_eq!(storage_key(&dir).expect("reads"), first);
        assert_ne!(first, [0; 32]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
