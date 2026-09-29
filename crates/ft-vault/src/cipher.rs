//! How bytes are sealed before they leave the phone (plan-drive §4.1). The format is the one
//! `age` uses in spirit —a stream of chunks, each authenticated, the last one marked so nothing
//! can be cut off the end— on XChaCha20-Poly1305, the AEAD the identity already uses at rest.
//! `age` itself is not in the tree (decision 2026-09-27: no crate that is not already here), so
//! this keeps to the same shape and the same primitives, and stays small enough to read.
//!
//! ```text
//! "FTV1" | salt (16) | chunk | chunk | … | last chunk
//! chunk  = XChaCha20-Poly1305(key = derive(vault key, salt, label), nonce = index | last, aad = header)
//! ```
//!
//! Every blob gets its own key, derived from the vault key, a fresh salt and the blob's label, so
//! a nonce never repeats across blobs and a blob copied under another name does not open.

use std::io::{Read, Write};

use anyhow::{bail, ensure, Context, Result};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

/// The bytes of plaintext in a chunk. Small enough for a phone, big enough to be quick.
pub const CHUNK: usize = 1 << 20;
const MAGIC: &[u8; 4] = b"FTV1";
const TAG: usize = 16;
const HEADER: usize = 4 + 16;

/// The key of one blob: the vault key, the salt of the blob and its label, through BLAKE3's KDF.
fn blob_key(key: &[u8; 32], salt: &[u8; 16], label: &str) -> [u8; 32] {
    let mut material = Vec::with_capacity(32 + 16 + label.len());
    material.extend_from_slice(key);
    material.extend_from_slice(salt);
    material.extend_from_slice(label.as_bytes());
    blake3::derive_key("FlickerTalk vault v1 blob key", &material)
}

/// The nonce of a chunk: its index, and whether it is the last. Unique within a blob.
fn nonce(index: u64, last: bool) -> XNonce {
    let mut bytes = [0u8; 24];
    bytes[..8].copy_from_slice(&index.to_be_bytes());
    bytes[23] = u8::from(last);
    XNonce::from(bytes)
}

/// Seals what `plain` yields into `out`, chunk by chunk. Returns how many bytes were written.
pub fn encrypt(key: &[u8; 32], label: &str, mut plain: impl Read, mut out: impl Write) -> Result<u64> {
    let salt: [u8; 16] = rand::random();
    let cipher = XChaCha20Poly1305::new(&blob_key(key, &salt, label).into());
    let mut header = Vec::with_capacity(HEADER);
    header.extend_from_slice(MAGIC);
    header.extend_from_slice(&salt);
    out.write_all(&header)?;
    let mut written = HEADER as u64;
    let mut buffer = vec![0u8; CHUNK];
    let mut index = 0u64;
    loop {
        let read = fill(&mut plain, &mut buffer)?;
        // A full chunk is never the last: a plaintext that ends on a chunk boundary ends with an
        // empty last chunk, so a stream cut at a boundary is still seen to be cut.
        let last = read < CHUNK;
        let sealed = cipher
            .encrypt(&nonce(index, last), Payload { msg: &buffer[..read], aad: &header })
            .map_err(|_| anyhow::anyhow!("cannot seal a chunk"))?;
        out.write_all(&sealed)?;
        written += sealed.len() as u64;
        index += 1;
        if last {
            break;
        }
    }
    Ok(written)
}

/// Reads as much as fits, across short reads, and says how much came.
fn fill(source: &mut impl Read, buffer: &mut [u8]) -> Result<usize> {
    let mut got = 0;
    while got < buffer.len() {
        let read = source.read(&mut buffer[got..])?;
        if read == 0 {
            break;
        }
        got += read;
    }
    Ok(got)
}

/// Opens what `encrypt` wrote into `out`. Fails on a wrong key or label, on any byte changed and
/// on a stream cut short. Returns how many bytes of plaintext came out.
pub fn decrypt(key: &[u8; 32], label: &str, mut sealed: impl Read, mut out: impl Write) -> Result<u64> {
    let mut header = [0u8; HEADER];
    ensure!(fill(&mut sealed, &mut header)? == HEADER, "not a sealed file");
    ensure!(&header[..4] == MAGIC, "not a sealed file");
    let salt: [u8; 16] = header[4..].try_into().expect("16 bytes");
    let cipher = XChaCha20Poly1305::new(&blob_key(key, &salt, label).into());
    let mut buffer = vec![0u8; CHUNK + TAG];
    let mut index = 0u64;
    let mut written = 0u64;
    loop {
        let read = fill(&mut sealed, &mut buffer)?;
        if read < TAG {
            bail!("the sealed file is cut short");
        }
        let last = read < CHUNK + TAG;
        let opened = cipher
            .decrypt(&nonce(index, last), Payload { msg: &buffer[..read], aad: &header })
            .map_err(|_| anyhow::anyhow!("the sealed file does not open: wrong key, or changed"))?;
        out.write_all(&opened)?;
        written += opened.len() as u64;
        index += 1;
        if last {
            break;
        }
    }
    Ok(written)
}

/// Seals a small value, in memory.
pub fn seal(key: &[u8; 32], label: &str, plain: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(plain.len() + HEADER + TAG);
    encrypt(key, label, plain, &mut out)?;
    Ok(out)
}

/// Opens a small value sealed with `seal`.
pub fn open(key: &[u8; 32], label: &str, sealed: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(sealed.len());
    decrypt(key, label, sealed, &mut out).context("cannot open the sealed value")?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_sealed_opens_with_the_same_key_and_label() {
        let key = [7u8; 32];
        for size in [0usize, 1, CHUNK - 1, CHUNK, CHUNK + 1, 3 * CHUNK + 17] {
            let plain: Vec<u8> = (0..size).map(|at| (at % 251) as u8).collect();
            let sealed = seal(&key, "blob-1", &plain).unwrap();
            assert_eq!(sealed.len(), HEADER + plain.len() + TAG * (plain.len() / CHUNK + 1), "size {size}: one tag per chunk, the last one always partial");
            assert_eq!(open(&key, "blob-1", &sealed).unwrap(), plain, "size {size}");
        }
    }

    #[test]
    fn nothing_but_the_right_key_and_label_opens_it_and_nothing_can_be_changed_or_cut() {
        let key = [1u8; 32];
        let plain = vec![9u8; CHUNK + 100];
        let sealed = seal(&key, "blob-1", &plain).unwrap();
        assert!(open(&[2u8; 32], "blob-1", &sealed).is_err(), "another key");
        assert!(open(&key, "blob-2", &sealed).is_err(), "another label: a blob copied under another name");
        let mut changed = sealed.clone();
        changed[HEADER + 10] ^= 1;
        assert!(open(&key, "blob-1", &changed).is_err(), "a byte changed");
        let mut appended = sealed.clone();
        appended.extend_from_slice(&sealed[HEADER..]);
        assert!(open(&key, "blob-1", &appended).is_err(), "a chunk glued on the end");
        assert!(open(&key, "blob-1", &sealed[..sealed.len() - 5]).is_err(), "cut short");
        assert!(open(&key, "blob-1", &sealed[..HEADER + CHUNK + TAG]).is_err(), "the last chunk missing");
        assert!(open(&key, "blob-1", b"garbage").is_err());
        // Two seals of the same bytes never look alike: a fresh salt each time.
        assert_ne!(seal(&key, "blob-1", &plain).unwrap(), sealed);
    }
}
