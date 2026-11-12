//! Nothing is installed unless the catalogue signed it, and installing grants nothing.

use std::path::{Path, PathBuf};

use anyhow::{bail, ensure, Context, Result};
use async_trait::async_trait;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::Serialize;
use ft_plugins::{installed, CatalogueEntry, HookName, Manifest, Permissions, Plugin, Sending};
use vodozemac::Ed25519PublicKey;

use ft_protocol::{Body, Packet};
use ft_storage::{Contact, PluginRef, Reminder};
pub use ft_storage::ChatHook;

use crate::native_calls::{BOARD, VIEWER};
use crate::web::{host_of, Fetch, WebAnswer, WebRequest};
use crate::{Core, Event};

pub const CATALOGUE_HOME: &str = "https://flickertalk.com/plugins";

const PACKAGE_LIMIT: u64 = 8 * 1024 * 1024;
const INDEX_LIMIT: u64 = 512 * 1024;

const FETCH_LIMIT: u64 = 8 * 1024 * 1024;

const MEMORY_KEY: usize = 64;
const MEMORY_VALUE: usize = 64 * 1024;
const MEMORY_KEYS: usize = 64;

const RECORD_KEY: usize = 128;
pub const RECORD_VALUE: usize = 16 * 1024 * 1024;
pub const LIVE_LIMIT: usize = 48 * 1024;
pub const CORE_VERSION: &str = "1.7.2";
const REMINDER_TEXT: usize = 200;
// Changing this changes every chat id plugins have stored
const PLUGIN_CHAT_CONTEXT: &str = "flickertalk 2026-10-02 plugin chat id";

// degrees (WGS 84), accuracy in metres, `at` in ms
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Fix {
    pub lat: f64,
    pub lon: f64,
    pub accuracy: f64,
    pub at: i64,
}

#[async_trait]
pub trait Locator: Send + Sync {
    async fn locate(&self) -> Result<Option<Fix>>;
}

const LOCATION_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

impl Fix {
    fn is_a_place(&self) -> bool {
        self.lat.is_finite()
            && self.lon.is_finite()
            && (-90.0..=90.0).contains(&self.lat)
            && (-180.0..=180.0).contains(&self.lon)
            && self.accuracy.is_finite()
            && self.accuracy >= 0.0
    }
}

async fn located(locator: &dyn Locator) -> Option<Fix> {
    match tokio::time::timeout(LOCATION_WAIT, locator.locate()).await {
        Ok(Ok(fix)) => fix.filter(Fix::is_a_place),
        _ => None,
    }
}

// per pair because that's all iOS reports
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PairStatus {
    Installed,
    Downloadable,
    Unsupported,
}

// Text never leaves the phone. Language packs download only when the user asks.
#[async_trait]
pub trait Translator: Send + Sync {
    async fn languages(&self) -> Result<Vec<String>>;
    async fn detect(&self, text: &str) -> Result<Option<String>>;
    async fn status(&self, from: &str, to: &str) -> Result<PairStatus>;
    async fn prepare(&self, from: &str, to: &str) -> Result<bool>;
    async fn remove(&self, from: &str, to: &str) -> Result<bool>;
    async fn translate(&self, text: &str, from: &str, to: &str) -> Result<Option<String>>;
}

pub const TRANSLATE_MAX_CHARS: usize = 5_000;

const TRANSLATE_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

// packs are ~30 MB
const PREPARE_WAIT: std::time::Duration = std::time::Duration::from_secs(5 * 60);

const LANGUAGE_TAG_MAX: usize = 35;

// Anything else never reaches native code
fn is_language_tag(tag: &str) -> bool {
    let mut parts = tag.split('-');
    let language = parts.next().unwrap_or_default();
    tag.len() <= LANGUAGE_TAG_MAX
        && (2..=3).contains(&language.len())
        && language.chars().all(|c| c.is_ascii_alphabetic())
        && parts.all(|part| (2..=8).contains(&part.len()) && part.chars().all(|c| c.is_ascii_alphanumeric()))
}

fn a_text_to_translate(text: &str) -> Result<()> {
    ensure!(!text.trim().is_empty(), "there is nothing to translate");
    ensure!(text.chars().count() <= TRANSLATE_MAX_CHARS, "a text to translate is {TRANSLATE_MAX_CHARS} characters at most");
    Ok(())
}

async fn within<T>(wait: std::time::Duration, asked: impl std::future::Future<Output = Result<T>>) -> Result<T> {
    tokio::time::timeout(wait, asked).await.map_err(|_| anyhow::anyhow!("the phone's translator did not answer in time"))?
}

async fn languages_of(translator: &dyn Translator) -> Result<Vec<String>> {
    let mut languages: Vec<String> = Vec::new();
    for tag in within(TRANSLATE_WAIT, translator.languages()).await? {
        if is_language_tag(&tag) && !languages.contains(&tag) {
            languages.push(tag);
        }
    }
    Ok(languages)
}

