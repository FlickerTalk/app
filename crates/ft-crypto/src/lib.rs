//! End-to-end encryption between two devices (Plan §28, §106), with Olm (vodozemac): the Double
//! Ratchet, asynchronous, with forward secrecy. This crate wraps the library; it implements no
//! primitive.
//!
//! First contact needs no server: the Contact Card carries the owner's Curve25519 key and an Olm
//! fallback key (`ContactKeys`). Whoever scans it opens a `Channel` and sends a pre-key message;
//! the owner answers from it. Several sessions may exist with one peer (both scanned at the same
//! time, say): a `Channel` keeps them all, decrypts with whichever fits and encrypts with the one
//! that last worked, so both sides converge.

use anyhow::{anyhow, bail, Context, Result};
use ft_identity::{Curve25519PublicKey, DeviceId, Identity};
use ft_protocol::{Sealed, SealedKind, PROTOCOL_VERSION};
use vodozemac::olm::{OlmMessage, Session, SessionConfig, SessionPickle};

/// At most this many sessions are kept per peer; the oldest go first.
const MAX_SESSIONS: usize = 4;

/// The keys a Contact Card publishes so that others can open a session with its owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContactKeys {
    pub exchange_key: Curve25519PublicKey,
    /// Reusable by every contact until rotated (Olm fallback key).
    pub fallback_key: Curve25519PublicKey,
}

/// The keys for this device's Contact Card, created on first use and stable afterwards.
pub fn contact_keys(identity: &mut Identity) -> ContactKeys {
    let account = identity.account_mut();
    // Never marked as published: vodozemac only reports an unpublished fallback key, and marking it
    // would make the next call rotate the key under every card already shared.
    if account.fallback_key().is_empty() {
        account.generate_fallback_key();
    }
    let fallback_key = *account.fallback_key().values().next().expect("a fallback key was just ensured");
    ContactKeys { exchange_key: account.curve25519_key(), fallback_key }
}

/// Opens the first message of someone not yet among the contacts, with the identity key its
/// pre-key message carries. Returns that key so the caller can check it against the Contact Card
/// inside the plaintext before trusting the sender.
pub fn accept_first_contact(identity: &mut Identity, sealed: &Sealed) -> Result<(Channel, Vec<u8>, Curve25519PublicKey)> {
    if sealed.kind != SealedKind::PreKey {
        bail!("a first contact starts with a pre-key message");
    }
    let OlmMessage::PreKey(pre_key) = OlmMessage::from_parts(0, &sealed.ciphertext).context("malformed Olm message")? else {
        bail!("a first contact starts with a pre-key message");
    };
    let sender_key = pre_key.identity_key();
    let created = identity
        .account_mut()
        .create_inbound_session(SessionConfig::version_1(), sender_key, &pre_key)
        .map_err(|error| anyhow!("cannot accept the session: {error}"))?;
    Ok((Channel { sessions: vec![created.session] }, created.plaintext, sender_key))
}

/// Encrypted conversation with one peer.
#[derive(Default)]
pub struct Channel {
    /// The last one is the one that worked most recently and encrypts.
    sessions: Vec<Session>,
}

