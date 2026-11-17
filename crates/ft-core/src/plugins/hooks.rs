use anyhow::{ensure, Context, Result};
use ft_plugins::HookName;

use super::translate::is_language_tag;
use super::{ChatHook, TRANSLATE_MAX_CHARS};
use crate::{Core, Event};

pub const NOTE_MAX_CHARS: usize = TRANSLATE_MAX_CHARS;

const SAMPLE_TEXTS: usize = 5;
const SAMPLE_WINDOW: i64 = 50;

impl Core {
    pub async fn set_message_note(&self, message_id: &str, id: &str, text: &str) -> Result<()> {
        let manifest = self.plugin_manifest(id)?;
        let bound = manifest.hooks.contains_key(&HookName::Message) || manifest.hooks.contains_key(&HookName::Composer);
        ensure!(bound, "{id} does not bind to messages");
        ensure!(self.granted_to(id).await?.reads_given_messages, "{id} may not read messages");
        self.usable(manifest.kind).await?;
        let text = text.trim();
        ensure!(!text.is_empty(), "nothing to note");
        ensure!(text.chars().count() <= NOTE_MAX_CHARS, "a note is as long as a message at most");
        let changed = self.noted_message(message_id).await?;
        self.store.set_message_note(message_id, id, text, crate::now()).await?;
        let _ = self.events.send(changed);
        Ok(())
    }

    pub async fn clear_message_note(&self, message_id: &str, id: &str) -> Result<()> {
        let changed = self.noted_message(message_id).await?;
        self.store.clear_message_note(message_id, id).await?;
        let _ = self.events.send(changed);
        Ok(())
    }

    async fn noted_message(&self, message_id: &str) -> Result<Event> {
        if let Some(message) = self.store.message(message_id).await? {
            ensure!(self.store.file(message_id).await?.is_none(), "a file has no words to note");
            ensure!(!self.store.is_deleted(message_id).await?, "that message was taken back");
            return Ok(Event::MessagesChanged { contact: message.contact });
        }
        let said = self.store.circle_message(message_id).await?.context("that message is not here")?;
        ensure!(said.kind == "text", "only a text has words to note");
        Ok(Event::CircleMessagesChanged { circle: said.circle })
    }

    pub async fn set_chat_hooks(&self, conversation: &str, id: &str, auto_message: bool, auto_composer: bool, lang: Option<&str>) -> Result<()> {
        let changed = if self.store.circle(conversation).await?.is_some() {
            Event::CirclesChanged
        } else {
            self.contact(conversation).await.context("no such conversation")?;
            Event::ContactsChanged
        };
        let manifest = self.plugin_manifest(id)?;
        ensure!(manifest.hooks.contains_key(&HookName::Settings), "{id} has no settings for a conversation");
        ensure!(!auto_message || manifest.hooks.contains_key(&HookName::Message), "{id} does not bind to messages");
        ensure!(!auto_composer || manifest.hooks.contains_key(&HookName::Composer), "{id} does not bind to the composer");
        ensure!(lang.is_none_or(is_language_tag), "that is not a language tag");
        if auto_message || auto_composer {
            self.usable(manifest.kind).await?;
        }
        self.store.set_chat_hooks(conversation, id, auto_message, auto_composer, lang).await?;
        let _ = self.events.send(changed);
        Ok(())
    }

    pub async fn chat_hooks(&self, conversation: &str) -> Result<Vec<ChatHook>> {
        self.store.chat_hooks(conversation).await
    }

    pub async fn hook_sample(&self, id: &str, conversation: &str) -> Result<String> {
        ensure!(self.plugin_manifest(id)?.hooks.contains_key(&HookName::Settings), "{id} has no settings for a conversation");
        let received: Vec<String> = if self.store.circle(conversation).await?.is_some() {
            let said = self.store.circle_messages(conversation, SAMPLE_WINDOW).await?;
            said.into_iter().filter(|one| !one.outgoing && one.kind == "text").map(|one| one.body).collect()
        } else {
            self.contact(conversation).await.context("no such conversation")?;
            let deleted = self.store.deleted(conversation).await?;
            let files: std::collections::HashSet<String> = self.store.files(conversation).await?.into_iter().map(|file| file.message_id).collect();
            let said = self.store.messages(conversation, SAMPLE_WINDOW).await?;
            said.into_iter()
                .filter(|one| !one.outgoing && !deleted.contains(&one.message_id) && !files.contains(&one.message_id))
                .map(|one| one.body)
                .collect()
        };
        if !self.granted_to(id).await?.reads_given_messages {
            return Ok(String::new());
        }
        let texts: Vec<String> = received.into_iter().filter(|text| !text.trim().is_empty()).collect();
        Ok(texts[texts.len().saturating_sub(SAMPLE_TEXTS)..].join("\n"))
    }
}
