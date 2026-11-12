use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::icon;
use crate::manifest::{check, is_id, Manifest};
use crate::package::{safe_path, Plugin, MANIFEST};

pub fn image_of(dir: &Path, id: &str) -> String {
    if !is_id(id) {
        return String::new();
    }
    match std::fs::read(dir.join(id).join(icon::IMAGE)) {
        Ok(bytes) if icon::check(&bytes).is_ok() => String::from_utf8(bytes).unwrap_or_default(),
        _ => String::new(),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::fixtures::{drawn_package, manifest_of, package, IMAGE_SVG};
    use crate::package::{open, sign_package};
    use vodozemac::Ed25519SecretKey;

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
