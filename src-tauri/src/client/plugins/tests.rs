use super::*;

fn plugin(id: &str, kind: &str) -> ft_core::plugins::InstalledPlugin {
    let manifest = format!(
        r#"{{"id":"{id}","name":"X","version":"1.0.0","minCoreVersion":"0.1.0","components":["ft-x"],"kind":"{kind}"}}"#
    );
    ft_core::plugins::InstalledPlugin {
        manifest: serde_json::from_str(&manifest).unwrap(),
        granted: ft_plugins::Permissions::default(),
        installed_at: 0,
        image: String::new(),
    }
}

#[test]
fn a_closed_tool_is_not_served_and_a_game_is() {
    let dir = Path::new("/tmp/ft");
    let served = served_of(
        vec![
            (plugin("com.example.code", "tool"), false),
            (plugin("game.example.chess", "game"), true),
        ],
        dir,
    );
    assert_eq!(served.keys().collect::<Vec<_>>(), ["game.example.chess"]);
    assert_eq!(
        served["game.example.chess"].dir,
        dir.join("plugins").join("game.example.chess")
    );
    let served = served_of(vec![(plugin("com.example.code", "tool"), true)], dir);
    assert!(
        served.contains_key("com.example.code"),
        "open in the free year or with the subscription"
    );
}

#[test]
fn each_served_plugin_loads_from_its_own_folder_only() {
    let served = served_of(
        vec![
            (plugin("com.example.code", "tool"), true),
            (plugin("game.example.chess", "game"), true),
        ],
        Path::new("/tmp/ft"),
    );
    let chess = &served["game.example.chess"].policy;
    assert!(
        chess.contains("script-src http://ftplugin.localhost/game.example.chess/ "),
        "{chess}"
    );
    assert!(!chess.contains("com.example.code"), "{chess}");
    let code = &served["com.example.code"].policy;
    assert!(
        code.contains("ftplugin://localhost/com.example.code/"),
        "{code}"
    );
    assert!(!code.contains("game.example.chess"), "{code}");
}

#[test]
fn the_location_permission_goes_to_the_screen_and_back() {
    let asked = ft_plugins::Permissions {
        location: true,
        ..ft_plugins::Permissions::default()
    };
    let shown = serde_json::to_value(PermissionsView::from(&asked)).unwrap();
    assert_eq!(shown["location"], serde_json::json!(true), "{shown}");
    let switched: PermissionsView = serde_json::from_value(serde_json::json!({
        "network": [], "messages": false, "send": "nothing", "location": true
    }))
    .unwrap();
    assert!(ft_plugins::Permissions::from(switched).location);
    let silent: PermissionsView = serde_json::from_value(
        serde_json::json!({ "network": [], "messages": false, "send": "nothing" }),
    )
    .unwrap();
    assert!(!ft_plugins::Permissions::from(silent).location);
}

#[test]
fn the_translate_permission_goes_to_the_screen_and_back() {
    let asked = ft_plugins::Permissions {
        translate: true,
        ..ft_plugins::Permissions::default()
    };
    let shown = serde_json::to_value(PermissionsView::from(&asked)).unwrap();
    assert_eq!(shown["translate"], serde_json::json!(true), "{shown}");
    let switched: PermissionsView = serde_json::from_value(serde_json::json!({
        "network": [], "messages": false, "send": "nothing", "translate": true
    }))
    .unwrap();
    assert!(ft_plugins::Permissions::from(switched).translate);
    let silent: PermissionsView = serde_json::from_value(
        serde_json::json!({ "network": [], "messages": false, "send": "nothing" }),
    )
    .unwrap();
    assert!(!ft_plugins::Permissions::from(silent).translate);
}

#[test]
fn every_seed_runs_on_this_core() {
    for package in BUNDLED_PLUGINS {
        let manifest = ft_plugins::open(package, &ft_plugins::catalogue())
            .expect("a seed is signed for us")
            .manifest;
        assert!(
            ft_plugins::version_at_least(
                ft_core::plugins::CORE_VERSION,
                &manifest.min_core_version
            ),
            "{} needs FlickerTalk {}",
            manifest.id,
            manifest.min_core_version
        );
    }
}

