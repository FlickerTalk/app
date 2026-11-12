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