impl Channel {
    /// First contact after scanning the peer's Contact Card.
    pub fn open(identity: &Identity, peer: &ContactKeys) -> Result<Self> {
        let session = identity
            .account()
            .create_outbound_session(SessionConfig::version_1(), peer.exchange_key, peer.fallback_key)
            .map_err(|error| anyhow!("cannot open a session: {error}"))?;
        Ok(Self { sessions: vec![session] })
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    pub fn encrypt(&mut self, me: &DeviceId, plaintext: &[u8]) -> Result<Sealed> {
        let session = self.sessions.last_mut().ok_or_else(|| anyhow!("no session with this contact yet"))?;
        let message = session.encrypt(plaintext).map_err(|error| anyhow!("cannot encrypt: {error}"))?;
        let (kind, ciphertext) = message.to_parts();
        let kind = if kind == 0 { SealedKind::PreKey } else { SealedKind::Normal };
        Ok(Sealed { version: PROTOCOL_VERSION, from: me.as_str().to_owned(), kind, ciphertext })
    }

    /// `peer_key` is the sender's Curve25519 key, from its Contact Card: a pre-key message that
    /// claims another identity is rejected.
    pub fn decrypt(&mut self, identity: &mut Identity, peer_key: Curve25519PublicKey, sealed: &Sealed) -> Result<Vec<u8>> {
        let kind = match sealed.kind {
            SealedKind::PreKey => 0,
            SealedKind::Normal => 1,
        };
        let message = OlmMessage::from_parts(kind, &sealed.ciphertext).context("malformed Olm message")?;

        for index in (0..self.sessions.len()).rev() {
            if let Ok(plaintext) = self.sessions[index].decrypt(&message) {
                let session = self.sessions.remove(index);
                self.sessions.push(session);
                return Ok(plaintext);
            }
        }

        let OlmMessage::PreKey(pre_key) = &message else {
            bail!("no session can decrypt this message");
        };
        if self.sessions.iter().any(|session| session.session_id() == pre_key.session_id()) {
            bail!("the message does not decrypt with its own session");
        }
        let created = identity
            .account_mut()
            .create_inbound_session(SessionConfig::version_1(), peer_key, pre_key)
            .map_err(|error| anyhow!("cannot accept the session: {error}"))?;
        self.sessions.push(created.session);
        if self.sessions.len() > MAX_SESSIONS {
            self.sessions.remove(0);
        }
        Ok(created.plaintext)
    }

    /// The encrypted form kept at rest: one encrypted session pickle per line.
    pub fn seal(&self, key: &[u8; 32]) -> String {
        self.sessions.iter().map(|session| session.pickle().encrypt(key)).collect::<Vec<_>>().join("\n")
    }

    pub fn unseal(sealed: &str, key: &[u8; 32]) -> Result<Self> {
        let sessions = sealed
            .lines()
            .filter(|line| !line.is_empty())
            .map(|line| {
                SessionPickle::from_encrypted(line, key)
                    .map(Session::from_pickle)
                    .map_err(|_| anyhow!("cannot open the session"))
            })
            .collect::<Result<_>>()?;
        Ok(Self { sessions })
    }
}

#[cfg(test)]
mod tests {
    use ft_identity::Identity;
    use ft_protocol::SealedKind;

    use super::*;

    const KEY: [u8; 32] = [3; 32];

    struct Device {
        identity: Identity,
    }

    impl Device {
        fn new() -> Self {
            Self { identity: Identity::generate() }
        }

        fn card_keys(&mut self) -> ContactKeys {
            contact_keys(&mut self.identity)
        }
    }

    #[test]
    fn two_devices_talk_after_one_scans_the_other() {
        let (mut alice, mut bob) = (Device::new(), Device::new());
        let bob_keys = bob.card_keys();

        let mut alice_to_bob = Channel::open(&alice.identity, &bob_keys).expect("opens");
        let first = alice_to_bob.encrypt(&alice.identity.device_id(), b"hello bob").expect("encrypts");
        assert_eq!(first.kind, SealedKind::PreKey);

        let mut bob_to_alice = Channel::default();
        let plaintext = bob_to_alice
            .decrypt(&mut bob.identity, alice.identity.exchange_key(), &first)
            .expect("bob reads it");
        assert_eq!(plaintext, b"hello bob");

        let reply = bob_to_alice.encrypt(&bob.identity.device_id(), b"hi alice").expect("encrypts");
        let plaintext = alice_to_bob.decrypt(&mut alice.identity, bob.identity.exchange_key(), &reply).expect("reads");
        assert_eq!(plaintext, b"hi alice");

        // Once Alice has heard back, the key exchange is over.
        let next = alice_to_bob.encrypt(&alice.identity.device_id(), b"again").expect("encrypts");
        assert_eq!(next.kind, SealedKind::Normal);
        assert_eq!(bob_to_alice.decrypt(&mut bob.identity, alice.identity.exchange_key(), &next).expect("reads"), b"again");
    }