// BCP-47 "und" = undetermined
async fn detected(translator: &dyn Translator, text: &str) -> Result<Option<String>> {
    a_text_to_translate(text)?;
    let guess = within(TRANSLATE_WAIT, translator.detect(text)).await?;
    Ok(guess.filter(|tag| is_language_tag(tag) && !tag.eq_ignore_ascii_case("und")))
}

async fn status_of(translator: &dyn Translator, from: &str, to: &str) -> Result<PairStatus> {
    if !is_language_tag(from) || !is_language_tag(to) {
        return Ok(PairStatus::Unsupported);
    }
    within(TRANSLATE_WAIT, translator.status(from, to)).await
}

async fn prepared(translator: &dyn Translator, from: &str, to: &str) -> Result<bool> {
    if !is_language_tag(from) || !is_language_tag(to) {
        return Ok(false);
    }
    within(PREPARE_WAIT, translator.prepare(from, to)).await
}

async fn removed(translator: &dyn Translator, from: &str, to: &str) -> Result<bool> {
    if !is_language_tag(from) || !is_language_tag(to) {
        return Ok(false);
    }
    within(TRANSLATE_WAIT, translator.remove(from, to)).await
}

async fn translated(translator: &dyn Translator, text: &str, from: &str, to: &str) -> Result<Option<String>> {
    a_text_to_translate(text)?;
    if !is_language_tag(from) || !is_language_tag(to) {
        return Ok(None);
    }
    within(TRANSLATE_WAIT, translator.translate(text, from, to)).await
}

pub const NOTE_MAX_CHARS: usize = TRANSLATE_MAX_CHARS;

const SAMPLE_TEXTS: usize = 5;
const SAMPLE_WINDOW: i64 = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPlugin {
    pub manifest: Manifest,
    pub granted: Permissions,
    pub installed_at: i64,
    pub image: String,
}

impl Core {
    // Finish an update swap that a killed app left halfway
    pub fn set_plugins_dir(&self, dir: PathBuf) {
        let _ = ft_plugins::recover(&dir);
        let _ = self.plugins_dir.set(dir);
    }

    fn plugins_home(&self) -> Result<&PathBuf> {
        self.plugins_dir.get().context("the plugins folder is not set")
    }

    // The signature is checked before anything is written
    pub async fn install_plugin(&self, package: &[u8], catalogue: &Ed25519PublicKey, granted: Permissions) -> Result<Manifest> {
        let plugin = ft_plugins::open(package, catalogue)?;
        ensure!(
            ft_plugins::version_at_least(CORE_VERSION, &plugin.manifest.min_core_version),
            "{} needs FlickerTalk {} or newer",
            plugin.manifest.id,
            plugin.manifest.min_core_version
        );
        // Keep earlier grants, but only those this version still asks for, never more
        let before = self.store.plugin(&plugin.manifest.id).await?;
        if before.is_none() {
            self.usable(plugin.manifest.kind).await?;
        }
        let had: Option<Permissions> = before.map(|installed| serde_json::from_str(&installed.granted).unwrap_or_default());
        let keep = match &had {
            Some(had) => narrowed(had, &plugin.manifest.permissions),
            None => {
                allowed(&plugin.manifest.permissions, &granted)?;
                granted
            }
        };
        let home = self.plugins_home()?;
        ft_plugins::install(&plugin, home)?;
        self.store.install_plugin(&plugin.manifest.id, &plugin.manifest.version, &serde_json::to_string(&keep)?).await?;
        if had.is_some() {
            self.store.grant_plugin(&plugin.manifest.id, &serde_json::to_string(&keep)?).await?;
        }
        let _ = self.events.send(Event::PluginsChanged);
        if had.as_ref().is_some_and(|had| had.remind) && !keep.remind && self.store.forget_reminders(&plugin.manifest.id).await? {
            let _ = self.events.send(Event::RemindersChanged);
        }
        Ok(plugin.manifest)
    }

    // The plugin frame has an opaque origin, so the browser gives it no storage
    pub async fn plugin_remembers(&self, id: &str, session: Option<&str>, key: &str) -> Result<Option<String>> {
        self.store.plugin_value(id, self.place(session)?, key).await
    }

    pub async fn plugin_remember(&self, id: &str, session: Option<&str>, key: &str, value: &str) -> Result<()> {
        let session = self.place(session)?;
        ensure!(!key.is_empty() && key.len() <= MEMORY_KEY, "that key is too long");
        ensure!(value.len() <= MEMORY_VALUE, "a plugin may not keep that much");
        ensure!(self.store.plugin(id).await?.is_some(), "{id} is not installed here");
        let keys = self.store.plugin_keys(id, session).await?;
        ensure!(
            keys.len() < MEMORY_KEYS || keys.iter().any(|known| known == key),
            "a plugin may not keep that many things"
        );
        self.store.set_plugin_value(id, session, key, value).await
    }

