//! Contact Card (Plan §32, §34, §106): the only way to find someone in v1. Its owner shows it as a
//! QR code or shares it as a link; whoever gets it can open an encrypted session with the owner
//! (ft-crypto) and reach them through the router, because the card carries the owner's route
//! capability. No server keeps a directory of cards or phone numbers (§31).
//!
//! The card is signed with the owner's identity key: a changed card is rejected.

use anyhow::{anyhow, bail, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ft_crypto::{contact_keys, ContactKeys};
use ft_identity::{verify, Curve25519PublicKey, DeviceId, Ed25519PublicKey, Identity, Signature};
use serde::{Deserialize, Serialize};

pub const CARD_VERSION: u16 = 1;
/// The fragment (after `#`) never reaches the web server that serves the landing page.
pub const LINK_PREFIX: &str = "https://flickertalk.com/add#";
const APP_LINK_PREFIX: &str = "flickertalk://add#";

/// 256 random bits that let their holder wake a device or leave it mail (§34). They travel only
/// inside the Contact Card; the router stores just their hash.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RouteCapability([u8; 32]);

impl RouteCapability {
    pub fn generate() -> Self {
        Self(rand::random())
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// What the router keeps to check a capability without knowing it.
    pub fn hash(&self) -> [u8; 32] {
        *blake3::hash(&self.0).as_bytes()
    }

    pub fn to_base64(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.0)
    }

    pub fn from_base64(text: &str) -> Result<Self> {
        let bytes = URL_SAFE_NO_PAD.decode(text).context("invalid capability")?;
        Ok(Self(bytes.try_into().map_err(|_| anyhow!("a capability has 32 bytes"))?))
    }
}

impl std::fmt::Debug for RouteCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RouteCapability(…)")
    }
}

/// Signed part of the card. Keys travel as raw bytes to keep the QR code small.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Body {
    version: u16,
    /// Name suggested by the owner; the person who scans can change it locally.
    name: Option<String>,
    #[serde(with = "serde_bytes")]
    signing_key: Vec<u8>,
    #[serde(with = "serde_bytes")]
    exchange_key: Vec<u8>,
    #[serde(with = "serde_bytes")]
    fallback_key: Vec<u8>,
    #[serde(with = "serde_bytes")]
    route_capability: Vec<u8>,
    /// Whether the owner uses the mailbox (§19).
    mailbox: bool,
}

#[derive(Serialize, Deserialize)]
struct Wire {
    #[serde(with = "serde_bytes")]
    body: Vec<u8>,
    #[serde(with = "serde_bytes")]
    signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContactCard {
    body: Body,
    signing_key: Ed25519PublicKey,
    exchange_key: Curve25519PublicKey,
    fallback_key: Curve25519PublicKey,
    route_capability: RouteCapability,
    signature: Vec<u8>,
}

impl ContactCard {
    pub fn create(identity: &mut Identity, name: Option<String>, route_capability: RouteCapability, mailbox: bool) -> Self {
        let keys = contact_keys(identity);
        let body = Body {
            version: CARD_VERSION,
            name,
            signing_key: identity.signing_key().as_bytes().to_vec(),
            exchange_key: keys.exchange_key.to_bytes().to_vec(),
            fallback_key: keys.fallback_key.to_bytes().to_vec(),
            route_capability: route_capability.as_bytes().to_vec(),
            mailbox,
        };
        let signature = identity.sign(&cbor(&body)).to_bytes().to_vec();
        Self {
            body,
            signing_key: identity.signing_key(),
            exchange_key: keys.exchange_key,
            fallback_key: keys.fallback_key,
            route_capability,
            signature,
        }
    }

    pub fn device_id(&self) -> DeviceId {
        DeviceId::from_signing_key(&self.signing_key)
    }

    pub fn name(&self) -> Option<&str> {
        self.body.name.as_deref()
    }

    pub fn mailbox(&self) -> bool {
        self.body.mailbox
    }

