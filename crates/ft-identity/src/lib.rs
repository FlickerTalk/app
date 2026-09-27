//! Cryptographic identity of the device (Plan §6–7, §106): there is no user, email or password,
//! the identity *is* the key. It is an Olm account (vodozemac): an Ed25519 key to sign and a
//! Curve25519 key for the end-to-end key exchange of ft-crypto.
//!
//! The private keys never leave the device. At rest the account is sealed with a 32-byte key kept
//! by the platform (ft-core decides where).

use std::fmt;

use anyhow::{anyhow, bail, Result};
use vodozemac::olm::{Account, AccountPickle};
pub use vodozemac::{Curve25519PublicKey, Ed25519PublicKey, Ed25519Signature as Signature};

pub const DEVICE_ID_PREFIX: &str = "ft_";

/// Public identifier of a device: `ft_` + base58(BLAKE3(Ed25519 public key)). Never sequential.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeviceId(String);

impl DeviceId {
    pub fn from_signing_key(key: &Ed25519PublicKey) -> Self {
        let hash = blake3::hash(key.as_bytes());
        Self(format!("{DEVICE_ID_PREFIX}{}", bs58::encode(hash.as_bytes()).into_string()))
    }

    pub fn parse(text: &str) -> Result<Self> {
        let encoded = text
            .strip_prefix(DEVICE_ID_PREFIX)
            .ok_or_else(|| anyhow!("a device id starts with {DEVICE_ID_PREFIX}"))?;
        let bytes = bs58::decode(encoded).into_vec().map_err(|_| anyhow!("a device id is base58"))?;
        if bytes.len() != blake3::OUT_LEN {
            bail!("a device id is a {}-byte BLAKE3 hash", blake3::OUT_LEN);
        }
        Ok(Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The device's own identity: its Olm account.
pub struct Identity {
    account: Account,
}

impl Identity {
    /// First run: fresh keys from the operating system's secure random source.
    pub fn generate() -> Self {
        Self { account: Account::new() }
    }

    pub fn device_id(&self) -> DeviceId {
        DeviceId::from_signing_key(&self.signing_key())
    }

    /// Ed25519: signs requests to the router and the Contact Card.
    pub fn signing_key(&self) -> Ed25519PublicKey {
        self.account.ed25519_key()
    }

    /// Curve25519: the identity key of the Olm key exchange.
    pub fn exchange_key(&self) -> Curve25519PublicKey {
        self.account.curve25519_key()
    }

    pub fn sign(&self, message: &[u8]) -> Signature {
        self.account.sign(message)
    }

    /// The encrypted form kept at rest.
    pub fn seal(&self, key: &[u8; 32]) -> String {
        self.account.pickle().encrypt(key)
    }

    pub fn unseal(sealed: &str, key: &[u8; 32]) -> Result<Self> {
        let pickle = AccountPickle::from_encrypted(sealed, key).map_err(|_| anyhow!("cannot open the identity"))?;
        Ok(Self { account: Account::from_pickle(pickle) })
    }

    /// For ft-crypto, which runs the Olm sessions on this account. Never exposed to the UI.
    pub fn account(&self) -> &Account {
        &self.account
    }

    pub fn account_mut(&mut self) -> &mut Account {
        &mut self.account
    }
}

pub fn verify(key: &Ed25519PublicKey, message: &[u8], signature: &Signature) -> Result<()> {
    key.verify(message, signature).map_err(|_| anyhow!("invalid signature"))
}

/// The "envelope" key (2026-09-24 review, A1): an X25519 key of its own, published in the Contact
/// Card, so that what goes through the router can be sealed for this device alone. It is not the
/// Olm identity key, which vodozemac keeps to itself, and it is stored sealed at rest like the
/// Olm account. Sealed boxes are NaCl's (`crypto_box`): nothing home-made.
pub struct EnvelopeKey {
    secret: crypto_box::SecretKey,
}

/// The public half, as a card carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvelopePublicKey([u8; 32]);

impl EnvelopePublicKey {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Seals `plaintext` so that only the holder of the secret key opens it, and nothing in the
    /// result says who sealed it (libsodium sealed box: ephemeral X25519 + XSalsa20-Poly1305).
    pub fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        crypto_box::PublicKey::from_bytes(self.0)
            .seal(&mut crypto_box::aead::OsRng, plaintext)
            .map_err(|_| anyhow!("cannot seal the envelope"))
    }
}

impl EnvelopeKey {
    pub fn generate() -> Self {
        Self { secret: crypto_box::SecretKey::generate(&mut crypto_box::aead::OsRng) }
    }

    pub fn public_key(&self) -> EnvelopePublicKey {
        EnvelopePublicKey(self.secret.public_key().to_bytes())
    }

    pub fn open(&self, sealed: &[u8]) -> Result<Vec<u8>> {
        self.secret.unseal(sealed).map_err(|_| anyhow!("the envelope is not for this device"))
    }

    /// The encrypted form kept at rest, under the same 32-byte key that seals the Olm account:
    /// XChaCha20-Poly1305 with a fresh nonce in front. Base64, like the Olm pickle.
    pub fn seal_at_rest(&self, key: &[u8; 32]) -> String {
        use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng};
        let cipher = chacha20poly1305::XChaCha20Poly1305::new(key.into());
        let nonce = chacha20poly1305::XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let mut out = nonce.to_vec();
        out.extend(cipher.encrypt(&nonce, self.secret.to_bytes().as_slice()).expect("encrypting 32 bytes cannot fail"));
        base64_encode(&out)
    }

    pub fn unseal_at_rest(sealed: &str, key: &[u8; 32]) -> Result<Self> {
        use chacha20poly1305::aead::{Aead, KeyInit};
        let bytes = base64_decode(sealed).ok_or_else(|| anyhow!("cannot open the envelope key"))?;
        if bytes.len() <= 24 {
            bail!("cannot open the envelope key");
        }
        let (nonce, ciphertext) = bytes.split_at(24);
        let cipher = chacha20poly1305::XChaCha20Poly1305::new(key.into());
        let secret = cipher.decrypt(nonce.into(), ciphertext).map_err(|_| anyhow!("cannot open the envelope key"))?;
        let secret: [u8; 32] = secret.try_into().map_err(|_| anyhow!("cannot open the envelope key"))?;
        Ok(Self { secret: crypto_box::SecretKey::from_bytes(secret) })
    }
}

impl fmt::Debug for EnvelopeKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EnvelopeKey(…)")
    }
}

