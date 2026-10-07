//! Plugins on this phone (issue app#3, Plan §48–§58). The core keeps two things in step: the
//! files of each plugin, under the plugins folder, and what the user granted it, in the database.
//! Nothing is installed unless the catalogue signed it, and nothing is granted by installing.

use std::path::{Path, PathBuf};

use anyhow::{bail, ensure, Context, Result};
use async_trait::async_trait;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::Serialize;
use ft_plugins::{installed, CatalogueEntry, Manifest, Permissions, Plugin, Sending};
use vodozemac::Ed25519PublicKey;

use ft_protocol::{Body, Packet};
use ft_storage::{Contact, PluginRef, Reminder};

use crate::web::{host_of, Fetch, WebAnswer, WebRequest};
use crate::{Core, Event};

/// Where the catalogue lives (§56): a signed index, and the packages next to it. Nothing travels
/// inside the app; the user picks what to install and the phone downloads it from here.
pub const CATALOGUE_HOME: &str = "https://flickertalk.com/plugins";

/// The most a package may weigh.
const PACKAGE_LIMIT: u64 = 8 * 1024 * 1024;
/// The most the index may weigh.
const INDEX_LIMIT: u64 = 512 * 1024;

/// The most a plugin may bring back from the network in one call.
const FETCH_LIMIT: u64 = 8 * 1024 * 1024;

/// How much a plugin may remember: enough for its settings, never a store of its own (§53).
const MEMORY_KEY: usize = 64;
const MEMORY_VALUE: usize = 64 * 1024;
const MEMORY_KEYS: usize = 64;

/// A plugin's records (2026-09-27): longer keys, big values, within the room the user granted.
const RECORD_KEY: usize = 128;
/// The most one record may hold: a board with pictures, never a movie.
pub const RECORD_VALUE: usize = 16 * 1024 * 1024;
/// The most a plugin may say to its twin in one go over `ft.live`: a chunk of a data channel.
pub const LIVE_LIMIT: usize = 48 * 1024;
/// What this FlickerTalk is, for a plugin's `minCoreVersion` (§51): every new capability of the
/// Plugin API bumps it, with the app's version. 1.1.0 (2026-09-27): records, reminders, the live
/// channel, "open with", refs and the user's cloud. 1.2.0 (2026-09-27): `views`, the tap that
/// shows a file in its viewer, and Quick Look on iOS. 1.2.1 (2026-09-29): no new capability, only
/// the app's version (the Store's price on the Plan screen). 1.2.2 (2026-09-30): none either
/// (Google Drive sign-in in the store builds, each platform with its own OAuth client). 1.3.0
/// (2026-10-02): `location`, the phone's current position once, for the location plugin; and
/// games (plan 10): `kind` in the manifest and the catalogue (`GAMES_SINCE`), and `onOpen.chat`,
/// the opaque id of the conversation a plugin is opened in. 1.3.1 (2026-10-03): no new capability,
/// only the app's version (a release of fixes). 1.3.2 (2026-10-03): none either (fixes).
/// 1.3.3 (2026-10-04): none either (same build as 1.3.2 with all Android ABIs).
/// 1.4.0 (2026-10-06): none either (chat features and more seeds; the Plugin API is unchanged).
/// 1.4.1 (2026-10-07): `takePhoto`, a photo from the camera, and `notify`, a toast over the app.
pub const CORE_VERSION: &str = "1.4.1";
/// The most a reminder's text may run to.
const REMINDER_TEXT: usize = 200;
/// What the key of a plugin's chat ids is derived for, from the storage key (2026-10-02).
const PLUGIN_CHAT_CONTEXT: &str = "flickertalk 2026-10-02 plugin chat id";

/// Where the phone is, as its location service said (2026-10-02): degrees (WGS 84), how far off
/// it may be in metres, and when the fix was taken, in milliseconds since the epoch.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Fix {
    pub lat: f64,
    pub lon: f64,
    pub accuracy: f64,
    pub at: i64,
}

