//! Nothing in a package is read before the signature checks out, nothing written outside its
//! folder.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use vodozemac::{Ed25519PublicKey, Ed25519SecretKey, Ed25519Signature};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, System, ZipArchive, ZipWriter};

// the only key trusted for plugins and the index
pub const CATALOGUE_KEY: &str = "4XXrMhV2sRZSK/RpFoheNAposE119EfQE8bqdRP8JcA";

pub const INDEX: &str = "catalogue.json";
// app 1.0.0 ignores minCoreVersion, so this index only lists what runs on LEGACY_CORE
pub const LEGACY_INDEX: &str = "index.json";
pub const LEGACY_CORE: &str = "1.0.0";

pub fn catalogue() -> Ed25519PublicKey {
    Ed25519PublicKey::from_base64(CATALOGUE_KEY).expect("the catalogue key is built in")
}

pub mod icon;

const SIGNATURE: &str = "signature";
const MANIFEST: &str = "module.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub min_core_version: String,
    pub components: Vec<String>,
    #[serde(default)]
    pub permissions: Permissions,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub opens: Vec<String>,
    // opens without the user choosing, so a viewer may not ask for the network
    #[serde(default)]
    pub views: Vec<String>,
    #[serde(default)]
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub locales: BTreeMap<String, Localized>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub icon: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub hooks: BTreeMap<HookName, HookDeclaration>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HookName {
    Message,
    Composer,
    Settings,
}

impl HookName {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Message => "message",
            Self::Composer => "composer",
            Self::Settings => "settings",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookDeclaration {
    pub label: String,
    pub icon: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<HookField>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookField {
    pub key: String,
    #[serde(rename = "type")]
    pub kind: FieldKind,
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldKind {
    Select,
    Toggle,
}

const FIELDS_LIMIT: usize = 8;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Localized {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    // not checked here, a catalogue listing a hook only a newer app knows must still read
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub hooks: BTreeMap<String, serde_json::Value>,
}

const NAME_LIMIT: usize = 64;
const SUMMARY_LIMIT: usize = 200;
const LOCALES_LIMIT: usize = 64;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Tool,
    Game,
    // a kind only a newer app knows, never offered, installed or run here
    #[serde(other)]
    Unknown,
}

// older apps would show a game as a tool, so games need at least this core
pub const GAMES_SINCE: &str = "1.3.0";

// installing grants nothing, each one is asked for and revoked on its own. A plugin never gets
// keys, the push token or the whole conversation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permissions {
    // empty = no network
    #[serde(default)]
    pub network: Vec<String>,
    #[serde(default, rename = "messages", with = "given")]
    pub reads_given_messages: bool,
    #[serde(default)]
    pub send: Sending,
    #[serde(default)]
    pub print: bool,
    // direct connection only, never the mailbox or the server
    #[serde(default)]
    pub live: bool,
    #[serde(default)]
    pub remind: bool,
    #[serde(default)]
    pub drive: bool,
    #[serde(default)]
    pub storage: Storage,
    #[serde(default)]
    pub location: bool,
    #[serde(default)]
    pub translate: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Storage {
    #[default]
    Small,
    Large,
}

impl Storage {
    pub fn quota(self) -> u64 {
        match self {
            Self::Small => 4 * 1024 * 1024,
            Self::Large => 256 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sending {
    #[default]
    Nothing,
    Propose,
    // never granted by default
    Auto,
}

mod given {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(reads: &bool, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(if *reads { "given" } else { "none" })
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
        match String::deserialize(deserializer)?.as_str() {
            "given" => Ok(true),
            "none" => Ok(false),
            other => Err(serde::de::Error::custom(format!("'{other}' is not a messages permission"))),
        }
    }
}

// the frame is sandboxed, so its origin is opaque and 'self' would match nothing
const PLUGIN_ORIGINS: &str = "http://ftplugin.localhost https://ftplugin.localhost ftplugin://localhost";

impl Permissions {
    pub fn content_security_policy(&self) -> String {
        let connect = if self.network.is_empty() {
            "'none'".to_owned()
        } else {
            self.network.iter().map(|host| format!("https://{host}")).collect::<Vec<_>>().join(" ")
        };
        format!(
            "default-src 'none'; script-src {PLUGIN_ORIGINS}; style-src {PLUGIN_ORIGINS} 'unsafe-inline'; \
             img-src {PLUGIN_ORIGINS} data: blob:; font-src {PLUGIN_ORIGINS}; connect-src {connect}; base-uri 'none'; \
             form-action 'none'; child-src 'none'; frame-ancestors http://tauri.localhost tauri://localhost"
        )
    }
}

pub struct Plugin {
    pub manifest: Manifest,
    files: BTreeMap<String, Vec<u8>>,
}

impl Plugin {
    pub fn file(&self, path: &str) -> Option<&[u8]> {
        self.files.get(path).map(Vec::as_slice)
    }

    pub fn files(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.files.iter().map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
    }

    pub fn image(&self) -> String {
        self.file(icon::IMAGE).map(|bytes| String::from_utf8_lossy(bytes).into_owned()).unwrap_or_default()
    }
}

pub fn image_of(dir: &Path, id: &str) -> String {
    if !is_id(id) {
        return String::new();
    }
    match std::fs::read(dir.join(id).join(icon::IMAGE)) {
        Ok(bytes) if icon::check(&bytes).is_ok() => String::from_utf8(bytes).unwrap_or_default(),
        _ => String::new(),
    }
}

pub fn digest(files: &BTreeMap<String, Vec<u8>>) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    for (path, bytes) in files {
        hasher.update(path.as_bytes());
        hasher.update(&[0]);
        hasher.update(blake3::hash(bytes).as_bytes());
    }
    *hasher.finalize().as_bytes()
}

// signature first, before anything inside is read as more than bytes
pub fn open(package: &[u8], catalogue: &Ed25519PublicKey) -> Result<Plugin> {
    let mut files = unpack(package)?;
    let signature = files.remove(SIGNATURE).context("the package carries no signature")?;
    let signature = Ed25519Signature::from_slice(&signature).context("the signature is not one")?;
    catalogue
        .verify(&digest(&files), &signature)
        .map_err(|_| anyhow::anyhow!("the package is not signed by the catalogue"))?;

    let manifest: Manifest =
        serde_json::from_slice(files.get(MANIFEST).context("the package carries no module.json")?)
            .context("module.json is not a manifest")?;
    check(&manifest)?;
    for path in files.keys() {
        safe_path(path)?;
    }
    if let Some(image) = files.get(icon::IMAGE) {
        icon::check(image)?;
    }
    Ok(Plugin { manifest, files })
}

// '~' is in no plugin id, so these folders are never taken for a plugin
const NEW: &str = "~new";
const OLD: &str = "~old";

pub fn install(plugin: &Plugin, dir: &Path) -> Result<PathBuf> {
    let home = dir.join(safe_path(&plugin.manifest.id)?);
    let fresh = dir.join(format!("{}{NEW}", plugin.manifest.id));
    let aside = dir.join(format!("{}{OLD}", plugin.manifest.id));
    let written = (|| -> Result<()> {
        if fresh.exists() {
            std::fs::remove_dir_all(&fresh)?;
        }
        for (path, bytes) in plugin.files() {
            let target = fresh.join(safe_path(path)?);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&target, bytes)?;
        }
        Ok(())
    })();
    if let Err(error) = written {
        let _ = std::fs::remove_dir_all(&fresh);
        return Err(error.context("cannot write the plugin"));
    }
    if aside.exists() {
        std::fs::remove_dir_all(&aside)?;
    }
    let replacing = home.exists();
    if replacing {
        std::fs::rename(&home, &aside).context("cannot move the installed plugin aside")?;
    }
    if let Err(error) = std::fs::rename(&fresh, &home) {
        if replacing {
            let _ = std::fs::rename(&aside, &home);
        }
        let _ = std::fs::remove_dir_all(&fresh);
        return Err(anyhow::Error::from(error).context("cannot put the new version in place"));
    }
    if replacing {
        let _ = std::fs::remove_dir_all(&aside);
    }
    Ok(home)
}

// run at core start, before anything is served
pub fn recover(dir: &Path) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let path = entry?.path();
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        if name.ends_with(NEW) {
            std::fs::remove_dir_all(&path)?;
        } else if let Some(id) = name.strip_suffix(OLD) {
            let home = dir.join(id);
            if home.exists() {
                std::fs::remove_dir_all(&path)?;
            } else {
                std::fs::rename(&path, &home)?;
            }
        }
    }
    Ok(())
}

pub fn installed(dir: &Path) -> Result<Vec<Manifest>> {
    let mut manifests = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(manifests),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let folder = entry?.path();
        let Ok(bytes) = std::fs::read(folder.join(MANIFEST)) else { continue };
        if let Ok(manifest) = serde_json::from_slice::<Manifest>(&bytes) {
            // only the plugin's own folder, not a ~new or ~old copy
            if check(&manifest).is_ok() && folder.file_name().is_some_and(|name| name == manifest.id.as_str()) {
                manifests.push(manifest);
            }
        }
    }
    manifests.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(manifests)
}

pub fn remove(id: &str, dir: &Path) -> Result<()> {
    let home = dir.join(safe_path(id)?);
    match std::fs::remove_dir_all(&home) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
        _ => Ok(()),
    }
}

