use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use super::{failed, now_ms, Client};

pub async fn refresh_served_plugins(app: &AppHandle, core: &Arc<ft_core::Core>, dir: &Path) {
    let mut plugins = Vec::new();
    for plugin in core.plugins().await.unwrap_or_default() {
        let open = core
            .may_use_plugin(&plugin.manifest.id, plugin.manifest.kind)
            .await
            .unwrap_or(false);
        plugins.push((plugin, open));
    }
    app.state::<crate::plugins::Plugins>()
        .set(served_of(plugins, dir));
}

fn served_of(
    plugins: Vec<(ft_core::plugins::InstalledPlugin, bool)>,
    dir: &Path,
) -> std::collections::HashMap<String, crate::plugins::Served> {
    let mut served = std::collections::HashMap::new();
    for (plugin, open) in plugins {
        if !open {
            continue;
        }
        let Some(component) = plugin.manifest.components.first().cloned() else {
            continue;
        };
        served.insert(
            plugin.manifest.id.clone(),
            crate::plugins::Served {
                dir: dir.join("plugins").join(&plugin.manifest.id),
                component,
                policy: crate::plugins::policy_for(&plugin.granted),
                version: plugin.manifest.version.clone(),
            },
        );
    }
    served
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginView {
    id: String,
    name: String,
    version: String,
    asks: PermissionsView,
    granted: PermissionsView,
    installed_at: i64,
    #[serde(default)]
    opens: Vec<String>,
    #[serde(default)]
    views: Vec<String>,
    kind: ft_plugins::Kind,
    locales: BTreeMap<String, ft_plugins::Localized>,
    #[serde(skip_serializing_if = "String::is_empty")]
    icon: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    image: String,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    hooks: BTreeMap<ft_plugins::HookName, ft_plugins::HookDeclaration>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PermissionsView {
    network: Vec<String>,
    messages: bool,
    send: String,
    #[serde(default)]
    print: bool,
    #[serde(default)]
    live: bool,
    #[serde(default)]
    remind: bool,
    #[serde(default)]
    drive: bool,
    #[serde(default = "small")]
    storage: String,
    #[serde(default)]
    location: bool,
    #[serde(default)]
    translate: bool,
}

fn small() -> String {
    "small".to_owned()
}

impl From<&ft_plugins::Permissions> for PermissionsView {
    fn from(permissions: &ft_plugins::Permissions) -> Self {
        Self {
            network: permissions.network.clone(),
            messages: permissions.reads_given_messages,
            send: match permissions.send {
                ft_plugins::Sending::Nothing => "nothing",
                ft_plugins::Sending::Propose => "propose",
                ft_plugins::Sending::Auto => "auto",
            }
            .to_owned(),
            print: permissions.print,
            live: permissions.live,
            remind: permissions.remind,
            drive: permissions.drive,
            storage: match permissions.storage {
                ft_plugins::Storage::Small => "small",
                ft_plugins::Storage::Large => "large",
            }
            .to_owned(),
            location: permissions.location,
            translate: permissions.translate,
        }
    }
}

impl From<PermissionsView> for ft_plugins::Permissions {
    fn from(granted: PermissionsView) -> Self {
        Self {
            network: granted.network,
            reads_given_messages: granted.messages,
            send: match granted.send.as_str() {
                "propose" => ft_plugins::Sending::Propose,
                "auto" => ft_plugins::Sending::Auto,
                _ => ft_plugins::Sending::Nothing,
            },
            print: granted.print,
            live: granted.live,
            remind: granted.remind,
            drive: granted.drive,
            storage: if granted.storage == "large" {
                ft_plugins::Storage::Large
            } else {
                ft_plugins::Storage::Small
            },
            location: granted.location,
            translate: granted.translate,
        }
    }
}

fn plugin_view(plugin: ft_core::plugins::InstalledPlugin) -> PluginView {
    PluginView {
        asks: PermissionsView::from(&plugin.manifest.permissions),
        granted: PermissionsView::from(&plugin.granted),
        id: plugin.manifest.id,
        name: plugin.manifest.name,
        version: plugin.manifest.version,
        installed_at: plugin.installed_at,
        opens: plugin.manifest.opens,
        views: plugin.manifest.views,
        kind: plugin.manifest.kind,
        locales: plugin.manifest.locales,
        icon: plugin.manifest.icon,
        image: plugin.image,
        hooks: plugin.manifest.hooks,
    }
}

#[tauri::command]
pub async fn core_plugins(client: State<'_, Client>) -> Result<Vec<PluginView>, String> {
    let core = client.core().await?;
    Ok(core
        .plugins()
        .await
        .map_err(failed)?
        .into_iter()
        .map(plugin_view)
        .collect())
}

#[tauri::command]
pub async fn core_plugin_grant(
    plugin: String,
    granted: PermissionsView,
    app: AppHandle,
    client: State<'_, Client>,
) -> Result<(), String> {
    let permissions = ft_plugins::Permissions::from(granted);
    let core = client.core().await?;
    core.grant_plugin(&plugin, permissions)
        .await
        .map_err(failed)?;
    refresh_served_plugins(&app, &core, client.dir()?).await;
    Ok(())
}

#[tauri::command]
pub async fn core_plugin_remove(
    plugin: String,
    app: AppHandle,
    client: State<'_, Client>,
) -> Result<(), String> {
    let core = client.core().await?;
    core.remove_plugin(&plugin).await.map_err(failed)?;
    refresh_served_plugins(&app, &core, client.dir()?).await;
    Ok(())
}

const BUNDLED_PLUGINS: &[&[u8]] = &[
    include_bytes!("../../resources/plugins/markdown.ftplugin"),
    include_bytes!("../../resources/plugins/images.ftplugin"),
    include_bytes!("../../resources/plugins/pdf.ftplugin"),
    include_bytes!("../../resources/plugins/redact.ftplugin"),
    include_bytes!("../../resources/plugins/sketch.ftplugin"),
    include_bytes!("../../resources/plugins/scanner.ftplugin"),
    include_bytes!("../../resources/plugins/countdown.ftplugin"),
    include_bytes!("../../resources/plugins/board.ftplugin"),
    include_bytes!("../../resources/plugins/clean.ftplugin"),
    include_bytes!("../../resources/plugins/drive.ftplugin"),
    include_bytes!("../../resources/plugins/list.ftplugin"),
    include_bytes!("../../resources/plugins/location.ftplugin"),
    include_bytes!("../../resources/plugins/notes.ftplugin"),
    include_bytes!("../../resources/plugins/poll.ftplugin"),
    include_bytes!("../../resources/plugins/sign.ftplugin"),
    include_bytes!("../../resources/plugins/split.ftplugin"),
    include_bytes!("../../resources/plugins/pdfviewer.ftplugin"),
    include_bytes!("../../resources/plugins/translate.ftplugin"),
    include_bytes!("../../resources/plugins/remind.ftplugin"),
    include_bytes!("../../resources/plugins/game.tictactoe.ftplugin"),
    include_bytes!("../../resources/plugins/game.fourinarow.ftplugin"),
    include_bytes!("../../resources/plugins/game.chess.ftplugin"),
    include_bytes!("../../resources/plugins/game.backgammon.ftplugin"),
    include_bytes!("../../resources/plugins/game.checkers.ftplugin"),
    include_bytes!("../../resources/plugins/game.dotsandboxes.ftplugin"),
    include_bytes!("../../resources/plugins/game.eights.ftplugin"),
    include_bytes!("../../resources/plugins/game.gomoku.ftplugin"),
    include_bytes!("../../resources/plugins/game.mancala.ftplugin"),
    include_bytes!("../../resources/plugins/game.reversi.ftplugin"),
    include_bytes!("../../resources/plugins/game.seabattle.ftplugin"),
    include_bytes!("../../resources/plugins/game.wordduel.ftplugin"),
    include_bytes!("../../resources/plugins/game.wordgrid.ftplugin"),
];

#[cfg(test)]
const SEED_LIMIT: u64 = 4 * 1024 * 1024;
#[cfg(test)]
const SEEDS_LIMIT: u64 = 8 * 1024 * 1024;

#[cfg(test)]
fn seeds_weight_problem(weights: impl IntoIterator<Item = u64>) -> Option<String> {
    let mut total = 0;
    for weight in weights {
        if weight > SEED_LIMIT {
            return Some(format!(
                "a plugin of {weight} bytes is a download, not a seed"
            ));
        }
        total += weight;
    }
    (total > SEEDS_LIMIT)
        .then(|| format!("the seeds weigh {total} bytes, which is no longer little"))
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OfferedPlugin {
    id: String,
    name: String,
    version: String,
    summary: String,
    size: u64,
    installed: bool,
    carried: bool,
    kind: ft_plugins::Kind,
    locales: BTreeMap<String, ft_plugins::Localized>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    icon: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    image: String,
}

fn newer(version: &str, than: &str) -> bool {
    let parts = |version: &str| {
        version
            .split('.')
            .map(|part| part.parse().unwrap_or(0))
            .collect::<Vec<u32>>()
    };
    parts(version) > parts(than)
}

fn seeds() -> Vec<OfferedPlugin> {
    BUNDLED_PLUGINS
        .iter()
        .filter_map(|package| {
            let plugin = ft_plugins::open(package, &ft_plugins::catalogue()).ok()?;
            let image = plugin.image();
            Some(offered_from(plugin.manifest, image, package.len() as u64))
        })
        .collect()
}

fn offered_from(manifest: ft_plugins::Manifest, image: String, size: u64) -> OfferedPlugin {
    OfferedPlugin {
        id: manifest.id,
        name: manifest.name,
        version: manifest.version,
        summary: manifest.summary,
        size,
        installed: false,
        carried: true,
        kind: manifest.kind,
        locales: manifest.locales,
        icon: manifest.icon,
        image,
    }
}

fn merged(
    carried: Vec<OfferedPlugin>,
    listed: &[ft_plugins::CatalogueEntry],
    here: &[String],
) -> Vec<OfferedPlugin> {
    let mut offered = carried;
    for entry in listed {
        let listed = OfferedPlugin {
            id: entry.id.clone(),
            name: entry.name.clone(),
            version: entry.version.clone(),
            summary: entry.summary.clone(),
            size: entry.size,
            installed: false,
            carried: false,
            kind: entry.kind,
            locales: entry.locales.clone(),
            icon: entry.icon.clone(),
            image: entry.image.clone(),
        };
        match offered.iter_mut().find(|one| one.id == entry.id) {
            Some(seed) if newer(&entry.version, &seed.version) => *seed = listed,
            Some(_) => {}
            None => offered.push(listed),
        }
    }
    for one in &mut offered {
        one.installed = here.contains(&one.id);
    }
    offered.sort_by(|a, b| a.name.cmp(&b.name));
    offered
}

fn seed_package(id: &str) -> Option<&'static [u8]> {
    BUNDLED_PLUGINS
        .iter()
        .find(|package| {
            ft_plugins::open(package, &ft_plugins::catalogue())
                .is_ok_and(|plugin| plugin.manifest.id == id)
        })
        .copied()
}

/// App Store rule 4.7, iOS downloads nothing.
fn downloads() -> bool {
    !cfg!(target_os = "ios")
}

#[tauri::command]
pub async fn core_catalogue(
    app: AppHandle,
    client: State<'_, Client>,
) -> Result<Vec<OfferedPlugin>, String> {
    let core = client.core().await?;
    let here: Vec<String> = core
        .plugins()
        .await
        .map_err(failed)?
        .into_iter()
        .map(|plugin| plugin.manifest.id)
        .collect();
    let listed = match downloads() {
        true => core
            .catalogue(client.web(), &ft_plugins::catalogue())
            .await
            .unwrap_or_default(),
        false => Vec::new(),
    };
    if !listed.is_empty() {
        update_downloaded(&app, core.clone(), listed.clone());
    }
    Ok(merged(seeds(), &listed, &here))
}

#[tauri::command]
pub async fn core_plugin_add(
    plugin: String,
    app: AppHandle,
    client: State<'_, Client>,
) -> Result<(), String> {
    let core = client.core().await?;
    let seed = seed_package(&plugin);
    let listed = match downloads() {
        true => core
            .catalogue(client.web(), &ft_plugins::catalogue())
            .await
            .unwrap_or_default(),
        false => Vec::new(),
    };
    let wanted = listed.iter().find(|entry| entry.id == plugin);
    let carried_version =
        seed.and_then(|package| ft_plugins::open(package, &ft_plugins::catalogue()).ok());

    match (wanted, seed) {
        (Some(entry), _)
            if carried_version
                .as_ref()
                .is_none_or(|carried| newer(&entry.version, &carried.manifest.version)) =>
        {
            core.add_plugin(entry, client.web(), &ft_plugins::catalogue())
                .await
                .map_err(failed)?;
        }
        (_, Some(package)) => {
            core.install_plugin(
                package,
                &ft_plugins::catalogue(),
                ft_plugins::Permissions::default(),
            )
            .await
            .map_err(failed)?;
        }
        _ => return Err("that tool is not offered here".to_owned()),
    }
    refresh_served_plugins(&app, &core, client.dir()?).await;
    Ok(())
}

#[derive(Default)]
pub struct OpenPlugins(std::sync::Mutex<HashMap<String, u32>>);

impl OpenPlugins {
    pub fn set(&self, id: &str, open: bool) {
        let mut frames = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let count = frames.entry(id.to_owned()).or_default();
        *count = if open {
            *count + 1
        } else {
            count.saturating_sub(1)
        };
        if *count == 0 {
            frames.remove(id);
        }
    }

    pub fn ids(&self) -> std::collections::HashSet<String> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .keys()
            .cloned()
            .collect()
    }
}

#[tauri::command]
pub async fn core_plugin_open(
    plugin: String,
    open: bool,
    frames: State<'_, OpenPlugins>,
    client: State<'_, Client>,
) -> Result<(), String> {
    if open {
        client
            .core()
            .await?
            .open_plugin(&plugin)
            .await
            .map_err(failed)?;
    }
    frames.set(&plugin, open);
    Ok(())
}

const UPDATE_EVERY_MS: i64 = 12 * 3_600_000;
const UPDATE_LOOK: &str = "plugin-updates.at";

fn updates_due(last: Option<i64>, now: i64) -> bool {
    last.is_none_or(|last| now - last >= UPDATE_EVERY_MS || last > now)
}

fn last_update_look(dir: &Path) -> Option<i64> {
    std::fs::read_to_string(dir.join(UPDATE_LOOK))
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn looked_for_updates(dir: &Path, now: i64) {
    let _ = std::fs::write(dir.join(UPDATE_LOOK), now.to_string());
}

fn update_downloaded(
    app: &AppHandle,
    core: Arc<ft_core::Core>,
    listed: Vec<ft_plugins::CatalogueEntry>,
) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let client = app.state::<Client>();
        let open = app.state::<OpenPlugins>().ids();
        let updated = core
            .update_plugins(&listed, client.web(), &ft_plugins::catalogue(), &open)
            .await;
        if !updated.is_empty() {
            if let Ok(dir) = client.dir() {
                refresh_served_plugins(&app, &core, dir).await;
            }
        }
    });
}

