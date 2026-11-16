use super::*;

#[test]
fn a_request_whose_slash_arrived_encoded_is_still_a_path() {
    assert_eq!(
        route("/com.example.x%2Fframe.html"),
        Some(("com.example.x".into(), "frame.html".into()))
    );
    assert_eq!(
        route("/com.example.x/dist%2Findex.js"),
        Some(("com.example.x".into(), "dist/index.js".into()))
    );
}

#[test]
fn a_request_names_a_plugin_and_a_file() {
    assert_eq!(
        route("/com.example.x/dist/index.js"),
        Some(("com.example.x".into(), "dist/index.js".into()))
    );
    assert_eq!(
        route("/com.example.x/frame.html"),
        Some(("com.example.x".into(), "frame.html".into()))
    );
    assert_eq!(route("/com.example.x/"), None);
    assert_eq!(route("/"), None);
}

#[test]
fn nothing_is_served_from_outside_the_plugins_folder() {
    let dir = Path::new("/tmp/ft-plugins/com.example.x");
    assert_eq!(
        file_in(dir, "dist/index.js"),
        Some(dir.join("dist/index.js"))
    );
    assert_eq!(file_in(dir, "../../etc/passwd"), None);
    assert_eq!(file_in(dir, "dist/../../secret"), None);
    assert_eq!(file_in(dir, ""), None);
}