pub fn sign_package(files: &[(String, Vec<u8>)], catalogue: &Ed25519SecretKey) -> Vec<u8> {
    let entries: BTreeMap<String, Vec<u8>> = files.iter().cloned().collect();
    let signature = catalogue.sign(&digest(&entries)).to_bytes().to_vec();
    pack(entries.into_iter().chain([(SIGNATURE.to_owned(), signature)]))
}

pub fn replace_file(package: &[u8], path: &str, bytes: &[u8]) -> Vec<u8> {
    let mut files = unpack(package).expect("a package");
    files.insert(path.to_owned(), bytes.to_vec());
    pack(files)
}

fn check(manifest: &Manifest) -> Result<()> {
    ensure!(!manifest.name.trim().is_empty(), "the plugin has no name");
    ensure!(is_id(&manifest.id), "'{}' is not a plugin id", manifest.id);
    ensure!(is_version(&manifest.version), "'{}' is not a version", manifest.version);
    ensure!(is_version(&manifest.min_core_version), "'{}' is not a version", manifest.min_core_version);
    ensure!(!manifest.components.is_empty(), "the plugin registers no component");
    for component in &manifest.components {
        ensure!(is_component(component), "'{component}' is not a web component name");
    }
    ensure!(manifest.permissions.network.len() <= 8, "a plugin may not ask for that many hosts");
    for host in &manifest.permissions.network {
        ensure!(is_host(host), "'{host}' is not a host a plugin may talk to");
    }
    ensure!(manifest.opens.len() <= 16, "a plugin may not open that many kinds of file");
    for kind in &manifest.opens {
        ensure!(is_media_type(kind), "'{kind}' is not a kind of file a plugin may open");
    }
    ensure!(manifest.views.len() <= 16, "a plugin may not be the viewer of that many kinds of file");
    for kind in &manifest.views {
        ensure!(is_media_type(kind) && !kind.contains('*'), "'{kind}' is not an exact kind of file a plugin may view");
        ensure!(manifest.opens.contains(kind), "a viewer of '{kind}' must open it too");
    }
    ensure!(
        manifest.views.is_empty() || manifest.permissions.network.is_empty(),
        "a viewer is handed files without the user choosing, so it may not ask for the network"
    );
    ensure!(manifest.locales.len() <= LOCALES_LIMIT, "a plugin may not name itself in that many languages");
    for (code, said) in &manifest.locales {
        ensure!(is_language(code), "'{code}' is not a language code");
        let fits = |text: &Option<String>, limit: usize| {
            text.as_deref().is_none_or(|text| !text.trim().is_empty() && text.chars().count() <= limit)
        };
        ensure!(fits(&said.name, NAME_LIMIT), "the name in '{code}' is empty or too long");
        ensure!(fits(&said.summary, SUMMARY_LIMIT), "the summary in '{code}' is empty or too long");
        for (hook, labels) in &said.hooks {
            let Some((_, declared)) = manifest.hooks.iter().find(|(name, _)| name.as_str() == hook) else {
                bail!("a label in '{code}' for '{hook}', a hook it does not bind to");
            };
            check_hook_labels(code, hook, declared, labels)?;
        }
    }
    ensure!(manifest.icon.is_empty() || is_icon(&manifest.icon), "'{}' is not the name of an icon", manifest.icon);
    check_hooks(manifest)?;
    match manifest.kind {
        Kind::Tool => {}
        Kind::Game => {
            // a game only talks to its twin and writes in the chat, nothing leaves the conversation
            let asks = &manifest.permissions;
            let playing = Permissions { live: asks.live, send: asks.send, ..Permissions::default() };
            ensure!(*asks == playing, "a game may ask only for the live channel and for sending");
            ensure!(asks.send != Sending::Auto, "a game proposes what goes in the chat; it never sends by itself");
            ensure!(
                manifest.opens.is_empty() && manifest.views.is_empty(),
                "a game is opened from the games, never handed a file"
            );
        }
        Kind::Unknown => bail!("this FlickerTalk does not know that kind of plugin"),
    }
    Ok(())
}

fn a_label(text: &str, limit: usize) -> bool {
    !text.trim().is_empty() && text.chars().count() <= limit
}

// same rule as the SDK
fn is_field_key(key: &str) -> bool {
    (1..=32).contains(&key.len()) && key.starts_with(|c: char| c.is_ascii_lowercase()) && key.chars().all(|c| c.is_ascii_alphanumeric())
}

fn check_hooks(manifest: &Manifest) -> Result<()> {
    ensure!(manifest.hooks.is_empty() || manifest.kind == Kind::Tool, "only a tool binds to the chat's hooks");
    let asks = &manifest.permissions;
    for (name, hook) in &manifest.hooks {
        let hook_name = name.as_str();
        ensure!(a_label(&hook.label, NAME_LIMIT), "the label of '{hook_name}' is empty or too long");
        ensure!(is_icon(&hook.icon), "'{}' is not the name of an icon", hook.icon);
        match name {
            HookName::Message => ensure!(asks.reads_given_messages, "the message hook reads the message: it needs messages: given"),
            HookName::Composer => {
                ensure!(asks.reads_given_messages, "the composer hook reads the draft: it needs messages: given");
                ensure!(asks.send == Sending::Propose, "the composer hook proposes a text: it needs send: propose");
            }
            HookName::Settings => {
                ensure!((1..=FIELDS_LIMIT).contains(&hook.fields.len()), "the settings hook draws one to {FIELDS_LIMIT} fields");
                for (at, field) in hook.fields.iter().enumerate() {
                    ensure!(is_field_key(&field.key), "'{}' is not the key of a setting", field.key);
                    ensure!(hook.fields[..at].iter().all(|before| before.key != field.key), "two settings under '{}'", field.key);
                    ensure!(a_label(&field.label, NAME_LIMIT), "the label of the setting '{}' is empty or too long", field.key);
                }
                ensure!(hook.note.is_empty() || a_label(&hook.note, SUMMARY_LIMIT), "the note of the settings is blank or too long");
            }
        }
        if *name != HookName::Settings {
            ensure!(hook.fields.is_empty() && hook.note.is_empty(), "only the settings hook has fields and a note");
        }
    }
    Ok(())
}

fn check_hook_labels(code: &str, hook: &str, declared: &HookDeclaration, labels: &serde_json::Value) -> Result<()> {
    let fits = |text: &serde_json::Value, limit: usize| text.as_str().is_some_and(|text| a_label(text, limit));
    if hook != HookName::Settings.as_str() {
        ensure!(fits(labels, NAME_LIMIT), "the label of '{hook}' in '{code}' is not a label, or is empty or too long");
        return Ok(());
    }
    let Some(said) = labels.as_object() else { bail!("the settings' labels in '{code}' are not an object") };
    for (key, value) in said {
        match key.as_str() {
            "label" => ensure!(fits(value, NAME_LIMIT), "the settings' label in '{code}' is empty or too long"),
            "note" => ensure!(fits(value, SUMMARY_LIMIT), "the settings' note in '{code}' is empty or too long"),
            "fields" => {
                let Some(fields) = value.as_object() else { bail!("the settings' field labels in '{code}' are not by key") };
                for (field, label) in fields {
                    ensure!(declared.fields.iter().any(|one| &one.key == field), "a label in '{code}' for '{field}', a setting it does not draw");
                    ensure!(fits(label, NAME_LIMIT), "the label of the setting '{field}' in '{code}' is empty or too long");
                }
            }
            // ignore what a newer app adds here
            _ => {}
        }
    }
    Ok(())
}

fn is_media_type(kind: &str) -> bool {
    let Some((kind, subtype)) = kind.split_once('/') else { return false };
    let token = |part: &str| {
        !part.is_empty()
            && part.len() <= 64
            && part.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
            && part.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '.' | '+' | '_'))
    };
    (kind == "*" && subtype == "*") || (token(kind) && (subtype == "*" || token(subtype)))
}

impl Manifest {
    pub fn views_kind(&self, mime: &str) -> bool {
        let mime = mime.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
        self.views.iter().any(|kind| kind == &mime)
    }

    pub fn opens_kind(&self, mime: &str) -> bool {
        let mime = mime.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
        self.opens.iter().any(|kind| {
            kind == "*/*"
                || kind == &mime
                || kind.strip_suffix("/*").is_some_and(|prefix| mime.split_once('/').is_some_and(|(top, _)| top == prefix))
        })
    }
}

// no wildcards, no plugin gets "the internet"
fn is_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.split('.').count() >= 2
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
}