pub(super) fn look_for_updates(app: &AppHandle, core: Arc<ft_core::Core>, dir: &Path) {
    if !downloads() || !updates_due(last_update_look(dir), now_ms()) {
        return;
    }
    let (app, dir) = (app.clone(), dir.to_path_buf());
    tauri::async_runtime::spawn(async move {
        let listed = {
            let client = app.state::<Client>();
            core.catalogue(client.web(), &ft_plugins::catalogue()).await
        };
        let Ok(listed) = listed else { return };
        looked_for_updates(&dir, now_ms());
        update_downloaded(&app, core, listed);
    });
}

pub(super) async fn update_installed_plugins(core: &Arc<ft_core::Core>) {
    let installed = core.plugins().await.unwrap_or_default();
    for package in BUNDLED_PLUGINS {
        let Ok(plugin) = ft_plugins::open(package, &ft_plugins::catalogue()) else {
            continue;
        };
        let Some(known) = installed
            .iter()
            .find(|one| one.manifest.id == plugin.manifest.id)
        else {
            continue;
        };
        if !newer(&plugin.manifest.version, &known.manifest.version) {
            continue;
        }
        let _ = core
            .install_plugin(package, &ft_plugins::catalogue(), known.granted.clone())
            .await;
    }
}

#[tauri::command]
pub async fn core_plugins_opening(
    mime: String,
    client: State<'_, Client>,
) -> Result<Vec<PluginView>, String> {
    let core = client.core().await?;
    let opening = core.plugins_opening(&mime).await.map_err(failed)?;
    Ok(core
        .plugins()
        .await
        .map_err(failed)?
        .into_iter()
        .filter(|plugin| {
            opening
                .iter()
                .any(|manifest| manifest.id == plugin.manifest.id)
        })
        .map(plugin_view)
        .collect())
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod view_tests;
