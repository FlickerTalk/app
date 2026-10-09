//! Serving a plugin to the WebView (issue app#3, Plan §55, §58). Each plugin is served from its
//! own folder under a scheme of its own, inside an iframe, with the content security policy that
//! its granted permissions allow: without the network permission it cannot reach anything at all.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use ft_plugins::Permissions;

/// What the WebView needs to run one plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Served {
    pub dir: PathBuf,
    /// The first web component of its manifest: what the frame puts on the page.
    pub component: String,
    /// The policy for its frame, from what the user granted it.
    pub policy: String,
    /// The version installed, named in the frame's addresses (2026-10-03).
    pub version: String,
}

/// The headers of everything the scheme serves: its kind, the plugin's policy, a frame without
/// an origin may read it, and nothing is kept in a cache, so an updated plugin runs its new code
/// the next time it opens (2026-10-03).
pub fn response_headers(kind: &str, policy: &str) -> Vec<(&'static str, String)> {
    vec![
        ("Content-Type", kind.to_owned()),
        ("Content-Security-Policy", policy.to_owned()),
        (ALLOW_OPAQUE_ORIGIN.0, ALLOW_OPAQUE_ORIGIN.1.to_owned()),
        ("Cross-Origin-Resource-Policy", "cross-origin".to_owned()),
        ("Cache-Control", "no-store".to_owned()),
    ]
}

/// What may be served right now, kept in step with what is installed and granted.
#[derive(Default)]
pub struct Plugins(RwLock<HashMap<String, Served>>);

impl Plugins {
    pub fn set(&self, plugins: HashMap<String, Served>) {
        *self.0.write().expect("plugins poisoned") = plugins;
    }

    pub fn get(&self, id: &str) -> Option<Served> {
        self.0.read().expect("plugins poisoned").get(id).cloned()
    }
}

/// The page that hosts a plugin. Its script is a file of its own: the policy allows no script
/// written inside the page, which is what keeps a plugin from slipping code into it.
/// The icons the app lends its tools (Ionicons, MIT). A plugin has no network and carries no
/// pictures of its own, so it asks the core for these and paints them in the colour of the app
/// (`mask-image` + `currentColor`), which is why they are served as they are: one path, no fill.
const ICONS: &[(&str, &[u8])] = &[
    ("add-outline", include_bytes!("../resources/icons/add-outline.svg")),
    ("alarm-outline", include_bytes!("../resources/icons/alarm-outline.svg")),
    ("arrow-back-outline", include_bytes!("../resources/icons/arrow-back-outline.svg")),
    ("arrow-redo-outline", include_bytes!("../resources/icons/arrow-redo-outline.svg")),
    ("arrow-undo-outline", include_bytes!("../resources/icons/arrow-undo-outline.svg")),
    ("arrow-up-outline", include_bytes!("../resources/icons/arrow-up-outline.svg")),
    ("brush-outline", include_bytes!("../resources/icons/brush-outline.svg")),
    ("calculator-outline", include_bytes!("../resources/icons/calculator-outline.svg")),
    ("camera-outline", include_bytes!("../resources/icons/camera-outline.svg")),
    ("chatbubble-outline", include_bytes!("../resources/icons/chatbubble-outline.svg")),
    ("checkmark-outline", include_bytes!("../resources/icons/checkmark-outline.svg")),
    ("close-outline", include_bytes!("../resources/icons/close-outline.svg")),
    ("cloud-done-outline", include_bytes!("../resources/icons/cloud-done-outline.svg")),
    ("cloud-outline", include_bytes!("../resources/icons/cloud-outline.svg")),
    ("cloud-upload-outline", include_bytes!("../resources/icons/cloud-upload-outline.svg")),
    ("color-palette-outline", include_bytes!("../resources/icons/color-palette-outline.svg")),
    ("crop-outline", include_bytes!("../resources/icons/crop-outline.svg")),
    ("document-text-outline", include_bytes!("../resources/icons/document-text-outline.svg")),
    ("download-outline", include_bytes!("../resources/icons/download-outline.svg")),
    ("ellipsis-horizontal-outline", include_bytes!("../resources/icons/ellipsis-horizontal-outline.svg")),
    ("expand-outline", include_bytes!("../resources/icons/expand-outline.svg")),
    ("eye-outline", include_bytes!("../resources/icons/eye-outline.svg")),
    ("folder-open-outline", include_bytes!("../resources/icons/folder-open-outline.svg")),
    ("folder-outline", include_bytes!("../resources/icons/folder-outline.svg")),
    ("grid-outline", include_bytes!("../resources/icons/grid-outline.svg")),
    ("hand-left-outline", include_bytes!("../resources/icons/hand-left-outline.svg")),
    ("image-outline", include_bytes!("../resources/icons/image-outline.svg")),
    ("key-outline", include_bytes!("../resources/icons/key-outline.svg")),
    ("link-outline", include_bytes!("../resources/icons/link-outline.svg")),
    ("location-outline", include_bytes!("../resources/icons/location-outline.svg")),
    ("lock-closed-outline", include_bytes!("../resources/icons/lock-closed-outline.svg")),
    ("move-outline", include_bytes!("../resources/icons/move-outline.svg")),
    ("options-outline", include_bytes!("../resources/icons/options-outline.svg")),
    ("pause-outline", include_bytes!("../resources/icons/pause-outline.svg")),
    ("pencil-outline", include_bytes!("../resources/icons/pencil-outline.svg")),
    ("play-outline", include_bytes!("../resources/icons/play-outline.svg")),
    ("refresh-outline", include_bytes!("../resources/icons/refresh-outline.svg")),
    ("remove-outline", include_bytes!("../resources/icons/remove-outline.svg")),
    ("resize-outline", include_bytes!("../resources/icons/resize-outline.svg")),
    ("save-outline", include_bytes!("../resources/icons/save-outline.svg")),
    ("search-outline", include_bytes!("../resources/icons/search-outline.svg")),
    ("send-outline", include_bytes!("../resources/icons/send-outline.svg")),
    ("square-outline", include_bytes!("../resources/icons/square-outline.svg")),
    ("text-outline", include_bytes!("../resources/icons/text-outline.svg")),
    ("time-outline", include_bytes!("../resources/icons/time-outline.svg")),
    ("trash-outline", include_bytes!("../resources/icons/trash-outline.svg")),
];