#[test]
#[ignore = "needs the web repo's catalogue on disk"]
fn every_seed_is_the_package_the_catalogue_serves() {
    let served = std::env::var("FT_CATALOGUE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/site/plugins")
        });
    for package in BUNDLED_PLUGINS {
        let manifest = ft_plugins::open(package, &ft_plugins::catalogue())
            .expect("a seed is signed for us")
            .manifest;
        let path = served
            .join(&manifest.id)
            .join(format!("{}.ftplugin", manifest.version));
        let listed = std::fs::read(&path).unwrap_or_else(|_| {
            panic!(
                "{} {} is not in the catalogue ({})",
                manifest.id,
                manifest.version,
                path.display()
            )
        });
        assert!(
            listed == *package,
            "the seed of {} {} is not the package the catalogue serves",
            manifest.id,
            manifest.version
        );
    }
}

#[test]
fn a_seed_that_writes_in_the_chat_asks_for_it() {
    for package in BUNDLED_PLUGINS {
        let plugin =
            ft_plugins::open(package, &ft_plugins::catalogue()).expect("a seed is signed for us");
        let code = String::from_utf8_lossy(plugin.file("dist/index.js").unwrap_or_default());
        let writes = ["ft.send(", "ft?.send(", "ft.say(", "ft?.say("]
            .iter()
            .any(|call| code.contains(call));
        if writes {
            assert_ne!(
                plugin.manifest.permissions.send,
                ft_plugins::Sending::Nothing,
                "{} writes in the chat without asking",
                plugin.manifest.id
            );
        }
    }
}

#[test]
fn the_app_carries_its_tools_and_games() {
    const TOOLS: [&str; 19] = [
        "markdown",
        "images",
        "pdf",
        "redact",
        "sketch",
        "scanner",
        "countdown",
        "board",
        "clean",
        "drive",
        "list",
        "location",
        "notes",
        "poll",
        "sign",
        "split",
        "pdfviewer",
        "translate",
        "remind",
    ];
    const GAMES: [&str; 13] = [
        "tictactoe",
        "fourinarow",
        "chess",
        "backgammon",
        "checkers",
        "dotsandboxes",
        "eights",
        "gomoku",
        "mancala",
        "reversi",
        "seabattle",
        "wordduel",
        "wordgrid",
    ];
    let offered = seeds();
    let wanted = TOOLS
        .iter()
        .map(|tool| (format!("com.flickertalk.{tool}"), ft_plugins::Kind::Tool))
        .chain(GAMES.iter().map(|game| {
            (
                format!("com.flickertalk.game.{game}"),
                ft_plugins::Kind::Game,
            )
        }));
    for (id, kind) in wanted {
        let seed = offered
            .iter()
            .find(|one| one.id == id)
            .unwrap_or_else(|| panic!("{id} is not carried"));
        assert_eq!(seed.kind, kind, "{id} is a {kind:?}");
        assert!(seed.carried, "{id} is inside the app");
    }
    assert_eq!(
        offered.len(),
        TOOLS.len() + GAMES.len(),
        "every package the app carries is one it means to carry"
    );
}

#[test]
fn every_seed_speaks_the_app_s_languages() {
    const LANGUAGES: [&str; 20] = [
        "es", "pt", "fr", "de", "it", "ro", "ru", "uk", "pl", "tr", "ar", "hi", "bn", "id", "vi",
        "th", "ja", "ko", "zh-CN", "zh-TW",
    ];
    const KEEP_THEIR_NAME: [&str; 2] = ["com.flickertalk.markdown", "com.flickertalk.pdf"];
    for package in BUNDLED_PLUGINS {
        let manifest = ft_plugins::open(package, &ft_plugins::catalogue())
            .expect("a seed is signed for us")
            .manifest;
        let mut codes: Vec<&str> = manifest.locales.keys().map(String::as_str).collect();
        let mut wanted = LANGUAGES.to_vec();
        codes.sort_unstable();
        wanted.sort_unstable();
        assert_eq!(codes, wanted, "{} speaks the app's languages", manifest.id);
        for code in LANGUAGES {
            let said = &manifest.locales[code];
            assert!(
                said.summary
                    .as_deref()
                    .is_some_and(|summary| !summary.trim().is_empty()),
                "{} has no summary in {code}",
                manifest.id
            );
            let named = said
                .name
                .as_deref()
                .is_some_and(|name| !name.trim().is_empty());
            assert_eq!(
                named,
                !KEEP_THEIR_NAME.contains(&manifest.id.as_str()),
                "{}: its name in {code}",
                manifest.id
            );
        }
    }
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
    assert_eq!(
        ids(&open),
        ["com.example.a", "com.example.b"],
        "one frame of it is still there"
    );
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