    pub async fn plugin_forget(&self, id: &str, session: Option<&str>, key: &str) -> Result<()> {
        self.store.forget_plugin_value(id, self.place(session)?, key).await
    }

    pub async fn plugin_memory_keys(&self, id: &str, session: Option<&str>) -> Result<Vec<String>> {
        self.store.plugin_keys(id, self.place(session)?).await
    }

    // A closed session's data goes to nobody, even a caller that names it
    fn place<'a>(&self, session: Option<&'a str>) -> Result<Option<&'a str>> {
        if let Some(session) = session {
            ensure!(self.is_session_open(session), "that session is not open");
        }
        Ok(session)
    }

    pub async fn plugin_record(&self, id: &str, session: Option<&str>, key: &str) -> Result<Option<Vec<u8>>> {
        self.store.plugin_record(id, self.place(session)?, key).await
    }

    pub async fn plugin_record_set(&self, id: &str, session: Option<&str>, key: &str, value: &[u8]) -> Result<()> {
        let session = self.place(session)?;
        ensure!(!key.is_empty() && key.len() <= RECORD_KEY, "that key is too long");
        ensure!(value.len() <= RECORD_VALUE, "a record may not hold that much");
        let quota = self.granted_to(id).await?.storage.quota();
        let held = self.store.plugin_record(id, session, key).await?.map_or(0, |old| old.len() as i64);
        let used = self.store.plugin_records_size(id, session).await? - held;
        ensure!(used + value.len() as i64 <= quota as i64, "the plugin has no room left for that");
        self.store.set_plugin_record(id, session, key, value).await
    }

    pub async fn plugin_record_forget(&self, id: &str, session: Option<&str>, key: &str) -> Result<()> {
        self.store.forget_plugin_record(id, self.place(session)?, key).await
    }

    pub async fn plugin_record_keys(&self, id: &str, session: Option<&str>, prefix: &str) -> Result<Vec<String>> {
        self.store.plugin_record_keys(id, self.place(session)?, prefix).await
    }

    pub async fn plugin_records_usage(&self, id: &str, session: Option<&str>) -> Result<(u64, u64)> {
        let session = self.place(session)?;
        let quota = self.granted_to(id).await?.storage.quota();
        Ok((self.store.plugin_records_size(id, session).await? as u64, quota))
    }

    // Opaque, tells the plugin nothing about the contact
    pub async fn plugin_ref(&self, id: &str, message_id: &str) -> Result<String> {
        if let Some(reference) = self.store.plugin_ref_for(id, message_id).await? {
            return Ok(reference);
        }
        let conversation = match self.store.message(message_id).await? {
            Some(message) => message.contact,
            None => self.store.circle_message(message_id).await?.context("that message is not here")?.circle,
        };
        let reference = format!("ref_{}", ft_protocol::MessageId::new());
        self.store.add_plugin_ref(&reference, id, &conversation, message_id).await?;
        Ok(reference)
    }

    pub async fn plugin_ref_target(&self, id: &str, session: Option<&str>, reference: &str) -> Result<Option<PluginRef>> {
        let session = self.place(session)?;
        let Some(target) = self.store.plugin_ref(reference).await? else { return Ok(None) };
        if target.plugin != id {
            return Ok(None);
        }
        if let Some(circle) = self.store.circle(&target.contact).await? {
            let here = circle.session.as_deref() == session && self.store.circle_message(&target.message_id).await?.is_some();
            return Ok(here.then_some(target));
        }
        let contact = self.store.contact(&target.contact).await?;
        if contact.is_none_or(|contact| contact.blocked || contact.session.as_deref() != session) {
            return Ok(None);
        }
        if self.store.message(&target.message_id).await?.is_none() {
            return Ok(None);
        }
        Ok(Some(target))
    }

    // A reminder set in a hidden session only rings while that session is open
    pub async fn set_reminder(&self, id: &str, session: Option<&str>, reminder: &str, at: i64, text: &str) -> Result<()> {
        let session = self.place(session)?.map(str::to_owned);
        ensure!(self.granted_to(id).await?.remind, "{id} may not set reminders");
        ensure!(!reminder.is_empty() && reminder.len() <= RECORD_KEY, "that reminder id is too long");
        ensure!(at > 0, "a reminder needs a time");
        let text: String = text.chars().take(REMINDER_TEXT).collect();
        self.store.set_reminder(&Reminder { plugin: id.to_owned(), session, id: reminder.to_owned(), at, text }).await?;
        let _ = self.events.send(Event::RemindersChanged);
        Ok(())
    }

    pub async fn cancel_reminder(&self, id: &str, session: Option<&str>, reminder: &str) -> Result<bool> {
        let gone = self.store.cancel_reminder(id, self.place(session)?, reminder).await?;
        if gone {
            let _ = self.events.send(Event::RemindersChanged);
        }
        Ok(gone)
    }

    pub async fn plugin_reminders(&self, id: &str, session: Option<&str>) -> Result<Vec<Reminder>> {
        let session = self.place(session)?;
        Ok(self.store.reminders(Some(id)).await?.into_iter().filter(|reminder| reminder.session.as_deref() == session).collect())
    }

    pub async fn ringing_reminder(&self, id: &str, reminder: &str) -> Result<Option<Reminder>> {
        Ok(self.reminders().await?.into_iter().find(|ringing| ringing.plugin == id && ringing.id == reminder))
    }

    pub async fn reminders(&self) -> Result<Vec<Reminder>> {
        Ok(self
            .store
            .reminders(None)
            .await?
            .into_iter()
            .filter(|reminder| reminder.session.as_deref().is_none_or(|session| self.is_session_open(session)))
            .collect())
    }

    pub async fn due_reminders(&self, at: i64) -> Result<Vec<Reminder>> {
        Ok(self.reminders().await?.into_iter().filter(|reminder| reminder.at <= at).collect())
    }

    // An unreachable conversation must fail exactly like one that doesn't exist
    pub async fn plugin_chat(&self, id: &str, contact: &str) -> Result<String> {
        ensure!(self.store.plugin(id).await?.is_some(), "{id} is not installed here");
        let open = |session: Option<&str>| session.is_none_or(|session| self.is_session_open(session));
        let reachable = match self.store.circle(contact).await? {
            Some(circle) => open(circle.session.as_deref()),
            None => self.store.contact(contact).await?.is_some_and(|contact| contact.accepted && !contact.blocked && open(contact.session.as_deref())),
        };
        ensure!(reachable, "that is not a contact of yours");
        let mut hasher = blake3::Hasher::new_keyed(&blake3::derive_key(PLUGIN_CHAT_CONTEXT, &self.key));
        // ids can't contain NUL, so this keeps them apart
        hasher.update(id.as_bytes()).update(&[0]).update(contact.as_bytes());
        Ok(URL_SAFE_NO_PAD.encode(hasher.finalize().as_bytes()))
    }

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

    // Direct connection only, never the mailbox
    pub async fn plugin_live_send(&self, id: &str, contact: &str, data: Vec<u8>) -> Result<bool> {
        ensure!(self.granted_to(id).await?.live, "{id} may not talk to the other side");
        ensure!(data.len() <= LIVE_LIMIT, "a plugin may not say that much at once");
        let contact = self.contact(contact).await?;
        ensure!(contact.accepted && !contact.blocked, "that is not a contact of yours");
        self.transmit_direct(&contact, &Packet::new(Body::PluginEvent { plugin: id.to_owned(), data })).await
    }

    pub(crate) async fn plugin_event_received(&self, contact: &Contact, plugin: String, data: Vec<u8>) -> Result<()> {
        if !contact.accepted || data.len() > LIVE_LIMIT {
            return Ok(());
        }
        let granted = match self.granted_to(&plugin).await {
            Ok(granted) => granted.live,
            Err(_) => false,
        };
        if granted {
            let _ = self.events.send(Event::PluginEvent { plugin, contact: contact.device_id.clone(), data });
        }
        Ok(())
    }

    // Pass the fix through, never keep or log it
    pub async fn plugin_location(&self, id: &str, locator: &dyn Locator) -> Result<Option<Fix>> {
        ensure!(self.granted_to(id).await?.location, "{id} may not ask where the phone is");
        Ok(located(locator).await)
    }

    // Pass the text through, never keep or log it

    async fn may_translate(&self, id: &str) -> Result<()> {
        ensure!(self.granted_to(id).await?.translate, "{id} may not translate");
        Ok(())
    }

    pub async fn plugin_translate_languages(&self, id: &str, translator: &dyn Translator) -> Result<Vec<String>> {
        self.may_translate(id).await?;
        languages_of(translator).await
    }

    pub async fn plugin_translate_detect(&self, id: &str, translator: &dyn Translator, text: &str) -> Result<Option<String>> {
        self.may_translate(id).await?;
        detected(translator, text).await
    }

    pub async fn plugin_translate_status(&self, id: &str, translator: &dyn Translator, from: &str, to: &str) -> Result<PairStatus> {
        self.may_translate(id).await?;
        status_of(translator, from, to).await
    }

    pub async fn plugin_translate_prepare(&self, id: &str, translator: &dyn Translator, from: &str, to: &str) -> Result<bool> {
        self.may_translate(id).await?;
        prepared(translator, from, to).await
    }

    pub async fn plugin_translate_remove(&self, id: &str, translator: &dyn Translator, from: &str, to: &str) -> Result<bool> {
        self.may_translate(id).await?;
        removed(translator, from, to).await
    }

    pub async fn plugin_translate_text(&self, id: &str, translator: &dyn Translator, text: &str, from: &str, to: &str) -> Result<Option<String>> {
        self.may_translate(id).await?;
        translated(translator, text, from, to).await
    }

    pub async fn plugins_opening(&self, mime: &str) -> Result<Vec<Manifest>> {
        Ok(self.plugins().await?.into_iter().map(|plugin| plugin.manifest).filter(|manifest| manifest.opens_kind(mime)).collect())
    }

    // Nothing is read until the index signature checks out
    pub async fn catalogue(&self, fetch: &dyn Fetch, catalogue: &Ed25519PublicKey) -> Result<Vec<CatalogueEntry>> {
        // index.json is a cut-down copy for app 1.0.0
        let index = fetch.get(&format!("{CATALOGUE_HOME}/{}", ft_plugins::INDEX), INDEX_LIMIT).await?;
        let signature = fetch.get(&format!("{CATALOGUE_HOME}/{}.sig", ft_plugins::INDEX), 1024).await?;
        let index = String::from_utf8(index).context("the index is not text")?;
        let signature = String::from_utf8(signature).context("the signature is not text")?;
        Ok(ft_plugins::catalogue_entries(&index, signature.trim(), catalogue)?
            .into_iter()
            .filter(|entry| entry.runs_on(CORE_VERSION) && entry.kind != ft_plugins::Kind::Unknown)
            .collect())
    }

    // Never open a package that isn't byte for byte what the index listed
    pub async fn add_plugin(
        &self,
        entry: &CatalogueEntry,
        fetch: &dyn Fetch,
        catalogue: &Ed25519PublicKey,
    ) -> Result<Manifest> {
        ensure!(entry.url.starts_with(&format!("{CATALOGUE_HOME}/")), "that plugin is not in our catalogue");
        ensure!(entry.size <= PACKAGE_LIMIT, "that plugin is too big");
        if self.store.plugin(&entry.id).await?.is_none() {
            self.usable(entry.kind).await?;
        }
        let package = fetch.get(&entry.url, PACKAGE_LIMIT).await?;
        ft_plugins::download(entry, &package, catalogue)?;
        self.install_plugin(&package, catalogue, Permissions::default()).await
    }

    pub async fn update_plugins(
        &self,
        listed: &[CatalogueEntry],
        fetch: &dyn Fetch,
        catalogue: &Ed25519PublicKey,
        open: &std::collections::HashSet<String>,
    ) -> Vec<String> {
        let mut updated = Vec::new();
        for plugin in self.plugins().await.unwrap_or_default() {
            let here = &plugin.manifest;
            let Some(entry) = listed.iter().find(|entry| entry.id == here.id) else { continue };
            let higher = !ft_plugins::version_at_least(&here.version, &entry.version);
            if !higher
                || open.contains(&here.id)
                || !entry.runs_on(CORE_VERSION)
                || entry.kind != here.kind
                || entry.size > PACKAGE_LIMIT
            {
                continue;
            }
            if self.add_plugin(entry, fetch, catalogue).await.is_ok() {
                updated.push(here.id.clone());
            }
        }
        updated
    }

    // This host check is the real one, the frame's CSP is only a second lock. No identity, key or cookie goes along.
    pub async fn plugin_fetch(&self, id: &str, request: WebRequest, fetch: &dyn Fetch) -> Result<WebAnswer> {
        let host = host_of(&request.url)?;
        let granted = self.granted_to(id).await?;
        ensure!(granted.network.iter().any(|allowed| allowed.eq_ignore_ascii_case(&host)), "{id} may not reach {host}");
        fetch.call(&request, FETCH_LIMIT).await
    }

    pub async fn plugin_sending(&self, id: &str) -> Result<Sending> {
        Ok(self.granted_to(id).await?.send)
    }

    pub async fn plugin_may_propose(&self, id: &str) -> Result<bool> {
        Ok(matches!(self.plugin_sending(id).await?, Sending::Propose | Sending::Auto))
    }

    // only with `auto`, which is never the default
    pub async fn plugin_send_file(&self, id: &str, contact: &str, path: &Path, name: &str, mime: &str) -> Result<String> {
        ensure!(self.plugin_sending(id).await? == Sending::Auto, "{id} may not send by itself");
        self.send_file(contact, path, name, mime).await
    }

    pub(crate) async fn granted_to(&self, id: &str) -> Result<Permissions> {
        let row = self.store.plugin(id).await?.with_context(|| format!("{id} is not installed here"))?;
        Ok(serde_json::from_str(&row.granted).unwrap_or_default())
    }

    pub async fn may_use(&self, kind: ft_plugins::Kind) -> Result<bool> {
        Ok(kind == ft_plugins::Kind::Game || self.access().await?.may(ft_billing::Doing::UseTool))
    }

    pub async fn may_use_plugin(&self, id: &str, kind: ft_plugins::Kind) -> Result<bool> {
        if self.may_use(kind).await? {
            return Ok(true);
        }
        if ![BOARD, VIEWER].contains(&id) {
            return Ok(false);
        }
        Ok(self.following(id).await? && self.plugin_manifest(id).is_ok_and(|manifest| manifest.permissions.live))
    }

    pub(crate) async fn usable(&self, kind: ft_plugins::Kind) -> Result<()> {
        ensure!(self.may_use(kind).await?, crate::NEEDS_SUBSCRIPTION);
        Ok(())
    }

    pub async fn open_plugin(&self, id: &str) -> Result<()> {
        let kind = self.plugin_manifest(id)?.kind;
        ensure!(self.may_use_plugin(id, kind).await?, crate::NEEDS_SUBSCRIPTION);
        Ok(())
    }

    pub async fn grant_plugin(&self, id: &str, granted: Permissions) -> Result<()> {
        let manifest = self.plugin_manifest(id)?;
        allowed(&manifest.permissions, &granted)?;
        self.store.grant_plugin(id, &serde_json::to_string(&granted)?).await?;
        let _ = self.events.send(Event::PluginsChanged);
        if !granted.remind && self.store.forget_reminders(id).await? {
            let _ = self.events.send(Event::RemindersChanged);
        }
        Ok(())
    }

    pub async fn plugins(&self) -> Result<Vec<InstalledPlugin>> {
        let home = self.plugins_home()?;
        let manifests = installed(home)?;
        let mut plugins = Vec::new();
        for row in self.store.plugins().await? {
            let Some(manifest) = manifests.iter().find(|manifest| manifest.id == row.id) else { continue };
            plugins.push(InstalledPlugin {
                manifest: manifest.clone(),
                granted: serde_json::from_str(&row.granted).unwrap_or_default(),
                installed_at: row.installed_at,
                image: ft_plugins::image_of(home, &row.id),
            });
        }
        Ok(plugins)
    }

    pub async fn remove_plugin(&self, id: &str) -> Result<()> {
        ft_plugins::remove(id, self.plugins_home()?)?;
        self.store.remove_plugin(id).await?;
        let _ = self.events.send(Event::PluginsChanged);
        let _ = self.events.send(Event::RemindersChanged);
        Ok(())
    }

    pub(crate) fn plugin_manifest(&self, id: &str) -> Result<Manifest> {
        installed(self.plugins_home()?)?
            .into_iter()
            .find(|manifest| manifest.id == id)
            .with_context(|| format!("{id} is not installed"))
    }
}

