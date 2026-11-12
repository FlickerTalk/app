use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use super::{failed, now_ms, Client};

pub async fn refresh_served_plugins(app: &AppHandle, core: &Arc<ft_core::Core>, dir: &Path) {
    let mut plugins = Vec::new();
    for plugin in core.plugins().await.unwrap_or_default() {
        let open = core.may_use_plugin(&plugin.manifest.id, plugin.manifest.kind).await.unwrap_or(false);
        plugins.push((plugin, open));
    }
    app.state::<crate::plugins::Plugins>().set(served_of(plugins, dir));
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
        let Some(component) = plugin.manifest.components.first().cloned() else { continue };
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
            storage: if granted.storage == "large" { ft_plugins::Storage::Large } else { ft_plugins::Storage::Small },
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
    core.grant_plugin(&plugin, permissions).await.map_err(failed)?;
    refresh_served_plugins(&app, &core, client.dir()?).await;
    Ok(())
}

#[tauri::command]
pub async fn core_plugin_remove(plugin: String, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
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
            return Some(format!("a plugin of {weight} bytes is a download, not a seed"));
        }
        total += weight;
    }
    (total > SEEDS_LIMIT).then(|| format!("the seeds weigh {total} bytes, which is no longer little"))
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
    let parts = |version: &str| version.split('.').map(|part| part.parse().unwrap_or(0)).collect::<Vec<u32>>();
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

fn merged(carried: Vec<OfferedPlugin>, listed: &[ft_plugins::CatalogueEntry], here: &[String]) -> Vec<OfferedPlugin> {
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
            ft_plugins::open(package, &ft_plugins::catalogue()).is_ok_and(|plugin| plugin.manifest.id == id)
        })
        .copied()
}

/// App Store rule 4.7, iOS downloads nothing.
fn downloads() -> bool {
    !cfg!(target_os = "ios")
}

#[tauri::command]
pub async fn core_catalogue(app: AppHandle, client: State<'_, Client>) -> Result<Vec<OfferedPlugin>, String> {
    let core = client.core().await?;
    let here: Vec<String> =
        core.plugins().await.map_err(failed)?.into_iter().map(|plugin| plugin.manifest.id).collect();
    let listed = match downloads() {
        true => core.catalogue(client.web(), &ft_plugins::catalogue()).await.unwrap_or_default(),
        false => Vec::new(),
    };
    if !listed.is_empty() {
        update_downloaded(&app, core.clone(), listed.clone());
    }
    Ok(merged(seeds(), &listed, &here))
}

#[tauri::command]
pub async fn core_plugin_add(plugin: String, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let core = client.core().await?;
    let seed = seed_package(&plugin);
    let listed = match downloads() {
        true => core.catalogue(client.web(), &ft_plugins::catalogue()).await.unwrap_or_default(),
        false => Vec::new(),
    };
    let wanted = listed.iter().find(|entry| entry.id == plugin);
    let carried_version = seed.and_then(|package| ft_plugins::open(package, &ft_plugins::catalogue()).ok());

    match (wanted, seed) {
        (Some(entry), _)
            if carried_version.as_ref().is_none_or(|carried| newer(&entry.version, &carried.manifest.version)) =>
        {
            core.add_plugin(entry, client.web(), &ft_plugins::catalogue()).await.map_err(failed)?;
        }
        (_, Some(package)) => {
            core.install_plugin(package, &ft_plugins::catalogue(), ft_plugins::Permissions::default())
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
        let mut frames = self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let count = frames.entry(id.to_owned()).or_default();
        *count = if open { *count + 1 } else { count.saturating_sub(1) };
        if *count == 0 {
            frames.remove(id);
        }
    }

    pub fn ids(&self) -> std::collections::HashSet<String> {
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).keys().cloned().collect()
    }
}