/// An icon by name, or nothing. A name is a name: never a path, never a way out of the list.
pub fn icon(name: &str) -> Option<(&'static str, &'static [u8])> {
    ICONS.iter().find(|(known, _)| *known == name).map(|(_, svg)| ("image/svg+xml", *svg))
}

/// The Ionic the app lends to every plugin and game frame (2026-10-09, Ioan; route B of the Ionic
/// plan): the app's own `@ionic/core` and `ionicons`, so a plugin is drawn with the components the
/// app is drawn with and carries none of them. A plugin targets the major version. Updating Ionic
/// in the app means `node scripts/build-frame-ionic.mjs` and these two; a test checks them against
/// `package-lock.json`.
pub const IONIC_VERSION: &str = "9.0.4";
pub const IONICONS_VERSION: &str = "8.1.0";

/// What a frame finds at `./ionic/<file>`: every component as one module, with the icons of
/// `ICONS` and of the apps grid registered by name; Ionic's global styles (ionic.bundle.css but for
/// the body rules of structure.css, which would pin the frame's body and stop a frame sized by its
/// content from growing); and the theme Ionic reads, derived from the app's colours
/// (`frame-theme.js`).
const IONIC: &[(&str, &[u8])] = &[
    ("ionic.js", include_bytes!("../resources/ionic/ionic.js")),
    ("ionic.css", include_bytes!("../resources/ionic/ionic.css")),
    ("theme.js", include_bytes!("frame-theme.js")),
];

/// A file of the lent Ionic by name, or nothing: a name, never a path.
pub fn ionic(file: &str) -> Option<(&'static str, &'static [u8])> {
    IONIC.iter().find(|(known, _)| *known == file).map(|(name, bytes)| (content_type(name), *bytes))
}

/// What the lent Ionic adds to the app, in bytes.
pub fn ionic_size() -> usize {
    IONIC.iter().map(|(_, bytes)| bytes.len()).sum()
}

/// `version` is the installed one's (a checked `x.y.z`): the frame asks for its script, and the
/// script for the plugin's code, at that version (2026-10-03, updates). Before the plugin, the
/// frame loads the lent Ionic (2026-10-09): its styles, the theme and the components, in that
/// order, and says which Ionic it is on the root (`data-ionic`). Ionic picks `ios` or `md` from
/// the user agent, which the frame shares with the app, as the app's own Ionic does.
pub fn frame_html(component: &str, version: &str) -> String {
    // A page of its own (`frame.html`), which the end-to-end tests serve too.
    include_str!("frame.html").replace("%IONIC%", IONIC_VERSION).replace("%VERSION%", version).replace("%COMPONENT%", component)
}

/// What the frame does: load the plugin, hand it the text the app sends, and say how tall it is.
/// It talks to the app only through `postMessage`; it never sees the app's window.
pub fn frame_js() -> &'static str {
    include_str!("frame.js")
}