// Refuse grants the plugin never asked for
fn allowed(asked: &Permissions, granted: &Permissions) -> Result<()> {
    for host in &granted.network {
        ensure!(asked.network.contains(host), "the plugin never asked to talk to {host}");
    }
    ensure!(
        !granted.reads_given_messages || asked.reads_given_messages,
        "the plugin never asked to read what you hand it"
    );
    if reach(granted.send) > reach(asked.send) {
        bail!("the plugin never asked to write in the chat that way");
    }
    ensure!(!granted.print || asked.print, "the plugin never asked to print");
    ensure!(!granted.live || asked.live, "the plugin never asked to talk to the other side");
    ensure!(!granted.remind || asked.remind, "the plugin never asked to set reminders");
    ensure!(!granted.drive || asked.drive, "the plugin never asked for your cloud");
    ensure!(!granted.location || asked.location, "the plugin never asked where the phone is");
    ensure!(!granted.translate || asked.translate, "the plugin never asked to translate");
    ensure!(
        granted.storage == ft_plugins::Storage::Small || asked.storage == ft_plugins::Storage::Large,
        "the plugin never asked for that much room"
    );
    Ok(())
}

fn narrowed(granted: &Permissions, asked: &Permissions) -> Permissions {
    Permissions {
        network: granted.network.iter().filter(|host| asked.network.contains(host)).cloned().collect(),
        reads_given_messages: granted.reads_given_messages && asked.reads_given_messages,
        send: if reach(granted.send) <= reach(asked.send) { granted.send } else { asked.send },
        print: granted.print && asked.print,
        live: granted.live && asked.live,
        remind: granted.remind && asked.remind,
        drive: granted.drive && asked.drive,
        storage: if asked.storage == ft_plugins::Storage::Large { granted.storage } else { ft_plugins::Storage::Small },
        location: granted.location && asked.location,
        translate: granted.translate && asked.translate,
    }
}

