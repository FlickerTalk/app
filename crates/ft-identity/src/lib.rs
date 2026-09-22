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

    // The sealed form is what goes to disk: it must not contain the private keys in the clear.
    #[test]
    fn the_sealed_identity_hides_the_keys() {
        let identity = Identity::generate();
        let sealed = identity.seal(&KEY);
        assert!(!sealed.contains(&identity.exchange_key().to_base64()));
    }
}
