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

/// The catalogue's public key: the only signature FlickerTalk trusts for a plugin or for the
/// index (§50). Its private half never leaves `infra/secrets/plugin-catalogue.key`.
pub const CATALOGUE_KEY: &str = "4XXrMhV2sRZSK/RpFoheNAposE119EfQE8bqdRP8JcA";

/// The catalogue's key, ready to verify with.
pub fn catalogue() -> Ed25519PublicKey {
    Ed25519PublicKey::from_base64(CATALOGUE_KEY).expect("the catalogue key is built in")
}

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
    /// What it asks the user for; nothing by default (§53).
    #[serde(default)]
    pub permissions: Permissions,
    /// One line about what it does, for the catalogue. In English, like the rest of the code.
    #[serde(default)]
    pub summary: String,
}

/// What a plugin may do. Each one is asked for, granted and revoked on its own: installing grants
/// nothing (§53). A plugin never gets keys, the push token or the whole conversation (§54, §55).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permissions {
    /// Hosts it may talk to. Empty means no network at all, which is the default (§55).
    #[serde(default)]
    pub network: Vec<String>,
    /// Whether it may read the message or selection the user hands it. It never reads more.
    #[serde(default, rename = "messages", with = "given")]
    pub reads_given_messages: bool,
    /// Whether it may put text in the chat, and whether the user still presses send.
    #[serde(default)]
    pub send: Sending,
    /// Whether it may ask the phone to print what it made. The user still picks the printer.
    #[serde(default)]
    pub print: bool,
}

/// How far a plugin goes when it writes in the chat.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sending {
    /// It writes nothing.
    #[default]
    Nothing,
    /// It fills the composer and the user presses send.
    Propose,
    /// It sends on the user's behalf; a permission of its own, never granted by default.
    Auto,
}

/// `"messages": "given"` in the manifest, and nothing else.
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

/// Where a plugin's own files come from, one form per platform. The frame is sandboxed, so its
/// origin is opaque and `'self'` would match nothing.
const PLUGIN_ORIGINS: &str = "http://ftplugin.localhost https://ftplugin.localhost ftplugin://localhost";

impl Permissions {
    /// The policy the WebView enforces on this plugin: its own files, no network beyond what it
    /// was granted, and no way to bring in code from anywhere else (§55, §58).
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
    ensure!(manifest.permissions.network.len() <= 8, "a plugin may not ask for that many hosts");
    for host in &manifest.permissions.network {
        ensure!(is_host(host), "'{host}' is not a host a plugin may talk to");
    }
    Ok(())
}

/// A host we can put in a content security policy: a name, lowercase, without scheme, port, path
/// or wildcard. No plugin gets "the internet" (§55).
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

    /// A manifest with the permissions a plugin asks for (§53, §55).
    fn manifest_with(permissions: &str) -> String {
        format!(
            r#"{{"id":"com.example.ai","name":"Assistant","version":"1.0.0","minCoreVersion":"0.1.0","components":["ft-ai"],"permissions":{permissions}}}"#
        )
    }

    // §53: nothing is granted by installing. A plugin that asks for nothing gets nothing.
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

    // §55: the domains are hosts we can put in a CSP, not wildcards or URLs.
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

    // The CSP the WebView gets for that plugin: its own files, and nothing else on the network.
    #[test]
    fn the_content_security_policy_matches_what_was_granted() {
        let none = Permissions::default();
        assert!(none.content_security_policy().contains("connect-src 'none'"));
        assert!(none.content_security_policy().starts_with("default-src 'none'"));
        // The plugin is shown inside the app's own frame: only the app may embed it, and it may
        // not embed anything itself.
        let policy = none.content_security_policy();
        // The frame is sandboxed, so its origin is opaque and `'self'` matches nothing: the
        // plugin's own scheme has to be named for its script to run at all.
        assert!(policy.contains("script-src http://ftplugin.localhost"), "{policy}");
        assert!(policy.contains("ftplugin://localhost"), "{policy}");
        assert!(!policy.contains("script-src 'self'"), "{policy}");
        assert!(policy.contains("frame-ancestors http://tauri.localhost tauri://localhost"), "{policy}");
        assert!(policy.contains("child-src 'none'"), "{policy}");
        // The tools work on pictures the user gave them: what the plugin draws is its own, and
        // never travels anywhere.
        assert!(policy.contains("img-src") && policy.contains("data: blob:"), "{policy}");
        assert!(!policy.contains("frame-ancestors 'none'"), "that would keep the app from showing it");

        let ai = Permissions { network: vec!["api.openai.com".to_owned()], ..Permissions::default() };
        assert!(ai.content_security_policy().contains("connect-src https://api.openai.com"));
        assert!(!ai.content_security_policy().contains("'unsafe-eval'"));
    }

    // The key that ships with the app is the trust root: nothing else opens a package.
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

/// What the tools that build the catalogue need: reading a folder and reading the catalogue's
/// key. Only the machine that holds the private key runs this (§50).
pub mod packing {
    use std::path::{Path, PathBuf};

    use anyhow::{Context, Result};
    use base64::engine::general_purpose::STANDARD_NO_PAD;
    use base64::Engine;
    use vodozemac::Ed25519SecretKey;

    /// Every file under `dir`, with the path the package will carry (relative, with `/`). The
    /// folder's own junk and the plugin's tests stay out: they are not part of what is signed.
    pub fn files_of(dir: &Path) -> Result<Vec<(String, Vec<u8>)>> {
        let mut files = Vec::new();
        let mut pending = vec![dir.to_path_buf()];
        while let Some(folder) = pending.pop() {
            for entry in std::fs::read_dir(&folder)? {
                let path = entry?.path();
                if path.is_dir() {
                    pending.push(path);
                } else if !path.file_name().is_some_and(|name| {
                    let name = name.to_string_lossy();
                    name.starts_with('.') || name.ends_with(".test.js")
                }) {
                    let name = path.strip_prefix(dir)?.to_string_lossy().replace('\\', "/");
                    files.push((name, std::fs::read(&path)?));
                }
            }
        }
        files.sort();
        Ok(files)
    }

    /// The catalogue's private key as `infra` keeps it: the 32-byte Ed25519 seed in base64.
    pub fn key_from(path: &Path) -> Result<Ed25519SecretKey> {
        let text = std::fs::read_to_string(path).context("cannot read the key file")?;
        let bytes = STANDARD_NO_PAD.decode(text.trim().trim_end_matches('=')).context("the key is not base64")?;
        let seed: [u8; 32] = bytes.as_slice().try_into().map_err(|_| anyhow::anyhow!("the key is not 32 bytes"))?;
        Ok(Ed25519SecretKey::from_slice(&seed))
    }

    /// Every folder of `dir` that holds a `module.json`, in a stable order.
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
            std::fs::write(dir.join("one/module.json"), b"{}").unwrap();
            std::fs::write(dir.join("one/dist/index.js"), b"code").unwrap();
            std::fs::write(dir.join("one/.DS_Store"), b"junk").unwrap();
            std::fs::write(dir.join("one/index.test.js"), b"tests").unwrap();

            let files = files_of(&dir.join("one")).unwrap();
            assert_eq!(
                files.iter().map(|(path, _)| path.as_str()).collect::<Vec<_>>(),
                ["dist/index.js", "module.json"],
                "the folder's own junk, and its tests, stay out"
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
