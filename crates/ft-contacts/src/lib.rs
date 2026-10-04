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
use ft_identity::{verify, Curve25519PublicKey, DeviceId, Ed25519PublicKey, EnvelopePublicKey, Identity, Signature};
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
    /// The owner's envelope key (2026-09-24 review, A1): whoever has it seals what goes through
    /// the router for the owner alone. Absent from cards made before it; a reader that does not
    /// know the field ignores it, and the signature covers the raw bytes either way.
    #[serde(default, with = "serde_bytes")]
    envelope_key: Option<Vec<u8>>,
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
    /// The body exactly as it was signed. A card is encoded from these bytes, never from `body`,
    /// so a field this version does not know survives being stored or passed on.
    signed_body: Vec<u8>,
    signing_key: Ed25519PublicKey,
    exchange_key: Curve25519PublicKey,
    fallback_key: Curve25519PublicKey,
    route_capability: RouteCapability,
    envelope_key: Option<EnvelopePublicKey>,
    signature: Vec<u8>,
}

impl ContactCard {
    /// `envelope` is the owner's envelope key (A1); `None` makes a card an older app would make.
    pub fn create(
        identity: &mut Identity,
        name: Option<String>,
        route_capability: RouteCapability,
        mailbox: bool,
        envelope: Option<EnvelopePublicKey>,
    ) -> Self {
        let keys = contact_keys(identity);
        let body = Body {
            version: CARD_VERSION,
            name,
            signing_key: identity.signing_key().as_bytes().to_vec(),
            exchange_key: keys.exchange_key.to_bytes().to_vec(),
            fallback_key: keys.fallback_key.to_bytes().to_vec(),
            route_capability: route_capability.as_bytes().to_vec(),
            mailbox,
            envelope_key: envelope.map(|key| key.as_bytes().to_vec()),
        };
        let signed_body = cbor(&body);
        let signature = identity.sign(&signed_body).to_bytes().to_vec();
        Self {
            body,
            signed_body,
            signing_key: identity.signing_key(),
            exchange_key: keys.exchange_key,
            fallback_key: keys.fallback_key,
            route_capability,
            envelope_key: envelope,
            signature,
        }
    }

    /// The owner's envelope key, if the card carries one: what to seal their mail with.
    pub fn envelope_key(&self) -> Option<EnvelopePublicKey> {
        self.envelope_key
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
        cbor(&Wire { body: self.signed_body.clone(), signature: self.signature.clone() })
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
        let envelope_key = match &body.envelope_key {
            Some(bytes) => Some(EnvelopePublicKey::from_bytes(key_bytes(bytes)?)),
            None => None,
        };
        Ok(Self {
            signing_key,
            exchange_key: Curve25519PublicKey::from_bytes(key_bytes(&body.exchange_key)?),
            fallback_key: Curve25519PublicKey::from_bytes(key_bytes(&body.fallback_key)?),
            route_capability: RouteCapability::from_bytes(key_bytes(&body.route_capability)?),
            envelope_key,
            signature: wire.signature,
            body,
            signed_body: wire.body,
        })
    }

    pub fn to_link(&self) -> String {
        format!("{LINK_PREFIX}{}", URL_SAFE_NO_PAD.encode(self.encode()))
    }

    /// Reads the card from its link, also when the link comes inside pasted text.
    pub fn from_link(link: &str) -> Result<Self> {
        let encoded =
            payload_after(link, &[LINK_PREFIX, APP_LINK_PREFIX]).ok_or_else(|| anyhow!("not a FlickerTalk contact link"))?;
        Self::decode(&URL_SAFE_NO_PAD.decode(encoded).context("not a FlickerTalk contact link")?)
    }

    #[cfg(test)]
    fn signed_by(mut self, other: &Identity) -> Self {
        self.signature = other.sign(&self.signed_body).to_bytes().to_vec();
        self
    }
}

/// Where a move invite points (§60): the new phone shows it as a QR code.
pub const MOVE_LINK_PREFIX: &str = "https://flickertalk.com/move#";

/// The new phone's invitation to receive an identity (§60): its own card, so the old phone can
/// reach it, and a one-time secret that only whoever reads the QR learns. No identity key.
#[derive(Debug, Clone)]
pub struct MoveInvite {
    pub card: ContactCard,
    pub secret: [u8; 32],
}

#[derive(Serialize, Deserialize)]
struct MoveInviteWire {
    #[serde(with = "serde_bytes")]
    card: Vec<u8>,
    #[serde(with = "serde_bytes")]
    secret: [u8; 32],
}

impl MoveInvite {
    pub fn new(card: ContactCard) -> Self {
        Self { card, secret: rand::random() }
    }

    pub fn to_link(&self) -> String {
        let wire = MoveInviteWire { card: self.card.encode(), secret: self.secret };
        format!("{MOVE_LINK_PREFIX}{}", URL_SAFE_NO_PAD.encode(cbor(&wire)))
    }