/// Who asks the phone where it is: the platform bridge (CoreLocation, Android's LocationManager).
/// One current fix, asking the user first if it was never asked; `None` when the user or the
/// phone refuses, location is off, or no fix comes in time.
#[async_trait]
pub trait Locator: Send + Sync {
    async fn locate(&self) -> Result<Option<Fix>>;
}

/// How long the core waits for the phone's fix. The bridges give up by themselves at ~15 s; this
/// is the backstop, so a plugin is never left waiting for a bridge that never answers.
const LOCATION_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

impl Fix {
    /// A place on Earth, with an accuracy that means something; a fix that is not one is nothing.
    fn is_a_place(&self) -> bool {
        self.lat.is_finite()
            && self.lon.is_finite()
            && (-90.0..=90.0).contains(&self.lat)
            && (-180.0..=180.0).contains(&self.lon)
            && self.accuracy.is_finite()
            && self.accuracy >= 0.0
    }
}

/// One fix from the phone, or nothing: refused, off, broken, too late or not a place.
async fn located(locator: &dyn Locator) -> Option<Fix> {
    match tokio::time::timeout(LOCATION_WAIT, locator.locate()).await {
        Ok(Ok(fix)) => fix.filter(Fix::is_a_place),
        _ => None,
    }
}

/// A plugin as the app shows it: what it is, and what it may do here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPlugin {
    pub manifest: Manifest,
    /// What the user granted, always a subset of what the manifest asks for.
    pub granted: Permissions,
    pub installed_at: i64,
}

impl Core {
    /// Where the plugins live; the app sets it to a folder of its own storage, when it starts. A
    /// swap an update left halfway (the app killed) is put right first (2026-10-03).
    pub fn set_plugins_dir(&self, dir: PathBuf) {
        let _ = ft_plugins::recover(&dir);
        let _ = self.plugins_dir.set(dir);
    }

    fn plugins_home(&self) -> Result<&PathBuf> {
        self.plugins_dir.get().context("the plugins folder is not set")
    }

    /// Installs a package, or updates one: the signature is checked before anything is written
    /// (§50), and what the user granted before is kept.
    pub async fn install_plugin(&self, package: &[u8], catalogue: &Ed25519PublicKey, granted: Permissions) -> Result<Manifest> {
        let plugin = ft_plugins::open(package, catalogue)?;
        ensure!(
            ft_plugins::version_at_least(CORE_VERSION, &plugin.manifest.min_core_version),
            "{} needs FlickerTalk {} or newer",
            plugin.manifest.id,
            plugin.manifest.min_core_version
        );
        // An update keeps what the user granted before, cut down to what this version asks for:
        // never more (§53), and a version that asks for less still installs (2026-10-03).
        let before = self.store.plugin(&plugin.manifest.id).await?;
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
        // Without `remind` any more, what it had set must not ring: the alarm clock is told again.
        if had.as_ref().is_some_and(|had| had.remind) && !keep.remind && self.store.forget_reminders(&plugin.manifest.id).await? {
            let _ = self.events.send(Event::RemindersChanged);
        }
        Ok(plugin.manifest)
    }

    /// What a plugin remembers between two openings. Its frame has no origin of its own, so the
    /// browser gives it no storage: the core keeps it, apart from every other plugin (§53), and
    /// apart for each place it is opened in (§108).
    pub async fn plugin_remembers(&self, id: &str, session: Option<&str>, key: &str) -> Result<Option<String>> {
        self.store.plugin_value(id, self.place(session)?, key).await
    }

    /// Keeps one value for a plugin. Small, few, and only for a plugin that is installed here.
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