fn reach(sending: Sending) -> u8 {
    match sending {
        Sending::Nothing => 0,
        Sending::Propose => 1,
        Sending::Auto => 2,
    }
}

pub fn files_of(plugin: &Plugin) -> Vec<String> {
    plugin.files().map(|(path, _)| path.to_owned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Silent;

    #[async_trait]
    impl Locator for Silent {
        async fn locate(&self) -> Result<Option<Fix>> {
            std::future::pending().await
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_phone_that_never_answers_is_nothing_after_a_while() {
        let started = tokio::time::Instant::now();
        assert_eq!(located(&Silent).await, None);
        let waited = started.elapsed();
        assert!(waited >= std::time::Duration::from_secs(15) && waited <= std::time::Duration::from_secs(30), "waited {waited:?}");
    }

    struct Mute;

    #[async_trait]
    impl Translator for Mute {
        async fn languages(&self) -> Result<Vec<String>> {
            std::future::pending().await
        }
        async fn detect(&self, _text: &str) -> Result<Option<String>> {
            std::future::pending().await
        }
        async fn status(&self, _from: &str, _to: &str) -> Result<PairStatus> {
            std::future::pending().await
        }
        async fn prepare(&self, _from: &str, _to: &str) -> Result<bool> {
            std::future::pending().await
        }
        async fn remove(&self, _from: &str, _to: &str) -> Result<bool> {
            std::future::pending().await
        }
        async fn translate(&self, _text: &str, _from: &str, _to: &str) -> Result<Option<String>> {
            std::future::pending().await
        }
    }

    async fn gave_up<T: std::fmt::Debug>(asked: impl std::future::Future<Output = Result<T>>) -> std::time::Duration {
        let started = tokio::time::Instant::now();
        let answer = asked.await;
        assert!(answer.is_err(), "a translator that never answers is an error, not {answer:?}");
        started.elapsed()
    }

    #[tokio::test(start_paused = true)]
    async fn a_translator_that_never_answers_is_an_error_after_a_while() {
        let half_a_minute = std::time::Duration::from_secs(30);
        for waited in [
            gave_up(translated(&Mute, "hola", "es", "en")).await,
            gave_up(detected(&Mute, "hola")).await,
            gave_up(languages_of(&Mute)).await,
            gave_up(status_of(&Mute, "es", "en")).await,
            gave_up(removed(&Mute, "es", "en")).await,
        ] {
            assert_eq!(waited, half_a_minute);
        }
        assert_eq!(gave_up(prepared(&Mute, "es", "en")).await, std::time::Duration::from_secs(5 * 60), "a download may take a while");
    }

    #[test]
    fn a_language_tag_is_what_bcp_47_looks_like() {
        for tag in ["es", "pt", "fil", "zh-Hans", "es-419", "sr-Latn-RS", "EN-us", "xx"] {
            assert!(is_language_tag(tag), "{tag} is a tag");
        }
        let long = format!("en-{}", ["abcdefgh"; 5].join("-"));
        for tag in ["", "e", "espa", "español", "es_ES", "es-", "-es", "es--419", "es-x", "es-123456789", "../es", "es en", "e1", long.as_str()] {
            assert!(!is_language_tag(tag), "{tag:?} is not a tag");
        }
    }

    #[test]
    fn a_grant_may_never_go_beyond_what_was_asked() {
        let asked = Permissions {
            network: vec!["api.openai.com".to_owned()],
            reads_given_messages: true,
            send: Sending::Propose,
            ..Permissions::default()
        };
        assert!(allowed(&asked, &Permissions::default()).is_ok(), "granting nothing is always fine");
        assert!(allowed(&asked, &asked).is_ok());
        assert!(allowed(&Permissions::default(), &asked).is_err(), "it asked for nothing");
        assert!(allowed(&asked, &Permissions { send: Sending::Auto, ..asked.clone() }).is_err());
        let elsewhere = Permissions { network: vec!["evil.example".to_owned()], ..Permissions::default() };
        assert!(allowed(&asked, &elsewhere).is_err());
        let printing = Permissions { print: true, ..asked.clone() };
        assert!(allowed(&asked, &printing).is_err(), "it never asked to print");
        assert!(allowed(&printing, &printing).is_ok());
        for wanted in [
            Permissions { live: true, ..Permissions::default() },
            Permissions { remind: true, ..Permissions::default() },
            Permissions { drive: true, ..Permissions::default() },
            Permissions { storage: ft_plugins::Storage::Large, ..Permissions::default() },
            Permissions { location: true, ..Permissions::default() },
            Permissions { translate: true, ..Permissions::default() },
        ] {
            assert!(allowed(&asked, &wanted).is_err(), "{wanted:?} was never asked for");
            assert!(allowed(&wanted, &wanted).is_ok());
        }
    }

    fn served_index(languages: impl Fn(usize) -> std::collections::BTreeMap<String, ft_plugins::Localized>, image: &str) -> String {
        const SERVED: [(&str, &str, usize); 16] = [
            ("game.chess", "Chess", 88), ("game.fourinarow", "Four in a Row", 96), ("game.tictactoe", "Tic-Tac-Toe", 94),
            ("board", "Board", 81), ("clean", "Clean", 123), ("drive", "My drive", 58), ("images", "Image", 91),
            ("list", "List", 84), ("markdown", "Markdown", 76), ("notes", "Notes", 56), ("pdf", "PDF", 70),
            ("poll", "Poll", 96), ("redact", "Cover", 78), ("sign", "Sign", 122), ("sketch", "Sketch", 53),
            ("split", "Split", 124),
        ];
        let entries: Vec<CatalogueEntry> = SERVED
            .iter()
            .map(|(id, name, summary)| CatalogueEntry {
                id: format!("com.flickertalk.{id}"),
                name: (*name).to_owned(),
                version: "1.0.1".to_owned(),
                min_core_version: "1.3.0".to_owned(),
                size: 123_456,
                hash: "ab".repeat(32),
                url: format!("{CATALOGUE_HOME}/com.flickertalk.{id}/1.0.1.ftplugin"),
                summary: "x".repeat(*summary),
                kind: if id.starts_with("game.") { ft_plugins::Kind::Game } else { ft_plugins::Kind::Tool },
                locales: languages(*summary),
                icon: "radio-button-on-outline".to_owned(),
                image: image.to_owned(),
            })
            .collect();
        serde_json::to_string_pretty(&serde_json::json!({ "plugins": entries })).unwrap()
    }

    const LANGUAGES: [(&str, &str); 20] = [
        ("es", "dibuja "), ("pt", "desenhe "), ("fr", "dessinez "), ("de", "zeichnen "), ("it", "disegna "),
        ("ro", "desenează "), ("ru", "рисуйте "), ("uk", "малюйте "), ("pl", "rysuj "), ("tr", "çizin "),
        ("ar", "ارسم "), ("hi", "चित्र बनाएं "), ("bn", "আঁকুন "), ("id", "gambar "), ("vi", "vẽ hình "),
        ("th", "วาดภาพ "), ("ja", "指で描く"), ("ko", "그리기 "), ("zh-CN", "用手指画"), ("zh-TW", "用手指畫"),
    ];

    #[test]
    fn a_fully_translated_index_fits_what_the_core_downloads() {
        let text = |word: &str, chars: usize| word.chars().cycle().take(chars).collect::<String>();
        let svg = |bytes: usize, filler: char| {
            let bare = r#"<svg viewBox="0 0 64 64"><desc></desc></svg>"#;
            format!(r#"<svg viewBox="0 0 64 64"><desc>{}</desc></svg>"#, filler.to_string().repeat(bytes - bare.len()))
        };
        let realistic = served_index(|summary| {
            LANGUAGES
                .iter()
                .map(|(code, word)| {
                    let dense = matches!(*code, "ja" | "ko" | "zh-CN" | "zh-TW");
                    let (name, summary) = if dense { (6, summary * 6 / 10) } else { (18, summary * 13 / 10) };
                    let said = ft_plugins::Localized { name: Some(text(word, name)), summary: Some(text(word, summary.min(200))), ..Default::default() };
                    ((*code).to_owned(), said)
                })
                .collect()
        }, &svg(1500, 'x'));
        let longest = served_index(|_| {
            LANGUAGES
                .iter()
                .map(|(code, _)| ((*code).to_owned(), ft_plugins::Localized { name: Some("न".repeat(64)), summary: Some("न".repeat(200)), ..Default::default() }))
                .collect()
        }, &svg(ft_plugins::icon::IMAGE_LIMIT, '"'));
        eprintln!("catalogue.json: {} bytes translated, {} bytes at the limits", realistic.len(), longest.len());
        assert!((realistic.len() as u64) < INDEX_LIMIT / 4, "{} bytes leaves little room to grow", realistic.len());
        assert!((longest.len() as u64) < INDEX_LIMIT, "{} bytes is more than the core downloads", longest.len());

        let catalogue = vodozemac::Ed25519SecretKey::new();
        let signature = catalogue.sign(realistic.as_bytes()).to_base64();
        let listed = ft_plugins::catalogue_entries(&realistic, &signature, &catalogue.public_key()).expect("signed");
        assert_eq!(listed.len(), 16);
        assert!(listed.iter().all(|entry| entry.locales.len() == 20));
    }

    #[test]
    fn a_narrowed_grant_always_fits_what_is_asked() {
        let all = Permissions {
            network: vec!["a.example.com".to_owned(), "b.example.com".to_owned()],
            reads_given_messages: true,
            send: Sending::Auto,
            print: true,
            live: true,
            remind: true,
            drive: true,
            storage: ft_plugins::Storage::Large,
            location: true,
            translate: true,
        };
        for asked in [Permissions::default(), Permissions { network: vec!["b.example.com".to_owned()], send: Sending::Propose, ..Permissions::default() }, all.clone()] {
            let left = narrowed(&all, &asked);
            assert!(allowed(&asked, &left).is_ok(), "{left:?} beyond {asked:?}");
            assert_eq!(narrowed(&left, &all), left, "and never more than before");
        }
        assert_eq!(narrowed(&all, &all), all);
        assert_eq!(narrowed(&Permissions::default(), &all), Permissions::default(), "nothing new is turned on");
    }

    #[test]
    fn this_core_is_offered_games() {
        assert!(ft_plugins::version_at_least(CORE_VERSION, ft_plugins::GAMES_SINCE), "{CORE_VERSION}");
    }
}
