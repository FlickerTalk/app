use std::collections::{HashMap, HashSet};

use ft_core::online::Online;
use ft_storage::{Conversation, FileRecord, Message};
use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_ft_platform::PlatformExt;

use super::{failed, located, state_name, Client, NoteView};

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
    #[serde(skip_serializing_if = "Option::is_none")]
    quote: Option<QuoteView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reactions: Option<ReactionsView>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pinned: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    edited: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    deleted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    scheduled_for: Option<i64>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    update_pending: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    notes: Vec<NoteView>,
}

#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
pub struct ReactionsView {
    #[serde(skip_serializing_if = "Option::is_none")]
    mine: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    theirs: Option<String>,
}

impl From<&ft_storage::Reactions> for ReactionsView {
    fn from(reactions: &ft_storage::Reactions) -> Self {
        Self { mine: reactions.mine.clone(), theirs: reactions.theirs.clone() }
    }
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
            quote: None,
            reactions: None,
            pinned: false,
            edited: false,
            deleted: false,
            scheduled_for: None,
            update_pending: false,
            notes: Vec::new(),
        }
    }

    pub fn noted<'a>(mut self, notes: impl Iterator<Item = &'a ft_storage::MessageNote>) -> Self {
        self.notes = notes.map(|note| NoteView { plugin: note.plugin.clone(), text: note.text.clone() }).collect();
        self
    }

    pub fn for_later(mut self, send_at: Option<i64>) -> Self {
        self.scheduled_for = send_at;
        self
    }

    pub fn pinned(mut self, pinned: bool) -> Self {
        self.pinned = pinned;
        self
    }

    pub fn marked(mut self, edited: bool, deleted: bool) -> Self {
        self.edited = edited;
        self.deleted = deleted;
        self
    }

    pub fn updating(mut self, waiting: bool) -> Self {
        self.update_pending = waiting;
        self
    }

    pub fn answering(mut self, quote: QuoteView) -> Self {
        self.quote = Some(quote);
        self
    }

    pub fn reacted(mut self, reactions: Option<&ft_storage::Reactions>) -> Self {
        self.reactions = reactions.map(ReactionsView::from);
        self
    }
}

#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub struct QuoteView {
    id: String,
    text: String,
    mine: bool,
    kind: &'static str,
}