#[test]
fn an_updated_plugin_is_never_served_from_a_cache() {
    let headers = response_headers("text/html; charset=utf-8", "default-src 'none'");
    assert!(
        headers.contains(&("Cache-Control", "no-store".to_owned())),
        "{headers:?}"
    );
    assert!(headers.contains(&("Content-Type", "text/html; charset=utf-8".to_owned())));
    assert!(headers.contains(&("Content-Security-Policy", "default-src 'none'".to_owned())));
    assert!(headers.contains(&(ALLOW_OPAQUE_ORIGIN.0, ALLOW_OPAQUE_ORIGIN.1.to_owned())));
    assert!(headers.contains(&("Cross-Origin-Resource-Policy", "cross-origin".to_owned())));
    assert!(frame_html("ft-x", "1.0.1").contains(r#"src="./frame.js?v=1.0.1""#));
    assert_ne!(frame_html("ft-x", "1.0.1"), frame_html("ft-x", "1.0.0"));
}

#[test]
fn the_frame_loads_the_plugin_and_shows_its_component() {
    let html = frame_html("ft-code-block", "1.0.0");
    assert!(html.contains("<ft-code-block id=\"view\">"));
    assert!(
        html.contains(r#"src="./frame.js?v=1.0.0""#),
        "the script is a file, never written in the page"
    );
    assert!(
        !html.contains("import "),
        "nothing of the script lives in the page"
    );
    assert!(
        html.contains("loading…"),
        "something shows even if the script never runs"
    );
    assert!(
        html.contains("color-scheme:light dark"),
        "the frame follows the app's colours"
    );

    let script = frame_js();
    assert!(script.contains("import(`./dist/index.js${new URL(import.meta.url).search}`)"));
    assert!(
        script.contains("globalThis.ft"),
        "the plugin is given its API"
    );
    assert!(
        script.find("globalThis.ft").unwrap() < script.find("import(`./dist/index.js").unwrap(),
        "the API is there before the plugin runs"
    );
}

#[test]
fn the_frame_hands_the_plugin_its_api() {
    let script = frame_js();
    assert!(
        script.contains("ft.open"),
        "the app opens it, with the text the user handed it"
    );
    assert!(
        script.contains("ft.file"),
        "the app answers the file the plugin asked for"
    );
    assert!(script.contains("ft.ready"), "the plugin says when it is up");
    for call in [
        "pickFile", "send", "say", "save", "print", "fetch", "store", "close", "records", "remind",
        "live", "openChat",
    ] {
        assert!(script.contains(call), "a plugin cannot {call}");
    }
    for given in ["lang", "file", "ref", "reminder", "live"] {
        assert!(script.contains(given), "onOpen says nothing of {given}");
    }
    assert!(script.contains(r#"said.type === "ft.live""#));
    assert!(
        script.contains(r#"typeof said.chat === "string" ? { chat: said.chat } : {}"#),
        "onOpen says nothing of chat"
    );
    assert!(script.contains(r#"ask("ft.drive", { op: "list""#));
    assert!(
        script.contains(r#"location: () => ask("ft.location", {})"#),
        "a plugin cannot ask where the phone is"
    );
    assert!(
        script.contains("translate: {"),
        "a plugin cannot translate on the phone"
    );
    for question in [
        "ft.translateLanguages",
        "ft.translateDetect",
        "ft.translateStatus",
        "ft.translatePrepare",
        "ft.translateRemove",
        "ft.translateText",
    ] {
        assert!(
            script.contains(&format!(r#"ask("{question}""#)),
            "a plugin cannot ask {question}"
        );
    }
    assert!(
        script.contains(r#"takePhoto: () => ask("ft.takePhoto", {})"#),
        "a plugin cannot ask for a photo from the camera"
    );
    assert!(
        script.contains(r#"post({ type: "ft.notify", text: String(text ?? ""), sticky: options?.sticky === true })"#),
        "a plugin cannot hand the app a notice"
    );
    assert!(
        script.contains("onHook(handler)"),
        "a plugin cannot bind to the chat's hooks"
    );
    assert!(
        script.contains(r#"said.type === "ft.hook""#) && script.contains(r#"type: "ft.hookDone""#)
    );
    assert!(
        script.contains(r#"post({ type: "ft.show" })"#),
        "a plugin running out of sight cannot ask to be seen"
    );
    assert!(
        script.contains("onClose"),
        "a plugin cannot say goodbye when the app closes it"
    );
    assert!(
        script.contains(r#"said.type === "ft.closing""#) && script.contains(r#"type: "ft.closed""#)
    );
    assert!(
        !script.contains("__TAURI"),
        "a plugin never reaches the app's own bridge"
    );
}

#[test]
fn the_policy_of_a_plugin_without_network_lets_it_reach_nothing() {
    let policy = policy_for("com.example.x", &Permissions::default());
    assert!(policy.contains("connect-src 'none'"));
    let with_network = Permissions {
        network: vec!["api.openai.com".to_owned()],
        ..Permissions::default()
    };
    assert!(
        policy_for("com.example.x", &with_network).contains("connect-src https://api.openai.com")
    );
}

#[test]
fn the_policy_of_a_plugin_names_its_own_folder() {
    let policy = policy_for("com.example.x", &Permissions::default());
    assert!(
        policy.contains("script-src http://ftplugin.localhost/com.example.x/ "),
        "{policy}"
    );
    assert!(
        !policy.contains("ftplugin://localhost;") && !policy.contains("ftplugin://localhost "),
        "{policy}"
    );
}

#[test]
fn a_request_inside_a_plugins_folder_never_reaches_another_plugin() {
    let dir = Path::new("/tmp/ft-plugins/com.example.a");
    for path in [
        "/com.example.a/..%2Fcom.example.b/dist/index.js",
        "/com.example.a/%2E%2E/com.example.b/dist/index.js",
        "/com.example.a%2F..%2Fcom.example.b%2Fdist%2Findex.js",
        "/com.example.a/dist/..%2F..%2Fcom.example.b/dist/index.js",
        "/com.example.a/ionic/..%2F..%2Fcom.example.b/dist/index.js",
        "/com.example.a/icon/..%2F..%2Fcom.example.b/icon.svg",
    ] {
        let (id, file) = route(path).expect("a plugin and a file");
        assert_eq!(id, "com.example.a", "{path}");
        assert_eq!(file_in(dir, &file), None, "{path}");
        assert!(ionic(file.trim_start_matches("ionic/")).is_none(), "{path}");
        let name = file.trim_start_matches("icon/").trim_end_matches(".svg");
        assert!(icon(name).is_none(), "{path}");
    }
}

#[test]
fn a_sandboxed_frame_may_read_its_own_files() {
    assert_eq!(ALLOW_OPAQUE_ORIGIN, ("Access-Control-Allow-Origin", "*"));
}

#[test]
fn the_core_lends_its_icons_to_the_plugins() {
    let (name, svg) = icon("pencil-outline").expect("the icon the tools write with");
    assert_eq!(name, "image/svg+xml");
    assert!(svg.starts_with(b"<svg"));
    assert!(
        icon("../../secret").is_none(),
        "an icon is a name, never a path"
    );
    assert!(icon("not-an-icon").is_none());
    for wanted in [
        "eye-outline",
        "folder-open-outline",
        "send-outline",
        "trash-outline",
        "image-outline",
        "alarm-outline",
        "calculator-outline",
        "cloud-outline",
        "search-outline",
        "location-outline",
        "camera-outline",
    ] {
        assert!(icon(wanted).is_some(), "{wanted} is missing");
    }
}

#[test]
fn an_icon_is_served_from_the_plugin_scheme() {
    assert_eq!(
        route("/com.example.x/icon/pencil-outline.svg"),
        Some(("com.example.x".into(), "icon/pencil-outline.svg".into()))
    );
}

#[test]
fn the_core_lends_its_ionic_to_every_plugin() {
    let (kind, script) = ionic("ionic.js").expect("the components");
    assert_eq!(kind, "text/javascript; charset=utf-8");
    let script = std::str::from_utf8(script).expect("text");
    assert!(
        script.starts_with(&format!(
            "/*! @ionic/core {IONIC_VERSION}, ionicons {IONICONS_VERSION}"
        )),
        "it says what it is"
    );
    assert!(
        script.contains("globalThis.ftIonic"),
        "the controllers are handed to the plugin"
    );
    assert!(
        script.contains("ion-button")
            && script.contains("ion-toast")
            && script.contains("ion-modal")
    );
    for banned in ["eval(", "new Function", "import(", "importScripts"] {
        assert!(!script.contains(banned), "the lent Ionic has {banned}");
    }
    for (name, _) in ICONS {
        assert!(
            script.contains(&format!(r#""{name}""#)),
            "{name} is not registered for ion-icon"
        );
    }
    let (kind, css) = ionic("ionic.css").expect("Ionic's global styles");
    assert_eq!(kind, "text/css; charset=utf-8");
    let css = std::str::from_utf8(css).expect("text");
    assert!(
        css.contains(".ion-color-primary"),
        "the colour classes are there"
    );
    assert!(
        !css.contains("position:fixed;width:100%;max-width:100%;height:100%"),
        "structure.css is not lent"
    );
    let (kind, theme) = ionic("theme.js").expect("the theme Ionic reads, from the app's colours");
    assert_eq!(kind, "text/javascript; charset=utf-8");
    assert!(std::str::from_utf8(theme)
        .expect("text")
        .contains("--ion-text-color-rgb"));
    assert!(
        ionic("../plugins.rs").is_none(),
        "a file is a name, never a path"
    );
    assert!(ionic("ionic.bundle.css").is_none());
}

#[test]
fn the_lent_ionic_is_the_one_the_app_is_built_with() {
    let lock: serde_json::Value = serde_json::from_str(include_str!("../../../package-lock.json"))
        .expect("the app's lock file");
    let version = |package: &str| {
        lock["packages"][format!("node_modules/{package}")]["version"]
            .as_str()
            .map(str::to_owned)
    };
    assert_eq!(
        version("@ionic/core").as_deref(),
        Some(IONIC_VERSION),
        "rebuild it: node scripts/build-frame-ionic.mjs"
    );
    assert_eq!(
        version("ionicons").as_deref(),
        Some(IONICONS_VERSION),
        "rebuild it: node scripts/build-frame-ionic.mjs"
    );
}

#[test]
fn the_lent_ionic_keeps_its_licences() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/ionic/LICENSE");
    let licence = std::fs::read_to_string(path)
        .expect("resources/ionic/LICENSE: rebuild it: node scripts/build-frame-ionic.mjs");
    assert!(
        licence.starts_with(&format!("@ionic/core {IONIC_VERSION}\n")),
        "the @ionic/core notice comes first, with its version"
    );
    assert!(
        licence.contains(&format!("\nionicons {IONICONS_VERSION}\n")),
        "the ionicons notice, with its version"
    );
    assert_eq!(
        licence.matches("Permission is hereby granted").count(),
        2,
        "both MIT notices, in full"
    );
}

#[test]
fn what_the_lent_ionic_weighs() {
    let total = ionic_size();
    println!("Ionic lent to the frames: {total} bytes");
    assert!(total > 500_000, "{total}: the components are missing");
    assert!(
        total < 1_600_000,
        "{total}: more than the app meant to carry for its plugins"
    );
}

#[test]
fn ionic_is_served_from_the_plugin_scheme() {
    assert_eq!(
        route("/com.example.x/ionic/ionic.js"),
        Some(("com.example.x".into(), "ionic/ionic.js".into()))
    );
}

#[test]
fn the_frame_of_a_tool_is_as_tall_as_its_window() {
    assert!(
        frame_html("ft-x", "1.0.0").contains("html[data-fill],html[data-fill] body{height:100%}")
    );
}

#[test]
fn the_frame_leaves_the_top_and_bottom_insets_to_the_app() {
    let html = frame_html("ft-x", "1.0.0");
    assert!(
        html.contains("html{--ion-safe-area-top:0px;--ion-safe-area-bottom:0px}"),
        "{html}"
    );
}

#[test]
fn the_frame_loads_ionic_before_the_plugin() {
    let html = frame_html("ft-x", "1.0.0");
    assert!(
        html.contains(&format!(r#"<html lang="en" data-ionic="{IONIC_VERSION}">"#)),
        "{html}"
    );
    let at = |needle: &str| {
        html.find(needle)
            .unwrap_or_else(|| panic!("{needle} is missing from {html}"))
    };
    let css = at(&format!(
        r#"<link rel="stylesheet" href="./ionic/ionic.css?v={IONIC_VERSION}">"#
    ));
    let own = at("<style>");
    let theme = at(&format!(
        r#"<script type="module" src="./ionic/theme.js?v={IONIC_VERSION}"></script>"#
    ));
    let components = at(&format!(
        r#"<script type="module" src="./ionic/ionic.js?v={IONIC_VERSION}"></script>"#
    ));
    let frame = at(r#"<script type="module" src="./frame.js?v=1.0.0"></script>"#);
    assert!(
        css < own && own < theme && theme < components && components < frame,
        "{html}"
    );
}

#[test]
fn files_are_served_as_what_they_are() {
    assert_eq!(
        content_type("dist/index.js"),
        "text/javascript; charset=utf-8"
    );
    assert_eq!(content_type("frame.html"), "text/html; charset=utf-8");
    assert_eq!(content_type("assets/logo.png"), "image/png");
    assert_eq!(
        content_type("module.json"),
        "application/json; charset=utf-8"
    );
    assert_eq!(content_type("weird.exe"), "application/octet-stream");
}
