//! Plugins on this phone (issue app#3, Plan §48–§58). The core keeps two things in step: the
//! files of each plugin, under the plugins folder, and what the user granted it, in the database.
//! Nothing is installed unless the catalogue signed it, and nothing is granted by installing.

use std::path::{Path, PathBuf};

use anyhow::{bail, ensure, Context, Result};
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
/// shows a file in its viewer, and Quick Look on iOS.
pub const CORE_VERSION: &str = "1.2.0";
/// The most a reminder's text may run to.
const REMINDER_TEXT: usize = 200;

/// A plugin as the app shows it: what it is, and what it may do here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPlugin {
    pub manifest: Manifest,
    /// What the user granted, always a subset of what the manifest asks for.
    pub granted: Permissions,
    pub installed_at: i64,
}

impl Core {
    /// Where the plugins live; the app sets it to a folder of its own storage.
    pub fn set_plugins_dir(&self, dir: PathBuf) {
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
        allowed(&plugin.manifest.permissions, &granted)?;
        let home = self.plugins_home()?;
        ft_plugins::install(&plugin, home)?;
        let before = self.store.plugin(&plugin.manifest.id).await?;
        let keep = match before {
            Some(installed) => installed.granted,
            None => serde_json::to_string(&granted)?,
        };
        self.store.install_plugin(&plugin.manifest.id, &plugin.manifest.version, &keep).await?;
        let _ = self.events.send(Event::PluginsChanged);
        Ok(plugin.manifest)
    }

    /// What a plugin remembers between two openings. Its frame has no origin of its own, so the
    /// browser gives it no storage: the core keeps it, apart from every other plugin (§53).
    pub async fn plugin_remembers(&self, id: &str, key: &str) -> Result<Option<String>> {
        self.store.plugin_value(id, key).await
    }

    /// Keeps one value for a plugin. Small, few, and only for a plugin that is installed here.
    pub async fn plugin_remember(&self, id: &str, key: &str, value: &str) -> Result<()> {
        ensure!(!key.is_empty() && key.len() <= MEMORY_KEY, "that key is too long");
        ensure!(value.len() <= MEMORY_VALUE, "a plugin may not keep that much");
        ensure!(self.store.plugin(id).await?.is_some(), "{id} is not installed here");
        let keys = self.store.plugin_keys(id).await?;
        ensure!(
            keys.len() < MEMORY_KEYS || keys.iter().any(|known| known == key),
            "a plugin may not keep that many things"
        );
        self.store.set_plugin_value(id, key, value).await
    }

    pub async fn plugin_forget(&self, id: &str, key: &str) -> Result<()> {
        self.store.forget_plugin_value(id, key).await
    }

    pub async fn plugin_memory_keys(&self, id: &str) -> Result<Vec<String>> {
        self.store.plugin_keys(id).await
    }

    // ---- Records (2026-09-27): what a plugin keeps beyond its settings ----

    pub async fn plugin_record(&self, id: &str, key: &str) -> Result<Option<Vec<u8>>> {
        self.store.plugin_record(id, key).await
    }

    /// Keeps one record, within the room the user granted the plugin (`storage`): the value
    /// replaces what the key held, and the whole of the plugin's records must fit the quota.
    pub async fn plugin_record_set(&self, id: &str, key: &str, value: &[u8]) -> Result<()> {
        ensure!(!key.is_empty() && key.len() <= RECORD_KEY, "that key is too long");
        ensure!(value.len() <= RECORD_VALUE, "a record may not hold that much");
        let quota = self.granted_to(id).await?.storage.quota();
        let held = self.store.plugin_record(id, key).await?.map_or(0, |old| old.len() as i64);
        let used = self.store.plugin_records_size(id).await? - held;
        ensure!(used + value.len() as i64 <= quota as i64, "the plugin has no room left for that");
        self.store.set_plugin_record(id, key, value).await
    }

    pub async fn plugin_record_forget(&self, id: &str, key: &str) -> Result<()> {
        self.store.forget_plugin_record(id, key).await
    }

    pub async fn plugin_record_keys(&self, id: &str, prefix: &str) -> Result<Vec<String>> {
        self.store.plugin_record_keys(id, prefix).await
    }

