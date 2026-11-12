//! key.ftv sits in the user's own cloud where anyone in that account can try phrases offline, hence
//! Argon2id and 12 characters at least. The app's own limit on tries only slows the app.

use std::fmt;

use anyhow::{anyhow, bail, ensure, Context, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use crate::cipher;

pub const MIN_CHARS: usize = 12;
pub const MAX_CHARS: usize = 100;
const KEY_VERSION: u32 = 2;
const SALT: usize = 16;
// anyone who can write to the cloud could ask for more
const MAX_MEMORY_KIB: u32 = 256 * 1024;
const MAX_PASSES: u32 = 10;
const MAX_LANES: u32 = 4;

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const SUGGESTED: usize = 30;
const GROUP: usize = 5;

// the only error that counts as a try
#[derive(Debug)]
pub struct WrongPhrase;

impl fmt::Display for WrongPhrase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("that phrase does not open this drive")
    }
}

impl std::error::Error for WrongPhrase {}

#[derive(Debug)]
pub struct OldDrive;

impl fmt::Display for OldDrive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("this drive was made by an older version of the app; set it up again")
    }
}

impl std::error::Error for OldDrive {}

// trim what pasting leaves and compose (NFC), so the same letters from another keyboard match
pub fn normalize(phrase: &str) -> Result<String> {
    let clean: String = phrase.trim().nfc().collect();
    let length = clean.chars().count();
    ensure!(length >= MIN_CHARS, "a recovery phrase has {MIN_CHARS} characters at least");
    ensure!(length <= MAX_CHARS, "a recovery phrase has {MAX_CHARS} characters at most");
    Ok(clean)
}

