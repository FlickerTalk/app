use std::collections::BTreeMap;

use anyhow::{bail, ensure, Result};
use serde::{Deserialize, Serialize};

use crate::permissions::{Permissions, Sending};

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

pub(crate) fn check(manifest: &Manifest) -> Result<()> {
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

pub(crate) fn is_id(id: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::fixtures::{manifest_of, package};
    use crate::package::open;
    use vodozemac::Ed25519SecretKey;

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
    fn a_manifest_that_does_not_hold_up_is_refused() {
        let catalogue = Ed25519SecretKey::new();
        let bad = r#"{"id":"com.example.x","name":"X","version":"1.0.0","minCoreVersion":"0.1.0","components":["translator"]}"#;
        let bytes = package(bad, b"", &catalogue);
        assert!(open(&bytes, &catalogue.public_key()).is_err(), "the component name is not one");

        let bad_id = r#"{"id":"../../etc","name":"X","version":"1.0.0","minCoreVersion":"0.1.0","components":["ft-x"]}"#;
        let bytes = package(bad_id, b"", &catalogue);
        assert!(open(&bytes, &catalogue.public_key()).is_err(), "the id is not a plugin id");
    }
}
