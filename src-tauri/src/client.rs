//! The app's bridge to ft-core (Plan §82, §106 M3): starts the core online and exposes thin
//! commands to the UI. No business logic here: every command delegates to the core, and what
//! crosses to the WebView are plain views (never keys, never the capability, §54).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use anyhow::Context;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use ft_core::online::{self, Online};
use ft_core::{Core, Event};
use ft_storage::{Conversation, FileRecord, Message, MessageState, Store};
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
    #[serde(skip_serializing_if = "Option::is_none")]
    file: Option<FileView>,
}

impl MessageView {
    pub fn new(message: &Message, file: Option<&FileRecord>) -> Self {
        Self {
            id: message.message_id.clone(),
            outgoing: message.outgoing,
            text: message.body.clone(),
            sent_at: message.sent_at,
            state: state_name(message.state),
            file: file.map(FileView::from),
        }
    }
}

/// A file message's transfer (§62–63). `path` is on this device, for showing images.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    name: String,
    size: i64,
    mime: String,
    /// From 0 to 1: chunks sent (outgoing) or received (incoming).
    progress: f64,
    /// `transferring`, `done` (the receiver has it and the hash matches) or `failed`.
    state: &'static str,
    path: String,
}

impl From<&FileRecord> for FileView {
    fn from(file: &FileRecord) -> Self {
        let progress = match file.chunks() {
            _ if file.complete => 1.0,
            0 => 0.0,
            chunks => file.chunks_done as f64 / chunks as f64,
        };
        let state = if file.complete {
            "done"
        } else if file.failed {
            "failed"
        } else {
            "transferring"
        };
        Self { name: file.name.clone(), size: file.size, mime: file.mime.clone(), progress, state, path: file.path.clone() }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationView {
    id: String,
    name: String,
    unread: i64,
    blocked: bool,
    /// Whether a direct connection with the contact is open now.
    connected: bool,
    last: Option<MessageView>,
}

impl ConversationView {
    pub fn new(conversation: &Conversation, connected: bool) -> Self {
        Self {
            id: conversation.contact.device_id.clone(),
            name: conversation.contact.name.clone(),
            unread: conversation.unread,
            blocked: conversation.contact.blocked,
            connected,
            last: conversation.last.as_ref().map(|last| MessageView::new(last, None)),
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

/// A random name for a file being copied in from the WebView.
pub fn new_upload_id() -> String {
    rand::random::<[u8; 16]>().iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Where an upload is written. Only ids made by `new_upload_id` are accepted.
pub fn upload_path(dir: &Path, id: &str) -> Result<PathBuf, String> {
    let valid = id.len() == 32 && id.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c));
    if !valid {
        return Err("invalid upload".to_owned());
    }
    Ok(dir.join("outgoing").join(id))
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

    async fn online(&self) -> Result<&Online, String> {
        self.online.get_or_try_init(|| self.start()).await.map_err(|error| error.to_string())
    }

    async fn core(&self) -> Result<Arc<Core>, String> {
        Ok(self.online().await?.core.clone())
    }

    fn dir(&self) -> Result<&Path, String> {
        self.dir.get().map(PathBuf::as_path).ok_or_else(|| "the app is not set up yet".to_owned())
    }

    async fn start(&self) -> anyhow::Result<Online> {
        let dir = self.dir.get().context("the app is not set up yet")?;
        let key = storage_key(dir)?;
        let store = Store::open(&dir.join("flickertalk.db")).await?;
        let online = online::start(store, key, ROUTER, SessionConfig::default()).await?;
        online.core.set_files_dir(dir.join("files"));

        if let Some(app) = self.app.get().cloned() {
            let mut events = online.core.events();
            tauri::async_runtime::spawn(async move {
                while let Ok(event) = events.recv().await {
                    let contact = match event {
                        Event::MessagesChanged { contact } => Some(contact),
                        Event::ContactsChanged | Event::ConnectionChanged { .. } => None,
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
    let online = client.online().await?;
    let connected = online.network.connected().await;
    let store = online.core.store();
    let mut views = Vec::new();
    for conversation in store.conversations().await.map_err(failed)? {
        let mut view = ConversationView::new(&conversation, connected.contains(&conversation.contact.device_id));
        if let Some(last) = view.last.as_mut() {
            last.file = store.file(&last.id).await.map_err(failed)?.as_ref().map(FileView::from);
        }
        views.push(view);
    }
    Ok(views)
}

#[tauri::command]
pub async fn core_messages(contact: String, limit: i64, client: State<'_, Client>) -> Result<Vec<MessageView>, String> {
    let core = client.core().await?;
    let files: HashMap<String, FileRecord> =
        core.store().files(&contact).await.map_err(failed)?.into_iter().map(|file| (file.message_id.clone(), file)).collect();
    let messages = core.store().messages(&contact, limit).await.map_err(failed)?;
    Ok(messages.iter().map(|message| MessageView::new(message, files.get(&message.message_id))).collect())
}

/// Starts copying a file from the WebView into the app; returns the upload's id.
#[tauri::command]
pub async fn core_upload_start(client: State<'_, Client>) -> Result<String, String> {
    let id = new_upload_id();
    let path = upload_path(client.dir()?, &id)?;
    let parent = path.parent().expect("uploads live in a directory");
    tokio::fs::create_dir_all(parent).await.map_err(failed)?;
    tokio::fs::File::create(&path).await.map_err(failed)?;
    Ok(id)
}

/// Appends a slice, in base64 (Android's IPC only carries JSON), to the upload.
#[tauri::command]
pub async fn core_upload_append(upload: String, data: String, client: State<'_, Client>) -> Result<(), String> {
    let bytes = BASE64.decode(data).map_err(failed)?;
    let path = upload_path(client.dir()?, &upload)?;
    let mut file = tokio::fs::OpenOptions::new().append(true).open(&path).await.map_err(failed)?;
    tokio::io::AsyncWriteExt::write_all(&mut file, &bytes).await.map_err(failed)
}

/// Offers the uploaded file to the contact; the file stays in the app until it is delivered.
#[tauri::command]
pub async fn core_send_file(contact: String, upload: String, name: String, mime: String, client: State<'_, Client>) -> Result<(), String> {
    let path = upload_path(client.dir()?, &upload)?;
    if !path.exists() {
        return Err("unknown upload".to_owned());
    }
    let core = client.core().await?;
    tauri::async_runtime::spawn(async move {
        let _ = core.send_file(&contact, &path, &name, &mime).await;
    });
    Ok(())
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
        let view = MessageView::new(&message(MessageState::Delivered, true), None);
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
        let view = serde_json::to_value(ConversationView::new(&conversation, true)).unwrap();
        assert_eq!(view["id"], "ft_bob");
        assert_eq!(view["connected"], true, "a direct connection is open");
        assert_eq!(view["name"], "Bob");
        assert_eq!(view["unread"], 2);
        assert_eq!(view["last"]["state"], "pending");
        assert!(view.get("card").is_none(), "the card, with the route capability, stays in Rust");
    }

    fn record(chunks_done: i64, complete: bool, failed: bool) -> ft_storage::FileRecord {
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
        }
    }

    // A file message shows what is known about the transfer, never more (§84).
    #[test]
    fn file_messages_carry_their_transfer() {
        let view = MessageView::new(&message(MessageState::Delivered, false), Some(&record(2, false, false)));
        assert_eq!(serde_json::to_value(&view).unwrap()["file"], serde_json::json!({
            "name": "photo.jpg", "size": 100, "mime": "image/jpeg", "progress": 0.5,
            "state": "transferring", "path": "/data/files/m1/photo.jpg"
        }));
        let done = serde_json::to_value(FileView::from(&record(4, true, false))).unwrap();
        assert_eq!((done["state"].as_str(), done["progress"].as_f64()), (Some("done"), Some(1.0)));
        assert_eq!(serde_json::to_value(FileView::from(&record(0, false, true))).unwrap()["state"], "failed");
        let empty = ft_storage::FileRecord { size: 0, ..record(0, true, false) };
        assert_eq!(serde_json::to_value(FileView::from(&empty)).unwrap()["progress"], 1.0);
    }

    #[test]
    fn a_text_has_no_file() {
        let view = serde_json::to_value(MessageView::new(&message(MessageState::Sent, true), None)).unwrap();
        assert!(view.get("file").is_none());
    }

    // Uploads are named by the app, never by the WebView: no way out of their directory.
    #[test]
    fn upload_ids_cannot_point_elsewhere() {
        let dir = Path::new("/data");
        let id = new_upload_id();
        assert_eq!(upload_path(dir, &id).unwrap(), dir.join("outgoing").join(&id));
        assert_ne!(new_upload_id(), id);
        for bad in ["../identity", "", "abc", "/etc/passwd", "0123456789abcdef0123456789abcdeg"] {
            assert!(upload_path(dir, bad).is_err(), "{bad}");
        }
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