    /// How much of its room a plugin uses, and how much it has: (used, quota), in bytes.
    pub async fn plugin_records_usage(&self, id: &str) -> Result<(u64, u64)> {
        let quota = self.granted_to(id).await?.storage.quota();
        Ok((self.store.plugin_records_size(id).await? as u64, quota))
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

    /// Where a plugin's ref leads, if the message and the contact are still here and it was
    /// this plugin's ref. `None` otherwise: a plugin cannot fish for someone else's.
    pub async fn plugin_ref_target(&self, id: &str, reference: &str) -> Result<Option<PluginRef>> {
        let Some(target) = self.store.plugin_ref(reference).await? else { return Ok(None) };
        if target.plugin != id || self.store.contact(&target.contact).await?.is_none_or(|contact| contact.blocked) {
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
    pub async fn set_reminder(&self, id: &str, reminder: &str, at: i64, text: &str) -> Result<()> {
        ensure!(self.granted_to(id).await?.remind, "{id} may not set reminders");
        ensure!(!reminder.is_empty() && reminder.len() <= RECORD_KEY, "that reminder id is too long");
        ensure!(at > 0, "a reminder needs a time");
        let text: String = text.chars().take(REMINDER_TEXT).collect();
        self.store.set_reminder(&Reminder { plugin: id.to_owned(), id: reminder.to_owned(), at, text }).await?;
        let _ = self.events.send(Event::RemindersChanged);
        Ok(())
    }

    pub async fn cancel_reminder(&self, id: &str, reminder: &str) -> Result<bool> {
        let gone = self.store.cancel_reminder(id, reminder).await?;
        if gone {
            let _ = self.events.send(Event::RemindersChanged);
        }
        Ok(gone)
    }

    /// A plugin's reminders, soonest first.
    pub async fn plugin_reminders(&self, id: &str) -> Result<Vec<Reminder>> {
        self.store.reminders(Some(id)).await
    }

    /// Every reminder of every plugin, soonest first: what the phone's alarm clock is told.
    pub async fn reminders(&self) -> Result<Vec<Reminder>> {
        self.store.reminders(None).await
    }

    /// Reminders whose time has come: the app shows them (or the OS did), and they go.
    pub async fn due_reminders(&self, at: i64) -> Result<Vec<Reminder>> {
        Ok(self.store.reminders(None).await?.into_iter().filter(|reminder| reminder.at <= at).collect())
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

    /// The plugins installed here that open a file of this kind, for "open with" (2026-09-27).
    pub async fn plugins_opening(&self, mime: &str) -> Result<Vec<Manifest>> {
        Ok(self.plugins().await?.into_iter().map(|plugin| plugin.manifest).filter(|manifest| manifest.opens_kind(mime)).collect())
    }

    /// What the catalogue offers, read only if the catalogue signed the index (§56). Reading it
    /// installs nothing.
    pub async fn catalogue(&self, fetch: &dyn Fetch, catalogue: &Ed25519PublicKey) -> Result<Vec<CatalogueEntry>> {
        let index = fetch.get(&format!("{CATALOGUE_HOME}/index.json"), INDEX_LIMIT).await?;
        let signature = fetch.get(&format!("{CATALOGUE_HOME}/index.json.sig"), 1024).await?;
        let index = String::from_utf8(index).context("the index is not text")?;
        let signature = String::from_utf8(signature).context("the signature is not text")?;
        // What needs a newer FlickerTalk is not offered: it would not install (§51).
        Ok(ft_plugins::catalogue_entries(&index, signature.trim(), catalogue)?.into_iter().filter(|entry| entry.runs_on(CORE_VERSION)).collect())
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
    ensure!(
        granted.storage == ft_plugins::Storage::Small || asked.storage == ft_plugins::Storage::Large,
        "the plugin never asked for that much room"
    );
    Ok(())
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
        ] {
            assert!(allowed(&asked, &wanted).is_err(), "{wanted:?} was never asked for");
            assert!(allowed(&wanted, &wanted).is_ok());
        }
    }
}