/// The type of a file we serve; anything we do not know is bytes, never markup.
pub fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

/// Percent-decoding, enough for a path: `%2F` is a slash, `%20` a space.
fn decoded(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&path[index + 1..index + 3], 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The plugin id and the file from a request path like `/com.example.x/dist/index.js`. The path
/// may arrive percent-encoded, as some WebViews do.
pub fn route(path: &str) -> Option<(String, String)> {
    let decoded = decoded(path);
    let trimmed = decoded.trim_start_matches('/');
    let (id, file) = trimmed.split_once('/')?;
    if id.is_empty() || file.is_empty() {
        return None;
    }
    Some((id.to_owned(), file.to_owned()))
}

/// A file inside the plugin's folder, or nothing: no request may climb out of it.
pub fn file_in(dir: &Path, file: &str) -> Option<PathBuf> {
    let mut path = dir.to_path_buf();
    for part in file.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.contains('\\') {
            return None;
        }
        path.push(part);
    }
    path.starts_with(dir).then_some(path)
}

/// The frame is sandboxed, so its origin is opaque: a module script is fetched with CORS and the
/// answer has to say that an opaque origin may read it. The files are the plugin's own, nothing
/// of the app or of the chat.
pub const ALLOW_OPAQUE_ORIGIN: (&str, &str) = ("Access-Control-Allow-Origin", "*");