fn is_icon(name: &str) -> bool {
    (1..=64).contains(&name.len()) && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

// lenient on purpose, a code only a newer app speaks must not refuse the plugin
fn is_language(code: &str) -> bool {
    (1..=16).contains(&code.len()) && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn is_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.split('.').count() >= 2
        && id.split('.').all(|part| {
            !part.is_empty() && part.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        })
}

fn is_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3 && parts.iter().all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

fn is_component(name: &str) -> bool {
    name.contains('-')
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn safe_path(path: &str) -> Result<PathBuf> {
    let candidate = Path::new(path);
    ensure!(!path.is_empty() && path.len() <= 255, "'{path}' is not a path inside the package");
    for part in candidate.components() {
        match part {
            std::path::Component::Normal(_) => {}
            _ => bail!("'{path}' leaves the plugin's folder"),
        }
    }
    Ok(candidate.to_path_buf())
}

// a zip bomb, whoever signed it
pub const UNPACKED_LIMIT: u64 = 32 * 1024 * 1024;
pub const ENTRY_LIMIT: usize = 256;

fn unpack(package: &[u8]) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut archive = ZipArchive::new(Cursor::new(package)).context("the package is not a .ftplugin")?;
    ensure!(archive.len() <= ENTRY_LIMIT, "the package holds too many files");
    let mut files = BTreeMap::new();
    let mut total: u64 = 0;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if entry.is_dir() {
            continue;
        }
        // check what the header claims and what really comes out
        total = total.saturating_add(entry.size());
        ensure!(total <= UNPACKED_LIMIT, "the package inflates beyond what a plugin may weigh");
        let name = entry.name()?.into_owned();
        let mut bytes = Vec::new();
        let allowed = UNPACKED_LIMIT - (total - entry.size());
        std::io::Read::take(&mut entry, allowed + 1).read_to_end(&mut bytes)?;
        ensure!(bytes.len() as u64 <= allowed, "the package inflates beyond what a plugin may weigh");
        ensure!(bytes.len() as u64 == entry.size(), "an entry is not the size its header says");
        files.insert(name, bytes);
    }
    Ok(files)
}

fn pack(files: impl IntoIterator<Item = (String, Vec<u8>)>) -> Vec<u8> {
    // the catalogue lists packages by hash, so every byte must come out as it did when published
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .compression_level(Some(6))
        .last_modified_time(DateTime::default())
        .unix_permissions(0o644)
        .system(System::Unix)
        .large_file(false);
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, bytes) in files {
        writer.start_file(path, options).expect("writes the entry");
        writer.write_all(&bytes).expect("writes the bytes");
    }
    writer.finish().expect("closes the package").into_inner()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogueEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub min_core_version: String,
    pub size: u64,
    pub hash: String,
    pub url: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub locales: BTreeMap<String, Localized>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub icon: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub image: String,
}

pub fn version_at_least(core_version: &str, min: &str) -> bool {
    fn parts(version: &str) -> Vec<u32> {
        version.split('.').map(|part| part.parse().unwrap_or(0)).collect()
    }
    parts(core_version) >= parts(min)
}

impl CatalogueEntry {
    pub fn runs_on(&self, core_version: &str) -> bool {
        version_at_least(core_version, &self.min_core_version)
    }
}

#[derive(Deserialize)]
struct Catalogue {
    plugins: Vec<CatalogueEntry>,
}

pub fn catalogue_entries(index: &str, signature: &str, catalogue: &Ed25519PublicKey) -> Result<Vec<CatalogueEntry>> {
    let signature = Ed25519Signature::from_base64(signature).context("the signature is not one")?;
    catalogue
        .verify(index.as_bytes(), &signature)
        .map_err(|_| anyhow::anyhow!("the catalogue index is not signed by the catalogue"))?;
    let listed: Catalogue = serde_json::from_str(index).context("the index is not a catalogue")?;
    Ok(listed.plugins)
}

