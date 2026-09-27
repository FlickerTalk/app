//! Circles (2026-09-27): a small, closed group of contacts, with no server knowing it exists.
//!
//! A circle is its **card**: who is in it, who runs it (the admins), its name, and whether only
//! the admins write. The card is signed by an admin and travels end-to-end encrypted to every
//! member, inside the same Olm channels the members already have with each other. Each change is
//! a new revision; a phone replaces the card it holds only with one signed by someone who was an
//! admin of the card it holds (the chain), so a member cannot add themselves or throw anyone out.
//!
//! Every member's own signed Contact Card rides inside, so whoever is put in a circle can open
//! an encrypted channel with every other member without scanning anyone.
//!
//! No I/O here, and no new cryptography: the identity's Ed25519 signature and the cards of
//! ft-contacts.

use std::collections::HashSet;

use anyhow::{anyhow, bail, ensure, Context, Result};
use ft_contacts::ContactCard;
use ft_identity::{verify, DeviceId, Ed25519PublicKey, Identity, Signature};
use serde::{Deserialize, Serialize};

pub const CIRCLE_VERSION: u16 = 1;
/// A circle is small: every message is encrypted and sent once per member.
pub const MAX_MEMBERS: usize = 32;
/// The most a circle's name may run to, in characters.
pub const NAME_LIMIT: usize = 40;

/// One member of a circle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub device_id: String,
    /// What their own card calls them; the phone that shows them may use its own name instead.
    pub name: Option<String>,
    /// Their signed Contact Card, as they handed it out: keys, route capability, envelope key.
    #[serde(with = "serde_bytes")]
    pub card: Vec<u8>,
}

impl Member {
    /// A member from their Contact Card, named as the card names them.
    pub fn from_card(card: &ContactCard) -> Self {
        Self { device_id: card.device_id().to_string(), name: card.name().map(str::to_owned), card: card.encode() }
    }
}

/// The signed part of the card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Body {
    version: u16,
    /// Random when the circle is made; never changes.
    id: String,
    /// Grows with every change; the highest wins.
    revision: u64,
    name: String,
    /// Who may change the card. Always members.
    admins: Vec<String>,
    members: Vec<Member>,
    /// Only the admins write; everyone else reads.
    admins_only: bool,
    /// The creator's clock, milliseconds.
    created_at: u64,
}

#[derive(Serialize, Deserialize)]
struct Wire {
    #[serde(with = "serde_bytes")]
    body: Vec<u8>,
    /// The Ed25519 key of the admin who signed this revision.
    #[serde(with = "serde_bytes")]
    signer: Vec<u8>,
    #[serde(with = "serde_bytes")]
    signature: Vec<u8>,
}

/// What a circle's card says, as the admin who signed it last left it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CircleCard {
    body: Body,
    signer: Ed25519PublicKey,
    signature: Vec<u8>,
}

/// The changes an admin makes to a card before signing the next revision.
#[derive(Debug, Clone)]
pub struct Draft {
    pub name: String,
    pub admins: Vec<String>,
    pub members: Vec<Member>,
    pub admins_only: bool,
}

/// How a card that arrives stands next to the one held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// A later revision, signed by an admin of the held one: it replaces it.
    Newer,
    /// The very same card.
    Same,
    /// Older than the held one, or the loser of a tie: ignored.
    Older,
}

impl CircleCard {
    /// The first revision: `identity` is the only admin, and a member. `members` are the others.
    pub fn create(identity: &Identity, name: &str, members: Vec<Member>, created_at: u64) -> Result<Self> {
        let me = identity.device_id().to_string();
        let body = Body {
            version: CIRCLE_VERSION,
            id: random_id(),
            revision: 1,
            name: name.to_owned(),
            admins: vec![me],
            members,
            admins_only: false,
            created_at,
        };
        Self::sign(body, identity)
    }

    /// The next revision, with the draft's changes, signed by `identity`, who must be an admin
    /// of this card: that is what the other members will check.
    pub fn revise(&self, identity: &Identity, change: impl FnOnce(&mut Draft)) -> Result<Self> {
        let me = identity.device_id();
        ensure!(self.is_admin(me.as_str()), "only an admin of the circle can change it");
        let mut draft = self.draft();
        change(&mut draft);
        let body = Body {
            version: CIRCLE_VERSION,
            id: self.body.id.clone(),
            revision: self.body.revision + 1,
            name: draft.name,
            admins: draft.admins,
            members: draft.members,
            admins_only: draft.admins_only,
            created_at: self.body.created_at,
        };
        Self::sign(body, identity)
    }

