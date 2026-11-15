use anyhow::{anyhow, bail, Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ft_crypto::{contact_keys, ContactKeys};
use ft_identity::{
    verify, Curve25519PublicKey, DeviceId, Ed25519PublicKey, EnvelopePublicKey, Identity, Signature,
};
use serde::{Deserialize, Serialize};

pub mod invite;

pub const CARD_VERSION: u16 = 1;
// the card goes after '#' so it never reaches the web server
pub const LINK_PREFIX: &str = "https://flickertalk.com/add#";
const APP_LINK_PREFIX: &str = "flickertalk://add#";

// only travels inside the Contact Card, the router stores just its hash
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

    pub fn hash(&self) -> [u8; 32] {
        *blake3::hash(&self.0).as_bytes()
    }

    pub fn to_base64(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.0)
    }

    pub fn from_base64(text: &str) -> Result<Self> {
        let bytes = URL_SAFE_NO_PAD.decode(text).context("invalid capability")?;
        Ok(Self(
            bytes
                .try_into()
                .map_err(|_| anyhow!("a capability has 32 bytes"))?,
        ))
    }
}

impl std::fmt::Debug for RouteCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RouteCapability(…)")
    }
}

// raw bytes keep the QR code small
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Body {
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
    // always encode from these bytes, never from body, so fields this version doesn't know survive
    signed_body: Vec<u8>,
    signing_key: Ed25519PublicKey,
    exchange_key: Curve25519PublicKey,
    fallback_key: Curve25519PublicKey,
    route_capability: RouteCapability,
    envelope_key: Option<EnvelopePublicKey>,
    signature: Vec<u8>,
}

impl ContactCard {
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
        ContactKeys {
            exchange_key: self.exchange_key,
            fallback_key: self.fallback_key,
        }
    }

    pub fn route_capability(&self) -> RouteCapability {
        self.route_capability
    }

    pub fn encode(&self) -> Vec<u8> {
        cbor(&Wire {
            body: self.signed_body.clone(),
            signature: self.signature.clone(),
        })
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let wire: Wire = ciborium::from_reader(bytes).context("not a contact card")?;
        let body: Body =
            ciborium::from_reader(wire.body.as_slice()).context("not a contact card")?;
        if body.version != CARD_VERSION {
            bail!("unsupported contact card version {}", body.version);
        }
        let signing_key = Ed25519PublicKey::from_slice(&key_bytes(&body.signing_key)?)
            .context("invalid identity key")?;
        let signature =
            Signature::from_slice(&wire.signature).map_err(|_| anyhow!("invalid signature"))?;
        verify(&signing_key, &wire.body, &signature)
            .context("the contact card was changed or forged")?;
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

    pub fn from_link(link: &str) -> Result<Self> {
        let encoded = payload_after(link, &[LINK_PREFIX, APP_LINK_PREFIX])
            .ok_or_else(|| anyhow!("not a FlickerTalk contact link"))?;
        Self::decode(
            &URL_SAFE_NO_PAD
                .decode(encoded)
                .context("not a FlickerTalk contact link")?,
        )
    }

    #[cfg(test)]
    fn signed_by(mut self, other: &Identity) -> Self {
        self.signature = other.sign(&self.signed_body).to_bytes().to_vec();
        self
    }
}

pub const MOVE_LINK_PREFIX: &str = "https://flickertalk.com/move#";

// no identity key in here
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
        Self {
            card,
            secret: rand::random(),
        }
    }

    pub fn to_link(&self) -> String {
        let wire = MoveInviteWire {
            card: self.card.encode(),
            secret: self.secret,
        };
        format!("{MOVE_LINK_PREFIX}{}", URL_SAFE_NO_PAD.encode(cbor(&wire)))
    }

    pub fn from_link(link: &str) -> Result<Self> {
        let encoded = payload_after(link, &[MOVE_LINK_PREFIX])
            .ok_or_else(|| anyhow!("not a FlickerTalk move link"))?;
        let bytes = URL_SAFE_NO_PAD
            .decode(encoded)
            .context("not a FlickerTalk move link")?;
        let wire: MoveInviteWire =
            ciborium::from_reader(bytes.as_slice()).context("not a FlickerTalk move link")?;
        Ok(Self {
            card: ContactCard::decode(&wire.card)?,
            secret: wire.secret,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    Add,
    Move,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedLink {
    pub kind: LinkKind,
    pub link: String,
    pub valid: bool,
}

// exact prefix only, never searched for inside text like a pasted link
pub fn opened_link(url: &str) -> Option<OpenedLink> {
    let (kind, fragment) = if let Some(fragment) = url.strip_prefix(LINK_PREFIX) {
        (LinkKind::Add, fragment)
    } else {
        (LinkKind::Move, url.strip_prefix(MOVE_LINK_PREFIX)?)
    };
    let whole = !fragment.is_empty()
        && fragment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    let valid = whole
        && match kind {
            LinkKind::Add => ContactCard::from_link(url).is_ok(),
            LinkKind::Move => MoveInvite::from_link(url).is_ok(),
        };
    Some(OpenedLink {
        kind,
        link: url.to_owned(),
        valid,
    })
}

pub fn move_proof(secret: &[u8; 32], old_device: &str, new_device: &str) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new_keyed(secret);
    hasher.update(b"FlickerTalk move v1\0");
    hasher.update(old_device.as_bytes());
    hasher.update(b"\0");
    hasher.update(new_device.as_bytes());
    *hasher.finalize().as_bytes()
}

pub fn fingerprint(one: &Ed25519PublicKey, other: &Ed25519PublicKey) -> String {
    let (first, second) = if one.as_bytes() <= other.as_bytes() {
        (one, other)
    } else {
        (other, one)
    };
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

fn payload_after<'a>(text: &'a str, prefixes: &[&str]) -> Option<&'a str> {
    let (start, prefix) = prefixes
        .iter()
        .filter_map(|prefix| Some((text.find(prefix)?, prefix)))
        .min_by_key(|(start, _)| *start)?;
    let rest = &text[start + prefix.len()..];
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .unwrap_or(rest.len());
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
mod tests;