#[tauri::command]
pub async fn core_plugin_open(plugin: String, open: bool, frames: State<'_, OpenPlugins>, client: State<'_, Client>) -> Result<(), String> {
    if open {
        client.core().await?.open_plugin(&plugin).await.map_err(failed)?;
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
    std::fs::read_to_string(dir.join(UPDATE_LOOK)).ok()?.trim().parse().ok()
}

fn looked_for_updates(dir: &Path, now: i64) {
    let _ = std::fs::write(dir.join(UPDATE_LOOK), now.to_string());
}

fn update_downloaded(app: &AppHandle, core: Arc<ft_core::Core>, listed: Vec<ft_plugins::CatalogueEntry>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let client = app.state::<Client>();
        let open = app.state::<OpenPlugins>().ids();
        let updated = core.update_plugins(&listed, client.web(), &ft_plugins::catalogue(), &open).await;
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
        let Ok(plugin) = ft_plugins::open(package, &ft_plugins::catalogue()) else { continue };
        let Some(known) = installed.iter().find(|one| one.manifest.id == plugin.manifest.id) else { continue };
        if !newer(&plugin.manifest.version, &known.manifest.version) {
            continue;
        }
        let _ = core.install_plugin(package, &ft_plugins::catalogue(), known.granted.clone()).await;
    }
}