    /// Where a plugin is open (2026-10-01, §108): the main list (`None`) or a hidden session,
    /// which must be open right now. What a plugin keeps belongs to the place it was kept in, and
    /// a closed session lends it to nobody, not even to whoever names it. The plugin never learns
    /// any of this: the app says where it opened it, and the core keeps each place apart.
    fn place<'a>(&self, session: Option<&'a str>) -> Result<Option<&'a str>> {
        if let Some(session) = session {
            ensure!(self.is_session_open(session), "that session is not open");
        }
        Ok(session)
    }

    // ---- Records (2026-09-27): what a plugin keeps beyond its settings ----

    pub async fn plugin_record(&self, id: &str, session: Option<&str>, key: &str) -> Result<Option<Vec<u8>>> {
        self.store.plugin_record(id, self.place(session)?, key).await
    }

    /// Keeps one record, within the room the user granted the plugin (`storage`): the value
    /// replaces what the key held, and the plugin's records in that place must fit the quota.
    /// Each place has the whole quota: what a session holds never shows in another's room.
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

    /// How much of its room a plugin uses in that place, and how much it has: (used, quota), in
    /// bytes.
    pub async fn plugin_records_usage(&self, id: &str, session: Option<&str>) -> Result<(u64, u64)> {
        let session = self.place(session)?;
        let quota = self.granted_to(id).await?.storage.quota();
        Ok((self.store.plugin_records_size(id, session).await? as u64, quota))
    }

    // ---- Refs: a way back to the message a plugin was opened with, and nothing more ----

    /// An opaque handle for the message the user hands a plugin: the same message gives the same
    /// ref, and the plugin learns nothing of the contact from it.
    pub async fn plugin_ref(&self, id: &str, message_id: &str) -> Result<String> {
        if let Some(reference) = self.store.plugin_ref_for(id, message_id).await? {
            return Ok(reference);
        }
        let message = self.store.message(message_id).await?.context("that message is not here")?;
        let reference = format!("ref_{}", ft_protocol::MessageId::new());
        self.store.add_plugin_ref(&reference, id, &message.contact, message_id).await?;
        Ok(reference)
    }

    /// Where a plugin's ref leads, if the message and the contact are still here, it was this
    /// plugin's ref, and the contact is in the place the plugin is open in (§108). `None`
    /// otherwise: a plugin cannot fish for someone else's, nor reach a session from outside it.
    pub async fn plugin_ref_target(&self, id: &str, session: Option<&str>, reference: &str) -> Result<Option<PluginRef>> {
        let session = self.place(session)?;
        let Some(target) = self.store.plugin_ref(reference).await? else { return Ok(None) };
        let contact = self.store.contact(&target.contact).await?;
        if target.plugin != id || contact.is_none_or(|contact| contact.blocked || contact.session.as_deref() != session) {
            return Ok(None);
        }
        if self.store.message(&target.message_id).await?.is_none() {
            return Ok(None);
        }
        Ok(Some(target))
    }

    // ---- Reminders (2026-09-27): a notification on this phone, at a time the plugin picked ----

    /// Sets (or moves) a reminder. Only a plugin granted `remind`; the text is only what the
    /// notification says if the user allows content on the lock screen.
    /// Set inside a hidden session, it rings only while the session is open (§108).
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

    /// A plugin's reminders in the place it is open in, soonest first.
    pub async fn plugin_reminders(&self, id: &str, session: Option<&str>) -> Result<Vec<Reminder>> {
        let session = self.place(session)?;
        Ok(self.store.reminders(Some(id)).await?.into_iter().filter(|reminder| reminder.session.as_deref() == session).collect())
    }

    /// The reminder a tap on its notification opens, if it may ring now: with the session it was
    /// set in, so the plugin opens there. Should the same id be set in two places, the first.
    pub async fn ringing_reminder(&self, id: &str, reminder: &str) -> Result<Option<Reminder>> {
        Ok(self.reminders().await?.into_iter().find(|ringing| ringing.plugin == id && ringing.id == reminder))
    }

    /// Every reminder that may ring now, soonest first: what the phone's alarm clock is told.
    /// One set inside a closed session is not, and the alarm clock is told again whenever a
    /// session opens, closes or goes (§108).
    pub async fn reminders(&self) -> Result<Vec<Reminder>> {
        Ok(self
            .store
            .reminders(None)
            .await?
            .into_iter()
            .filter(|reminder| reminder.session.as_deref().is_none_or(|session| self.is_session_open(session)))
            .collect())
    }

    /// Reminders whose time has come: the app shows them (or the OS did), and they go.
    pub async fn due_reminders(&self, at: i64) -> Result<Vec<Reminder>> {
        Ok(self.reminders().await?.into_iter().filter(|reminder| reminder.at <= at).collect())
    }

    // ---- The conversation a plugin is opened in (2026-10-02) ----

    /// The id of the conversation with `contact` for plugin `id` on this phone, handed to the
    /// plugin when the app opens it in that chat, so what it keeps per conversation stays there.
    /// Derived, never stored: BLAKE3 keyed with a key of its own derived from this phone's
    /// storage key, over the plugin's id and the contact's. Stable across restarts and updates (and
    /// a move to a new phone, which takes the key along); its own for each plugin, so two cannot
    /// match theirs; and opaque, since the plugin never has the key. 32 bytes, base64url without
    /// padding: 43 characters of `[A-Za-z0-9_-]`.
    ///
    /// Only for a contact the user chose and did not block, in the main list or in a hidden
    /// session that is open (§108): anyone else gets the same error as nobody at all.
    pub async fn plugin_chat(&self, id: &str, contact: &str) -> Result<String> {
        ensure!(self.store.plugin(id).await?.is_some(), "{id} is not installed here");
        let reachable = self.store.contact(contact).await?.is_some_and(|contact| {
            contact.accepted && !contact.blocked && contact.session.as_deref().is_none_or(|session| self.is_session_open(session))
        });
        ensure!(reachable, "that is not a contact of yours");
        let mut hasher = blake3::Hasher::new_keyed(&blake3::derive_key(PLUGIN_CHAT_CONTEXT, &self.key));
        // Neither id can hold a NUL, so the two are told apart.
        hasher.update(id.as_bytes()).update(&[0]).update(contact.as_bytes());
        Ok(URL_SAFE_NO_PAD.encode(hasher.finalize().as_bytes()))
    }

    // ---- ft.live (2026-09-27): a plugin talks to its twin on the other side ----

    /// Sends what a plugin says to the same plugin on the contact's phone, over the direct
    /// connection only (never the mailbox): `false` if the contact cannot be reached now. The
    /// core never reads the bytes; it only checks that the user granted this plugin the channel.
    pub async fn plugin_live_send(&self, id: &str, contact: &str, data: Vec<u8>) -> Result<bool> {
        ensure!(self.granted_to(id).await?.live, "{id} may not talk to the other side");
        ensure!(data.len() <= LIVE_LIMIT, "a plugin may not say that much at once");
        let contact = self.contact(contact).await?;
        ensure!(contact.accepted && !contact.blocked, "that is not a contact of yours");
        self.transmit_direct(&contact, &Packet::new(Body::PluginEvent { plugin: id.to_owned(), data })).await
    }

    /// What a plugin on the other side said: handed to its twin here only if it is installed and
    /// granted the channel; dropped otherwise, and never stored.
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

    // ---- ft.location (2026-10-02): where the phone is, once ----

    /// The phone's current position, once, for a plugin the user granted `location`; the phone
    /// is not even asked otherwise. `None` when the user or the phone refuses, location is off or
    /// no fix comes in time. The core only passes it on: nothing of it is kept or logged (§100).
    pub async fn plugin_location(&self, id: &str, locator: &dyn Locator) -> Result<Option<Fix>> {
        ensure!(self.granted_to(id).await?.location, "{id} may not ask where the phone is");
        Ok(located(locator).await)
    }

    /// The plugins installed here that open a file of this kind, for "open with" (2026-09-27).
    pub async fn plugins_opening(&self, mime: &str) -> Result<Vec<Manifest>> {
        Ok(self.plugins().await?.into_iter().map(|plugin| plugin.manifest).filter(|manifest| manifest.opens_kind(mime)).collect())
    }

    /// What the catalogue offers, read only if the catalogue signed the index (§56). Reading it
    /// installs nothing.
    pub async fn catalogue(&self, fetch: &dyn Fetch, catalogue: &Ed25519PublicKey) -> Result<Vec<CatalogueEntry>> {
        // The index for cores from 1.1.0 on (index.json is the app 1.0.0's, cut down for it).
        let index = fetch.get(&format!("{CATALOGUE_HOME}/{}", ft_plugins::INDEX), INDEX_LIMIT).await?;
        let signature = fetch.get(&format!("{CATALOGUE_HOME}/{}.sig", ft_plugins::INDEX), 1024).await?;
        let index = String::from_utf8(index).context("the index is not text")?;
        let signature = String::from_utf8(signature).context("the signature is not text")?;
        // What needs a newer FlickerTalk is not offered, nor a kind of plugin this one does not
        // know: neither would install (§51).
        Ok(ft_plugins::catalogue_entries(&index, signature.trim(), catalogue)?
            .into_iter()
            .filter(|entry| entry.runs_on(CORE_VERSION) && entry.kind != ft_plugins::Kind::Unknown)
            .collect())
    }

    /// Downloads what the catalogue listed and installs it. Installing grants nothing (§53), and
    /// a package that is not byte for byte what was listed is never opened (§50).
    pub async fn add_plugin(
        &self,
        entry: &CatalogueEntry,
        fetch: &dyn Fetch,
        catalogue: &Ed25519PublicKey,
    ) -> Result<Manifest> {
        ensure!(entry.url.starts_with(&format!("{CATALOGUE_HOME}/")), "that plugin is not in our catalogue");
        ensure!(entry.size <= PACKAGE_LIMIT, "that plugin is too big");
        let package = fetch.get(&entry.url, PACKAGE_LIMIT).await?;
        ft_plugins::download(entry, &package, catalogue)?;
        self.install_plugin(&package, catalogue, Permissions::default()).await
    }

    /// Updates what the user installed from the catalogue when its signed index lists a higher
    /// version (2026-10-03), and returns what was updated. Only what is installed here; never one
    /// that is open (it waits for a later pass), that needs a newer FlickerTalk (§51), that would
    /// change kind, or that weighs more than a package may. Each one is downloaded and checked as
    /// a new install is (§50), and swapped in whole: whatever fails leaves the installed version.
    /// What the user granted is kept, never widened (`install_plugin`).
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

    /// A call a plugin asked the core to make for it (§55). The core checks the host against what
    /// the user granted **this** plugin: the policy of the frame is a second lock, never the only
    /// one. Nothing of the phone —no identity, no key, no cookie— travels with it.
    pub async fn plugin_fetch(&self, id: &str, request: WebRequest, fetch: &dyn Fetch) -> Result<WebAnswer> {
        let host = host_of(&request.url)?;
        let granted = self.granted_to(id).await?;
        ensure!(granted.network.iter().any(|allowed| allowed.eq_ignore_ascii_case(&host)), "{id} may not reach {host}");
        fetch.call(&request, FETCH_LIMIT).await
    }

    /// How far a plugin may write in the chat (A2): what the user granted it, or nothing if it
    /// is not installed here.
    pub async fn plugin_sending(&self, id: &str) -> Result<Sending> {
        Ok(self.granted_to(id).await?.send)
    }

    /// Whether a plugin may put something in the composer for the user to send (A2).
    pub async fn plugin_may_propose(&self, id: &str) -> Result<bool> {
        Ok(matches!(self.plugin_sending(id).await?, Sending::Propose | Sending::Auto))
    }

    /// A plugin sends a file to the contact by itself (A2): only with the `auto` permission,
    /// which the user grants on its own and which is never the default. With `propose` the app
    /// puts the file in the composer instead, and the user presses send; with nothing, nothing.
    pub async fn plugin_send_file(&self, id: &str, contact: &str, path: &Path, name: &str, mime: &str) -> Result<String> {
        ensure!(self.plugin_sending(id).await? == Sending::Auto, "{id} may not send by itself");
        self.send_file(contact, path, name, mime).await
    }

    /// What the user granted a plugin that is installed here.
    pub(crate) async fn granted_to(&self, id: &str) -> Result<Permissions> {
        let row = self.store.plugin(id).await?.with_context(|| format!("{id} is not installed here"))?;
        Ok(serde_json::from_str(&row.granted).unwrap_or_default())
    }

    /// What the user grants or takes back, at any time. Never more than the plugin asked for.
    pub async fn grant_plugin(&self, id: &str, granted: Permissions) -> Result<()> {
        let manifest = self.plugin_manifest(id)?;
        allowed(&manifest.permissions, &granted)?;
        self.store.grant_plugin(id, &serde_json::to_string(&granted)?).await?;
        let _ = self.events.send(Event::PluginsChanged);
        // Without `remind`, what it had set must not ring: the alarm clock is told again.
        if !granted.remind && self.store.forget_reminders(id).await? {
            let _ = self.events.send(Event::RemindersChanged);
        }
        Ok(())
    }

    /// The plugins installed here, with what each one was granted.
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
            });
        }
        Ok(plugins)
    }

    /// Takes a plugin off this phone: its files and what it was granted.
    pub async fn remove_plugin(&self, id: &str) -> Result<()> {
        ft_plugins::remove(id, self.plugins_home()?)?;
        self.store.remove_plugin(id).await?;
        let _ = self.events.send(Event::PluginsChanged);
        // Its reminders went with it; the alarm clock still holds them until it is told.
        let _ = self.events.send(Event::RemindersChanged);
        Ok(())
    }

    fn plugin_manifest(&self, id: &str) -> Result<Manifest> {
        installed(self.plugins_home()?)?
            .into_iter()
            .find(|manifest| manifest.id == id)
            .with_context(|| format!("{id} is not installed"))
    }
}