// The pickle uses the same alphabet: base64, no padding.
fn base64_encode(bytes: &[u8]) -> String {
    vodozemac::base64_encode(bytes)
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    vodozemac::base64_decode(text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 32] = [7; 32];

    #[test]
    fn the_device_id_derives_from_the_signing_key() {
        let identity = Identity::generate();
        let hash = blake3::hash(identity.signing_key().as_bytes());
        let expected = format!("ft_{}", bs58::encode(hash.as_bytes()).into_string());
        assert_eq!(identity.device_id().as_str(), expected);
    }

    // Plan §6: never sequential, never guessable.
    #[test]
    fn every_new_identity_is_different() {
        assert_ne!(Identity::generate().device_id(), Identity::generate().device_id());
    }

    #[test]
    fn device_ids_are_parsed_and_validated() {
        let id = Identity::generate().device_id();
        assert_eq!(DeviceId::parse(id.as_str()).expect("a valid id"), id);
        assert!(DeviceId::parse("ft_0OIl").is_err(), "not base58");
        assert!(DeviceId::parse("xx_abc").is_err(), "wrong prefix");
        assert!(DeviceId::parse("ft_abc").is_err(), "too short for a BLAKE3 hash");
    }

    #[test]
    fn signatures_verify_only_for_the_signed_bytes() {
        let identity = Identity::generate();
        let signature = identity.sign(b"PUT /v1/device/push");
        assert!(verify(&identity.signing_key(), b"PUT /v1/device/push", &signature).is_ok());
        assert!(verify(&identity.signing_key(), b"DELETE /v1/device", &signature).is_err());
    }

    #[test]
    fn signatures_travel_as_text() {
        let identity = Identity::generate();
        let signature = identity.sign(b"hello");
        let parsed = Signature::from_base64(&signature.to_base64()).expect("parses");
        assert!(verify(&identity.signing_key(), b"hello", &parsed).is_ok());
    }

    #[test]
    fn the_identity_survives_sealing_and_unsealing() {
        let identity = Identity::generate();
        let sealed = identity.seal(&KEY);
        let restored = Identity::unseal(&sealed, &KEY).expect("opens with the right key");
        assert_eq!(restored.device_id(), identity.device_id());
        assert_eq!(restored.exchange_key(), identity.exchange_key());
    }

    #[test]
    fn a_wrong_key_cannot_unseal_the_identity() {
        let sealed = Identity::generate().seal(&KEY);
        assert!(Identity::unseal(&sealed, &[8; 32]).is_err());
    }

    // A1: what is sealed for a device opens only there, and names nobody.
    #[test]
    fn an_envelope_opens_only_with_its_key() {
        let (bob, carol) = (EnvelopeKey::generate(), EnvelopeKey::generate());
        let sealed = bob.public_key().seal(b"for bob").expect("seals");
        assert_eq!(bob.open(&sealed).expect("bob opens"), b"for bob");
        assert!(carol.open(&sealed).is_err());
        assert_ne!(sealed, bob.public_key().seal(b"for bob").expect("seals"), "a fresh ephemeral key each time");
        let mut tampered = sealed.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert!(bob.open(&tampered).is_err());
    }

    #[test]
    fn the_envelope_key_survives_sealing_at_rest_and_not_a_wrong_key() {
        let key = EnvelopeKey::generate();
        let sealed = key.seal_at_rest(&KEY);
        let back = EnvelopeKey::unseal_at_rest(&sealed, &KEY).expect("opens");
        assert_eq!(back.public_key(), key.public_key());
        assert!(EnvelopeKey::unseal_at_rest(&sealed, &[8; 32]).is_err());
        assert!(!sealed.contains(&base64_encode(&key.secret.to_bytes())), "the secret is not in the clear");
    }

    // The sealed form is what goes to disk: it must not contain the private keys in the clear.
    #[test]
    fn the_sealed_identity_hides_the_keys() {
        let identity = Identity::generate();
        let sealed = identity.seal(&KEY);
        assert!(!sealed.contains(&identity.exchange_key().to_base64()));
    }
}