    /// What the card says, ready to be changed.
    pub fn draft(&self) -> Draft {
        Draft {
            name: self.body.name.clone(),
            admins: self.body.admins.clone(),
            members: self.body.members.clone(),
            admins_only: self.body.admins_only,
        }
    }

    fn sign(body: Body, identity: &Identity) -> Result<Self> {
        check(&body)?;
        let signature = identity.sign(&cbor(&body)).to_bytes().to_vec();
        Ok(Self { body, signer: identity.signing_key(), signature })
    }

    pub fn encode(&self) -> Vec<u8> {
        cbor(&Wire { body: cbor(&self.body), signer: self.signer.as_bytes().to_vec(), signature: self.signature.clone() })
    }

    /// Only a card whose signature matches its signer, and that is well formed, is accepted.
    /// Whether the signer may sign it at all is `judge` (a known circle) or `stands_alone` (a
    /// new one).
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let wire: Wire = ciborium::from_reader(bytes).context("not a circle card")?;
        let signer = Ed25519PublicKey::from_slice(&key_bytes(&wire.signer)?).context("invalid signer key")?;
        let signature = Signature::from_slice(&wire.signature).map_err(|_| anyhow!("invalid signature"))?;
        verify(&signer, &wire.body, &signature).context("the circle card was changed or forged")?;
        let body: Body = ciborium::from_reader(wire.body.as_slice()).context("not a circle card")?;
        if body.version != CIRCLE_VERSION {
            bail!("unsupported circle card version {}", body.version);
        }
        check(&body)?;
        Ok(Self { body, signer, signature: wire.signature })
    }

    pub fn id(&self) -> &str {
        &self.body.id
    }

    pub fn revision(&self) -> u64 {
        self.body.revision
    }

    pub fn name(&self) -> &str {
        &self.body.name
    }

    pub fn admins(&self) -> &[String] {
        &self.body.admins
    }

    pub fn members(&self) -> &[Member] {
        &self.body.members
    }

    pub fn admins_only(&self) -> bool {
        self.body.admins_only
    }

    pub fn created_at(&self) -> u64 {
        self.body.created_at
    }

    /// The admin who signed this revision.
    pub fn signer(&self) -> DeviceId {
        DeviceId::from_signing_key(&self.signer)
    }

    pub fn is_member(&self, device_id: &str) -> bool {
        self.body.members.iter().any(|member| member.device_id == device_id)
    }

    pub fn is_admin(&self, device_id: &str) -> bool {
        self.body.admins.iter().any(|admin| admin == device_id)
    }

    pub fn member(&self, device_id: &str) -> Option<&Member> {
        self.body.members.iter().find(|member| member.device_id == device_id)
    }

    /// Whether `device_id` may write in the circle.
    pub fn may_write(&self, device_id: &str) -> bool {
        self.is_member(device_id) && (!self.body.admins_only || self.is_admin(device_id))
    }

    /// A card of a circle this phone did not know: it is trusted when whoever signed it is an
    /// admin of what it says. From then on the chain applies.
    pub fn stands_alone(&self) -> bool {
        self.is_admin(self.signer().as_str())
    }

    /// Whether `next` may replace this card: the same circle, signed by an admin of **this**
    /// card, and a later revision. Two admins who change the same revision at once are told
    /// apart by their device id, the lower one winning, the same on every phone.
    pub fn judge(&self, next: &CircleCard) -> Result<Standing> {
        ensure!(next.body.id == self.body.id, "a card of another circle");
        let signer = next.signer();
        ensure!(self.is_admin(signer.as_str()), "signed by someone who is not an admin of the circle");
        if next.body.revision > self.body.revision {
            return Ok(Standing::Newer);
        }
        if next.body.revision < self.body.revision {
            return Ok(Standing::Older);
        }
        if next.signature == self.signature {
            return Ok(Standing::Same);
        }
        let held = self.signer();
        Ok(if signer.as_str() < held.as_str() { Standing::Newer } else { Standing::Older })
    }
}