impl QuoteView {
    pub fn of(id: &str, quoted: Option<&Message>, file: Option<&FileRecord>) -> Self {
        match (quoted, file) {
            (Some(message), Some(file)) => Self { id: id.to_owned(), text: file.name.clone(), mine: message.outgoing, kind: "file" },
            (Some(message), None) => Self { id: id.to_owned(), text: message.body.clone(), mine: message.outgoing, kind: "text" },
            (None, _) => Self { id: id.to_owned(), text: String::new(), mine: false, kind: "gone" },
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    name: String,
    size: i64,
    mime: String,
    progress: f64,
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
        } else if file.waiting {
            "waiting"
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

#[tauri::command]
pub async fn core_conversations(client: State<'_, Client>) -> Result<Vec<ConversationView>, String> {
    let online = client.online().await?;
    let conversations = online.core.store().conversations().await.map_err(failed)?;
    conversation_views(&online, conversations).await
}

pub(super) async fn conversation_views(online: &Online, conversations: Vec<Conversation>) -> Result<Vec<ConversationView>, String> {
    let connected = online.network.connected().await;
    let store = online.core.store();
    let mut views = Vec::new();
    for conversation in conversations {
        let mut view = ConversationView::new(&conversation, connected.contains(&conversation.contact.device_id));
        if let Some(last) = view.last.as_mut() {
            last.file = store.file(&last.id).await.map_err(failed)?.map(|file| FileView::from(&located(&online.core, file)));
        }
        views.push(view);
    }
    Ok(views)
}

#[tauri::command]
pub async fn core_messages(contact: String, limit: i64, client: State<'_, Client>) -> Result<Vec<MessageView>, String> {
    let core = client.core().await?;
    let files: HashMap<String, FileRecord> = core
        .store()
        .files(&contact)
        .await
        .map_err(failed)?
        .into_iter()
        .map(|file| (file.message_id.clone(), located(&core, file)))
        .collect();
    let messages = core.store().messages(&contact, limit).await.map_err(failed)?;
    let replies = core.store().replies(&contact).await.map_err(failed)?;
    let mut quoted: HashMap<String, Message> = messages.iter().map(|message| (message.message_id.clone(), message.clone())).collect();
    for quoted_id in replies.values() {
        if !quoted.contains_key(quoted_id) {
            if let Some(message) = core.store().message(quoted_id).await.map_err(failed)?.filter(|message| message.contact == contact) {
                quoted.insert(quoted_id.clone(), message);
            }
        }
    }
    let reactions = core.store().reactions(&contact).await.map_err(failed)?;
    let pinned: HashSet<String> = core.store().pinned(&contact).await.map_err(failed)?.into_iter().collect();
    let edited = core.store().edited(&contact).await.map_err(failed)?;
    let deleted = core.store().deleted(&contact).await.map_err(failed)?;
    let later = core.store().scheduled(&contact).await.map_err(failed)?;
    let changing = core.updates_waiting(&contact).await.map_err(failed)?;
    let notes = core.store().notes(&contact).await.map_err(failed)?;
    let moment = ft_core::now();
    Ok(messages
        .iter()
        .map(|message| {
            let view = MessageView::new(message, files.get(&message.message_id))
                .reacted(reactions.get(&message.message_id))
                .pinned(pinned.contains(&message.message_id))
                .marked(edited.contains(&message.message_id), deleted.contains(&message.message_id))
                .for_later(later.get(&message.message_id).copied().filter(|send_at| *send_at > moment))
                .updating(changing.contains(&message.message_id))
                .noted(notes.iter().filter(|note| note.message_id == message.message_id));
            match replies.get(&message.message_id) {
                Some(quoted_id) => view.answering(QuoteView::of(quoted_id, quoted.get(quoted_id), files.get(quoted_id))),
                None => view,
            }
        })
        .collect())
}

#[tauri::command]
pub async fn core_forget_message(message: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.forget_message(&message).await.map_err(failed)
}

#[tauri::command]
pub async fn core_schedule(contact: String, text: String, send_at: i64, reply_to: Option<String>, client: State<'_, Client>) -> Result<String, String> {
    client.core().await?.schedule_text(&contact, &text, send_at, reply_to.as_deref()).await.map_err(failed)
}

#[tauri::command]
pub async fn core_search(contact: String, query: String, client: State<'_, Client>) -> Result<Vec<MessageView>, String> {
    let core = client.core().await?;
    let found = core.search(&contact, &query).await.map_err(failed)?;
    let mut views = Vec::with_capacity(found.len());
    for message in &found {
        let file = core.store().file(&message.message_id).await.map_err(failed)?.map(|file| located(&core, file));
        views.push(MessageView::new(message, file.as_ref()));
    }
    Ok(views)
}

#[tauri::command]
pub async fn core_edit(message: String, text: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.edit_message(&message, &text).await.map_err(failed)
}

#[tauri::command]
pub async fn core_delete_everyone(message: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.delete_for_everyone(&message).await.map_err(failed)
}

#[tauri::command]
pub async fn core_pin(message: String, pinned: bool, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.pin_message(&message, pinned).await.map_err(failed)
}

#[tauri::command]
pub async fn core_react(contact: String, message: String, emoji: Option<String>, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.react(&contact, &message, emoji.as_deref()).await.map_err(failed)
}

#[tauri::command]
pub async fn core_forward(message: String, contact: String, client: State<'_, Client>) -> Result<(), String> {
    client.core().await?.forward(&message, &contact).await.map_err(failed).map(|_| ())
}

#[tauri::command]
pub async fn core_share_message(message: String, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let core = client.core().await?;
    let stored = core.store().message(&message).await.map_err(failed)?.ok_or("unknown message")?;
    match core.store().file(&message).await.map_err(failed)? {
        Some(file) if file.complete => {
            let path = core.file_path(&file);
            app.platform().share_file(&path.to_string_lossy(), &file.name, &file.mime).map_err(failed)
        }
        Some(_) => Err("that file is not here whole yet".to_owned()),
        None => app.platform().share_text(&stored.body).map_err(failed),
    }
}

#[tauri::command]
pub async fn core_send(contact: String, text: String, reply_to: Option<String>, client: State<'_, Client>) -> Result<String, String> {
    if text.trim().is_empty() {
        return Err("nothing to send".to_owned());
    }
    let core = client.core().await?;
    let sent = core.queue_text(&contact, &text, reply_to.as_deref()).await.map_err(failed)?;
    let queued = sent.clone();
    tauri::async_runtime::spawn(async move {
        core.deliver_queued(&queued).await;
    });
    Ok(sent)
}

#[tauri::command]
pub async fn core_resend(message: String, client: State<'_, Client>) -> Result<(), String> {
    let core = client.core().await?;
    tauri::async_runtime::spawn(async move {
        let _ = core.resend(&message).await;
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
pub async fn core_typing(contact: String, client: State<'_, Client>) -> Result<bool, String> {
    client.core().await?.typing(&contact).await.map_err(failed)
}

#[cfg(test)]
mod tests {
    use ft_storage::{Contact, MessageState};

    use super::*;
    use crate::client::tests::record;

    fn message(state: MessageState, outgoing: bool) -> Message {
        Message { message_id: "m1".to_owned(), contact: "ft_bob".to_owned(), outgoing, body: "hi".to_owned(), sent_at: 42, received_at: 42, state }
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
            keep_for: 0,
            burn_after_read: 0,
            session: None,
            rules: Default::default(),
            accepted: true,
            via_circle: false,
            unblocked_at: None,
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
        let waiting = ft_storage::FileRecord { waiting: true, ..record(0, false, false) };
        assert_eq!(serde_json::to_value(FileView::from(&waiting)).unwrap()["state"], "waiting", "A4: it waits for a tap");
        let empty = ft_storage::FileRecord { size: 0, ..record(0, true, false) };
        assert_eq!(serde_json::to_value(FileView::from(&empty)).unwrap()["progress"], 1.0);
    }

    #[test]
    fn a_text_has_no_file() {
        let view = serde_json::to_value(MessageView::new(&message(MessageState::Sent, true), None)).unwrap();
        assert!(view.get("file").is_none());
    }

    #[test]
    fn a_quote_is_the_text_a_files_name_or_gone() {
        let message = Message { message_id: "m1".into(), contact: "ft_bob".into(), outgoing: false, body: "dinner?".into(), sent_at: 1, received_at: 1, state: ft_storage::MessageState::Read };
        let text = serde_json::to_value(QuoteView::of("m1", Some(&message), None)).unwrap();
        assert_eq!(text, serde_json::json!({ "id": "m1", "text": "dinner?", "mine": false, "kind": "text" }));
        let gone = serde_json::to_value(QuoteView::of("m0", None, None)).unwrap();
        assert_eq!(gone, serde_json::json!({ "id": "m0", "text": "", "mine": false, "kind": "gone" }));
        let view = serde_json::to_value(MessageView::new(&message, None).answering(QuoteView::of("m0", None, None))).unwrap();
        assert_eq!(view["quote"]["kind"], "gone");
        assert!(serde_json::to_value(MessageView::new(&message, None)).unwrap().get("quote").is_none(), "no quote, no field");
    }

    #[test]
    fn a_message_for_later_carries_its_time() {
        let message = Message { message_id: "m1".into(), contact: "ft_bob".into(), outgoing: true, body: "hi".into(), sent_at: 9, received_at: 1, state: ft_storage::MessageState::Pending };
        assert_eq!(serde_json::to_value(MessageView::new(&message, None).for_later(Some(9))).unwrap()["scheduledFor"], 9);
        assert!(serde_json::to_value(MessageView::new(&message, None).for_later(None)).unwrap().get("scheduledFor").is_none());
    }

    #[test]
    fn a_message_says_whether_it_was_edited_or_taken_back() {
        let message = Message { message_id: "m1".into(), contact: "ft_bob".into(), outgoing: true, body: String::new(), sent_at: 1, received_at: 1, state: ft_storage::MessageState::Read };
        let view = serde_json::to_value(MessageView::new(&message, None).marked(true, false)).unwrap();
        assert_eq!((view["edited"].clone(), view.get("deleted").cloned()), (serde_json::json!(true), None));
        let view = serde_json::to_value(MessageView::new(&message, None).marked(false, true)).unwrap();
        assert_eq!((view.get("edited").cloned(), view["deleted"].clone()), (None, serde_json::json!(true)));
    }

    #[test]
    fn a_message_says_whether_a_change_of_it_still_waits() {
        let message = Message { message_id: "m1".into(), contact: "ft_bob".into(), outgoing: true, body: "hi".into(), sent_at: 1, received_at: 1, state: ft_storage::MessageState::Delivered };
        let view = serde_json::to_value(MessageView::new(&message, None).updating(true)).unwrap();
        assert_eq!((view["updatePending"].clone(), view["state"].clone()), (serde_json::json!(true), serde_json::json!("delivered")));
        assert!(serde_json::to_value(MessageView::new(&message, None).updating(false)).unwrap().get("updatePending").is_none());
    }

    #[test]
    fn a_pinned_message_says_so() {
        let message = Message { message_id: "m1".into(), contact: "ft_bob".into(), outgoing: false, body: "hi".into(), sent_at: 1, received_at: 1, state: ft_storage::MessageState::Read };
        assert_eq!(serde_json::to_value(MessageView::new(&message, None).pinned(true)).unwrap()["pinned"], true);
        assert!(serde_json::to_value(MessageView::new(&message, None)).unwrap().get("pinned").is_none());
    }

    #[test]
    fn a_message_carries_the_notes_tools_left_under_it() {
        let message = Message { message_id: "m1".into(), contact: "ft_bob".into(), outgoing: false, body: "see you at six".into(), sent_at: 1, received_at: 1, state: ft_storage::MessageState::Read };
        let notes = [ft_storage::MessageNote { message_id: "m1".into(), plugin: "com.flickertalk.translate".into(), text: "nos vemos a las seis".into() }];
        let view = serde_json::to_value(MessageView::new(&message, None).noted(notes.iter())).unwrap();
        assert_eq!(view["notes"], serde_json::json!([{ "plugin": "com.flickertalk.translate", "text": "nos vemos a las seis" }]));
        assert!(serde_json::to_value(MessageView::new(&message, None).noted([].iter())).unwrap().get("notes").is_none());
    }

    #[test]
    fn a_message_carries_the_reactions_of_each_side() {
        let message = Message { message_id: "m1".into(), contact: "ft_bob".into(), outgoing: false, body: "hi".into(), sent_at: 1, received_at: 1, state: ft_storage::MessageState::Read };
        let both = ft_storage::Reactions { mine: Some("👍".into()), theirs: Some("❤️".into()) };
        let view = serde_json::to_value(MessageView::new(&message, None).reacted(Some(&both))).unwrap();
        assert_eq!(view["reactions"], serde_json::json!({ "mine": "👍", "theirs": "❤️" }));
        let theirs = ft_storage::Reactions { mine: None, theirs: Some("❤️".into()) };
        let view = serde_json::to_value(MessageView::new(&message, None).reacted(Some(&theirs))).unwrap();
        assert_eq!(view["reactions"], serde_json::json!({ "theirs": "❤️" }));
        assert!(serde_json::to_value(MessageView::new(&message, None).reacted(None)).unwrap().get("reactions").is_none());
    }
}
