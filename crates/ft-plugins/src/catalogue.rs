use std::collections::BTreeMap;

use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use vodozemac::{Ed25519PublicKey, Ed25519Signature};

use crate::manifest::{Kind, Localized};
use crate::package::{open, Plugin};

// the only key trusted for plugins and the index
pub const CATALOGUE_KEY: &str = "4XXrMhV2sRZSK/RpFoheNAposE119EfQE8bqdRP8JcA";

pub const INDEX: &str = "catalogue.json";
// app 1.0.0 ignores minCoreVersion, so this index only lists what runs on LEGACY_CORE
pub const LEGACY_INDEX: &str = "index.json";
pub const LEGACY_CORE: &str = "1.0.0";

pub fn catalogue() -> Ed25519PublicKey {
    Ed25519PublicKey::from_base64(CATALOGUE_KEY).expect("the catalogue key is built in")
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
    use crate::manifest::GAMES_SINCE;
    use crate::package::fixtures::{manifest_of, package, IMAGE_SVG};
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

    #[test]
    fn the_catalogue_is_read_with_labels_of_hooks_this_app_does_not_know() {
        let catalogue = Ed25519SecretKey::new();
        let index = index_of(&"ab".repeat(32))
            .replacen(r#""summary":"#, r#""locales":{"es":{"name":"Traductor","hooks":{"message":"Traducir","reaction":"Reaccionar"}}},"summary":"#, 1);
        let listed = catalogue_entries(&index, &catalogue.sign(index.as_bytes()).to_base64(), &catalogue.public_key()).expect("still a catalogue");
        assert_eq!(listed[0].locales["es"].hooks["message"], "Traducir");
    }

    #[test]
    fn the_catalogue_key_ships_with_the_app() {
        let key = catalogue();
        assert_eq!(key.to_base64(), CATALOGUE_KEY);
        let stranger = Ed25519SecretKey::new();
        let bytes = package(&manifest_of("com.example.code"), b"", &stranger);
        assert!(open(&bytes, &key).is_err(), "someone else's signature is not the catalogue's");
    }
}