#[tauri::command]
pub async fn core_plugins_opening(mime: String, client: State<'_, Client>) -> Result<Vec<PluginView>, String> {
    let core = client.core().await?;
    let opening = core.plugins_opening(&mime).await.map_err(failed)?;
    Ok(core
        .plugins()
        .await
        .map_err(failed)?
        .into_iter()
        .filter(|plugin| opening.iter().any(|manifest| manifest.id == plugin.manifest.id))
        .map(plugin_view)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closed_tool_is_not_served_and_a_game_is() {
        let plugin = |id: &str, kind: &str| ft_core::plugins::InstalledPlugin {
            manifest: serde_json::from_str(&format!(
                r#"{{"id":"{id}","name":"X","version":"1.0.0","minCoreVersion":"0.1.0","components":["ft-x"],"kind":"{kind}"}}"#
            ))
            .unwrap(),
            granted: ft_plugins::Permissions::default(),
            installed_at: 0,
            image: String::new(),
        };
        let dir = Path::new("/tmp/ft");
        let served = served_of(vec![(plugin("com.example.code", "tool"), false), (plugin("game.example.chess", "game"), true)], dir);
        assert_eq!(served.keys().collect::<Vec<_>>(), ["game.example.chess"]);
        assert_eq!(served["game.example.chess"].dir, dir.join("plugins").join("game.example.chess"));
        let served = served_of(vec![(plugin("com.example.code", "tool"), true)], dir);
        assert!(served.contains_key("com.example.code"), "open in the free year or with the subscription");
    }

    #[test]
    fn the_location_permission_goes_to_the_screen_and_back() {
        let asked = ft_plugins::Permissions { location: true, ..ft_plugins::Permissions::default() };
        let shown = serde_json::to_value(PermissionsView::from(&asked)).unwrap();
        assert_eq!(shown["location"], serde_json::json!(true), "{shown}");
        let switched: PermissionsView = serde_json::from_value(serde_json::json!({
            "network": [], "messages": false, "send": "nothing", "location": true
        }))
        .unwrap();
        assert!(ft_plugins::Permissions::from(switched).location);
        let silent: PermissionsView = serde_json::from_value(serde_json::json!({ "network": [], "messages": false, "send": "nothing" })).unwrap();
        assert!(!ft_plugins::Permissions::from(silent).location);
    }

    #[test]
    fn the_translate_permission_goes_to_the_screen_and_back() {
        let asked = ft_plugins::Permissions { translate: true, ..ft_plugins::Permissions::default() };
        let shown = serde_json::to_value(PermissionsView::from(&asked)).unwrap();
        assert_eq!(shown["translate"], serde_json::json!(true), "{shown}");
        let switched: PermissionsView = serde_json::from_value(serde_json::json!({
            "network": [], "messages": false, "send": "nothing", "translate": true
        }))
        .unwrap();
        assert!(ft_plugins::Permissions::from(switched).translate);
        let silent: PermissionsView = serde_json::from_value(serde_json::json!({ "network": [], "messages": false, "send": "nothing" })).unwrap();
        assert!(!ft_plugins::Permissions::from(silent).translate);
    }

    #[test]
    fn what_travels_inside_the_app_stays_tiny() {
        for package in BUNDLED_PLUGINS {
            let plugin = ft_plugins::open(package, &ft_plugins::catalogue()).expect("a seed is not signed for us");
            assert!(!plugin.manifest.components.is_empty(), "{} shows nothing", plugin.manifest.id);
            assert!(plugin.file("dist/index.js").is_some(), "{} has no code", plugin.manifest.id);
        }
        let weights = BUNDLED_PLUGINS.iter().map(|package| package.len() as u64);
        assert_eq!(seeds_weight_problem(weights), None);
    }

    #[test]
    fn every_seed_runs_on_this_core() {
        for package in BUNDLED_PLUGINS {
            let manifest = ft_plugins::open(package, &ft_plugins::catalogue()).expect("a seed is signed for us").manifest;
            assert!(
                ft_plugins::version_at_least(ft_core::plugins::CORE_VERSION, &manifest.min_core_version),
                "{} needs FlickerTalk {}",
                manifest.id,
                manifest.min_core_version
            );
        }
    }

    #[test]
    fn a_heavy_seed_is_a_download() {
        assert!(seeds_weight_problem([5 * 1024 * 1024]).is_some(), "a seed of 5 MiB is refused");
        assert_eq!(seeds_weight_problem([3 * 1024 * 1024 + 512 * 1024]), None, "the PDF viewer travels");
        assert!(seeds_weight_problem([SEED_LIMIT + 1]).is_some(), "a seed just over the limit is refused");
        assert_eq!(seeds_weight_problem([SEED_LIMIT]), None, "a seed at the limit travels");
        let many = vec![SEED_LIMIT; (SEEDS_LIMIT / SEED_LIMIT) as usize + 1];
        assert!(seeds_weight_problem(many).is_some(), "seeds that are light one by one but heavy together are refused");
    }

    #[test]
    #[ignore = "needs the web repo's catalogue on disk"]
    fn every_seed_is_the_package_the_catalogue_serves() {
        let served = std::env::var("FT_CATALOGUE_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/site/plugins"));
        for package in BUNDLED_PLUGINS {
            let manifest = ft_plugins::open(package, &ft_plugins::catalogue()).expect("a seed is signed for us").manifest;
            let path = served.join(&manifest.id).join(format!("{}.ftplugin", manifest.version));
            let listed = std::fs::read(&path).unwrap_or_else(|_| panic!("{} {} is not in the catalogue ({})", manifest.id, manifest.version, path.display()));
            assert!(listed == *package, "the seed of {} {} is not the package the catalogue serves", manifest.id, manifest.version);
        }
    }

    #[test]
    fn a_seed_that_writes_in_the_chat_asks_for_it() {
        for package in BUNDLED_PLUGINS {
            let plugin = ft_plugins::open(package, &ft_plugins::catalogue()).expect("a seed is signed for us");
            let code = String::from_utf8_lossy(plugin.file("dist/index.js").unwrap_or_default());
            let writes = ["ft.send(", "ft?.send(", "ft.say(", "ft?.say("].iter().any(|call| code.contains(call));
            if writes {
                assert_ne!(plugin.manifest.permissions.send, ft_plugins::Sending::Nothing, "{} writes in the chat without asking", plugin.manifest.id);
            }
        }
    }

    #[test]
    fn the_app_carries_its_tools_and_games() {
        const TOOLS: [&str; 19] = [
            "markdown", "images", "pdf", "redact", "sketch", "scanner", "countdown", "board", "clean", "drive", "list", "location",
            "notes", "poll", "sign", "split", "pdfviewer", "translate", "remind",
        ];
        const GAMES: [&str; 13] = [
            "tictactoe", "fourinarow", "chess", "backgammon", "checkers", "dotsandboxes", "eights", "gomoku", "mancala", "reversi",
            "seabattle", "wordduel", "wordgrid",
        ];
        let offered = seeds();
        let wanted = TOOLS
            .iter()
            .map(|tool| (format!("com.flickertalk.{tool}"), ft_plugins::Kind::Tool))
            .chain(GAMES.iter().map(|game| (format!("com.flickertalk.game.{game}"), ft_plugins::Kind::Game)));
        for (id, kind) in wanted {
            let seed = offered.iter().find(|one| one.id == id).unwrap_or_else(|| panic!("{id} is not carried"));
            assert_eq!(seed.kind, kind, "{id} is a {kind:?}");
            assert!(seed.carried, "{id} is inside the app");
        }
        assert_eq!(offered.len(), TOOLS.len() + GAMES.len(), "every package the app carries is one it means to carry");
    }

    #[test]
    fn every_seed_speaks_the_app_s_languages() {
        const LANGUAGES: [&str; 20] =
            ["es", "pt", "fr", "de", "it", "ro", "ru", "uk", "pl", "tr", "ar", "hi", "bn", "id", "vi", "th", "ja", "ko", "zh-CN", "zh-TW"];
        const KEEP_THEIR_NAME: [&str; 2] = ["com.flickertalk.markdown", "com.flickertalk.pdf"];
        for package in BUNDLED_PLUGINS {
            let manifest = ft_plugins::open(package, &ft_plugins::catalogue()).expect("a seed is signed for us").manifest;
            let mut codes: Vec<&str> = manifest.locales.keys().map(String::as_str).collect();
            let mut wanted = LANGUAGES.to_vec();
            codes.sort_unstable();
            wanted.sort_unstable();
            assert_eq!(codes, wanted, "{} speaks the app's languages", manifest.id);
            for code in LANGUAGES {
                let said = &manifest.locales[code];
                assert!(said.summary.as_deref().is_some_and(|summary| !summary.trim().is_empty()), "{} has no summary in {code}", manifest.id);
                let named = said.name.as_deref().is_some_and(|name| !name.trim().is_empty());
                assert_eq!(named, !KEEP_THEIR_NAME.contains(&manifest.id.as_str()), "{}: its name in {code}", manifest.id);
            }
        }
    }

    fn entry(id: &str, version: &str) -> ft_plugins::CatalogueEntry {
        ft_plugins::CatalogueEntry {
            id: id.to_owned(),
            name: "Tool".to_owned(),
            version: version.to_owned(),
            min_core_version: "0.1.0".to_owned(),
            size: 10,
            hash: "ab".repeat(32),
            url: format!("{}/{id}/{version}.ftplugin", ft_core::CATALOGUE_HOME),
            summary: "Does a thing.".to_owned(),
            kind: ft_plugins::Kind::Tool,
            locales: Default::default(),
            icon: String::new(),
            image: String::new(),
        }
    }

    fn carried(id: &str, version: &str) -> OfferedPlugin {
        OfferedPlugin {
            id: id.to_owned(),
            name: "Tool".to_owned(),
            version: version.to_owned(),
            summary: "Does a thing.".to_owned(),
            size: 4096,
            installed: false,
            carried: true,
            kind: ft_plugins::Kind::Tool,
            locales: Default::default(),
            icon: String::new(),
            image: String::new(),
        }
    }

    #[test]
    fn the_list_joins_what_is_carried_and_what_is_offered_keeping_the_newer() {
        let seeds = vec![carried("com.flickertalk.sketch", "1.0.0"), carried("com.flickertalk.pdf", "2.0.0")];
        let listed = vec![entry("com.flickertalk.sketch", "1.1.0"), entry("com.flickertalk.ocr", "1.0.0")];

        let offered = merged(seeds, &listed, &[]);
        let by_id = |id: &str| offered.iter().find(|one| one.id == id).expect("listed").clone();

        assert_eq!(by_id("com.flickertalk.sketch").version, "1.1.0", "the catalogue has a newer one");
        assert!(!by_id("com.flickertalk.sketch").carried, "so it is a download");
        assert_eq!(by_id("com.flickertalk.pdf").version, "2.0.0", "nothing newer is offered");
        assert!(by_id("com.flickertalk.pdf").carried);
        assert_eq!(by_id("com.flickertalk.ocr").size, 10, "what is only in the catalogue is a download");
        assert!(!by_id("com.flickertalk.ocr").carried);
        assert_eq!(offered.len(), 3);
    }

    #[test]
    fn the_list_keeps_whether_each_one_is_a_tool_or_a_game() {
        let seeds = vec![carried("com.flickertalk.sketch", "1.0.0")];
        let chess = ft_plugins::CatalogueEntry { kind: ft_plugins::Kind::Game, ..entry("com.flickertalk.game.chess", "1.0.0") };
        let offered = serde_json::to_value(merged(seeds, &[chess], &[])).unwrap();
        let kinds: Vec<(&str, &str)> = offered
            .as_array()
            .unwrap()
            .iter()
            .map(|one| (one["id"].as_str().unwrap(), one["kind"].as_str().unwrap_or("missing")))
            .collect();
        assert!(kinds.contains(&("com.flickertalk.game.chess", "game")), "{kinds:?}");
        assert!(kinds.contains(&("com.flickertalk.sketch", "tool")), "{kinds:?}");
    }

    #[test]
    fn an_installed_plugin_says_whether_it_is_a_game() {
        let manifest: ft_plugins::Manifest = serde_json::from_str(
            r#"{"id":"com.flickertalk.game.chess","name":"Chess","version":"1.0.0","minCoreVersion":"1.3.0","components":["ft-chess"],"kind":"game","permissions":{"live":true}}"#,
        )
        .unwrap();
        let view = plugin_view(ft_core::plugins::InstalledPlugin { manifest, granted: Default::default(), installed_at: 7, image: String::new() });
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(json["kind"], "game");
        assert_eq!((json["id"].as_str(), json["installedAt"].as_i64()), (Some("com.flickertalk.game.chess"), Some(7)));
        assert_eq!((json["asks"]["live"].as_bool(), json["granted"]["live"].as_bool()), (Some(true), Some(false)));
    }

    #[test]
    fn every_view_of_a_plugin_keeps_its_translations() {
        let manifest: ft_plugins::Manifest = serde_json::from_str(
            r#"{"id":"com.flickertalk.list","name":"List","version":"1.0.0","minCoreVersion":"1.3.0","components":["ft-list"],"summary":"A list.","locales":{"es":{"name":"Listas","summary":"Una lista."}}}"#,
        )
        .unwrap();
        let view = plugin_view(ft_core::plugins::InstalledPlugin { manifest: manifest.clone(), granted: Default::default(), installed_at: 7, image: String::new() });
        assert_eq!(serde_json::to_value(&view).unwrap()["locales"]["es"]["name"], "Listas");

        let seed = serde_json::to_value(offered_from(manifest, String::new(), 4096)).unwrap();
        assert_eq!((seed["locales"]["es"]["summary"].as_str(), seed["carried"].as_bool()), (Some("Una lista."), Some(true)));

        let spanish = |name: &str| {
            [("es".to_owned(), ft_plugins::Localized { name: Some(name.to_owned()), ..Default::default() })].into_iter().collect()
        };
        let seeds = vec![
            OfferedPlugin { locales: spanish("Dibujo"), ..carried("com.flickertalk.sketch", "1.0.0") },
            OfferedPlugin { locales: spanish("Imagen"), ..carried("com.flickertalk.images", "2.0.0") },
        ];
        let listed = vec![
            ft_plugins::CatalogueEntry { locales: spanish("Boceto"), ..entry("com.flickertalk.sketch", "1.1.0") },
            ft_plugins::CatalogueEntry { locales: spanish("Foto"), ..entry("com.flickertalk.images", "1.0.0") },
            ft_plugins::CatalogueEntry { locales: spanish("Ajedrez"), ..entry("com.flickertalk.game.chess", "1.0.0") },
        ];
        let offered = serde_json::to_value(merged(seeds, &listed, &[])).unwrap();
        let spanish_of = |id: &str| {
            let one = offered.as_array().unwrap().iter().find(|one| one["id"] == id).unwrap();
            one["locales"]["es"]["name"].as_str().unwrap_or("missing").to_owned()
        };
        assert_eq!(spanish_of("com.flickertalk.sketch"), "Boceto", "the catalogue's is newer");
        assert_eq!(spanish_of("com.flickertalk.images"), "Imagen", "the app's is newer");
        assert_eq!(spanish_of("com.flickertalk.game.chess"), "Ajedrez", "only in the catalogue");
    }

    #[test]
    fn an_installed_tool_says_which_hooks_it_binds_to() {
        let manifest: ft_plugins::Manifest = serde_json::from_str(
            r#"{"id":"com.flickertalk.translate","name":"Translator","version":"1.1.0","minCoreVersion":"1.7.0","components":["ft-translate"],"permissions":{"messages":"given","send":"propose"},"hooks":{"message":{"label":"Translate","icon":"language-outline"},"settings":{"label":"Translation","icon":"language-outline","fields":[{"key":"lang","type":"select","label":"Language"},{"key":"receive","type":"toggle","label":"Translate what I receive"}],"note":"Packs are downloaded once."}},"locales":{"es":{"name":"Traductor","hooks":{"message":"Traducir","settings":{"label":"Traducción","fields":{"lang":"Idioma"}}}}}}"#,
        )
        .unwrap();
        let plain = ft_plugins::Manifest { hooks: Default::default(), ..manifest.clone() };
        let view = |manifest: ft_plugins::Manifest| {
            serde_json::to_value(plugin_view(ft_core::plugins::InstalledPlugin { manifest, granted: Default::default(), installed_at: 7, image: String::new() })).unwrap()
        };
        let json = view(manifest);
        assert_eq!(
            json["hooks"],
            serde_json::json!({
                "message": { "label": "Translate", "icon": "language-outline" },
                "settings": {
                    "label": "Translation",
                    "icon": "language-outline",
                    "fields": [
                        { "key": "lang", "type": "select", "label": "Language" },
                        { "key": "receive", "type": "toggle", "label": "Translate what I receive" }
                    ],
                    "note": "Packs are downloaded once."
                }
            })
        );
        assert_eq!(json["locales"]["es"]["hooks"]["settings"]["fields"]["lang"], "Idioma");
        assert!(view(plain).get("hooks").is_none(), "bound to none, nothing written");
    }

    #[test]
    fn every_view_of_a_plugin_keeps_its_icon() {
        let drawn: ft_plugins::Manifest = serde_json::from_str(
            r#"{"id":"com.flickertalk.list","name":"List","version":"1.0.0","minCoreVersion":"1.3.0","components":["ft-list"],"icon":"list-outline"}"#,
        )
        .unwrap();
        let plain = ft_plugins::Manifest { icon: String::new(), ..drawn.clone() };
        let installed = |manifest: &ft_plugins::Manifest| {
            serde_json::to_value(plugin_view(ft_core::plugins::InstalledPlugin { manifest: manifest.clone(), granted: Default::default(), installed_at: 7, image: String::new() }))
                .unwrap()
        };
        assert_eq!(installed(&drawn)["icon"], "list-outline");
        assert!(installed(&plain).get("icon").is_none(), "no icon, nothing written");

        let seed = serde_json::to_value(offered_from(drawn, String::new(), 4096)).unwrap();
        assert_eq!(seed["icon"], "list-outline");
        assert!(serde_json::to_value(offered_from(plain, String::new(), 4096)).unwrap().get("icon").is_none());

        let listed = vec![
            ft_plugins::CatalogueEntry { icon: "brush-outline".to_owned(), ..entry("com.flickertalk.sketch", "1.1.0") },
            entry("com.flickertalk.pdf", "1.0.0"),
        ];
        let offered = serde_json::to_value(merged(vec![], &listed, &[])).unwrap();
        let icon_of = |id: &str| offered.as_array().unwrap().iter().find(|one| one["id"] == id).unwrap().get("icon").cloned();
        assert_eq!(icon_of("com.flickertalk.sketch"), Some(serde_json::json!("brush-outline")));
        assert_eq!(icon_of("com.flickertalk.pdf"), None);
    }

    #[test]
    fn every_view_of_a_plugin_keeps_its_image() {
        const IMAGE: &str = r#"<svg viewBox="0 0 64 64"><rect width="64" height="64"/></svg>"#;
        let manifest: ft_plugins::Manifest = serde_json::from_str(
            r#"{"id":"com.flickertalk.list","name":"List","version":"1.0.0","minCoreVersion":"1.3.0","components":["ft-list"]}"#,
        )
        .unwrap();
        let installed = |image: &str| {
            serde_json::to_value(plugin_view(ft_core::plugins::InstalledPlugin {
                manifest: manifest.clone(),
                granted: Default::default(),
                installed_at: 7,
                image: image.to_owned(),
            }))
            .unwrap()
        };
        assert_eq!(installed(IMAGE)["image"], IMAGE);
        assert!(installed("").get("image").is_none(), "no image, nothing written");
        assert_eq!(serde_json::to_value(offered_from(manifest.clone(), IMAGE.to_owned(), 4096)).unwrap()["image"], IMAGE);
        let listed = vec![
            ft_plugins::CatalogueEntry { image: IMAGE.to_owned(), ..entry("com.flickertalk.sketch", "1.1.0") },
            entry("com.flickertalk.pdf", "1.0.0"),
        ];
        let offered = serde_json::to_value(merged(vec![], &listed, &[])).unwrap();
        let image_of = |id: &str| offered.as_array().unwrap().iter().find(|one| one["id"] == id).unwrap().get("image").cloned();
        assert_eq!(image_of("com.flickertalk.sketch"), Some(serde_json::json!(IMAGE)));
        assert_eq!(image_of("com.flickertalk.pdf"), None);
    }

    #[test]
    fn a_tool_already_here_shows_as_installed() {
        let here = ["com.flickertalk.sketch".to_owned()];
        let offered = merged(vec![carried("com.flickertalk.sketch", "1.0.0")], &[], &here);
        assert!(offered[0].installed);
    }

    #[test]
    fn a_plugin_is_open_until_every_frame_of_it_has_closed() {
        let open = OpenPlugins::default();
        let ids = |open: &OpenPlugins| {
            let mut ids: Vec<String> = open.ids().into_iter().collect();
            ids.sort();
            ids
        };
        open.set("com.example.a", true);
        open.set("com.example.a", true);
        open.set("com.example.b", true);
        open.set("com.example.a", false);
        assert_eq!(ids(&open), ["com.example.a", "com.example.b"], "one frame of it is still there");
        open.set("com.example.a", false);
        open.set("com.example.a", false);
        assert_eq!(ids(&open), ["com.example.b"]);
        open.set("com.example.a", true);
        assert_eq!(ids(&open), ["com.example.a", "com.example.b"]);
    }

    #[test]
    fn updates_are_looked_for_at_most_every_twelve_hours() {
        let now = 1_800_000_000_000_i64;
        let hour = 3_600_000;
        assert!(updates_due(None, now), "never looked");
        assert!(!updates_due(Some(now - 11 * hour), now));
        assert!(updates_due(Some(now - 12 * hour), now));
        assert!(updates_due(Some(now + hour), now), "the clock went back");

        let dir = std::env::temp_dir().join(format!("ft-updates-{now}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(last_update_look(&dir), None);
        looked_for_updates(&dir, now);
        assert_eq!(last_update_look(&dir), Some(now));
    }

    #[test]
    fn a_version_is_newer_only_when_it_really_is() {
        assert!(newer("1.1.0", "1.0.9"));
        assert!(newer("1.0.10", "1.0.9"));
        assert!(newer("2.0.0", "1.9.9"));
        assert!(!newer("1.0.0", "1.0.0"));
        assert!(!newer("1.0.0", "1.0.1"));
    }
}
