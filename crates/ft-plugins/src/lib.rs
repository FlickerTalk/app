//! Plugin packages (Plan §48–§58): what a `.ftplugin` is, how its signature is checked and where
//! it is installed. **Nothing inside a package is read as code before the signature is verified**
//! (§50), and nothing is ever written outside the plugin's own folder.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use vodozemac::{Ed25519PublicKey, Ed25519SecretKey, Ed25519Signature};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

/// Where the signature lives inside the package; everything else is what gets signed.
const SIGNATURE: &str = "signature";
/// The manifest, read only after the signature checks out.
const MANIFEST: &str = "module.json";

/// What `module.json` says about a plugin (§49).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    /// The oldest FlickerTalk this plugin runs on (§51).
    pub min_core_version: String,
    /// The web components it registers.
    pub components: Vec<String>,
}

/// A package whose signature and manifest already checked out.
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
}

/// What the catalogue signs: the name and the hash of every file, in order, hashed together. A
/// single changed byte, a new file or a missing one changes it.
pub fn digest(files: &BTreeMap<String, Vec<u8>>) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    for (path, bytes) in files {
        hasher.update(path.as_bytes());
        hasher.update(&[0]);
        hasher.update(blake3::hash(bytes).as_bytes());
    }
    *hasher.finalize().as_bytes()
}

/// Opens a package: checks the signature **before** anything inside is read as anything but
/// bytes, then the manifest and every path (§50).
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
    Ok(Plugin { manifest, files })
}

/// Installs an open package under `dir/<id>`, replacing any older copy.
pub fn install(plugin: &Plugin, dir: &Path) -> Result<PathBuf> {
    let home = dir.join(&plugin.manifest.id);
    if home.exists() {
        std::fs::remove_dir_all(&home).context("cannot replace the installed plugin")?;
    }
    for (path, bytes) in plugin.files() {
        let target = home.join(safe_path(path)?);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target, bytes)?;
    }
    Ok(home)
}

/// The manifests of the plugins installed under `dir`.
pub fn installed(dir: &Path) -> Result<Vec<Manifest>> {
    let mut manifests = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(manifests),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let path = entry?.path().join(MANIFEST);
        let Ok(bytes) = std::fs::read(&path) else { continue };
        if let Ok(manifest) = serde_json::from_slice::<Manifest>(&bytes) {
            if check(&manifest).is_ok() {
                manifests.push(manifest);
            }
        }
    }
    manifests.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(manifests)
}

/// Takes an installed plugin off this phone.
pub fn remove(id: &str, dir: &Path) -> Result<()> {
    let home = dir.join(safe_path(id)?);
    match std::fs::remove_dir_all(&home) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
        _ => Ok(()),
    }
}

/// Builds a signed package: what the catalogue does, and what the tests use.
pub fn sign_package(files: &[(String, Vec<u8>)], catalogue: &Ed25519SecretKey) -> Vec<u8> {
    let entries: BTreeMap<String, Vec<u8>> = files.iter().cloned().collect();
    let signature = catalogue.sign(&digest(&entries)).to_bytes().to_vec();
    pack(entries.into_iter().chain([(SIGNATURE.to_owned(), signature)]))
}

/// Same bytes with one file swapped: what an attacker would try after the package was signed.
pub fn replace_file(package: &[u8], path: &str, bytes: &[u8]) -> Vec<u8> {
    let mut files = unpack(package).expect("a package");
    files.insert(path.to_owned(), bytes.to_vec());
    pack(files)
}

/// A manifest we are willing to run (§49): an id that is a name and not a path, a version, and
/// components a browser can register (a custom element's name carries a dash).
fn check(manifest: &Manifest) -> Result<()> {
    ensure!(!manifest.name.trim().is_empty(), "the plugin has no name");
    ensure!(is_id(&manifest.id), "'{}' is not a plugin id", manifest.id);
    ensure!(is_version(&manifest.version), "'{}' is not a version", manifest.version);
    ensure!(is_version(&manifest.min_core_version), "'{}' is not a version", manifest.min_core_version);
    ensure!(!manifest.components.is_empty(), "the plugin registers no component");
    for component in &manifest.components {
        ensure!(is_component(component), "'{component}' is not a web component name");
    }
    Ok(())
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

/// A path that stays inside the plugin's own folder: relative, no `..`, no root, no drive.
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

fn unpack(package: &[u8]) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut archive = ZipArchive::new(Cursor::new(package)).context("the package is not a .ftplugin")?;
    let mut files = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        files.insert(name, bytes);
    }
    Ok(files)
}

fn pack(files: impl IntoIterator<Item = (String, Vec<u8>)>) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, bytes) in files {
        writer.start_file(path, SimpleFileOptions::default()).expect("writes the entry");
        writer.write_all(&bytes).expect("writes the bytes");
    }
    writer.finish().expect("closes the package").into_inner()
}

/// What the catalogue lists for a plugin (§56). The index is a static file on
/// plugins.flickertalk.com, signed with the same key as the packages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogueEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub min_core_version: String,
    /// Bytes of the package, so the app can say what it is about to download.
    pub size: u64,
    /// BLAKE3 of the package, in hex.
    pub hash: String,
    pub url: String,
    #[serde(default)]
    pub summary: String,
}

impl CatalogueEntry {
    /// Whether this FlickerTalk is new enough for it (§51).
    pub fn runs_on(&self, core_version: &str) -> bool {
        fn parts(version: &str) -> Vec<u32> {
            version.split('.').map(|part| part.parse().unwrap_or(0)).collect()
        }
        parts(core_version) >= parts(&self.min_core_version)
    }
}

#[derive(Deserialize)]
struct Catalogue {
    plugins: Vec<CatalogueEntry>,
}

/// Reads the catalogue's index, but only if the catalogue signed it (base64 signature of the
/// index as it was served, byte for byte).
pub fn catalogue_entries(index: &str, signature: &str, catalogue: &Ed25519PublicKey) -> Result<Vec<CatalogueEntry>> {
    let signature = Ed25519Signature::from_base64(signature).context("the signature is not one")?;
    catalogue
        .verify(index.as_bytes(), &signature)
        .map_err(|_| anyhow::anyhow!("the catalogue index is not signed by the catalogue"))?;
    let listed: Catalogue = serde_json::from_str(index).context("the index is not a catalogue")?;
    Ok(listed.plugins)
}

/// Opens a downloaded package, checking first that it is exactly what the catalogue listed: the
/// signature alone would also accept an older, signed version (§50).
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

    // §56: the catalogue is a static index, signed like the packages, so a changed listing is
    // noticed before anything is downloaded.
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
        };
        assert!(!entry.runs_on("0.1.0"));
        assert!(entry.runs_on("9.0.0"));
        assert!(entry.runs_on("9.1.0"));
    }

    /// A package as its author would build it: the manifest and the files of `dist/`.
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
    fn a_manifest_that_does_not_hold_up_is_refused() {
        let catalogue = Ed25519SecretKey::new();
        // A web component's name must have a dash: without it the browser refuses to register it.
        let bad = r#"{"id":"com.example.x","name":"X","version":"1.0.0","minCoreVersion":"0.1.0","components":["translator"]}"#;
        let bytes = package(bad, b"", &catalogue);
        assert!(open(&bytes, &catalogue.public_key()).is_err(), "the component name is not one");

        let bad_id = r#"{"id":"../../etc","name":"X","version":"1.0.0","minCoreVersion":"0.1.0","components":["ft-x"]}"#;
        let bytes = package(bad_id, b"", &catalogue);
        assert!(open(&bytes, &catalogue.public_key()).is_err(), "the id is not a plugin id");
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
