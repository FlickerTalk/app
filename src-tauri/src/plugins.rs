use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use ft_plugins::Permissions;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Served {
    pub dir: PathBuf,
    pub component: String,
    pub policy: String,
    pub version: String,
}

/// no-store so an updated plugin runs its new code the next time it opens
pub fn response_headers(kind: &str, policy: &str) -> Vec<(&'static str, String)> {
    vec![
        ("Content-Type", kind.to_owned()),
        ("Content-Security-Policy", policy.to_owned()),
        (ALLOW_OPAQUE_ORIGIN.0, ALLOW_OPAQUE_ORIGIN.1.to_owned()),
        ("Cross-Origin-Resource-Policy", "cross-origin".to_owned()),
        ("Cache-Control", "no-store".to_owned()),
    ]
}

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

/// Plugins paint these with mask-image + currentColor, so each SVG is one path with no fill.
const ICONS: &[(&str, &[u8])] = &[
    (
        "add-outline",
        include_bytes!("../resources/icons/add-outline.svg"),
    ),
    (
        "alarm-outline",
        include_bytes!("../resources/icons/alarm-outline.svg"),
    ),
    (
        "arrow-back-outline",
        include_bytes!("../resources/icons/arrow-back-outline.svg"),
    ),
    (
        "arrow-redo-outline",
        include_bytes!("../resources/icons/arrow-redo-outline.svg"),
    ),
    (
        "arrow-undo-outline",
        include_bytes!("../resources/icons/arrow-undo-outline.svg"),
    ),
    (
        "arrow-up-outline",
        include_bytes!("../resources/icons/arrow-up-outline.svg"),
    ),
    (
        "brush-outline",
        include_bytes!("../resources/icons/brush-outline.svg"),
    ),
    (
        "calculator-outline",
        include_bytes!("../resources/icons/calculator-outline.svg"),
    ),
    (
        "camera-outline",
        include_bytes!("../resources/icons/camera-outline.svg"),
    ),
    (
        "chatbubble-outline",
        include_bytes!("../resources/icons/chatbubble-outline.svg"),
    ),
    (
        "checkmark-outline",
        include_bytes!("../resources/icons/checkmark-outline.svg"),
    ),
    (
        "close-outline",
        include_bytes!("../resources/icons/close-outline.svg"),
    ),
    (
        "cloud-done-outline",
        include_bytes!("../resources/icons/cloud-done-outline.svg"),
    ),
    (
        "cloud-outline",
        include_bytes!("../resources/icons/cloud-outline.svg"),
    ),
    (
        "cloud-upload-outline",
        include_bytes!("../resources/icons/cloud-upload-outline.svg"),
    ),
    (
        "color-palette-outline",
        include_bytes!("../resources/icons/color-palette-outline.svg"),
    ),
    (
        "crop-outline",
        include_bytes!("../resources/icons/crop-outline.svg"),
    ),
    (
        "document-text-outline",
        include_bytes!("../resources/icons/document-text-outline.svg"),
    ),
    (
        "download-outline",
        include_bytes!("../resources/icons/download-outline.svg"),
    ),
    (
        "ellipsis-horizontal-outline",
        include_bytes!("../resources/icons/ellipsis-horizontal-outline.svg"),
    ),
    (
        "expand-outline",
        include_bytes!("../resources/icons/expand-outline.svg"),
    ),
    (
        "eye-outline",
        include_bytes!("../resources/icons/eye-outline.svg"),
    ),
    (
        "folder-open-outline",
        include_bytes!("../resources/icons/folder-open-outline.svg"),
    ),
    (
        "folder-outline",
        include_bytes!("../resources/icons/folder-outline.svg"),
    ),
    (
        "grid-outline",
        include_bytes!("../resources/icons/grid-outline.svg"),
    ),
    (
        "hand-left-outline",
        include_bytes!("../resources/icons/hand-left-outline.svg"),
    ),
    (
        "image-outline",
        include_bytes!("../resources/icons/image-outline.svg"),
    ),
    (
        "key-outline",
        include_bytes!("../resources/icons/key-outline.svg"),
    ),
    (
        "link-outline",
        include_bytes!("../resources/icons/link-outline.svg"),
    ),
    (
        "location-outline",
        include_bytes!("../resources/icons/location-outline.svg"),
    ),
    (
        "lock-closed-outline",
        include_bytes!("../resources/icons/lock-closed-outline.svg"),
    ),
    (
        "move-outline",
        include_bytes!("../resources/icons/move-outline.svg"),
    ),
    (
        "options-outline",
        include_bytes!("../resources/icons/options-outline.svg"),
    ),
    (
        "pause-outline",
        include_bytes!("../resources/icons/pause-outline.svg"),
    ),
    (
        "pencil-outline",
        include_bytes!("../resources/icons/pencil-outline.svg"),
    ),
    (
        "play-outline",
        include_bytes!("../resources/icons/play-outline.svg"),
    ),
    (
        "refresh-outline",
        include_bytes!("../resources/icons/refresh-outline.svg"),
    ),
    (
        "remove-outline",
        include_bytes!("../resources/icons/remove-outline.svg"),
    ),
    (
        "resize-outline",
        include_bytes!("../resources/icons/resize-outline.svg"),
    ),
    (
        "save-outline",
        include_bytes!("../resources/icons/save-outline.svg"),
    ),
    (
        "search-outline",
        include_bytes!("../resources/icons/search-outline.svg"),
    ),
    (
        "send-outline",
        include_bytes!("../resources/icons/send-outline.svg"),
    ),
    (
        "square-outline",
        include_bytes!("../resources/icons/square-outline.svg"),
    ),
    (
        "text-outline",
        include_bytes!("../resources/icons/text-outline.svg"),
    ),
    (
        "time-outline",
        include_bytes!("../resources/icons/time-outline.svg"),
    ),
    (
        "trash-outline",
        include_bytes!("../resources/icons/trash-outline.svg"),
    ),
];

