use std::path::PathBuf;

use anyhow::{ensure, Context, Result};
use ft_plugins::{installed, Manifest, Permissions, Plugin};
use vodozemac::Ed25519PublicKey;

use super::grants::{allowed, narrowed};
use super::CORE_VERSION;
use crate::{Core, Event};

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
        self.plugins_dir
            .get()
            .context("the plugins folder is not set")
    }

    // The signature is checked before anything is written
    pub async fn install_plugin(
        &self,
        package: &[u8],
        catalogue: &Ed25519PublicKey,
        granted: Permissions,
    ) -> Result<Manifest> {
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
        let had: Option<Permissions> =
            before.map(|installed| serde_json::from_str(&installed.granted).unwrap_or_default());
        let keep = match &had {
            Some(had) => narrowed(had, &plugin.manifest.permissions),
            None => {
                allowed(&plugin.manifest.permissions, &granted)?;
                granted
            }
        };
        let home = self.plugins_home()?;
        ft_plugins::install(&plugin, home)?;
        self.store
            .install_plugin(
                &plugin.manifest.id,
                &plugin.manifest.version,
                &serde_json::to_string(&keep)?,
            )
            .await?;
        if had.is_some() {
            self.store
                .grant_plugin(&plugin.manifest.id, &serde_json::to_string(&keep)?)
                .await?;
        }
        let _ = self.events.send(Event::PluginsChanged);
        if had.as_ref().is_some_and(|had| had.remind)
            && !keep.remind
            && self.store.forget_reminders(&plugin.manifest.id).await?
        {
            let _ = self.events.send(Event::RemindersChanged);
        }
        Ok(plugin.manifest)
    }

    pub async fn plugins_opening(&self, mime: &str) -> Result<Vec<Manifest>> {
        Ok(self
            .plugins()
            .await?
            .into_iter()
            .map(|plugin| plugin.manifest)
            .filter(|manifest| manifest.opens_kind(mime))
            .collect())
    }

    pub async fn plugins(&self) -> Result<Vec<InstalledPlugin>> {
        let home = self.plugins_home()?;
        let manifests = installed(home)?;
        let mut plugins = Vec::new();
        for row in self.store.plugins().await? {
            let Some(manifest) = manifests.iter().find(|manifest| manifest.id == row.id) else {
                continue;
            };
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

pub fn files_of(plugin: &Plugin) -> Vec<String> {
    plugin.files().map(|(path, _)| path.to_owned()).collect()
}