/// What a plugin's frame is allowed, from what the user granted it.
pub fn policy_for(granted: &Permissions) -> String {
    granted.content_security_policy()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_whose_slash_arrived_encoded_is_still_a_path() {
        assert_eq!(route("/com.example.x%2Fframe.html"), Some(("com.example.x".into(), "frame.html".into())));
        assert_eq!(route("/com.example.x/dist%2Findex.js"), Some(("com.example.x".into(), "dist/index.js".into())));
    }

    #[test]
    fn a_request_names_a_plugin_and_a_file() {
        assert_eq!(route("/com.example.x/dist/index.js"), Some(("com.example.x".into(), "dist/index.js".into())));
        assert_eq!(route("/com.example.x/frame.html"), Some(("com.example.x".into(), "frame.html".into())));
        assert_eq!(route("/com.example.x/"), None);
        assert_eq!(route("/"), None);
    }

    #[test]
    fn nothing_is_served_from_outside_the_plugins_folder() {
        let dir = Path::new("/tmp/ft-plugins/com.example.x");
        assert_eq!(file_in(dir, "dist/index.js"), Some(dir.join("dist/index.js")));
        assert_eq!(file_in(dir, "../../etc/passwd"), None);
        assert_eq!(file_in(dir, "dist/../../secret"), None);
        assert_eq!(file_in(dir, ""), None);
    }

    // 2026-10-03 (updates): after an update the next opening runs the new code. Nothing the scheme
    // serves is kept in a cache, and the frame names the version it loads (`?v=`), which the frame
    // script passes on to the plugin's code, so no cache can hand it the old one by its address.
    #[test]
    fn an_updated_plugin_is_never_served_from_a_cache() {
        let headers = response_headers("text/html; charset=utf-8", "default-src 'none'");
        assert!(headers.contains(&("Cache-Control", "no-store".to_owned())), "{headers:?}");
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
        assert!(html.contains(r#"src="./frame.js?v=1.0.0""#), "the script is a file, never written in the page");
        assert!(!html.contains("import "), "nothing of the script lives in the page");
        assert!(html.contains("loading…"), "something shows even if the script never runs");
        // Without this the frame paints itself white in a dark app and the plugin is unreadable.
        assert!(html.contains("color-scheme:light dark"), "the frame follows the app's colours");

        let script = frame_js();
        assert!(script.contains("import(`./dist/index.js${new URL(import.meta.url).search}`)"));
        assert!(script.contains("globalThis.ft"), "the plugin is given its API");
        assert!(
            script.find("globalThis.ft").unwrap() < script.find("import(`./dist/index.js").unwrap(),
            "the API is there before the plugin runs"
        );
        assert!(script.contains("ft.open"), "the app opens it, with the text the user handed it");
        assert!(script.contains("ft.file"), "the app answers the file the plugin asked for");
        assert!(script.contains("ft.ready"), "the plugin says when it is up");
        // The whole surface a plugin has: everything else it might try is not there (§53).
        for call in ["pickFile", "send", "say", "save", "print", "fetch", "store", "close", "records", "remind", "live", "openChat"] {
            assert!(script.contains(call), "a plugin cannot {call}");
        }
        // 2026-09-27: what the plugin is opened with says the language, a file, a ref and
        // whether the live channel is there; what the other side says comes as `ft.live`.
        for given in ["lang", "file", "ref", "reminder", "live"] {
            assert!(script.contains(given), "onOpen says nothing of {given}");
        }
        assert!(script.contains(r#"said.type === "ft.live""#));
        // 2026-10-02: the chat it was opened in, as the core's opaque id, and only when there
        // is one: opened from Settings, `chat` is not there at all.
        assert!(script.contains(r#"typeof said.chat === "string" ? { chat: said.chat } : {}"#), "onOpen says nothing of chat");
        // The user's cloud, one question with an operation, never bytes.
        assert!(script.contains(r#"ask("ft.drive", { op: "list""#));
        // 2026-10-02: the phone's position, once; anything but a place comes back as null.
        assert!(script.contains(r#"location: () => ask("ft.location", {})"#), "a plugin cannot ask where the phone is");
        // 2026-10-06: a photo taken now with the phone's camera app, answered like a picked file.
        assert!(script.contains(r#"takePhoto: () => ask("ft.takePhoto", {})"#), "a plugin cannot ask for a photo from the camera");
        // 2026-10-06: a notice the app shows as its single toast at the top; fire and forget, like say.
        assert!(
            script.contains(r#"post({ type: "ft.notify", text: String(text ?? ""), sticky: options?.sticky === true })"#),
            "a plugin cannot hand the app a notice"
        );
        // 2026-10-02: told the window is closing, the frame runs the plugin's goodbye and answers.
        assert!(script.contains("onClose"), "a plugin cannot say goodbye when the app closes it");
        assert!(script.contains(r#"said.type === "ft.closing""#) && script.contains(r#"type: "ft.closed""#));
        assert!(!script.contains("__TAURI"), "a plugin never reaches the app's own bridge");
    }

    // §55: a plugin without the network permission cannot reach anything.
    #[test]
    fn the_policy_of_a_plugin_without_network_lets_it_reach_nothing() {
        let policy = policy_for(&Permissions::default());
        assert!(policy.contains("connect-src 'none'"));
        let with_network = Permissions { network: vec!["api.openai.com".to_owned()], ..Permissions::default() };
        assert!(policy_for(&with_network).contains("connect-src https://api.openai.com"));
    }

    // Without this the module of a sandboxed frame never loads, and the plugin shows nothing.
    #[test]
    fn a_sandboxed_frame_may_read_its_own_files() {
        assert_eq!(ALLOW_OPAQUE_ORIGIN, ("Access-Control-Allow-Origin", "*"));
    }

    // The tools look like the app because the app lends them its icons (Ioan, 2026-09-23): a
    // plugin has no network and carries no pictures, so the core serves them.
    #[test]
    fn the_core_lends_its_icons_to_the_plugins() {
        let (name, svg) = icon("pencil-outline").expect("the icon the tools write with");
        assert_eq!(name, "image/svg+xml");
        assert!(svg.starts_with(b"<svg"));
        assert!(icon("../../secret").is_none(), "an icon is a name, never a path");
        assert!(icon("not-an-icon").is_none());
        // Everything the tools ask for is really there.
        for wanted in ["eye-outline", "folder-open-outline", "send-outline", "trash-outline", "image-outline", "alarm-outline", "calculator-outline", "cloud-outline", "search-outline", "location-outline", "camera-outline"] {
            assert!(icon(wanted).is_some(), "{wanted} is missing");
        }
    }

    #[test]
    fn an_icon_is_served_from_the_plugin_scheme() {
        assert_eq!(route("/com.example.x/icon/pencil-outline.svg"), Some(("com.example.x".into(), "icon/pencil-outline.svg".into())));
    }

    // 2026-10-09 (Ioan, route B of the Ionic plan): the app lends its own Ionic to every frame,
    // so a plugin or a game is drawn with the components the app is drawn with, and carries none.
    #[test]
    fn the_core_lends_its_ionic_to_every_plugin() {
        let (kind, script) = ionic("ionic.js").expect("the components");
        assert_eq!(kind, "text/javascript; charset=utf-8");
        let script = std::str::from_utf8(script).expect("text");
        assert!(script.starts_with(&format!("/*! @ionic/core {IONIC_VERSION}, ionicons {IONICONS_VERSION}")), "it says what it is");
        assert!(script.contains("globalThis.ftIonic"), "the controllers are handed to the plugin");
        assert!(script.contains("ion-button") && script.contains("ion-toast") && script.contains("ion-modal"));
        // Under the frame's policy: no code made from text, and nothing loaded later by address.
        for banned in ["eval(", "new Function", "import(", "importScripts"] {
            assert!(!script.contains(banned), "the lent Ionic has {banned}");
        }
        // The icons the core lends by file are drawn by name too: `<ion-icon name="…">`, no fetch.
        for (name, _) in ICONS {
            assert!(script.contains(&format!(r#""{name}""#)), "{name} is not registered for ion-icon");
        }
        let (kind, css) = ionic("ionic.css").expect("Ionic's global styles");
        assert_eq!(kind, "text/css; charset=utf-8");
        let css = std::str::from_utf8(css).expect("text");
        assert!(css.contains(".ion-color-primary"), "the colour classes are there");
        // Ionic's structure.css fixes the body to the frame: a frame sized by its content (the game
        // room) would never grow, and an old plugin in a tall frame would never scroll.
        assert!(!css.contains("position:fixed;width:100%;max-width:100%;height:100%"), "structure.css is not lent");
        let (kind, theme) = ionic("theme.js").expect("the theme Ionic reads, from the app's colours");
        assert_eq!(kind, "text/javascript; charset=utf-8");
        assert!(std::str::from_utf8(theme).expect("text").contains("--ion-text-color-rgb"));
        assert!(ionic("../plugins.rs").is_none(), "a file is a name, never a path");
        assert!(ionic("ionic.bundle.css").is_none());
    }

    #[test]
    fn the_lent_ionic_is_the_one_the_app_is_built_with() {
        let lock: serde_json::Value = serde_json::from_str(include_str!("../../package-lock.json")).expect("the app's lock file");
        let version = |package: &str| lock["packages"][format!("node_modules/{package}")]["version"].as_str().map(str::to_owned);
        assert_eq!(version("@ionic/core").as_deref(), Some(IONIC_VERSION), "rebuild it: node scripts/build-frame-ionic.mjs");
        assert_eq!(version("ionicons").as_deref(), Some(IONICONS_VERSION), "rebuild it: node scripts/build-frame-ionic.mjs");
    }

    // What the app carries for it, said aloud and kept in check: it is in every build of the app.
    #[test]
    fn what_the_lent_ionic_weighs() {
        let total = ionic_size();
        println!("Ionic lent to the frames: {total} bytes");
        assert!(total > 500_000, "{total}: the components are missing");
        assert!(total < 1_600_000, "{total}: more than the app meant to carry for its plugins");
    }

    #[test]
    fn ionic_is_served_from_the_plugin_scheme() {
        assert_eq!(route("/com.example.x/ionic/ionic.js"), Some(("com.example.x".into(), "ionic/ionic.js".into())));
    }

    // The frame loads Ionic's styles, the theme and the components before the plugin, and says
    // which Ionic it lends (`data-ionic`), each at that version so no cache answers an old one.
    #[test]
    fn the_frame_of_a_tool_is_as_tall_as_its_window() {
        // Told by the app (`fill`, `frame.js`): then the page and its body take the frame's height.
        assert!(frame_html("ft-x", "1.0.0").contains("html[data-fill],html[data-fill] body{height:100%}"));
    }

    #[test]
    fn the_frame_loads_ionic_before_the_plugin() {
        let html = frame_html("ft-x", "1.0.0");
        assert!(html.contains(&format!(r#"<html lang="en" data-ionic="{IONIC_VERSION}">"#)), "{html}");
        let at = |needle: &str| html.find(needle).unwrap_or_else(|| panic!("{needle} is missing from {html}"));
        let css = at(&format!(r#"<link rel="stylesheet" href="./ionic/ionic.css?v={IONIC_VERSION}">"#));
        let own = at("<style>");
        let theme = at(&format!(r#"<script type="module" src="./ionic/theme.js?v={IONIC_VERSION}"></script>"#));
        let components = at(&format!(r#"<script type="module" src="./ionic/ionic.js?v={IONIC_VERSION}"></script>"#));
        let frame = at(r#"<script type="module" src="./frame.js?v=1.0.0"></script>"#);
        // The frame's own style after Ionic's: its see-through page wins over Ionic's painted body.
        assert!(css < own && own < theme && theme < components && components < frame, "{html}");
    }

    #[test]
    fn files_are_served_as_what_they_are() {
        assert_eq!(content_type("dist/index.js"), "text/javascript; charset=utf-8");
        assert_eq!(content_type("frame.html"), "text/html; charset=utf-8");
        assert_eq!(content_type("assets/logo.png"), "image/png");
        assert_eq!(content_type("module.json"), "application/json; charset=utf-8");
        assert_eq!(content_type("weird.exe"), "application/octet-stream");
    }
}