pub fn icon(name: &str) -> Option<(&'static str, &'static [u8])> {
    ICONS
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, svg)| ("image/svg+xml", *svg))
}

pub const IONIC_VERSION: &str = "9.0.4";
#[cfg(test)]
pub const IONICONS_VERSION: &str = "8.1.0";

const IONIC: &[(&str, &[u8])] = &[
    ("ionic.js", include_bytes!("../resources/ionic/ionic.js")),
    ("ionic.css", include_bytes!("../resources/ionic/ionic.css")),
    ("theme.js", include_bytes!("frame-theme.js")),
];

pub fn ionic(file: &str) -> Option<(&'static str, &'static [u8])> {
    IONIC
        .iter()
        .find(|(known, _)| *known == file)
        .map(|(name, bytes)| (content_type(name), *bytes))
}

#[cfg(test)]
pub fn ionic_size() -> usize {
    IONIC.iter().map(|(_, bytes)| bytes.len()).sum()
}

pub fn frame_html(component: &str, version: &str) -> String {
    include_str!("frame.html")
        .replace("%IONIC%", IONIC_VERSION)
        .replace("%VERSION%", version)
        .replace("%COMPONENT%", component)
}

pub fn frame_js() -> &'static str {
    include_str!("frame.js")
}

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

/// some WebViews percent-encode the path, slashes too
pub fn route(path: &str) -> Option<(String, String)> {
    let decoded = decoded(path);
    let trimmed = decoded.trim_start_matches('/');
    let (id, file) = trimmed.split_once('/')?;
    if id.is_empty() || file.is_empty() {
        return None;
    }
    Some((id.to_owned(), file.to_owned()))
}

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

/// The sandboxed frame has an opaque origin and loads module scripts with CORS, hence `*`. Every
/// frame has the same opaque origin, so this cannot tell plugins apart: the frame's policy does.
pub const ALLOW_OPAQUE_ORIGIN: (&str, &str) = ("Access-Control-Allow-Origin", "*");

pub fn policy_for(id: &str, granted: &Permissions) -> String {
    granted.content_security_policy(id)
}

#[cfg(test)]
mod tests;