/// What every card must satisfy, made here or received.
fn check(body: &Body) -> Result<()> {
    let name = body.name.trim();
    ensure!(!name.is_empty(), "a circle needs a name");
    ensure!(name.chars().count() <= NAME_LIMIT, "a circle's name is at most {NAME_LIMIT} characters");
    ensure!(!body.members.is_empty(), "a circle has at least one member");
    ensure!(body.members.len() <= MAX_MEMBERS, "a circle has at most {MAX_MEMBERS} members");
    ensure!(body.revision >= 1, "revisions start at 1");
    let mut seen = HashSet::new();
    for member in &body.members {
        ensure!(seen.insert(member.device_id.as_str()), "a member is listed twice");
        let card = ContactCard::decode(&member.card).context("a member's contact card is not valid")?;
        ensure!(card.device_id().as_str() == member.device_id, "a member's contact card belongs to someone else");
    }
    ensure!(!body.admins.is_empty(), "a circle has at least one admin");
    for admin in &body.admins {
        ensure!(seen.contains(admin.as_str()), "an admin must be a member");
    }
    Ok(())
}

fn random_id() -> String {
    let bytes: [u8; 16] = rand::random();
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
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
    use super::*;
    use ft_contacts::RouteCapability;

    struct Person {
        identity: Identity,
        member: Member,
    }

    fn person(name: &str) -> Person {
        let mut identity = Identity::generate();
        let card = ContactCard::create(&mut identity, Some(name.to_owned()), RouteCapability::generate(), true, None);
        let member = Member::from_card(&card);
        Person { identity, member }
    }

    fn id(person: &Person) -> String {
        person.identity.device_id().to_string()
    }

    fn circle(creator: &Person, others: &[&Person]) -> CircleCard {
        let mut members = vec![creator.member.clone()];
        members.extend(others.iter().map(|other| other.member.clone()));
        CircleCard::create(&creator.identity, "Friends", members, 1_700_000_000_000).expect("creates")
    }

    #[test]
    fn a_card_survives_the_wire_and_says_who_is_in() {
        let (alice, bob) = (person("Alice"), person("Bob"));
        let card = circle(&alice, &[&bob]);
        let back = CircleCard::decode(&card.encode()).expect("decodes");
        assert_eq!(back, card);
        assert_eq!(back.name(), "Friends");
        assert_eq!(back.revision(), 1);
        assert!(back.is_member(&id(&bob)));
        assert!(back.is_admin(&id(&alice)));
        assert!(!back.is_admin(&id(&bob)));
        assert_eq!(back.signer().to_string(), id(&alice));
        assert!(back.stands_alone(), "signed by its own admin");
        assert_eq!(back.member(&id(&bob)).and_then(|member| member.name.as_deref()), Some("Bob"));
        assert_eq!(back.id().len(), 32);
    }

    #[test]
    fn a_changed_or_forged_card_is_rejected() {
        let (alice, bob) = (person("Alice"), person("Bob"));
        let card = circle(&alice, &[&bob]);
        let mut bytes = card.encode();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        assert!(CircleCard::decode(&bytes).is_err());

        // Bob signs Alice's body as if it were his: the signer does not match the signature.
        let forged = CircleCard { body: card.body.clone(), signer: bob.identity.signing_key(), signature: card.signature.clone() };
        assert!(CircleCard::decode(&forged.encode()).is_err());
        assert!(CircleCard::decode(b"\xff\x00").is_err());
    }

    #[test]
    fn only_an_admin_revises_and_the_revision_grows() {
        let (alice, bob, carol) = (person("Alice"), person("Bob"), person("Carol"));
        let first = circle(&alice, &[&bob]);
        let second = first
            .revise(&alice.identity, |draft| {
                draft.members.push(carol.member.clone());
                draft.name = "Close friends".to_owned();
            })
            .expect("alice revises");
        assert_eq!(second.revision(), 2);
        assert_eq!(second.id(), first.id());
        assert!(second.is_member(&id(&carol)));
        assert_eq!(second.name(), "Close friends");
        assert_eq!(first.judge(&second).expect("judged"), Standing::Newer);

        assert!(first.revise(&bob.identity, |_| {}).is_err(), "bob is no admin");
    }

    // The chain: a card is replaced only by one signed by an admin of the card held. A member
    // who signs a card naming themselves admin gets nowhere.
    #[test]
    fn a_member_cannot_make_themselves_admin() {
        let (alice, bob) = (person("Alice"), person("Bob"));
        let held = circle(&alice, &[&bob]);
        let mut body = held.body.clone();
        body.revision = 2;
        body.admins.push(id(&bob));
        let forged = CircleCard::sign(body, &bob.identity).expect("signs");
        assert!(CircleCard::decode(&forged.encode()).is_ok(), "well formed on its own");
        assert!(held.judge(&forged).is_err(), "but not signed by an admin of the held card");
        assert!(!forged.stands_alone() || forged.is_admin(&id(&bob)));
    }

    #[test]
    fn a_new_circle_is_trusted_only_from_its_own_admin() {
        let (alice, bob) = (person("Alice"), person("Bob"));
        let card = circle(&alice, &[&bob]);
        let mut body = card.body.clone();
        body.admins = vec![id(&alice)];
        let signed_by_bob = CircleCard::sign(body, &bob.identity).expect("signs");
        assert!(!signed_by_bob.stands_alone());
    }

    #[test]
    fn older_and_same_cards_are_told_from_newer_ones() {
        let (alice, bob) = (person("Alice"), person("Bob"));
        let first = circle(&alice, &[&bob]);
        let second = first.revise(&alice.identity, |draft| draft.admins_only = true).expect("revises");
        assert_eq!(second.judge(&first).expect("judged"), Standing::Older);
        assert_eq!(second.judge(&second).expect("judged"), Standing::Same);
        assert!(second.admins_only());
        assert!(second.may_write(&id(&alice)));
        assert!(!second.may_write(&id(&bob)));
        assert!(first.may_write(&id(&bob)));
    }

    // Two admins change the same revision at once: every phone picks the same winner.
    #[test]
    fn a_tie_is_settled_by_the_lower_device_id() {
        let (alice, bob, carol, dave) = (person("Alice"), person("Bob"), person("Carol"), person("Dave"));
        let held = circle(&alice, &[&bob]).revise(&alice.identity, |draft| draft.admins.push(id(&bob))).expect("bob is admin too");
        let by_alice = held.revise(&alice.identity, |draft| draft.members.push(carol.member.clone())).expect("alice");
        let by_bob = held.revise(&bob.identity, |draft| draft.members.push(dave.member.clone())).expect("bob");
        assert_eq!(by_alice.revision(), by_bob.revision());
        let lower_is_alice = id(&alice) < id(&bob);
        assert_eq!(held.judge(&by_alice).unwrap(), Standing::Newer);
        assert_eq!(held.judge(&by_bob).unwrap(), Standing::Newer);
        // Whoever got one first: the other one wins only if its signer is the lower id.
        assert_eq!(by_alice.judge(&by_bob).unwrap(), if lower_is_alice { Standing::Older } else { Standing::Newer });
        assert_eq!(by_bob.judge(&by_alice).unwrap(), if lower_is_alice { Standing::Newer } else { Standing::Older });
    }

    #[test]
    fn a_card_of_another_circle_is_not_a_revision() {
        let (alice, bob) = (person("Alice"), person("Bob"));
        let one = circle(&alice, &[&bob]);
        let other = circle(&alice, &[&bob]);
        assert!(one.judge(&other).is_err());
    }

    #[test]
    fn a_card_must_be_well_formed() {
        let (alice, bob) = (person("Alice"), person("Bob"));
        let alone = || vec![alice.member.clone()];
        assert!(CircleCard::create(&alice.identity, "  ", alone(), 1).is_err(), "a name");
        assert!(CircleCard::create(&alice.identity, &"x".repeat(NAME_LIMIT + 1), alone(), 1).is_err(), "a short name");
        assert!(CircleCard::create(&alice.identity, "ok", vec![], 1).is_err(), "at least the creator");
        assert!(CircleCard::create(&alice.identity, "ok", vec![bob.member.clone()], 1).is_err(), "the admin must be a member");
        assert!(CircleCard::create(&alice.identity, "ok", vec![alice.member.clone(), alice.member.clone()], 1).is_err(), "no twice");
        let mut wrong = bob.member.clone();
        wrong.device_id = id(&alice);
        assert!(CircleCard::create(&alice.identity, "ok", vec![alice.member.clone(), wrong], 1).is_err(), "cards match ids");
        let mut crowd = alone();
        for n in 0..MAX_MEMBERS {
            crowd.push(person(&format!("p{n}")).member);
        }
        assert!(CircleCard::create(&alice.identity, "ok", crowd, 1).is_err(), "too many");
    }

    // An admin who leaves the admins in their own revision is still the one who signed it: the
    // chain checks the card held, not the new one.
    #[test]
    fn an_admin_may_hand_over_and_step_down() {
        let (alice, bob) = (person("Alice"), person("Bob"));
        let held = circle(&alice, &[&bob]);
        let handed = held.revise(&alice.identity, |draft| draft.admins = vec![id(&bob)]).expect("hands over");
        assert_eq!(held.judge(&handed).unwrap(), Standing::Newer);
        assert!(!handed.is_admin(&id(&alice)));
        assert!(handed.revise(&alice.identity, |_| {}).is_err(), "alice no longer");
        assert!(handed.revise(&bob.identity, |_| {}).is_ok());
    }
}
