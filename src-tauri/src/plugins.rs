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

/// The page that hosts a plugin: it loads the plugin's script, puts its component on the page and
/// talks to the app only through `postMessage`. The plugin never sees the app's window.
pub fn frame_html(component: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<style>html,body{{margin:0;padding:0;background:transparent}}</style>
</head>
<body>
<{component} id="view"></{component}>
<script type="module">
import "./dist/index.js";
const view = document.getElementById("view");
const tell = () => parent.postMessage({{ type: "ft.height", height: document.documentElement.scrollHeight }}, "*");
addEventListener("message", (event) => {{
  if (event.data && event.data.type === "ft.render") {{
    view.setAttribute("text", String(event.data.text ?? ""));
    requestAnimationFrame(tell);
  }}
}});
new ResizeObserver(tell).observe(document.documentElement);
parent.postMessage({{ type: "ft.ready" }}, "*");
</script>
</body>
</html>
"#
    )
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
        assert!(html.contains(r#"import "./dist/index.js""#));
        assert!(html.contains("ft.render"), "the app hands it the text through postMessage");
    }

    // §55: a plugin without the network permission cannot reach anything.
    #[test]
    fn the_policy_of_a_plugin_without_network_lets_it_reach_nothing() {
        let policy = policy_for(&Permissions::default());
        assert!(policy.contains("connect-src 'none'"));
        let with_network = Permissions { network: vec!["api.openai.com".to_owned()], ..Permissions::default() };
        assert!(policy_for(&with_network).contains("connect-src https://api.openai.com"));
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
