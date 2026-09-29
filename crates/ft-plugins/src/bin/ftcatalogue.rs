//! Builds the catalogue (Plan §56): packs and signs every plugin of a folder, writes them where
//! the site serves them, and leaves a signed index next to them. The app downloads that index,
//! checks the signature and only then downloads a package.
//!
//!     ftcatalogue <plugins dir> <out dir> <key file> [base url]
//!
//! The private key never leaves `infra/secrets/plugin-catalogue.key`, so this runs on the machine
//! that holds it.

use std::path::Path;

use anyhow::{bail, Context, Result};
use ft_plugins::packing::{files_of, key_from, plugin_folders};
use ft_plugins::{open, sign_package, CatalogueEntry, Manifest, INDEX, LEGACY_CORE, LEGACY_INDEX};
use vodozemac::Ed25519SecretKey;

/// Where the catalogue is served from, unless another is asked for.
const BASE: &str = "https://flickertalk.com/plugins";

/// What the index says about a package that was just built.
fn entry_of(manifest: &Manifest, package: &[u8], base: &str) -> CatalogueEntry {
    CatalogueEntry {
        url: format!("{}/{}/{}.ftplugin", base.trim_end_matches('/'), manifest.id, manifest.version),
        id: manifest.id.clone(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        min_core_version: manifest.min_core_version.clone(),
        size: package.len() as u64,
        hash: blake3::hash(package).to_hex().to_string(),
        summary: manifest.summary.clone(),
    }
}

/// The index as it is served and signed: the bytes are what the signature covers.
fn index_of(entries: &[CatalogueEntry]) -> Result<String> {
    Ok(serde_json::to_string_pretty(&serde_json::json!({ "plugins": entries }))?)
}

/// Packs every plugin of `dir` into `out`, and writes the signed index. Returns what was listed.
fn build(dir: &Path, out: &Path, key: &Ed25519SecretKey, base: &str) -> Result<Vec<CatalogueEntry>> {
    let folders = plugin_folders(dir)?;
    if folders.is_empty() {
        bail!("{} holds no plugin", dir.display());
    }
    let mut entries = Vec::new();
    for folder in folders {
        let files = files_of(&folder)?;
        let package = sign_package(&files, key);
        // Never list a package we could not open ourselves.
        let plugin = open(&package, &key.public_key()).with_context(|| format!("{}", folder.display()))?;
        let entry = entry_of(&plugin.manifest, &package, base);
        let home = out.join(&plugin.manifest.id);
        std::fs::create_dir_all(&home)?;
        std::fs::write(home.join(format!("{}.ftplugin", plugin.manifest.version)), &package)?;
        entries.push(entry);
    }
    std::fs::create_dir_all(out)?;
    write_index(out, INDEX, &entries, key)?;
    // What the app 1.0.0 reads: it does not look at minCoreVersion, so only what runs there.
    let legacy: Vec<CatalogueEntry> = entries.iter().filter(|entry| entry.runs_on(LEGACY_CORE)).cloned().collect();
    write_index(out, LEGACY_INDEX, &legacy, key)?;
    Ok(entries)
}

/// Writes one index and its signature next to it.
fn write_index(out: &Path, name: &str, entries: &[CatalogueEntry], key: &Ed25519SecretKey) -> Result<()> {
    let index = index_of(entries)?;
    std::fs::write(out.join(name), &index)?;
    std::fs::write(out.join(format!("{name}.sig")), key.sign(index.as_bytes()).to_base64())?;
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (dir, out, key, base) = match args.as_slice() {
        [dir, out, key] => (dir, out, key, BASE.to_owned()),
        [dir, out, key, base] => (dir, out, key, base.clone()),
        _ => bail!("ftcatalogue <plugins dir> <out dir> <key file> [base url]"),
    };
    let key = key_from(Path::new(key))?;
    let entries = build(Path::new(dir), Path::new(out), &key, &base)?;
    for entry in &entries {
        println!("{} v{} ({} bytes) {}", entry.id, entry.version, entry.size, entry.url);
    }
    let legacy = entries.iter().filter(|entry| entry.runs_on(LEGACY_CORE)).count();
    println!("{out}/{INDEX}: {} plugins; {out}/{LEGACY_INDEX} (app {LEGACY_CORE}): {legacy}", entries.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ft_plugins::catalogue_entries;

    fn a_plugin(dir: &Path, id: &str) {
        a_plugin_for(dir, id, "0.1.0");
    }

    fn a_plugin_for(dir: &Path, id: &str, min_core: &str) {
        std::fs::create_dir_all(dir.join("dist")).unwrap();
        std::fs::write(
            dir.join("module.json"),
            format!(
                r#"{{"id":"{id}","name":"Sketch","version":"1.2.0","minCoreVersion":"{min_core}","components":["ft-sketch"],"summary":"Draw with a finger."}}"#
            ),
        )
        .unwrap();
        std::fs::write(dir.join("dist/index.js"), b"export const hi = 1;\n").unwrap();
        std::fs::write(dir.join("index.test.js"), b"// not packed").unwrap();
    }

    // §56: what the app reads is a signed index, and every listing says exactly which bytes to
    // expect, so a package swapped on the way is noticed before it is opened.
    #[test]
    fn writes_the_packages_and_an_index_the_app_can_trust() {
        let home = std::env::temp_dir().join(format!("ftcat-{}", blake3::hash(b"build").to_hex()));
        let _ = std::fs::remove_dir_all(&home);
        a_plugin(&home.join("src/sketch"), "com.flickertalk.sketch");
        let key = Ed25519SecretKey::new();

        let entries = build(&home.join("src"), &home.join("site"), &key, "https://flickertalk.com/plugins/").unwrap();
        assert_eq!(entries.len(), 1);

        let package = std::fs::read(home.join("site/com.flickertalk.sketch/1.2.0.ftplugin")).unwrap();
        let index = std::fs::read_to_string(home.join("site/index.json")).unwrap();
        let signature = std::fs::read_to_string(home.join("site/index.json.sig")).unwrap();

        let listed = catalogue_entries(&index, &signature, &key.public_key()).expect("the index is signed");
        assert_eq!(listed[0].id, "com.flickertalk.sketch");
        assert_eq!(listed[0].url, "https://flickertalk.com/plugins/com.flickertalk.sketch/1.2.0.ftplugin");
        assert_eq!(listed[0].summary, "Draw with a finger.");
        assert_eq!(listed[0].size, package.len() as u64);
        assert_eq!(listed[0].hash, blake3::hash(&package).to_hex().to_string());
        ft_plugins::download(&listed[0], &package, &key.public_key()).expect("the package is what was listed");
    }

    // 2026-09-28: the app 1.0.0 reads index.json and ignores minCoreVersion, so a plugin that needs
    // a newer core would be offered there and break. index.json keeps only what 1.0.0 runs; every
    // newer core reads catalogue.json, signed the same way, with everything.
    #[test]
    fn what_needs_a_newer_core_is_listed_only_where_a_newer_core_reads() {
        let home = std::env::temp_dir().join(format!("ftcat-{}", blake3::hash(b"two indexes").to_hex()));
        let _ = std::fs::remove_dir_all(&home);
        a_plugin_for(&home.join("src/sketch"), "com.flickertalk.sketch", "0.1.0");
        a_plugin_for(&home.join("src/notes"), "com.flickertalk.notes", "1.1.0");
        let key = Ed25519SecretKey::new();
        build(&home.join("src"), &home.join("site"), &key, BASE).unwrap();

        let listed = |name: &str| {
            let index = std::fs::read_to_string(home.join("site").join(name)).unwrap();
            let signature = std::fs::read_to_string(home.join("site").join(format!("{name}.sig"))).unwrap();
            let mut ids: Vec<String> =
                catalogue_entries(&index, &signature, &key.public_key()).expect("signed").into_iter().map(|entry| entry.id).collect();
            ids.sort();
            ids
        };
        assert_eq!(listed(ft_plugins::LEGACY_INDEX), ["com.flickertalk.sketch"], "1.0.0 is offered only what runs there");
        assert_eq!(listed(ft_plugins::INDEX), ["com.flickertalk.notes", "com.flickertalk.sketch"]);
    }

    #[test]
    fn a_folder_without_plugins_builds_nothing() {
        let home = std::env::temp_dir().join(format!("ftcat-{}", blake3::hash(b"empty").to_hex()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join("src")).unwrap();
        assert!(build(&home.join("src"), &home.join("site"), &Ed25519SecretKey::new(), BASE).is_err());
    }
}
