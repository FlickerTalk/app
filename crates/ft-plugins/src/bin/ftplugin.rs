//! Packs a folder into a signed `.ftplugin` (Plan §49–§50). The catalogue's private key never
//! leaves `infra/secrets/plugin-catalogue.key`, so this runs on the machine that holds it.
//!
//!     ftplugin <folder> <out.ftplugin> <key file>
//!
//! The key file holds the 32-byte Ed25519 seed in base64, as `infra` keeps it.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::STANDARD_NO_PAD;
use base64::Engine;
use ft_plugins::{open, sign_package};
use vodozemac::Ed25519SecretKey;

/// Every file under `dir`, with the path the package will carry (relative, with `/`).
fn files_of(dir: &Path) -> Result<Vec<(String, Vec<u8>)>> {
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

fn key_from(path: &Path) -> Result<Ed25519SecretKey> {
    let text = std::fs::read_to_string(path).context("cannot read the key file")?;
    let bytes = STANDARD_NO_PAD.decode(text.trim().trim_end_matches('=')).context("the key is not base64")?;
    let seed: [u8; 32] = bytes.as_slice().try_into().map_err(|_| anyhow::anyhow!("the key is not 32 bytes"))?;
    Ok(Ed25519SecretKey::from_slice(&seed))
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [folder, out, key] = args.as_slice() else {
        bail!("ftplugin <folder> <out.ftplugin> <key file>");
    };
    let key = key_from(Path::new(key))?;
    let files = files_of(Path::new(folder))?;
    if !files.iter().any(|(path, _)| path == "module.json") {
        bail!("{folder} has no module.json");
    }
    let package = sign_package(&files, &key);
    // Never write a package we could not open ourselves.
    let plugin = open(&package, &key.public_key())?;
    std::fs::write(PathBuf::from(out), &package)?;
    println!("{} {} v{} ({} bytes)", out, plugin.manifest.id, plugin.manifest.version, package.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_every_file_of_the_folder_in_a_stable_order() {
        let dir = std::env::temp_dir().join(format!("ftplugin-{}", blake3::hash(b"files").to_hex()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("dist")).unwrap();
        std::fs::write(dir.join("module.json"), b"{}").unwrap();
        std::fs::write(dir.join("dist/index.js"), b"code").unwrap();
        std::fs::write(dir.join(".DS_Store"), b"junk").unwrap();
        std::fs::write(dir.join("index.test.js"), b"tests").unwrap();

        let files = files_of(&dir).unwrap();
        assert_eq!(
            files.iter().map(|(path, _)| path.as_str()).collect::<Vec<_>>(),
            ["dist/index.js", "module.json"],
            "the folder's own junk, and its tests, stay out"
        );
    }

    #[test]
    fn reads_the_key_as_infra_keeps_it() {
        let dir = std::env::temp_dir().join(format!("ftplugin-key-{}", blake3::hash(b"key").to_hex()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("catalogue.key");
        let secret = Ed25519SecretKey::new();
        std::fs::write(&path, secret.to_base64()).unwrap();
        assert_eq!(key_from(&path).unwrap().public_key(), secret.public_key());
    }
}