/// A grant a plugin never asked for is not a grant: it is a way in (§53).
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
    ensure!(
        granted.storage == ft_plugins::Storage::Small || asked.storage == ft_plugins::Storage::Large,
        "the plugin never asked for that much room"
    );
    Ok(())
}

/// What is left of a grant once the plugin asks for `asked`: each permission only if it is still
/// asked for, never more than before (2026-10-03, updates).
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
    }
}

fn reach(sending: Sending) -> u8 {
    match sending {
        Sending::Nothing => 0,
        Sending::Propose => 1,
        Sending::Auto => 2,
    }
}

/// The files of an open package, for the app to serve them to the WebView.
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

    // 2026-10-02: the bridge times out by itself (~15 s); should it never answer, the core stops
    // waiting too, so a plugin is never left hanging.
    #[tokio::test(start_paused = true)]
    async fn a_phone_that_never_answers_is_nothing_after_a_while() {
        let started = tokio::time::Instant::now();
        assert_eq!(located(&Silent).await, None);
        let waited = started.elapsed();
        assert!(waited >= std::time::Duration::from_secs(15) && waited <= std::time::Duration::from_secs(30), "waited {waited:?}");
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
        // 2026-09-27: the same for the live channel, reminders, the drive and the room.
        for wanted in [
            Permissions { live: true, ..Permissions::default() },
            Permissions { remind: true, ..Permissions::default() },
            Permissions { drive: true, ..Permissions::default() },
            Permissions { storage: ft_plugins::Storage::Large, ..Permissions::default() },
            // 2026-10-02: the phone's position.
            Permissions { location: true, ..Permissions::default() },
        ] {
            assert!(allowed(&asked, &wanted).is_err(), "{wanted:?} was never asked for");
            assert!(allowed(&wanted, &wanted).is_ok());
        }
    }

    /// The index as `ftcatalogue` writes it, for the sixteen plugins served on 2026-10-02 (their
    /// ids, names and the length of their English summaries), each with `locales` from `languages`.
    fn served_index(languages: impl Fn(usize) -> std::collections::BTreeMap<String, ft_plugins::Localized>) -> String {
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
            })
            .collect();
        serde_json::to_string_pretty(&serde_json::json!({ "plugins": entries })).unwrap()
    }

    /// The app's twenty other languages, each with a word of its script to build text from.
    const LANGUAGES: [(&str, &str); 20] = [
        ("es", "dibuja "), ("pt", "desenhe "), ("fr", "dessinez "), ("de", "zeichnen "), ("it", "disegna "),
        ("ro", "desenează "), ("ru", "рисуйте "), ("uk", "малюйте "), ("pl", "rysuj "), ("tr", "çizin "),
        ("ar", "ارسم "), ("hi", "चित्र बनाएं "), ("bn", "আঁকুন "), ("id", "gambar "), ("vi", "vẽ hình "),
        ("th", "วาดภาพ "), ("ja", "指で描く"), ("ko", "그리기 "), ("zh-CN", "用手指画"), ("zh-TW", "用手指畫"),
    ];

    // 2026-10-02 (plan of the catalogue's translations): with every plugin named and summed up in
    // all twenty languages, the index still fits what the core downloads (`INDEX_LIMIT`), with
    // translations a third longer than the English, and even with every text at its longest in a
    // script of three bytes a character. It stays signed and readable byte for byte.
    #[test]
    fn a_fully_translated_index_fits_what_the_core_downloads() {
        let text = |word: &str, chars: usize| word.chars().cycle().take(chars).collect::<String>();
        let realistic = served_index(|summary| {
            LANGUAGES
                .iter()
                .map(|(code, word)| {
                    // Chinese, Japanese and Korean say it in about half the characters.
                    let dense = matches!(*code, "ja" | "ko" | "zh-CN" | "zh-TW");
                    let (name, summary) = if dense { (6, summary * 6 / 10) } else { (18, summary * 13 / 10) };
                    let said = ft_plugins::Localized { name: Some(text(word, name)), summary: Some(text(word, summary.min(200))) };
                    ((*code).to_owned(), said)
                })
                .collect()
        });
        let longest = served_index(|_| {
            LANGUAGES
                .iter()
                .map(|(code, _)| ((*code).to_owned(), ft_plugins::Localized { name: Some("न".repeat(64)), summary: Some("न".repeat(200)) }))
                .collect()
        });
        eprintln!("catalogue.json: {} bytes translated, {} bytes at the limits", realistic.len(), longest.len());
        assert!((realistic.len() as u64) < INDEX_LIMIT / 4, "{} bytes leaves little room to grow", realistic.len());
        assert!((longest.len() as u64) < INDEX_LIMIT, "{} bytes is more than the core downloads", longest.len());

        let catalogue = vodozemac::Ed25519SecretKey::new();
        let signature = catalogue.sign(realistic.as_bytes()).to_base64();
        let listed = ft_plugins::catalogue_entries(&realistic, &signature, &catalogue.public_key()).expect("signed");
        assert_eq!(listed.len(), 16);
        assert!(listed.iter().all(|entry| entry.locales.len() == 20));
    }

    // Whatever was granted and whatever is asked, what is left always fits what is asked.
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
        };
        for asked in [Permissions::default(), Permissions { network: vec!["b.example.com".to_owned()], send: Sending::Propose, ..Permissions::default() }, all.clone()] {
            let left = narrowed(&all, &asked);
            assert!(allowed(&asked, &left).is_ok(), "{left:?} beyond {asked:?}");
            assert_eq!(narrowed(&left, &all), left, "and never more than before");
        }
        assert_eq!(narrowed(&all, &all), all);
        assert_eq!(narrowed(&Permissions::default(), &all), Permissions::default(), "nothing new is turned on");
    }

    // 1.3.0 (2026-10-02) also tells games from tools, so it is a core games are published for: a
    // game asks for `GAMES_SINCE`, and a core below it is never offered one.
    #[test]
    fn this_core_is_offered_games() {
        assert!(ft_plugins::version_at_least(CORE_VERSION, ft_plugins::GAMES_SINCE), "{CORE_VERSION}");
    }
}
