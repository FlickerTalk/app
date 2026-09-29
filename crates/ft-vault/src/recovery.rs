//! The recovery phrase (plan-recuperacion, decision 2026-09-28): the user chooses it, keeps it
//! wherever they like, and it is never stored anywhere, the phone included. It replaces the code
//! the app used to generate (decision 2026-09-27). A phrase has far less entropy than 150 random
//! bits and `key.ftv` sits in the user's own cloud, where whoever gets into that account can try
//! phrases on their own machine, so the phrase is stretched with Argon2id (64 MiB, 3 passes) and
//! must be 12 characters at least. The app's own limit on tries (the core) only slows the app.
//!
//! ```text
//! key.ftv  {"version":2,"kdf":{"kdf":"argon2id","m":65536,"t":3,"p":1,"salt":"…"},"key":"…"}
//! ```
//!
//! `key` is the drive's key sealed (`cipher::seal`) with what the phrase and the salt give.

use std::fmt;

use anyhow::{anyhow, bail, ensure, Context, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use crate::cipher;

/// How long a phrase may be, in characters (not bytes).
pub const MIN_CHARS: usize = 12;
pub const MAX_CHARS: usize = 100;
/// The version of `key.ftv` with a phrase; the first one had a generated code.
const KEY_VERSION: u32 = 2;
const SALT: usize = 16;
/// What a key file may ask of the phone: whoever can write in the cloud could ask for more.
const MAX_MEMORY_KIB: u32 = 256 * 1024;
const MAX_PASSES: u32 = 10;
const MAX_LANES: u32 = 4;

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const SUGGESTED: usize = 30;
const GROUP: usize = 5;

/// The phrase is not the one of this drive: the only failure that counts as a try.
#[derive(Debug)]
pub struct WrongPhrase;

impl fmt::Display for WrongPhrase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("that phrase does not open this drive")
    }
}

impl std::error::Error for WrongPhrase {}

/// The drive was made by the first version, with a generated code: it has to be made again.
#[derive(Debug)]
pub struct OldDrive;

impl fmt::Display for OldDrive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("this drive was made by an older version of the app; set it up again")
    }
}

impl std::error::Error for OldDrive {}

/// A phrase as it counts: without the spaces pasting leaves at either end, and composed (NFC),
/// so the same letters typed on another keyboard are the same phrase. Nothing else changes.
pub fn normalize(phrase: &str) -> Result<String> {
    let clean: String = phrase.trim().nfc().collect();
    let length = clean.chars().count();
    ensure!(length >= MIN_CHARS, "a recovery phrase has {MIN_CHARS} characters at least");
    ensure!(length <= MAX_CHARS, "a recovery phrase has {MAX_CHARS} characters at most");
    Ok(clean)
}

/// A strong phrase for whoever wants one: 30 symbols of Crockford's base32 (150 bits) from the
/// phone's randomness, grouped for reading: `XXXXX-XXXXX-XXXXX-XXXXX-XXXXX-XXXXX`.
pub fn suggest() -> String {
    let bytes: [u8; SUGGESTED] = rand::random();
    let raw: String = bytes.iter().map(|byte| ALPHABET[(byte % 32) as usize] as char).collect();
    raw.as_bytes().chunks(GROUP).map(|group| std::str::from_utf8(group).expect("ascii")).collect::<Vec<_>>().join("-")
}

/// How a phrase is stretched into a key, kept in `key.ftv` so another phone can do it again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stretch {
    pub kdf: String,
    /// Memory, in KiB.
    pub m: u32,
    /// Passes.
    pub t: u32,
    /// Lanes.
    pub p: u32,
    pub salt: String,
}

impl Stretch {
    /// Argon2id with 64 MiB and three passes, and a salt of its own.
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

/// The key that seals the drive's key, from the phrase and how to stretch it. Slow on purpose.
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

/// `key.ftv` for a drive's key and a phrase, with a fresh salt.
pub fn seal_key(phrase: &str, key: &[u8; 32]) -> Result<Vec<u8>> {
    let stretch = Stretch::fresh();
    let wrap = wrap_key(phrase, &stretch)?;
    let file = KeyFile { version: KEY_VERSION, kdf: stretch, key: STANDARD.encode(cipher::seal(&wrap, "key", key)?) };
    Ok(serde_json::to_vec(&file)?)
}

/// Whether a `key.ftv` is of this version (a phrase), not of the first one (a code).
pub fn is_current(file: &[u8]) -> bool {
    file.first() == Some(&b'{') && serde_json::from_slice::<KeyFile>(file).is_ok_and(|file| file.version == KEY_VERSION)
}

/// The drive's key, out of `key.ftv` with the phrase. `WrongPhrase` if it is not the one.
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

    // Decision 2026-09-28 (plan-recuperacion): a phrase the user chooses, 12 to 100 characters,
    // exactly the same phrase opens the drive; the app can suggest one.
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

        // A drive of the first version, sealed with a generated code: made again, never opened.
        let old = cipher::seal(&[7; 32], "key", &key).unwrap();
        assert!(!is_current(&old));
        assert!(open_key("correct horse battery", &old).unwrap_err().downcast_ref::<OldDrive>().is_some());

        // Whoever can write in the cloud cannot make the phone stretch without end.
        let text = String::from_utf8(file.clone()).unwrap();
        let greedy = text.replace("\"m\":65536", "\"m\":4194304");
        assert_ne!(greedy, text);
        assert!(open_key("correct horse battery", greedy.as_bytes()).is_err());
    }
}
