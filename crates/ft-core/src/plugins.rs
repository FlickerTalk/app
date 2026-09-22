//! Plugins on this phone (issue app#3, Plan §48–§58). The core keeps two things in step: the
//! files of each plugin, under the plugins folder, and what the user granted it, in the database.
//! Nothing is installed unless the catalogue signed it, and nothing is granted by installing.

use std::path::PathBuf;

use anyhow::{bail, ensure, Context, Result};
use ft_plugins::{installed, Manifest, Permissions, Plugin, Sending};
use vodozemac::Ed25519PublicKey;

use crate::{Core, Event};

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

    /// What the user grants or takes back, at any time. Never more than the plugin asked for.
    pub async fn grant_plugin(&self, id: &str, granted: Permissions) -> Result<()> {
        let manifest = self.plugin_manifest(id)?;
        allowed(&manifest.permissions, &granted)?;
        self.store.grant_plugin(id, &serde_json::to_string(&granted)?).await?;
        let _ = self.events.send(Event::PluginsChanged);
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
        };
        assert!(allowed(&asked, &Permissions::default()).is_ok(), "granting nothing is always fine");
        assert!(allowed(&asked, &asked).is_ok());
        assert!(allowed(&Permissions::default(), &asked).is_err(), "it asked for nothing");
        assert!(allowed(&asked, &Permissions { send: Sending::Auto, ..asked.clone() }).is_err());
        let elsewhere = Permissions { network: vec!["evil.example".to_owned()], ..Permissions::default() };
        assert!(allowed(&asked, &elsewhere).is_err());
    }
}