pub fn suggest() -> String {
    let bytes: [u8; SUGGESTED] = rand::random();
    let raw: String = bytes.iter().map(|byte| ALPHABET[(byte % 32) as usize] as char).collect();
    raw.as_bytes().chunks(GROUP).map(|group| std::str::from_utf8(group).expect("ascii")).collect::<Vec<_>>().join("-")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stretch {
    pub kdf: String,
    // KiB
    pub m: u32,
    pub t: u32,
    pub p: u32,
    pub salt: String,
}

impl Stretch {
    pub fn fresh() -> Self {
        let salt: [u8; SALT] = rand::random();
        Self { kdf: "argon2id".to_owned(), m: 64 * 1024, t: 3, p: 1, salt: STANDARD.encode(salt) }
    }

    fn check(&self) -> Result<Vec<u8>> {
        ensure!(self.kdf == "argon2id", "the drive's key is stretched in a way this app does not know");
        ensure!(self.m <= MAX_MEMORY_KIB && self.t <= MAX_PASSES && self.p <= MAX_LANES, "the drive's key asks too much of this phone");
        let salt = STANDARD.decode(&self.salt).context("the drive's key is corrupt")?;
        ensure!(salt.len() == SALT, "the drive's key is corrupt");
        Ok(salt)
    }
}

pub fn wrap_key(phrase: &str, stretch: &Stretch) -> Result<[u8; 32]> {
    let phrase = normalize(phrase)?;
    let salt = stretch.check()?;
    let params = argon2::Params::new(stretch.m, stretch.t, stretch.p, Some(32)).map_err(|error| anyhow!("the drive's key is corrupt: {error}"))?;
    let argon = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut key = [0u8; 32];
    argon.hash_password_into(phrase.as_bytes(), &salt, &mut key).map_err(|error| anyhow!("cannot stretch the phrase: {error}"))?;
    Ok(key)
}

#[derive(Serialize, Deserialize)]
struct KeyFile {
    version: u32,
    kdf: Stretch,
    key: String,
}

pub fn seal_key(phrase: &str, key: &[u8; 32]) -> Result<Vec<u8>> {
    let stretch = Stretch::fresh();
    let wrap = wrap_key(phrase, &stretch)?;
    let file = KeyFile { version: KEY_VERSION, kdf: stretch, key: STANDARD.encode(cipher::seal(&wrap, "key", key)?) };
    Ok(serde_json::to_vec(&file)?)
}

pub fn is_current(file: &[u8]) -> bool {
    file.first() == Some(&b'{') && serde_json::from_slice::<KeyFile>(file).is_ok_and(|file| file.version == KEY_VERSION)
}

pub fn open_key(phrase: &str, file: &[u8]) -> Result<[u8; 32]> {
    if file.first() != Some(&b'{') {
        bail!(OldDrive);
    }
    let file: KeyFile = serde_json::from_slice(file).context("the drive's key is corrupt")?;
    ensure!(file.version == KEY_VERSION, "the drive was made by a newer app");
    let wrap = wrap_key(phrase, &file.kdf)?;
    let sealed = STANDARD.decode(&file.key).context("the drive's key is corrupt")?;
    let key = cipher::open(&wrap, "key", &sealed).map_err(|_| anyhow!(WrongPhrase))?;
    key.try_into().map_err(|_| anyhow!("the drive's key is corrupt"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cipher;

    #[test]
    fn a_phrase_is_what_the_user_wrote_trimmed_and_composed() {
        assert_eq!(normalize("  my long phrase here \n").unwrap(), "my long phrase here");
        assert_eq!(normalize("cafe\u{301} con leche!").unwrap(), normalize("café con leche!").unwrap(), "the same letters, however typed");
        assert_ne!(normalize("My long phrase").unwrap(), normalize("my long phrase").unwrap(), "case counts");
        assert_ne!(normalize("my long  phrase").unwrap(), normalize("my long phrase").unwrap(), "inner spaces count");
        assert!(normalize("eleven char").is_err(), "11 characters");
        assert!(normalize("twelve chars").is_ok());
        assert!(normalize("🔑🔑🔑🔑🔑🔑🔑🔑🔑🔑🔑🔑").is_ok(), "characters, not bytes");
        assert!(normalize(&"x".repeat(MAX_CHARS)).is_ok());
        assert!(normalize(&"x".repeat(MAX_CHARS + 1)).is_err());
        assert!(normalize("            ").is_err(), "only spaces");
    }

    #[test]
    fn a_suggested_phrase_is_strong_and_valid() {
        let phrase = suggest();
        assert_eq!(phrase.chars().count(), 35);
        assert!(normalize(&phrase).is_ok());
        assert_ne!(suggest(), phrase);
    }

    #[test]
    fn the_key_comes_from_the_phrase_and_its_salt_with_argon2id() {
        let stretch = Stretch::fresh();
        assert_eq!((stretch.kdf.as_str(), stretch.m, stretch.t, stretch.p), ("argon2id", 64 * 1024, 3, 1));
        assert_ne!(Stretch::fresh().salt, stretch.salt, "a salt of its own");
        let key = wrap_key("the same phrase", &stretch).unwrap();
        assert_eq!(wrap_key("  the same phrase ", &stretch).unwrap(), key);
        assert_ne!(wrap_key("another phrase!", &stretch).unwrap(), key);
        assert_ne!(wrap_key("the same phrase", &Stretch::fresh()).unwrap(), key, "another salt, another key");
        assert!(wrap_key("short", &stretch).is_err());
    }

    #[test]
    fn the_key_file_opens_with_the_phrase_and_nothing_else() {
        let key: [u8; 32] = rand::random();
        let file = seal_key("correct horse battery", &key).unwrap();
        assert!(is_current(&file));
        assert_eq!(open_key("correct horse battery", &file).unwrap(), key);
        let wrong = open_key("correct horse battery!", &file).unwrap_err();
        assert!(wrong.downcast_ref::<WrongPhrase>().is_some(), "{wrong}");
        assert!(open_key("short", &file).unwrap_err().downcast_ref::<WrongPhrase>().is_none(), "not a phrase at all is not a wrong try");

        let old = cipher::seal(&[7; 32], "key", &key).unwrap();
        assert!(!is_current(&old));
        assert!(open_key("correct horse battery", &old).unwrap_err().downcast_ref::<OldDrive>().is_some());

        let text = String::from_utf8(file.clone()).unwrap();
        let greedy = text.replace("\"m\":65536", "\"m\":4194304");
        assert_ne!(greedy, text);
        assert!(open_key("correct horse battery", greedy.as_bytes()).is_err());
    }
}
