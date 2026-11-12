//! Usage: ftplugin <folder> <out.ftplugin> <key file>, the key file holding the 32-byte Ed25519 seed in base64

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use ft_plugins::packing::{files_of, key_from};
use ft_plugins::{open, sign_package};

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
    // never write a package we couldn't open ourselves
    let plugin = open(&package, &key.public_key())?;
    std::fs::write(PathBuf::from(out), &package)?;
    println!("{} {} v{} ({} bytes)", out, plugin.manifest.id, plugin.manifest.version, package.len());
    Ok(())
}