    pub fn from_link(link: &str) -> Result<Self> {
        let encoded = payload_after(link, &[MOVE_LINK_PREFIX]).ok_or_else(|| anyhow!("not a FlickerTalk move link"))?;
        let bytes = URL_SAFE_NO_PAD.decode(encoded).context("not a FlickerTalk move link")?;
        let wire: MoveInviteWire = ciborium::from_reader(bytes.as_slice()).context("not a FlickerTalk move link")?;
        Ok(Self { card: ContactCard::decode(&wire.card)?, secret: wire.secret })
    }
}

/// What the old phone shows to prove it read the QR: bound to the secret and to both phones.
pub fn move_proof(secret: &[u8; 32], old_device: &str, new_device: &str) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new_keyed(secret);
    hasher.update(b"FlickerTalk move v1\0");
    hasher.update(old_device.as_bytes());
    hasher.update(b"\0");
    hasher.update(new_device.as_bytes());
    *hasher.finalize().as_bytes()
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

/// The base64url run that follows the first of `prefixes` found anywhere in `text`. A link is
/// often pasted with the message it was shared in, or followed by punctuation or more text.
fn payload_after<'a>(text: &'a str, prefixes: &[&str]) -> Option<&'a str> {
    let (start, prefix) = prefixes.iter().filter_map(|prefix| Some((text.find(prefix)?, prefix))).min_by_key(|(start, _)| *start)?;
    let rest = &text[start + prefix.len()..];
    let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_')).unwrap_or(rest.len());
    Some(&rest[..end])
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

    // §60: the new phone's QR pairs the two phones: its (temporary) card and a one-time secret,
    // never an identity key.
    #[test]
    fn a_move_invite_travels_as_a_link() {
        let mut identity = Identity::generate();
        let invite = MoveInvite::new(card_of(&mut identity));
        let link = invite.to_link();
        assert!(link.starts_with(MOVE_LINK_PREFIX));
        let back = MoveInvite::from_link(&link).expect("parses");
        assert_eq!(back.secret, invite.secret);
        assert_eq!(back.card.device_id(), identity.device_id());
        assert_ne!(MoveInvite::new(card_of(&mut identity)).secret, invite.secret, "a fresh secret each time");
        assert!(MoveInvite::from_link("https://flickertalk.com/move#garbage").is_err());
        assert!(MoveInvite::from_link(&card_of(&mut identity).to_link()).is_err(), "a contact link is not an invite");
    }

    // Only whoever read the QR knows the secret: the proof ties it to both phones.
    #[test]
    fn a_move_proof_needs_the_secret_and_both_phones() {
        let secret = [7; 32];
        let proof = move_proof(&secret, "ft_old", "ft_new");
        assert_eq!(proof, move_proof(&secret, "ft_old", "ft_new"));
        assert_ne!(proof, move_proof(&[8; 32], "ft_old", "ft_new"));
        assert_ne!(proof, move_proof(&secret, "ft_other", "ft_new"));
        assert_ne!(proof, move_proof(&secret, "ft_old", "ft_other"));
    }

    fn card_of(identity: &mut Identity) -> ContactCard {
        let envelope = ft_identity::EnvelopeKey::generate().public_key();
        ContactCard::create(identity, Some("Bob".to_owned()), RouteCapability::generate(), true, Some(envelope))
    }

    // A1: the card carries the envelope key; an older card without it still reads, and reads the
    // same to an older app (the field is optional and the signature covers the raw bytes).
    #[test]
    fn a_card_carries_its_envelope_key_or_none() {
        let mut bob = Identity::generate();
        let with = card_of(&mut bob);
        assert!(with.envelope_key().is_some());
        assert_eq!(ContactCard::decode(&with.encode()).expect("decodes").envelope_key(), with.envelope_key());
        let without = ContactCard::create(&mut bob, None, RouteCapability::generate(), true, None);
        assert_eq!(ContactCard::decode(&without.encode()).expect("decodes").envelope_key(), None);
        assert!(with.to_link().len() < 600, "{} characters", with.to_link().len());
    }

    // A card made by a newer app, with a field this version does not know: what a future version
    // would send once it adds one to the body.
    fn card_with_an_unknown_field(identity: &mut Identity) -> Vec<u8> {
        #[derive(Serialize)]
        struct FutureBody {
            version: u16,
            name: Option<String>,
            #[serde(with = "serde_bytes")]
            signing_key: Vec<u8>,
            #[serde(with = "serde_bytes")]
            exchange_key: Vec<u8>,
            #[serde(with = "serde_bytes")]
            fallback_key: Vec<u8>,
            #[serde(with = "serde_bytes")]
            route_capability: Vec<u8>,
            mailbox: bool,
            #[serde(with = "serde_bytes")]
            envelope_key: Option<Vec<u8>>,
            avatar_colour: u32,
        }
        let keys = contact_keys(identity);
        let body = cbor(&FutureBody {
            version: CARD_VERSION,
            name: Some("Bob".to_owned()),
            signing_key: identity.signing_key().as_bytes().to_vec(),
            exchange_key: keys.exchange_key.to_bytes().to_vec(),
            fallback_key: keys.fallback_key.to_bytes().to_vec(),
            route_capability: RouteCapability::generate().as_bytes().to_vec(),
            mailbox: true,
            envelope_key: Some(ft_identity::EnvelopeKey::generate().public_key().as_bytes().to_vec()),
            avatar_colour: 0x00ff_8800,
        });
        let signature = identity.sign(&body).to_bytes().to_vec();
        cbor(&Wire { body, signature })
    }

    // A card is kept and passed on as it was signed: a field this version does not know is not
    // dropped, or the card would no longer verify once stored or forwarded.
    #[test]
    fn a_card_with_an_unknown_field_survives_being_passed_on() {
        let mut bob = Identity::generate();
        let bytes = card_with_an_unknown_field(&mut bob);
        let card = ContactCard::decode(&bytes).expect("a newer card reads");
        assert_eq!(card.device_id(), bob.device_id());
        assert_eq!(card.encode(), bytes, "encoded as it was signed");
        let again = ContactCard::decode(&card.encode()).expect("still verifies once passed on");
        assert_eq!(again, card);
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
        let forged = ContactCard::create(&mut bob, None, RouteCapability::generate(), true, None).signed_by(&mallory);
        assert!(ContactCard::decode(&forged.encode()).is_err());
    }

    #[test]
    fn links_that_are_not_cards_are_rejected() {
        assert!(ContactCard::from_link("https://flickertalk.com/add#bm90IGEgY2FyZA").is_err());
        assert!(ContactCard::from_link("https://example.com/").is_err());
    }

    /// How a link reaches the paste field: inside the share sentence, with whatever the chat app
    /// or the person put around it.
    fn pasted_forms(link: &str) -> Vec<String> {
        vec![
            format!("Add me on FlickerTalk: {link}"),
            format!("Hi!\nAdd me on FlickerTalk: {link}\nSee you there"),
            format!("{link}."),
            format!("({link})"),
            format!("{link}\n"),
            format!("  \t{link}  \n"),
            format!("Add me on FlickerTalk: {link}. Thanks"),
        ]
    }

    // The share sheet sends "Add me on FlickerTalk: <link>", and copying the message copies all of
    // it: the card is found wherever the link sits in the pasted text.
    #[test]
    fn a_card_link_is_found_inside_pasted_text() {
        let card = card_of(&mut Identity::generate());
        let link = card.to_link();
        for text in pasted_forms(&link) {
            assert_eq!(ContactCard::from_link(&text).expect(&text), card, "{text:?}");
        }
        let app_link = link.replacen(LINK_PREFIX, APP_LINK_PREFIX, 1);
        assert_eq!(ContactCard::from_link(&format!("Open {app_link}, please.")).expect("app link"), card);
    }

    #[test]
    fn pasted_text_without_a_valid_card_is_rejected() {
        let error = ContactCard::from_link("Add me on FlickerTalk!").expect_err("no link");
        assert_eq!(error.to_string(), "not a FlickerTalk contact link");
        assert!(ContactCard::from_link("Add me on FlickerTalk: https://flickertalk.com/add#garbage.").is_err());
        assert!(ContactCard::from_link("Add me on FlickerTalk: https://flickertalk.com/add#bm90IGEgY2FyZA").is_err());
        let invite = MoveInvite::new(card_of(&mut Identity::generate())).to_link();
        assert!(ContactCard::from_link(&format!("Scan this: {invite}")).is_err(), "a move invite is not a card");
    }

    #[test]
    fn a_move_invite_is_found_inside_pasted_text() {
        let mut identity = Identity::generate();
        let invite = MoveInvite::new(card_of(&mut identity));
        for text in pasted_forms(&invite.to_link()) {
            let back = MoveInvite::from_link(&text).expect(&text);
            assert_eq!(back.secret, invite.secret, "{text:?}");
            assert_eq!(back.card.device_id(), identity.device_id(), "{text:?}");
        }
    }

    #[test]
    fn pasted_text_without_a_valid_invite_is_rejected() {
        let error = MoveInvite::from_link("Move to my new phone").expect_err("no link");
        assert_eq!(error.to_string(), "not a FlickerTalk move link");
        assert!(MoveInvite::from_link("Move here: https://flickertalk.com/move#garbage.").is_err());
        let card = card_of(&mut Identity::generate()).to_link();
        assert!(MoveInvite::from_link(&format!("Add me on FlickerTalk: {card}")).is_err(), "a card is not an invite");
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