    pub fn signing_key(&self) -> Ed25519PublicKey {
        self.signing_key
    }

    pub fn contact_keys(&self) -> ContactKeys {
        ContactKeys { exchange_key: self.exchange_key, fallback_key: self.fallback_key }
    }

    pub fn route_capability(&self) -> RouteCapability {
        self.route_capability
    }

    pub fn encode(&self) -> Vec<u8> {
        cbor(&Wire { body: cbor(&self.body), signature: self.signature.clone() })
    }

    /// Only a card whose signature matches its own identity key is accepted.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let wire: Wire = ciborium::from_reader(bytes).context("not a contact card")?;
        let body: Body = ciborium::from_reader(wire.body.as_slice()).context("not a contact card")?;
        if body.version != CARD_VERSION {
            bail!("unsupported contact card version {}", body.version);
        }
        let signing_key = Ed25519PublicKey::from_slice(&key_bytes(&body.signing_key)?).context("invalid identity key")?;
        let signature = Signature::from_slice(&wire.signature).map_err(|_| anyhow!("invalid signature"))?;
        verify(&signing_key, &wire.body, &signature).context("the contact card was changed or forged")?;
        Ok(Self {
            signing_key,
            exchange_key: Curve25519PublicKey::from_bytes(key_bytes(&body.exchange_key)?),
            fallback_key: Curve25519PublicKey::from_bytes(key_bytes(&body.fallback_key)?),
            route_capability: RouteCapability::from_bytes(key_bytes(&body.route_capability)?),
            signature: wire.signature,
            body,
        })
    }

    pub fn to_link(&self) -> String {
        format!("{LINK_PREFIX}{}", URL_SAFE_NO_PAD.encode(self.encode()))
    }

    pub fn from_link(link: &str) -> Result<Self> {
        let encoded = link
            .trim()
            .strip_prefix(LINK_PREFIX)
            .or_else(|| link.trim().strip_prefix(APP_LINK_PREFIX))
            .ok_or_else(|| anyhow!("not a FlickerTalk contact link"))?;
        Self::decode(&URL_SAFE_NO_PAD.decode(encoded).context("not a FlickerTalk contact link")?)
    }

    #[cfg(test)]
    fn signed_by(mut self, other: &Identity) -> Self {
        self.signature = other.sign(&cbor(&self.body)).to_bytes().to_vec();
        self
    }
}

/// Safety number of a pair of contacts (§29): the same on both phones, compared in person to
/// rule out a swapped key. 12 groups of 4 hex digits from BLAKE3 over both identity keys, sorted.
pub fn fingerprint(one: &Ed25519PublicKey, other: &Ed25519PublicKey) -> String {
    let (first, second) = if one.as_bytes() <= other.as_bytes() { (one, other) } else { (other, one) };
    let mut hasher = blake3::Hasher::new_derive_key("FlickerTalk contact fingerprint v1");
    hasher.update(first.as_bytes());
    hasher.update(second.as_bytes());
    let hash = hasher.finalize();
    hash.as_bytes()[..24]
        .chunks(2)
        .map(|pair| format!("{:02x}{:02x}", pair[0], pair[1]))
        .collect::<Vec<_>>()
        .join(" ")
}

fn key_bytes(bytes: &[u8]) -> Result<[u8; 32]> {
    bytes.try_into().map_err(|_| anyhow!("a key has 32 bytes"))
}

fn cbor<T: Serialize>(value: &T) -> Vec<u8> {
    let mut bytes = Vec::new();
    ciborium::into_writer(value, &mut bytes).expect("encoding into memory cannot fail");
    bytes
}

#[cfg(test)]
mod tests {
    use ft_crypto::Channel;
    use ft_identity::Identity;

    use super::*;

    fn card_of(identity: &mut Identity) -> ContactCard {
        ContactCard::create(identity, Some("Bob".to_owned()), RouteCapability::generate(), true)
    }