// the signature alone would also accept an older signed version, so check the hash too
pub fn download(entry: &CatalogueEntry, package: &[u8], catalogue: &Ed25519PublicKey) -> Result<Plugin> {
    ensure!(blake3::hash(package).to_hex().as_str() == entry.hash, "the download is not what the catalogue listed");
    let plugin = open(package, catalogue)?;
    ensure!(plugin.manifest.id == entry.id, "the package is not the plugin that was listed");
    ensure!(plugin.manifest.version == entry.version, "the package is not the version that was listed");
    Ok(plugin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vodozemac::Ed25519SecretKey;

    fn index_of(hash: &str) -> String {
        format!(
            r#"{{"plugins":[{{"id":"com.example.translator","name":"Translator","version":"1.2.0","minCoreVersion":"0.1.0","size":2048,"hash":"{hash}","url":"https://plugins.flickertalk.com/plugins/translator/1.2.0.ftplugin","summary":"Translates a message you choose."}}]}}"#
        )
    }

    #[test]
    fn the_catalogue_is_read_only_if_the_catalogue_signed_it() {
        let catalogue = Ed25519SecretKey::new();
        let index = index_of(&"ab".repeat(32));
        let signature = catalogue.sign(index.as_bytes()).to_base64();

        let listed = catalogue_entries(&index, &signature, &catalogue.public_key()).expect("signed");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "com.example.translator");
        assert_eq!(listed[0].url, "https://plugins.flickertalk.com/plugins/translator/1.2.0.ftplugin");

        let stranger = Ed25519SecretKey::new();
        let forged = stranger.sign(index.as_bytes()).to_base64();
        assert!(catalogue_entries(&index, &forged, &catalogue.public_key()).is_err(), "not our catalogue");
    }

    #[test]
    fn a_download_that_is_not_what_the_catalogue_listed_is_refused() {
        let catalogue = Ed25519SecretKey::new();
        let bytes = package(&manifest_of("com.example.translator"), b"export default {}", &catalogue);
        let index = index_of(&blake3::hash(&bytes).to_hex());
        let signature = catalogue.sign(index.as_bytes()).to_base64();
        let listed = catalogue_entries(&index, &signature, &catalogue.public_key()).expect("signed");

        let plugin = download(&listed[0], &bytes, &catalogue.public_key()).expect("what was listed");
        assert_eq!(plugin.manifest.id, "com.example.translator");

        let other = package(&manifest_of("com.example.translator"), b"steal_everything()", &catalogue);
        assert!(download(&listed[0], &other, &catalogue.public_key()).is_err(), "signed, but not the listed bytes");
    }

    #[test]
    fn a_plugin_for_a_newer_core_is_not_offered() {
        let entry = CatalogueEntry {
            id: "com.example.translator".to_owned(),
            name: "Translator".to_owned(),
            version: "1.2.0".to_owned(),
            min_core_version: "9.0.0".to_owned(),
            size: 10,
            hash: "ab".repeat(32),
            url: "https://plugins.flickertalk.com/x.ftplugin".to_owned(),
            summary: String::new(),
            kind: Kind::Tool,
            locales: BTreeMap::new(),
            icon: String::new(),
            image: String::new(),
        };
        assert!(!entry.runs_on("0.1.0"));
        assert!(entry.runs_on("9.0.0"));
        assert!(entry.runs_on("9.1.0"));
    }

    fn game_with(rest: &str) -> String {
        format!(
            r#"{{"id":"com.flickertalk.game.chess","name":"Chess","version":"1.0.0","minCoreVersion":"1.3.0","components":["ft-chess"],"kind":"game"{rest}}}"#
        )
    }

    #[test]
    fn a_plugin_is_a_tool_unless_it_says_it_is_a_game() {
        let catalogue = Ed25519SecretKey::new();
        let plain = open(&package(&manifest_of("com.example.code"), b"", &catalogue), &catalogue.public_key()).unwrap();
        assert_eq!(plain.manifest.kind, Kind::Tool);
        let said = manifest_of("com.example.code").replacen('{', r#"{"kind":"tool","#, 1);
        assert_eq!(open(&package(&said, b"", &catalogue), &catalogue.public_key()).unwrap().manifest.kind, Kind::Tool);

        let chess = game_with(r#","permissions":{"live":true,"send":"propose"}"#);
        let game = open(&package(&chess, b"", &catalogue), &catalogue.public_key()).unwrap();
        assert_eq!(game.manifest.kind, Kind::Game);
        assert_eq!(serde_json::to_value(Kind::Tool).unwrap(), "tool");
        assert_eq!(serde_json::to_value(Kind::Game).unwrap(), "game");
    }

    #[test]
    fn a_game_may_ask_only_for_the_live_channel_and_sending() {
        let catalogue = Ed25519SecretKey::new();
        let opens = |manifest: &str| open(&package(manifest, b"", &catalogue), &catalogue.public_key());
        assert!(opens(&game_with("")).is_ok(), "a game that asks for nothing");
        assert!(opens(&game_with(r#","permissions":{"live":true,"send":"propose"}"#)).is_ok());
        for wrong in [
            r#","permissions":{"live":true,"network":["api.example.com"]}"#,
            r#","permissions":{"live":true,"messages":"given"}"#,
            r#","permissions":{"live":true,"print":true}"#,
            r#","permissions":{"live":true,"remind":true}"#,
            r#","permissions":{"live":true,"drive":true}"#,
            r#","permissions":{"live":true,"storage":"large"}"#,
            r#","permissions":{"live":true,"location":true}"#,
            r#","permissions":{"live":true,"translate":true}"#,
            r#","permissions":{"live":true,"send":"auto"}"#,
            r#","opens":["image/*"]"#,
            r#","opens":["application/x-ftchess"],"views":["application/x-ftchess"]"#,
        ] {
            assert!(opens(&game_with(wrong)).is_err(), "a game with {wrong} should be refused");
            let tool = game_with(wrong).replace(r#""kind":"game""#, r#""kind":"tool""#);
            assert!(opens(&tool).is_ok(), "a tool with {wrong} is fine");
        }
    }

    #[test]
    fn a_kind_this_core_does_not_know_is_not_run_but_does_not_break_the_catalogue() {
        let catalogue = Ed25519SecretKey::new();
        let widget = manifest_of("com.example.widget").replacen('{', r#"{"kind":"widget","#, 1);
        assert!(open(&package(&widget, b"", &catalogue), &catalogue.public_key()).is_err(), "not a kind this core runs");

        let entry = |id: &str, kind: &str| {
            format!(
                r#"{{"id":"{id}","name":"X","version":"1.0.0","minCoreVersion":"9.0.0","size":1,"hash":"{}","url":"https://flickertalk.com/plugins/x.ftplugin"{kind}}}"#,
                "ab".repeat(32)
            )
        };
        let index = format!(
            r#"{{"plugins":[{},{},{}]}}"#,
            entry("com.example.tool", ""),
            entry("com.example.chess", r#","kind":"game""#),
            entry("com.example.widget", r#","kind":"widget""#)
        );
        let signature = catalogue.sign(index.as_bytes()).to_base64();
        let listed = catalogue_entries(&index, &signature, &catalogue.public_key()).expect("still a catalogue");
        assert_eq!(listed.iter().map(|entry| entry.kind).collect::<Vec<_>>(), [Kind::Tool, Kind::Game, Kind::Unknown]);
    }

    #[test]
    fn a_core_without_games_is_not_offered_one() {
        let entry = CatalogueEntry {
            id: "com.flickertalk.game.chess".to_owned(),
            name: "Chess".to_owned(),
            version: "1.0.0".to_owned(),
            min_core_version: GAMES_SINCE.to_owned(),
            size: 10,
            hash: "ab".repeat(32),
            url: "https://flickertalk.com/plugins/x.ftplugin".to_owned(),
            summary: String::new(),
            kind: Kind::Game,
            locales: BTreeMap::new(),
            icon: String::new(),
            image: String::new(),
        };
        assert!(!entry.runs_on("1.2.2"));
        assert!(entry.runs_on(GAMES_SINCE));
    }

    fn manifest_in(locales: &str) -> String {
        manifest_of("com.example.translator").replacen('{', &format!(r#"{{"locales":{locales},"#), 1)
    }

    #[test]
    fn a_manifest_may_carry_its_name_and_summary_in_other_languages() {
        let catalogue = Ed25519SecretKey::new();
        let opens = |manifest: &str| open(&package(manifest, b"", &catalogue), &catalogue.public_key());
        let plain = opens(&manifest_of("com.example.translator")).unwrap();
        assert!(plain.manifest.locales.is_empty(), "nothing translated unless it says so");

        let said = manifest_in(r#"{"es":{"name":"Traductor","summary":"Traduce un mensaje."},"zh-TW":{"name":"翻譯"},"de":{"summary":"Übersetzt."}}"#);
        let translated = opens(&said).unwrap().manifest;
        assert_eq!(translated.locales["es"].name.as_deref(), Some("Traductor"));
        assert_eq!(translated.locales["es"].summary.as_deref(), Some("Traduce un mensaje."));
        assert_eq!((translated.locales["zh-TW"].name.as_deref(), translated.locales["zh-TW"].summary.as_deref()), (Some("翻譯"), None));
        assert_eq!(translated.locales["de"].name, None, "a format keeps its English name");
        assert!(opens(&manifest_in(r#"{"es":{"name":"Traductor","tagline":"Nuevo"}}"#)).is_ok());
    }

    #[test]
    fn a_translated_name_or_summary_keeps_the_limits_of_the_english_one() {
        let catalogue = Ed25519SecretKey::new();
        let opens = |manifest: &str| open(&package(manifest, b"", &catalogue), &catalogue.public_key());
        let longest = format!(r#"{{"hi":{{"name":"{}","summary":"{}"}}}}"#, "न".repeat(64), "न".repeat(200));
        assert!(opens(&manifest_in(&longest)).is_ok(), "64 and 200 characters, whatever their bytes");
        for wrong in [
            r#"{"es":{"name":""}}"#.to_owned(),
            r#"{"es":{"name":"   "}}"#.to_owned(),
            format!(r#"{{"es":{{"name":"{}"}}}}"#, "x".repeat(65)),
            r#"{"es":{"summary":" "}}"#.to_owned(),
            format!(r#"{{"es":{{"summary":"{}"}}}}"#, "y".repeat(201)),
            r#"{"es":{"name":7}}"#.to_owned(),
            r#"{"es":"Traductor"}"#.to_owned(),
            r#"{"":{"name":"Traductor"}}"#.to_owned(),
            r#"{"../es":{"name":"Traductor"}}"#.to_owned(),
            format!(r#"{{"{}":{{"name":"Traductor"}}}}"#, "x".repeat(17)),
        ] {
            assert!(opens(&manifest_in(&wrong)).is_err(), "{wrong} should be refused");
        }
        let many = |count: usize| {
            let languages: Vec<String> = (0..count).map(|index| format!(r#""l{index:03}":{{"name":"x"}}"#)).collect();
            format!("{{{}}}", languages.join(","))
        };
        assert!(opens(&manifest_in(&many(64))).is_ok());
        assert!(opens(&manifest_in(&many(65))).is_err(), "no more than 64 languages");
    }

    #[test]
    fn the_catalogue_carries_the_translations_and_older_apps_read_it_still() {
        let catalogue = Ed25519SecretKey::new();
        let read = |index: &str| catalogue_entries(index, &catalogue.sign(index.as_bytes()).to_base64(), &catalogue.public_key());
        let plain = read(&index_of(&"ab".repeat(32))).expect("an index without translations");
        assert!(plain[0].locales.is_empty());
        let without = serde_json::to_value(&plain[0]).unwrap();
        assert!(without.get("locales").is_none(), "an untranslated entry is written as before: {without}");

        let translated = index_of(&"ab".repeat(32)).replacen(
            r#""summary":"#,
            r#""locales":{"es":{"name":"Traductor","summary":"Traduce el mensaje que eliges."}},"summary":"#,
            1,
        );
        let listed = read(&translated).expect("still signed, still a catalogue");
        assert_eq!(listed[0].locales["es"].name.as_deref(), Some("Traductor"));
        assert_eq!(serde_json::to_value(&listed[0]).unwrap()["locales"]["es"]["summary"], "Traduce el mensaje que eliges.");

        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct Before {
            id: String,
            name: String,
            version: String,
            #[serde(rename = "minCoreVersion")]
            min_core_version: String,
            size: u64,
            hash: String,
            url: String,
            #[serde(default)]
            summary: String,
        }
        #[derive(Deserialize)]
        struct Catalogue12 {
            plugins: Vec<Before>,
        }
        let old: Catalogue12 = serde_json::from_str(&translated).expect("an older app reads it");
        assert_eq!(old.plugins[0].name, "Translator", "and shows the English name");
    }

    fn manifest_drawn(icon: &str) -> String {
        manifest_of("com.example.translator").replacen('{', &format!(r#"{{"icon":"{icon}","#), 1)
    }

    #[test]
    fn a_manifest_may_name_the_icon_of_its_tile() {
        let catalogue = Ed25519SecretKey::new();
        let drawn = open(&package(&manifest_drawn("image-outline"), b"", &catalogue), &catalogue.public_key()).unwrap();
        assert_eq!(drawn.manifest.icon, "image-outline");
        assert_eq!(serde_json::to_value(&drawn.manifest).unwrap()["icon"], "image-outline");

        let plain = open(&package(&manifest_of("com.example.translator"), b"", &catalogue), &catalogue.public_key()).unwrap();
        assert_eq!(plain.manifest.icon, "");
        let written = serde_json::to_value(&plain.manifest).unwrap();
        assert!(written.get("icon").is_none(), "no icon, nothing written: {written}");
    }

    #[test]
    fn an_icon_that_is_not_an_ionicon_name_is_refused() {
        let catalogue = Ed25519SecretKey::new();
        let opens = |manifest: &str| open(&package(manifest, b"", &catalogue), &catalogue.public_key());
        assert!(opens(&manifest_drawn(&"x".repeat(64))).is_ok(), "64 characters is a name");
        assert!(opens(&manifest_drawn("logo-markdown")).is_ok());
        assert!(opens(&manifest_drawn("radio-button-on-outline")).is_ok());
        let longest = "x".repeat(65);
        for wrong in ["Image-Outline", "image outline", "../image", "image_outline", "imagé", "<svg>", "image/outline", longest.as_str()] {
            assert!(opens(&manifest_drawn(wrong)).is_err(), "{wrong:?} should be refused");
        }
        assert!(opens(&manifest_of("com.example.translator").replacen('{', r#"{"icon":7,"#, 1)).is_err(), "a name, not a number");
    }

    #[test]
    fn the_catalogue_carries_the_icon() {
        let catalogue = Ed25519SecretKey::new();
        let read = |index: &str| catalogue_entries(index, &catalogue.sign(index.as_bytes()).to_base64(), &catalogue.public_key());
        let plain = read(&index_of(&"ab".repeat(32))).expect("an index without icons");
        assert_eq!(plain[0].icon, "");
        let without = serde_json::to_value(&plain[0]).unwrap();
        assert!(without.get("icon").is_none(), "no icon, nothing written: {without}");

        let drawn = index_of(&"ab".repeat(32)).replacen(r#""summary":"#, r#""icon":"language-outline","summary":"#, 1);
        let listed = read(&drawn).expect("still signed, still a catalogue");
        assert_eq!(listed[0].icon, "language-outline");
        assert_eq!(serde_json::to_value(&listed[0]).unwrap()["icon"], "language-outline");
    }

    const IMAGE_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><title>Échecs</title><rect width="64" height="64" rx="18" fill="#f0742a"/></svg>"##;

    fn drawn_package(image: &str, signer: &Ed25519SecretKey) -> Vec<u8> {
        let files = vec![
            ("module.json".to_owned(), manifest_of("com.example.translator").into_bytes()),
            ("icon.svg".to_owned(), image.as_bytes().to_vec()),
            ("dist/index.js".to_owned(), b"".to_vec()),
        ];
        sign_package(&files, signer)
    }

    #[test]
    fn a_package_may_carry_the_image_of_its_tile_and_a_bad_one_is_refused() {
        let catalogue = Ed25519SecretKey::new();
        let drawn = open(&drawn_package(IMAGE_SVG, &catalogue), &catalogue.public_key()).expect("a good icon");
        assert_eq!(drawn.image(), IMAGE_SVG);
        let plain = open(&package(&manifest_of("com.example.translator"), b"", &catalogue), &catalogue.public_key()).unwrap();
        assert_eq!(plain.image(), "", "no icon.svg, no image");
        for wrong in [
            r#"<svg viewBox="0 0 64 64"><script>alert(1)</script></svg>"#,
            r#"<svg viewBox="0 0 64 64" onload="alert(1)"/>"#,
            r#"<svg viewBox="0 0 64 32"/>"#,
            "not an svg",
        ] {
            assert!(open(&drawn_package(wrong, &catalogue), &catalogue.public_key()).is_err(), "{wrong} should take the package down");
        }
    }

    #[test]
    fn the_image_of_an_installed_plugin_is_read_from_its_folder() {
        let catalogue = Ed25519SecretKey::new();
        let home = std::env::temp_dir().join(format!("ft-plugins-image-{}", blake3::hash(IMAGE_SVG.as_bytes()).to_hex()));
        let _ = std::fs::remove_dir_all(&home);
        install(&open(&drawn_package(IMAGE_SVG, &catalogue), &catalogue.public_key()).unwrap(), &home).unwrap();
        assert_eq!(image_of(&home, "com.example.translator"), IMAGE_SVG);
        assert_eq!(image_of(&home, "com.example.other"), "", "nothing installed, no image");
        std::fs::write(home.join("com.example.translator").join(icon::IMAGE), "<svg><script/></svg>").unwrap();
        assert_eq!(image_of(&home, "com.example.translator"), "", "one changed on the disk is not drawn");
        assert_eq!(image_of(&home, "../etc"), "", "never outside the folder");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn the_catalogue_carries_the_image() {
        let catalogue = Ed25519SecretKey::new();
        let read = |index: &str| catalogue_entries(index, &catalogue.sign(index.as_bytes()).to_base64(), &catalogue.public_key());
        let plain = read(&index_of(&"ab".repeat(32))).unwrap();
        assert_eq!(plain[0].image, "");
        assert!(serde_json::to_value(&plain[0]).unwrap().get("image").is_none());
        let image = serde_json::to_string(IMAGE_SVG).unwrap();
        let drawn = index_of(&"ab".repeat(32)).replacen(r#""summary":"#, &format!(r#""image":{image},"summary":"#), 1);
        let listed = read(&drawn).expect("still signed, still a catalogue");
        assert_eq!(listed[0].image, IMAGE_SVG);
        assert_eq!(serde_json::to_value(&listed[0]).unwrap()["image"], IMAGE_SVG);
    }

    fn package(manifest: &str, script: &[u8], signer: &Ed25519SecretKey) -> Vec<u8> {
        let mut files = vec![("module.json".to_owned(), manifest.as_bytes().to_vec())];
        files.push(("dist/index.js".to_owned(), script.to_vec()));
        sign_package(&files, signer)
    }

    fn manifest_of(id: &str) -> String {
        format!(
            r#"{{"id":"{id}","name":"Translator","version":"1.2.0","minCoreVersion":"0.1.0","components":["ft-translator"]}}"#
        )
    }

    fn manifest_with(permissions: &str) -> String {
        format!(
            r#"{{"id":"com.example.ai","name":"Assistant","version":"1.0.0","minCoreVersion":"0.1.0","components":["ft-ai"],"permissions":{permissions}}}"#
        )
    }

    #[test]
    fn a_plugin_asks_for_what_it_needs_and_nothing_else() {
        let catalogue = Ed25519SecretKey::new();
        let plain = open(&package(&manifest_of("com.example.code"), b"", &catalogue), &catalogue.public_key()).unwrap();
        assert_eq!(plain.manifest.permissions, Permissions::default());
        assert!(plain.manifest.permissions.network.is_empty(), "no network unless it asks");
        assert_eq!(plain.manifest.permissions.send, Sending::Nothing);

        let asking = manifest_with(r#"{"messages":"given","send":"propose","network":["api.openai.com"]}"#);
        let ai = open(&package(&asking, b"", &catalogue), &catalogue.public_key()).unwrap();
        assert_eq!(ai.manifest.permissions.network, ["api.openai.com"]);
        assert_eq!(ai.manifest.permissions.send, Sending::Propose);
        assert!(ai.manifest.permissions.reads_given_messages);
        assert!(!ai.manifest.permissions.print, "printing is asked for on its own");

        let printer = manifest_with(r#"{"print":true}"#);
        let opened = open(&package(&printer, b"", &catalogue), &catalogue.public_key()).unwrap();
        assert!(opened.manifest.permissions.print);
        assert!(opened.manifest.permissions.network.is_empty());
    }

    #[test]
    fn a_plugin_asks_for_the_phones_position_on_its_own() {
        let catalogue = Ed25519SecretKey::new();
        let asking = open(&package(&manifest_with(r#"{"location":true,"send":"propose"}"#), b"", &catalogue), &catalogue.public_key()).unwrap();
        let said = serde_json::to_value(&asking.manifest.permissions).unwrap();
        assert_eq!(said["location"], serde_json::json!(true), "the manifest asked for the position: {said}");
        let plain = open(&package(&manifest_of("com.example.code"), b"", &catalogue), &catalogue.public_key()).unwrap();
        assert_eq!(serde_json::to_value(&plain.manifest.permissions).unwrap()["location"], serde_json::json!(false), "never unless asked");
        for wrong in [r#"{"location":"always"}"#, r#"{"location":"background"}"#, r#"{"location":1}"#] {
            assert!(open(&package(&manifest_with(wrong), b"", &catalogue), &catalogue.public_key()).is_err(), "{wrong} is not a yes or a no");
        }
        let before: Permissions = serde_json::from_str(r#"{"network":[],"messages":"none","send":"nothing"}"#).unwrap();
        assert_eq!(serde_json::to_value(&before).unwrap()["location"], serde_json::json!(false));
    }

    #[test]
    fn a_plugin_asks_to_translate_on_its_own() {
        let catalogue = Ed25519SecretKey::new();
        let asking = open(&package(&manifest_with(r#"{"translate":true,"messages":"given","send":"propose"}"#), b"", &catalogue), &catalogue.public_key()).unwrap();
        let said = serde_json::to_value(&asking.manifest.permissions).unwrap();
        assert_eq!(said["translate"], serde_json::json!(true), "the manifest asked for the translator: {said}");
        let plain = open(&package(&manifest_of("com.example.code"), b"", &catalogue), &catalogue.public_key()).unwrap();
        assert_eq!(serde_json::to_value(&plain.manifest.permissions).unwrap()["translate"], serde_json::json!(false), "never unless asked");
        for wrong in [r#"{"translate":"always"}"#, r#"{"translate":1}"#, r#"{"translate":"cloud"}"#] {
            assert!(open(&package(&manifest_with(wrong), b"", &catalogue), &catalogue.public_key()).is_err(), "{wrong} is not a yes or a no");
        }
        let before: Permissions = serde_json::from_str(r#"{"network":[],"messages":"none","send":"nothing"}"#).unwrap();
        assert_eq!(serde_json::to_value(&before).unwrap()["translate"], serde_json::json!(false));
    }

    fn hooked(hooks: &str, permissions: &str) -> String {
        format!(
            r#"{{"id":"com.example.translate","name":"Translator","version":"1.1.0","minCoreVersion":"1.7.0","components":["ft-translate"],"permissions":{permissions},"hooks":{hooks}}}"#
        )
    }

    const SETTINGS: &str = r#"{"label":"Translation","icon":"language-outline","fields":[{"key":"lang","type":"select","label":"Language"},{"key":"receive","type":"toggle","label":"Translate what I receive"},{"key":"send","type":"toggle","label":"Send translated"}],"note":"Language packs are downloaded once; translation happens on the phone."}"#;
    fn every_hook() -> String {
        format!(r#"{{"message":{{"label":"Translate","icon":"language-outline"}},"composer":{{"label":"Translate","icon":"language-outline"}},"settings":{SETTINGS}}}"#)
    }
    const READS_AND_PROPOSES: &str = r#"{"messages":"given","send":"propose","translate":true}"#;

    #[test]
    fn a_tool_may_bind_to_the_chats_hooks() {
        let catalogue = Ed25519SecretKey::new();
        let opens = |manifest: &str| open(&package(manifest, b"", &catalogue), &catalogue.public_key());
        let tool = opens(&hooked(&every_hook(), READS_AND_PROPOSES)).expect("a tool with its hooks").manifest;
        assert_eq!(tool.hooks.len(), 3);
        assert_eq!(tool.hooks[&HookName::Message].label, "Translate");
        assert_eq!(tool.hooks[&HookName::Message].icon, "language-outline");
        assert!(tool.hooks[&HookName::Message].fields.is_empty());
        let settings = &tool.hooks[&HookName::Settings];
        assert_eq!(settings.label, "Translation");
        assert_eq!(
            settings.fields,
            vec![
                HookField { key: "lang".to_owned(), kind: FieldKind::Select, label: "Language".to_owned() },
                HookField { key: "receive".to_owned(), kind: FieldKind::Toggle, label: "Translate what I receive".to_owned() },
                HookField { key: "send".to_owned(), kind: FieldKind::Toggle, label: "Send translated".to_owned() },
            ]
        );
        assert_eq!(settings.note, "Language packs are downloaded once; translation happens on the phone.");
        let said = serde_json::to_value(&tool).unwrap();
        assert_eq!(said["hooks"]["composer"]["icon"], "language-outline", "{said}");
        assert_eq!(said["hooks"]["settings"]["fields"][0], serde_json::json!({ "key": "lang", "type": "select", "label": "Language" }));
        assert!(said["hooks"]["message"].get("fields").is_none() && said["hooks"]["message"].get("note").is_none(), "{said}");

        let plain = opens(&manifest_of("com.example.code")).unwrap().manifest;
        assert!(plain.hooks.is_empty(), "no hooks unless it says so");
        assert!(serde_json::to_value(&plain).unwrap().get("hooks").is_none(), "none, nothing written");
        let settings_only = hooked(&format!(r#"{{"settings":{SETTINGS}}}"#), "{}");
        assert_eq!(opens(&settings_only).unwrap().manifest.hooks.len(), 1);
    }

    #[test]
    fn a_hook_the_app_does_not_know_or_whose_needs_are_not_asked_for_is_refused() {
        let catalogue = Ed25519SecretKey::new();
        let opens = |manifest: &str| open(&package(manifest, b"", &catalogue), &catalogue.public_key());
        let message = r#"{"message":{"label":"Translate","icon":"language-outline"}}"#;
        let composer = r#"{"composer":{"label":"Translate","icon":"language-outline"}}"#;
        assert!(opens(&hooked(message, r#"{"messages":"given"}"#)).is_ok());
        assert!(opens(&hooked(composer, r#"{"messages":"given","send":"propose"}"#)).is_ok());
        let settings = |fields: &str, note: &str| format!(r#"{{"settings":{{"label":"Translation","icon":"language-outline","fields":{fields}{note}}}}}"#);
        let field = |key: &str, kind: &str, label: &str| format!(r#"{{"key":"{key}","type":"{kind}","label":"{label}"}}"#);
        let nine = format!("[{}]", (0..9).map(|at| field(&format!("f{at}"), "toggle", "On")).collect::<Vec<_>>().join(","));
        let wrong: Vec<(String, &str)> = vec![
            (message.to_owned(), "{}"),
            (composer.to_owned(), r#"{"send":"propose"}"#),
            (composer.to_owned(), r#"{"messages":"given"}"#),
            (composer.to_owned(), r#"{"messages":"given","send":"auto"}"#),
            (r#"{"chat":{"label":"Chat language","icon":"language-outline"}}"#.to_owned(), "{}"),
            (r#"{"reaction":{"label":"React","icon":"happy-outline"}}"#.to_owned(), READS_AND_PROPOSES),
            (r#"{"message":{"label":"","icon":"language-outline"}}"#.to_owned(), READS_AND_PROPOSES),
            (r#"{"message":{"label":"   ","icon":"language-outline"}}"#.to_owned(), READS_AND_PROPOSES),
            (format!(r#"{{"message":{{"label":"{}","icon":"language-outline"}}}}"#, "x".repeat(65)), READS_AND_PROPOSES),
            (r#"{"message":{"label":"Translate","icon":"Language Outline"}}"#.to_owned(), READS_AND_PROPOSES),
            (r#"{"message":{"label":"Translate","icon":""}}"#.to_owned(), READS_AND_PROPOSES),
            (r#"{"message":{"label":"Translate"}}"#.to_owned(), READS_AND_PROPOSES),
            (r#"{"message":"Translate"}"#.to_owned(), READS_AND_PROPOSES),
            (format!(r#"{{"message":{{"label":"Translate","icon":"language-outline","fields":[{}]}}}}"#, field("lang", "select", "Language")), READS_AND_PROPOSES),
            (r#"{"message":{"label":"Translate","icon":"language-outline","note":"A note"}}"#.to_owned(), READS_AND_PROPOSES),
            (settings("[]", ""), "{}"),
            (settings(&nine, ""), "{}"),
            (settings(&format!("[{}]", field("lang", "slider", "Language")), ""), "{}"),
            (settings(&format!("[{}]", field("Lang", "select", "Language")), ""), "{}"),
            (settings(&format!("[{}]", field("send_now", "toggle", "Send")), ""), "{}"),
            (settings(&format!("[{}]", field("9lives", "toggle", "Lives")), ""), "{}"),
            (settings(&format!("[{}]", field(&"k".repeat(33), "toggle", "Long")), ""), "{}"),
            (settings(&format!("[{}]", field("", "select", "Language")), ""), "{}"),
            (settings(&format!("[{}]", field("lang", "select", "")), ""), "{}"),
            (settings(&format!("[{},{}]", field("lang", "select", "Language"), field("lang", "toggle", "Again")), ""), "{}"),
            (settings(&format!("[{}]", field("lang", "select", "Language")), &format!(r#","note":"{}""#, "n".repeat(201))), "{}"),
            (settings(&format!("[{}]", field("lang", "select", "Language")), r#","note":"  ""#), "{}"),
        ];
        for (hooks, permissions) in &wrong {
            assert!(opens(&hooked(hooks, permissions)).is_err(), "{hooks} with {permissions} should be refused");
        }
        let eight = format!("[{}]", (0..8).map(|at| field(&format!("sendTranslated{at}"), "toggle", "On")).collect::<Vec<_>>().join(","));
        assert!(opens(&hooked(&settings(&eight, r#","note":"A note""#), "{}")).is_ok(), "eight fields and a note");
        let game = game_with(&format!(r#","permissions":{{"live":true}},"hooks":{{"settings":{SETTINGS}}}"#));
        assert!(opens(&game).is_err(), "a game binds to no hook of the chat");
    }

    #[test]
    fn a_hooks_labels_may_come_in_other_languages() {
        let catalogue = Ed25519SecretKey::new();
        let opens = |manifest: &str| open(&package(manifest, b"", &catalogue), &catalogue.public_key());
        let with = |locales: &str| hooked(&every_hook(), READS_AND_PROPOSES).replacen('{', &format!(r#"{{"locales":{locales},"#), 1);
        let spanish = r#"{"es":{"name":"Traductor","hooks":{"message":"Traducir","settings":{"label":"Traducción","fields":{"lang":"Idioma","send":"Enviar traducido"},"note":"Los paquetes se descargan una vez."}}}}"#;
        let tool = opens(&with(spanish)).unwrap().manifest;
        assert_eq!(tool.locales["es"].hooks["message"], "Traducir");
        assert_eq!(tool.locales["es"].hooks["settings"]["fields"]["lang"], "Idioma");
        assert!(!tool.locales["es"].hooks.contains_key("composer"), "a label left out is the English one");
        assert_eq!(serde_json::to_value(&tool).unwrap()["locales"]["es"]["hooks"]["settings"]["note"], "Los paquetes se descargan una vez.");
        assert!(serde_json::to_value(&opens(&manifest_in(r#"{"es":{"name":"Traductor"}}"#)).unwrap().manifest).unwrap()["locales"]["es"].get("hooks").is_none());
        for wrong in [
            r#"{"es":{"hooks":{"reaction":"Reaccionar"}}}"#.to_owned(),
            r#"{"es":{"hooks":{"message":""}}}"#.to_owned(),
            format!(r#"{{"es":{{"hooks":{{"message":"{}"}}}}}}"#, "x".repeat(65)),
            r#"{"es":{"hooks":{"message":7}}}"#.to_owned(),
            r#"{"es":{"hooks":{"message":{"label":"Traducir"}}}}"#.to_owned(),
            r#"{"es":{"hooks":{"settings":{"fields":{"speed":"Velocidad"}}}}}"#.to_owned(),
            r#"{"es":{"hooks":{"settings":{"fields":{"lang":""}}}}}"#.to_owned(),
            r#"{"es":{"hooks":{"settings":{"label":"  "}}}}"#.to_owned(),
            format!(r#"{{"es":{{"hooks":{{"settings":{{"note":"{}"}}}}}}}}"#, "n".repeat(201)),
            r#"{"es":{"hooks":{"settings":{"fields":["Idioma"]}}}}"#.to_owned(),
        ] {
            assert!(opens(&with(&wrong)).is_err(), "{wrong} should be refused");
        }
        let unbound = hooked(&format!(r#"{{"settings":{SETTINGS}}}"#), "{}").replacen('{', r#"{"locales":{"es":{"hooks":{"message":"Traducir"}}},"#, 1);
        assert!(opens(&unbound).is_err());
    }

    #[test]
    fn the_catalogue_is_read_with_labels_of_hooks_this_app_does_not_know() {
        let catalogue = Ed25519SecretKey::new();
        let index = index_of(&"ab".repeat(32))
            .replacen(r#""summary":"#, r#""locales":{"es":{"name":"Traductor","hooks":{"message":"Traducir","reaction":"Reaccionar"}}},"summary":"#, 1);
        let listed = catalogue_entries(&index, &catalogue.sign(index.as_bytes()).to_base64(), &catalogue.public_key()).expect("still a catalogue");
        assert_eq!(listed[0].locales["es"].hooks["message"], "Traducir");
    }

    #[test]
    fn a_plugin_asks_for_the_live_channel_reminders_the_drive_and_room_on_their_own() {
        let catalogue = Ed25519SecretKey::new();
        let asking = manifest_with(r#"{"live":true,"remind":true,"drive":true,"storage":"large"}"#);
        let opened = open(&package(&asking, b"", &catalogue), &catalogue.public_key()).unwrap();
        let permissions = &opened.manifest.permissions;
        assert!(permissions.live && permissions.remind && permissions.drive);
        assert_eq!(permissions.storage, Storage::Large);
        assert!(Storage::Large.quota() > Storage::Small.quota());
        let plain = open(&package(&manifest_of("com.example.code"), b"", &catalogue), &catalogue.public_key()).unwrap();
        assert!(!plain.manifest.permissions.live && !plain.manifest.permissions.remind && !plain.manifest.permissions.drive);
        assert_eq!(plain.manifest.permissions.storage, Storage::Small);
        assert!(open(&package(&manifest_with(r#"{"storage":"huge"}"#), b"", &catalogue), &catalogue.public_key()).is_err());

        let opens = r#"{"id":"com.example.board","name":"Board","version":"1.0.0","minCoreVersion":"0.1.0","components":["ft-board"],"opens":["application/x-ftboard","image/*"]}"#;
        let board = open(&package(opens, b"", &catalogue), &catalogue.public_key()).unwrap();
        assert!(board.manifest.opens_kind("application/x-ftboard"));
        assert!(board.manifest.opens_kind("image/png"));
        assert!(board.manifest.opens_kind("IMAGE/JPEG; charset=x"));
        assert!(!board.manifest.opens_kind("video/mp4"));
        assert!(!plain.manifest.opens_kind("image/png"), "it opens nothing unless it says so");
        let viewer = r#"{"id":"com.example.pdf","name":"PDF","version":"1.0.0","minCoreVersion":"0.1.0","components":["ft-pdf"],"opens":["application/pdf"],"views":["application/pdf"]}"#;
        let pdf = open(&package(viewer, b"", &catalogue), &catalogue.public_key()).unwrap();
        assert!(pdf.manifest.views_kind("application/pdf"));
        assert!(pdf.manifest.views_kind("Application/PDF; charset=x"));
        assert!(!pdf.manifest.views_kind("image/png"));
        assert!(!board.manifest.views_kind("image/png"), "opening is not viewing");
        for wrong in [
            r#""opens":["application/pdf"],"views":["application/*"]"#,
            r#""opens":["application/pdf"],"views":["*/*"]"#,
            r#""opens":["image/*"],"views":["application/pdf"]"#,
            r#""opens":["application/pdf"],"views":["application/pdf"],"permissions":{"network":["api.example.com"]}"#,
        ] {
            let bad = format!(r#"{{"id":"com.example.x","name":"X","version":"1.0.0","minCoreVersion":"0.1.0","components":["ft-x"],{wrong}}}"#);
            assert!(open(&package(&bad, b"", &catalogue), &catalogue.public_key()).is_err(), "{wrong}");
        }
        let any = r#"{"id":"com.example.drive","name":"Drive","version":"1.0.0","minCoreVersion":"0.1.0","components":["ft-drive"],"opens":["*/*"]}"#;
        assert!(open(&package(any, b"", &catalogue), &catalogue.public_key()).unwrap().manifest.opens_kind("video/mp4"));
        for wrong in ["png", "image/", "*/png", "image/*; q=1", "../x"] {
            let bad = format!(r#"{{"id":"com.example.x","name":"X","version":"1.0.0","minCoreVersion":"0.1.0","components":["ft-x"],"opens":["{wrong}"]}}"#);
            assert!(open(&package(&bad, b"", &catalogue), &catalogue.public_key()).is_err(), "{wrong} should be refused");
        }
    }

    #[test]
    fn a_plugin_cannot_ask_for_the_whole_internet() {
        let catalogue = Ed25519SecretKey::new();
        for asked in [
            r#"{"network":["*"]}"#,
            r#"{"network":["*.com"]}"#,
            r#"{"network":["https://api.openai.com"]}"#,
            r#"{"network":["api.openai.com/v1/chat"]}"#,
            r#"{"network":["API.OPENAI.COM"]}"#,
            r#"{"network":[""]}"#,
        ] {
            let bytes = package(&manifest_with(asked), b"", &catalogue);
            assert!(open(&bytes, &catalogue.public_key()).is_err(), "{asked} should be refused");
        }
    }

    #[test]
    fn the_content_security_policy_matches_what_was_granted() {
        let none = Permissions::default();
        assert!(none.content_security_policy().contains("connect-src 'none'"));
        assert!(none.content_security_policy().starts_with("default-src 'none'"));
        let policy = none.content_security_policy();
        assert!(policy.contains("script-src http://ftplugin.localhost"), "{policy}");
        assert!(policy.contains("ftplugin://localhost"), "{policy}");
        assert!(!policy.contains("script-src 'self'"), "{policy}");
        assert!(policy.contains("frame-ancestors http://tauri.localhost tauri://localhost"), "{policy}");
        assert!(policy.contains("child-src 'none'"), "{policy}");
        assert!(policy.contains("img-src") && policy.contains("data: blob:"), "{policy}");
        assert!(!policy.contains("frame-ancestors 'none'"), "that would keep the app from showing it");

        let ai = Permissions { network: vec!["api.openai.com".to_owned()], ..Permissions::default() };
        assert!(ai.content_security_policy().contains("connect-src https://api.openai.com"));
        assert!(!ai.content_security_policy().contains("'unsafe-eval'"));
    }

    #[test]
    fn the_catalogue_key_ships_with_the_app() {
        let key = catalogue();
        assert_eq!(key.to_base64(), CATALOGUE_KEY);
        let stranger = Ed25519SecretKey::new();
        let bytes = package(&manifest_of("com.example.code"), b"", &stranger);
        assert!(open(&bytes, &key).is_err(), "someone else's signature is not the catalogue's");
    }

    #[test]
    fn a_package_signed_by_the_catalogue_is_accepted() {
        let catalogue = Ed25519SecretKey::new();
        let bytes = package(&manifest_of("com.example.translator"), b"export default {}", &catalogue);

        let plugin = open(&bytes, &catalogue.public_key()).expect("the catalogue signed it");
        assert_eq!(plugin.manifest.id, "com.example.translator");
        assert_eq!(plugin.manifest.version, "1.2.0");
        assert_eq!(plugin.manifest.components, ["ft-translator"]);
        assert_eq!(plugin.file("dist/index.js"), Some(&b"export default {}"[..]));
    }

    #[test]
    fn a_package_someone_else_signed_is_refused() {
        let (catalogue, stranger) = (Ed25519SecretKey::new(), Ed25519SecretKey::new());
        let bytes = package(&manifest_of("com.example.translator"), b"export default {}", &stranger);
        assert!(open(&bytes, &catalogue.public_key()).is_err(), "only the catalogue's key counts");
    }

    #[test]
    fn a_changed_file_breaks_the_signature() {
        let catalogue = Ed25519SecretKey::new();
        let bytes = package(&manifest_of("com.example.translator"), b"export default {}", &catalogue);
        let tampered = replace_file(&bytes, "dist/index.js", b"steal_everything()");
        assert!(open(&tampered, &catalogue.public_key()).is_err(), "the bytes are not the signed ones");
    }

    #[test]
    fn packs_the_same_bytes_the_published_packages_were_packed_with() {
        let script: String =
            (0..64).map(|n| format!("export const tile{n} = {{ row: {}, column: {} }};\n", n / 8, n % 8)).collect();
        let files = [
            ("dist/index.js".to_owned(), script.into_bytes()),
            (
                "module.json".to_owned(),
                br#"{"id":"com.flickertalk.sketch","name":"Sketch","version":"1.0.0","minCoreVersion":"1.0.0","components":["ft-sketch"]}"#.to_vec(),
            ),
            ("signature".to_owned(), vec![7; 64]),
        ];
        assert_eq!(
            blake3::hash(&pack(files)).to_hex().as_str(),
            "6fc841b8fe08143ebfea5f9994770be15d56ceea7ce4431cd35e80a1f6ef568f"
        );
    }

    #[test]
    fn a_manifest_that_does_not_hold_up_is_refused() {
        let catalogue = Ed25519SecretKey::new();
        let bad = r#"{"id":"com.example.x","name":"X","version":"1.0.0","minCoreVersion":"0.1.0","components":["translator"]}"#;
        let bytes = package(bad, b"", &catalogue);
        assert!(open(&bytes, &catalogue.public_key()).is_err(), "the component name is not one");

        let bad_id = r#"{"id":"../../etc","name":"X","version":"1.0.0","minCoreVersion":"0.1.0","components":["ft-x"]}"#;
        let bytes = package(bad_id, b"", &catalogue);
        assert!(open(&bytes, &catalogue.public_key()).is_err(), "the id is not a plugin id");
    }

    #[test]
    fn a_package_that_inflates_beyond_the_limit_is_refused() {
        let catalogue = Ed25519SecretKey::new();
        let bomb = vec![0u8; UNPACKED_LIMIT as usize + 1];
        let files = vec![
            ("module.json".to_owned(), manifest_of("com.example.bomb").into_bytes()),
            ("dist/index.js".to_owned(), bomb),
        ];
        let bytes = sign_package(&files, &catalogue);
        assert!(bytes.len() < 1024 * 1024, "zeros compress well: {} bytes", bytes.len());
        assert!(open(&bytes, &catalogue.public_key()).is_err(), "a bomb is not a plugin");
        let ok = package(&manifest_of("com.example.fine"), &[1u8; 100_000], &catalogue);
        assert!(open(&ok, &catalogue.public_key()).is_ok());
    }

    #[test]
    fn a_package_that_wants_to_write_outside_its_folder_is_refused() {
        let catalogue = Ed25519SecretKey::new();
        let files = vec![
            ("module.json".to_owned(), manifest_of("com.example.translator").into_bytes()),
            ("../../../etc/passwd".to_owned(), b"nope".to_vec()),
        ];
        let bytes = sign_package(&files, &catalogue);
        assert!(open(&bytes, &catalogue.public_key()).is_err(), "no path may leave the package");
    }

    fn version_of(id: &str, version: &str, script: &[u8], catalogue: &Ed25519SecretKey) -> Plugin {
        let manifest = manifest_of(id).replace(r#""version":"1.2.0""#, &format!(r#""version":"{version}""#));
        open(&package(&manifest, script, catalogue), &catalogue.public_key()).expect("opens")
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ft-plugins-{name}-{}", blake3::hash(format!("{:?}", std::time::SystemTime::now()).as_bytes()).to_hex()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn folders(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }

    #[test]
    fn a_new_version_replaces_the_old_one_whole() {
        let catalogue = Ed25519SecretKey::new();
        let dir = scratch("swap");
        install(&version_of("com.example.code", "1.0.0", b"old()", &catalogue), &dir).unwrap();
        let home = install(&version_of("com.example.code", "1.0.1", b"new()", &catalogue), &dir).unwrap();
        assert_eq!(std::fs::read(home.join("dist/index.js")).unwrap(), b"new()");
        assert_eq!(folders(&dir), ["com.example.code"], "no copy is left beside it");
        assert_eq!(installed(&dir).unwrap().iter().map(|one| one.version.as_str()).collect::<Vec<_>>(), ["1.0.1"]);
    }

    #[test]
    fn an_install_that_fails_leaves_the_installed_version_as_it_was() {
        let catalogue = Ed25519SecretKey::new();
        let dir = scratch("failed");
        install(&version_of("com.example.code", "1.0.0", b"old()", &catalogue), &dir).unwrap();
        let manifest = manifest_of("com.example.code").replace("1.2.0", "1.0.1");
        let files = vec![
            ("module.json".to_owned(), manifest.into_bytes()),
            ("dist".to_owned(), b"a file".to_vec()),
            ("dist/index.js".to_owned(), b"new()".to_vec()),
        ];
        let broken = open(&sign_package(&files, &catalogue), &catalogue.public_key()).expect("opens");
        assert!(install(&broken, &dir).is_err());
        assert_eq!(std::fs::read(dir.join("com.example.code/dist/index.js")).unwrap(), b"old()");
        assert_eq!(folders(&dir), ["com.example.code"]);
        assert_eq!(installed(&dir).unwrap()[0].version, "1.0.0");
    }

    #[test]
    fn a_swap_cut_short_is_put_right_at_start() {
        let catalogue = Ed25519SecretKey::new();
        let dir = scratch("recover");
        install(&version_of("com.example.code", "1.0.0", b"old()", &catalogue), &dir).unwrap();
        install(&version_of("com.example.list", "1.0.0", b"list()", &catalogue), &dir).unwrap();
        std::fs::rename(dir.join("com.example.code"), dir.join("com.example.code~old")).unwrap();
        std::fs::create_dir_all(dir.join("com.example.code~new/dist")).unwrap();
        std::fs::create_dir_all(dir.join("com.example.list~old")).unwrap();
        std::fs::write(dir.join("com.example.list~old/module.json"), manifest_of("com.example.list")).unwrap();
        assert_eq!(installed(&dir).unwrap().iter().map(|one| one.id.as_str()).collect::<Vec<_>>(), ["com.example.list"], "never a copy");

        recover(&dir).unwrap();
        assert_eq!(folders(&dir), ["com.example.code", "com.example.list"]);
        assert_eq!(std::fs::read(dir.join("com.example.code/dist/index.js")).unwrap(), b"old()");
        assert_eq!(installed(&dir).unwrap().len(), 2);
        recover(&scratch("empty").join("missing")).expect("no folder yet is nothing to put right");
    }

    #[test]
    fn installing_puts_the_files_in_the_plugin_folder_and_removing_takes_them_away() {
        let catalogue = Ed25519SecretKey::new();
        let bytes = package(&manifest_of("com.example.translator"), b"export default {}", &catalogue);
        let plugin = open(&bytes, &catalogue.public_key()).expect("opens");
        let dir = std::env::temp_dir().join(format!("ft-plugins-{}", blake3::hash(&bytes).to_hex()));

        let home = install(&plugin, &dir).expect("installs");
        assert_eq!(home, dir.join("com.example.translator"));
        assert_eq!(std::fs::read(home.join("dist/index.js")).expect("reads"), b"export default {}");
        assert_eq!(
            installed(&dir).expect("lists").into_iter().map(|m| m.id).collect::<Vec<_>>(),
            ["com.example.translator"]
        );

        remove("com.example.translator", &dir).expect("removes");
        assert!(installed(&dir).expect("lists").is_empty());
        assert!(!home.exists());
    }
}

#[cfg(test)]
mod policy_shape {
    use super::*;

    #[test]
    fn the_policy_is_one_line_without_double_spaces() {
        let policy = Permissions { network: vec!["api.openai.com".to_owned()], ..Permissions::default() }
            .content_security_policy();
        assert!(!policy.contains('\n') && !policy.contains("  "), "{policy}");
        assert!(policy.ends_with("frame-ancestors http://tauri.localhost tauri://localhost"), "{policy}");
    }
}

pub mod packing {
    use std::path::{Path, PathBuf};

    use anyhow::{Context, Result};
    use base64::engine::general_purpose::STANDARD_NO_PAD;
    use base64::Engine;
    use vodozemac::Ed25519SecretKey;

    pub fn files_of(dir: &Path) -> Result<Vec<(String, Vec<u8>)>> {
        let mut files = vec![("module.json".to_owned(), std::fs::read(dir.join("module.json"))?)];
        let image = dir.join(crate::icon::IMAGE);
        if image.is_file() {
            files.push((crate::icon::IMAGE.to_owned(), std::fs::read(image)?));
        }
        let mut pending = vec![dir.join("dist")];
        while let Some(folder) = pending.pop() {
            for entry in std::fs::read_dir(&folder)? {
                let path = entry?.path();
                if path.is_dir() {
                    pending.push(path);
                } else if !path.file_name().is_some_and(|name| name.to_string_lossy().starts_with('.')) {
                    let name = path.strip_prefix(dir)?.to_string_lossy().replace('\\', "/");
                    files.push((name, std::fs::read(&path)?));
                }
            }
        }
        files.sort();
        Ok(files)
    }

    pub fn key_from(path: &Path) -> Result<Ed25519SecretKey> {
        let text = std::fs::read_to_string(path).context("cannot read the key file")?;
        let bytes = STANDARD_NO_PAD.decode(text.trim().trim_end_matches('=')).context("the key is not base64")?;
        let seed: [u8; 32] = bytes.as_slice().try_into().map_err(|_| anyhow::anyhow!("the key is not 32 bytes"))?;
        Ok(Ed25519SecretKey::from_slice(&seed))
    }

    pub fn plugin_folders(dir: &Path) -> Result<Vec<PathBuf>> {
        let mut folders: Vec<PathBuf> = std::fs::read_dir(dir)?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.join("module.json").is_file())
            .collect();
        folders.sort();
        Ok(folders)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn takes_every_file_of_the_folder_in_a_stable_order() {
            let dir = std::env::temp_dir().join(format!("ftpack-{}", blake3::hash(b"files").to_hex()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(dir.join("one/dist")).unwrap();
            std::fs::create_dir_all(dir.join("one/node_modules/vitest")).unwrap();
            std::fs::write(dir.join("one/module.json"), b"{}").unwrap();
            std::fs::write(dir.join("one/dist/index.js"), b"code").unwrap();
            std::fs::write(dir.join("one/dist/.DS_Store"), b"junk").unwrap();
            std::fs::write(dir.join("one/index.test.js"), b"tests").unwrap();
            std::fs::write(dir.join("one/package.json"), b"{}").unwrap();
            std::fs::write(dir.join("one/node_modules/vitest/huge.js"), b"x".repeat(1000)).unwrap();

            let files = files_of(&dir.join("one")).unwrap();
            assert_eq!(
                files.iter().map(|(path, _)| path.as_str()).collect::<Vec<_>>(),
                ["dist/index.js", "module.json"],
                "only what runs on the phone is packed"
            );
            assert_eq!(plugin_folders(&dir).unwrap(), vec![dir.join("one")]);
        }

        #[test]
        fn reads_the_key_as_infra_keeps_it() {
            let dir = std::env::temp_dir().join(format!("ftpack-key-{}", blake3::hash(b"key").to_hex()));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("catalogue.key");
            let secret = Ed25519SecretKey::new();
            std::fs::write(&path, secret.to_base64()).unwrap();
            assert_eq!(key_from(&path).unwrap().public_key(), secret.public_key());
        }
    }
}
