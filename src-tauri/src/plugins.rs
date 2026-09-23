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
    ("arrow-undo-outline", include_bytes!("../resources/icons/arrow-undo-outline.svg")),
    ("arrow-up-outline", include_bytes!("../resources/icons/arrow-up-outline.svg")),
    ("brush-outline", include_bytes!("../resources/icons/brush-outline.svg")),
    ("close-outline", include_bytes!("../resources/icons/close-outline.svg")),
    ("crop-outline", include_bytes!("../resources/icons/crop-outline.svg")),
    ("document-text-outline", include_bytes!("../resources/icons/document-text-outline.svg")),
    ("download-outline", include_bytes!("../resources/icons/download-outline.svg")),
    ("eye-outline", include_bytes!("../resources/icons/eye-outline.svg")),
    ("folder-open-outline", include_bytes!("../resources/icons/folder-open-outline.svg")),
    ("grid-outline", include_bytes!("../resources/icons/grid-outline.svg")),
    ("image-outline", include_bytes!("../resources/icons/image-outline.svg")),
    ("options-outline", include_bytes!("../resources/icons/options-outline.svg")),
    ("pencil-outline", include_bytes!("../resources/icons/pencil-outline.svg")),
    ("refresh-outline", include_bytes!("../resources/icons/refresh-outline.svg")),
    ("resize-outline", include_bytes!("../resources/icons/resize-outline.svg")),
    ("send-outline", include_bytes!("../resources/icons/send-outline.svg")),
    ("square-outline", include_bytes!("../resources/icons/square-outline.svg")),
    ("trash-outline", include_bytes!("../resources/icons/trash-outline.svg")),
];

/// An icon by name, or nothing. A name is a name: never a path, never a way out of the list.
pub fn icon(name: &str) -> Option<(&'static str, &'static [u8])> {
    ICONS.iter().find(|(known, _)| *known == name).map(|(_, svg)| ("image/svg+xml", *svg))
}

pub fn frame_html(component: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<style>html,body{{margin:0;padding:0;background:transparent}}</style>
<script type="module" src="./frame.js"></script>
</head>
<body>
<p id="fallback" style="font:13px system-ui;color:#888">loading…</p>
<{component} id="view"></{component}>
</body>
</html>
"#
    )
}

/// What the frame does: load the plugin, hand it the text the app sends, and say how tall it is.
/// It talks to the app only through `postMessage`; it never sees the app's window.
pub fn frame_js() -> &'static str {
    r#"// The API a plugin has (plugin-sdk, §53). It is the only way out of the frame: the plugin
// never sees the app, the chat, the keys or another plugin. Everything here is asked for, and the
// core checks what the user granted before doing any of it.
const post = (message) => parent.postMessage(message, "*");
const opened = [];
const waiting = new Map();
let asked = 0;

/** Asks the app for something and waits for its answer, one call at a time. */
const ask = (type, message) => {
  const id = `q${(asked += 1)}`;
  return new Promise((resolve) => {
    waiting.set(id, resolve);
    post({ ...message, type, id });
  });
};

globalThis.ft = {
  /** Called when the app opens the plugin: the text the user handed it, and the app's colours. */
  onOpen(handler) {
    opened.push(handler);
  },
  /** Asks the app to ask the user for a file. Resolves with {name, mime, data} or null. */
  pickFile(accept) {
    return ask("ft.pickFile", { accept: accept ?? "" });
  },
  /** Hands a file to the chat; the app sends it. `data` is base64. */
  send(name, mime, data) {
    post({ type: "ft.made", name: String(name), mime: String(mime), data: String(data) });
  },
  /** Puts a text in the composer, for the user to look at before sending it. */
  say(text) {
    post({ type: "ft.text", text: String(text) });
  },
  /** Saves a file on the phone instead of sending it. */
  save(name, mime, data) {
    return ask("ft.save", { name: String(name), mime: String(mime), data: String(data) });
  },
  /** Prints a file, if the user granted this plugin printing. The printer is the user's. */
  print(name, mime, data) {
    return ask("ft.print", { name: String(name), mime: String(mime), data: String(data) });
  },
  /** A call the core makes for the plugin, only to a host the user granted it. */
  fetch(url, options = {}) {
    return ask("ft.fetch", {
      url: String(url),
      method: String(options.method ?? "GET"),
      headers: Object.entries(options.headers ?? {}).map(([name, value]) => [String(name), String(value)]),
      body: options.body ?? null,
    });
  },
  /** What this plugin remembers. Its frame has no storage of its own (§53). */
  store: {
    get: (key) => ask("ft.read", { key: String(key) }),
    set: (key, value) => ask("ft.write", { key: String(key), value: String(value) }),
    forget: (key) => ask("ft.forget", { key: String(key) }),
  },
  /** Closes the plugin's window. */
  close() {
    post({ type: "ft.close" });
  },
};

await import("./dist/index.js");

const fallback = document.getElementById("fallback");
if (fallback) fallback.remove();

const view = document.getElementById("view");
const tell = () => post({ type: "ft.height", height: document.documentElement.scrollHeight });

addEventListener("message", (event) => {
  const said = event.data;
  if (!said || typeof said.type !== "string") return;
  if (said.type === "ft.open") {
    const text = String(said.text ?? "");
    if (view) view.setAttribute("text", text);
    if (said.dark) document.documentElement.dataset.dark = "1";
    for (const handler of opened) handler({ text, dark: Boolean(said.dark) });
    requestAnimationFrame(tell);
  } else if (said.type === "ft.file") {
    const answer = waiting.get(said.id);
    waiting.delete(said.id);
    if (answer) answer(said.name ? { name: said.name, mime: said.mime, data: said.data } : null);
  } else if (said.type === "ft.done") {
    const answer = waiting.get(said.id);
    waiting.delete(said.id);
    if (answer) answer(said.answer ?? null);
  }
});

new ResizeObserver(tell).observe(document.documentElement);
post({ type: "ft.ready" });
"#
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

    #[test]
    fn the_frame_loads_the_plugin_and_shows_its_component() {
        let html = frame_html("ft-code-block");
        assert!(html.contains("<ft-code-block id=\"view\">"));
        assert!(html.contains(r#"src="./frame.js""#), "the script is a file, never written in the page");
        assert!(!html.contains("import "), "nothing of the script lives in the page");
        assert!(html.contains("loading…"), "something shows even if the script never runs");

        let script = frame_js();
        assert!(script.contains(r#"import("./dist/index.js")"#));
        assert!(script.contains("globalThis.ft"), "the plugin is given its API");
        assert!(
            script.find("globalThis.ft").unwrap() < script.find(r#"import("./dist/index.js")"#).unwrap(),
            "the API is there before the plugin runs"
        );
        assert!(script.contains("ft.open"), "the app opens it, with the text the user handed it");
        assert!(script.contains("ft.file"), "the app answers the file the plugin asked for");
        assert!(script.contains("ft.ready"), "the plugin says when it is up");
        // The whole surface a plugin has: everything else it might try is not there (§53).
        for call in ["pickFile", "send", "say", "save", "print", "fetch", "store", "close"] {
            assert!(script.contains(call), "a plugin cannot {call}");
        }
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
        for wanted in ["eye-outline", "folder-open-outline", "send-outline", "trash-outline", "image-outline"] {
            assert!(icon(wanted).is_some(), "{wanted} is missing");
        }
    }

    #[test]
    fn an_icon_is_served_from_the_plugin_scheme() {
        assert_eq!(route("/com.example.x/icon/pencil-outline.svg"), Some(("com.example.x".into(), "icon/pencil-outline.svg".into())));
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
