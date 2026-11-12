use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, ensure, Context, Result};
use vodozemac::{Ed25519PublicKey, Ed25519SecretKey, Ed25519Signature};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, System, ZipArchive, ZipWriter};

use crate::icon;
use crate::manifest::{check, Manifest};

const SIGNATURE: &str = "signature";
pub(crate) const MANIFEST: &str = "module.json";

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

pub(crate) fn safe_path(path: &str) -> Result<PathBuf> {
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

#[cfg(test)]
pub(crate) mod fixtures {
    use super::sign_package;
    use vodozemac::Ed25519SecretKey;

    pub(crate) const IMAGE_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><title>Échecs</title><rect width="64" height="64" rx="18" fill="#f0742a"/></svg>"##;

    pub(crate) fn drawn_package(image: &str, signer: &Ed25519SecretKey) -> Vec<u8> {
        let files = vec![
            ("module.json".to_owned(), manifest_of("com.example.translator").into_bytes()),
            ("icon.svg".to_owned(), image.as_bytes().to_vec()),
            ("dist/index.js".to_owned(), b"".to_vec()),
        ];
        sign_package(&files, signer)
    }

    pub(crate) fn package(manifest: &str, script: &[u8], signer: &Ed25519SecretKey) -> Vec<u8> {
        let mut files = vec![("module.json".to_owned(), manifest.as_bytes().to_vec())];
        files.push(("dist/index.js".to_owned(), script.to_vec()));
        sign_package(&files, signer)
    }

    pub(crate) fn manifest_of(id: &str) -> String {
        format!(
            r#"{{"id":"{id}","name":"Translator","version":"1.2.0","minCoreVersion":"0.1.0","components":["ft-translator"]}}"#
        )
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{drawn_package, manifest_of, package, IMAGE_SVG};
    use super::*;
    use vodozemac::Ed25519SecretKey;

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
}