    #[test]
    fn a_card_names_its_owner() {
        let mut bob = Identity::generate();
        let card = card_of(&mut bob);
        assert_eq!(card.device_id(), bob.device_id());
        assert_eq!(card.name(), Some("Bob"));
        assert!(card.mailbox());
    }

    #[test]
    fn a_scanned_card_opens_an_encrypted_channel_with_its_owner() {
        let (alice, mut bob) = (Identity::generate(), Identity::generate());
        let scanned = ContactCard::from_link(&card_of(&mut bob).to_link()).expect("valid card");

        let mut channel = Channel::open(&alice, &scanned.contact_keys()).expect("opens");
        let sealed = channel.encrypt(&alice.device_id(), b"hi").expect("encrypts");
        let read = Channel::default().decrypt(&mut bob, alice.exchange_key(), &sealed).expect("bob reads");
        assert_eq!(read, b"hi");
    }

    #[test]
    fn cards_travel_as_links() {
        let card = card_of(&mut Identity::generate());
        let link = card.to_link();
        assert!(link.starts_with("https://flickertalk.com/add#"), "{link}");
        assert_eq!(ContactCard::from_link(&link).expect("parses"), card);
    }

    // The part after `#` never reaches a web server: the card is not leaked by opening the link.
    #[test]
    fn the_card_rides_in_the_link_fragment() {
        let link = card_of(&mut Identity::generate()).to_link();
        assert!(!link.contains('?'));
    }

    #[test]
    fn the_link_fits_comfortably_in_a_qr_code() {
        let link = card_of(&mut Identity::generate()).to_link();
        assert!(link.len() < 600, "{} characters", link.len());
    }

    #[test]
    fn a_changed_card_is_rejected() {
        let card = card_of(&mut Identity::generate());
        let mut bytes = card.encode();
        let position = bytes.windows(3).position(|w| w == b"Bob").expect("the name is in the card");
        bytes[position] = b'R';
        assert!(ContactCard::decode(&bytes).is_err());
    }

    #[test]
    fn a_card_signed_by_someone_else_is_rejected() {
        let mut bob = Identity::generate();
        let mallory = Identity::generate();
        let forged = ContactCard::create(&mut bob, None, RouteCapability::generate(), true).signed_by(&mallory);
        assert!(ContactCard::decode(&forged.encode()).is_err());
    }

    #[test]
    fn links_that_are_not_cards_are_rejected() {
        assert!(ContactCard::from_link("https://flickertalk.com/add#bm90IGEgY2FyZA").is_err());
        assert!(ContactCard::from_link("https://example.com/").is_err());
    }

    // §29: both phones compute the same safety number and the users compare it in person.
    #[test]
    fn both_sides_see_the_same_fingerprint() {
        let (alice, bob) = (Identity::generate(), Identity::generate());
        let at_alice = fingerprint(&alice.signing_key(), &bob.signing_key());
        assert_eq!(at_alice, fingerprint(&bob.signing_key(), &alice.signing_key()));
        assert_eq!(at_alice.split(' ').count(), 12);
        assert!(at_alice.split(' ').all(|group| group.len() == 4 && group.chars().all(|c| c.is_ascii_hexdigit())));
    }

    #[test]
    fn another_pair_has_another_fingerprint() {
        let (alice, bob, mallory) = (Identity::generate(), Identity::generate(), Identity::generate());
        assert_ne!(fingerprint(&alice.signing_key(), &bob.signing_key()), fingerprint(&alice.signing_key(), &mallory.signing_key()));
    }

    // §34: the router only ever sees a hash of the capability.
    #[test]
    fn route_capabilities_are_random_and_hashed_for_the_router() {
        let capability = RouteCapability::generate();
        assert_ne!(capability, RouteCapability::generate());
        assert_eq!(capability.hash(), *blake3::hash(capability.as_bytes()).as_bytes());
        assert_eq!(RouteCapability::from_base64(&capability.to_base64()).expect("parses"), capability);
    }
}