    #[test]
    fn the_sender_is_named_so_the_receiver_can_pick_the_channel() {
        let (alice, mut bob) = (Device::new(), Device::new());
        let mut channel = Channel::open(&alice.identity, &bob.card_keys()).expect("opens");
        let sealed = channel.encrypt(&alice.identity.device_id(), b"x").expect("encrypts");
        assert_eq!(sealed.from, alice.identity.device_id().as_str());
    }

    #[test]
    fn nobody_else_can_read_it() {
        let (alice, mut bob, mut carol) = (Device::new(), Device::new(), Device::new());
        let mut channel = Channel::open(&alice.identity, &bob.card_keys()).expect("opens");
        let sealed = channel.encrypt(&alice.identity.device_id(), b"secret").expect("encrypts");
        let _ = carol.card_keys();
        assert!(Channel::default().decrypt(&mut carol.identity, alice.identity.exchange_key(), &sealed).is_err());
    }

    #[test]
    fn a_tampered_message_is_rejected() {
        let (alice, mut bob) = (Device::new(), Device::new());
        let mut channel = Channel::open(&alice.identity, &bob.card_keys()).expect("opens");
        let mut sealed = channel.encrypt(&alice.identity.device_id(), b"pay 10").expect("encrypts");
        let last = sealed.ciphertext.len() - 1;
        sealed.ciphertext[last] ^= 1;
        assert!(Channel::default().decrypt(&mut bob.identity, alice.identity.exchange_key(), &sealed).is_err());
    }

    // The same Contact Card is scanned by many people: its fallback key serves them all.
    #[test]
    fn one_contact_card_serves_many_contacts() {
        let (alice, dave, mut bob) = (Device::new(), Device::new(), Device::new());
        let bob_keys = bob.card_keys();
        for sender in [&alice, &dave] {
            let mut channel = Channel::open(&sender.identity, &bob_keys).expect("opens");
            let sealed = channel.encrypt(&sender.identity.device_id(), b"hi").expect("encrypts");
            let read = Channel::default().decrypt(&mut bob.identity, sender.identity.exchange_key(), &sealed);
            assert_eq!(read.expect("bob reads every sender"), b"hi");
        }
    }

    #[test]
    fn the_card_keys_stay_the_same_until_rotated() {
        let mut bob = Device::new();
        assert_eq!(bob.card_keys(), bob.card_keys());
    }

    // Both scanned each other at the same time: two sessions, and they must converge.
    #[test]
    fn simultaneous_first_contact_converges() {
        let (mut alice, mut bob) = (Device::new(), Device::new());
        let (alice_keys, bob_keys) = (alice.card_keys(), bob.card_keys());
        let mut at_alice = Channel::open(&alice.identity, &bob_keys).expect("opens");
        let mut at_bob = Channel::open(&bob.identity, &alice_keys).expect("opens");

        let from_alice = at_alice.encrypt(&alice.identity.device_id(), b"a1").expect("encrypts");
        let from_bob = at_bob.encrypt(&bob.identity.device_id(), b"b1").expect("encrypts");
        assert_eq!(at_bob.decrypt(&mut bob.identity, alice.identity.exchange_key(), &from_alice).expect("reads"), b"a1");
        assert_eq!(at_alice.decrypt(&mut alice.identity, bob.identity.exchange_key(), &from_bob).expect("reads"), b"b1");

        for round in 0..3 {
            let text = format!("round {round}");
            let sealed = at_alice.encrypt(&alice.identity.device_id(), text.as_bytes()).expect("encrypts");
            assert_eq!(at_bob.decrypt(&mut bob.identity, alice.identity.exchange_key(), &sealed).expect("reads"), text.as_bytes());
            let sealed = at_bob.encrypt(&bob.identity.device_id(), text.as_bytes()).expect("encrypts");
            assert_eq!(at_alice.decrypt(&mut alice.identity, bob.identity.exchange_key(), &sealed).expect("reads"), text.as_bytes());
        }
    }

    // Messages from the mailbox may arrive late and out of order.
    #[test]
    fn out_of_order_messages_still_decrypt() {
        let (mut alice, mut bob) = (Device::new(), Device::new());
        let mut at_alice = Channel::open(&alice.identity, &bob.card_keys()).expect("opens");
        let mut at_bob = Channel::default();
        let hello = at_alice.encrypt(&alice.identity.device_id(), b"hello").expect("encrypts");
        at_bob.decrypt(&mut bob.identity, alice.identity.exchange_key(), &hello).expect("reads");
        let ack = at_bob.encrypt(&bob.identity.device_id(), b"ok").expect("encrypts");
        at_alice.decrypt(&mut alice.identity, bob.identity.exchange_key(), &ack).expect("reads");

        let first = at_alice.encrypt(&alice.identity.device_id(), b"1").expect("encrypts");
        let second = at_alice.encrypt(&alice.identity.device_id(), b"2").expect("encrypts");
        assert_eq!(at_bob.decrypt(&mut bob.identity, alice.identity.exchange_key(), &second).expect("reads"), b"2");
        assert_eq!(at_bob.decrypt(&mut bob.identity, alice.identity.exchange_key(), &first).expect("reads"), b"1");
    }

    #[test]
    fn channels_survive_sealing_mid_conversation() {
        let (mut alice, mut bob) = (Device::new(), Device::new());
        let mut at_alice = Channel::open(&alice.identity, &bob.card_keys()).expect("opens");
        let mut at_bob = Channel::default();
        let hello = at_alice.encrypt(&alice.identity.device_id(), b"hello").expect("encrypts");
        at_bob.decrypt(&mut bob.identity, alice.identity.exchange_key(), &hello).expect("reads");

        let mut at_bob = Channel::unseal(&at_bob.seal(&KEY), &KEY).expect("reopens");
        let reply = at_bob.encrypt(&bob.identity.device_id(), b"still here").expect("encrypts");
        assert_eq!(at_alice.decrypt(&mut alice.identity, bob.identity.exchange_key(), &reply).expect("reads"), b"still here");
        assert!(Channel::unseal(&at_bob.seal(&KEY), &[4; 32]).is_err());
    }

    // Bob does not know Alice yet: her first message is opened with the key it carries, and
    // the caller must then check that key against the Contact Card inside.
    #[test]
    fn a_first_contact_is_accepted_from_its_pre_key_message() {
        let (alice, mut bob) = (Device::new(), Device::new());
        let mut at_alice = Channel::open(&alice.identity, &bob.card_keys()).expect("opens");
        let first = at_alice.encrypt(&alice.identity.device_id(), b"my card").expect("encrypts");

        let (mut at_bob, plaintext, sender_key) = accept_first_contact(&mut bob.identity, &first).expect("accepts");
        assert_eq!(plaintext, b"my card");
        assert_eq!(sender_key, alice.identity.exchange_key());

        let reply = at_bob.encrypt(&bob.identity.device_id(), b"welcome").expect("encrypts");
        let mut alice_identity = alice.identity;
        assert_eq!(at_alice.decrypt(&mut alice_identity, bob.identity.exchange_key(), &reply).expect("reads"), b"welcome");
    }

    #[test]
    fn only_pre_key_messages_can_start_a_first_contact() {
        let (mut alice, mut bob) = (Device::new(), Device::new());
        let mut at_alice = Channel::open(&alice.identity, &bob.card_keys()).expect("opens");
        let mut at_bob = Channel::default();
        let hello = at_alice.encrypt(&alice.identity.device_id(), b"hello").expect("encrypts");
        at_bob.decrypt(&mut bob.identity, alice.identity.exchange_key(), &hello).expect("reads");
        let ack = at_bob.encrypt(&bob.identity.device_id(), b"ok").expect("encrypts");
        at_alice.decrypt(&mut alice.identity, bob.identity.exchange_key(), &ack).expect("reads");
        let normal = at_alice.encrypt(&alice.identity.device_id(), b"normal").expect("encrypts");
        assert!(accept_first_contact(&mut bob.identity, &normal).is_err());
    }

    #[test]
    fn an_empty_channel_cannot_encrypt() {
        let alice = Device::new();
        assert!(Channel::default().encrypt(&alice.identity.device_id(), b"x").is_err());
    }
}
