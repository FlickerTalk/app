use ft_crypto::Channel;
use ft_identity::Identity;

use super::*;

#[test]
fn a_move_invite_travels_as_a_link() {
    let mut identity = Identity::generate();
    let invite = MoveInvite::new(card_of(&mut identity));
    let link = invite.to_link();
    assert!(link.starts_with(MOVE_LINK_PREFIX));
    let back = MoveInvite::from_link(&link).expect("parses");
    assert_eq!(back.secret, invite.secret);
    assert_eq!(back.card.device_id(), identity.device_id());
    assert_ne!(
        MoveInvite::new(card_of(&mut identity)).secret,
        invite.secret,
        "a fresh secret each time"
    );
    assert!(MoveInvite::from_link("https://flickertalk.com/move#garbage").is_err());
    assert!(
        MoveInvite::from_link(&card_of(&mut identity).to_link()).is_err(),
        "a contact link is not an invite"
    );
}

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
    ContactCard::create(
        identity,
        Some("Bob".to_owned()),
        RouteCapability::generate(),
        true,
        Some(envelope),
    )
}

#[test]
fn a_card_carries_its_envelope_key_or_none() {
    let mut bob = Identity::generate();
    let with = card_of(&mut bob);
    assert!(with.envelope_key().is_some());
    assert_eq!(
        ContactCard::decode(&with.encode())
            .expect("decodes")
            .envelope_key(),
        with.envelope_key()
    );
    let without = ContactCard::create(&mut bob, None, RouteCapability::generate(), true, None);
    assert_eq!(
        ContactCard::decode(&without.encode())
            .expect("decodes")
            .envelope_key(),
        None
    );
    assert!(
        with.to_link().len() < 600,
        "{} characters",
        with.to_link().len()
    );
}

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
        envelope_key: Some(
            ft_identity::EnvelopeKey::generate()
                .public_key()
                .as_bytes()
                .to_vec(),
        ),
        avatar_colour: 0x00ff_8800,
    });
    let signature = identity.sign(&body).to_bytes().to_vec();
    cbor(&Wire { body, signature })
}

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
    let sealed = channel
        .encrypt(&alice.device_id(), b"hi")
        .expect("encrypts");
    let read = Channel::default()
        .decrypt(&mut bob, alice.exchange_key(), &sealed)
        .expect("bob reads");
    assert_eq!(read, b"hi");
}

#[test]
fn cards_travel_as_links() {
    let card = card_of(&mut Identity::generate());
    let link = card.to_link();
    assert!(link.starts_with("https://flickertalk.com/add#"), "{link}");
    assert_eq!(ContactCard::from_link(&link).expect("parses"), card);
}

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
    let position = bytes
        .windows(3)
        .position(|w| w == b"Bob")
        .expect("the name is in the card");
    bytes[position] = b'R';
    assert!(ContactCard::decode(&bytes).is_err());
}

#[test]
fn a_card_signed_by_someone_else_is_rejected() {
    let mut bob = Identity::generate();
    let mallory = Identity::generate();
    let forged = ContactCard::create(&mut bob, None, RouteCapability::generate(), true, None)
        .signed_by(&mallory);
    assert!(ContactCard::decode(&forged.encode()).is_err());
}

#[test]
fn links_that_are_not_cards_are_rejected() {
    assert!(ContactCard::from_link("https://flickertalk.com/add#bm90IGEgY2FyZA").is_err());
    assert!(ContactCard::from_link("https://example.com/").is_err());
}

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

#[test]
fn a_card_link_is_found_inside_pasted_text() {
    let card = card_of(&mut Identity::generate());
    let link = card.to_link();
    for text in pasted_forms(&link) {
        assert_eq!(
            ContactCard::from_link(&text).expect(&text),
            card,
            "{text:?}"
        );
    }
    let app_link = link.replacen(LINK_PREFIX, APP_LINK_PREFIX, 1);
    assert_eq!(
        ContactCard::from_link(&format!("Open {app_link}, please.")).expect("app link"),
        card
    );
}

#[test]
fn pasted_text_without_a_valid_card_is_rejected() {
    let error = ContactCard::from_link("Add me on FlickerTalk!").expect_err("no link");
    assert_eq!(error.to_string(), "not a FlickerTalk contact link");
    assert!(
        ContactCard::from_link("Add me on FlickerTalk: https://flickertalk.com/add#garbage.")
            .is_err()
    );
    assert!(ContactCard::from_link(
        "Add me on FlickerTalk: https://flickertalk.com/add#bm90IGEgY2FyZA"
    )
    .is_err());
    let invite = MoveInvite::new(card_of(&mut Identity::generate())).to_link();
    assert!(
        ContactCard::from_link(&format!("Scan this: {invite}")).is_err(),
        "a move invite is not a card"
    );
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
    assert!(
        MoveInvite::from_link(&format!("Add me on FlickerTalk: {card}")).is_err(),
        "a card is not an invite"
    );
}

#[test]
fn an_opened_contact_link_goes_to_add_contact() {
    let link = card_of(&mut Identity::generate()).to_link();
    assert_eq!(
        opened_link(&link),
        Some(OpenedLink {
            kind: LinkKind::Add,
            link: link.clone(),
            valid: true
        })
    );
}

#[test]
fn an_opened_move_link_goes_to_the_move_screen() {
    let link = MoveInvite::new(card_of(&mut Identity::generate())).to_link();
    assert_eq!(
        opened_link(&link),
        Some(OpenedLink {
            kind: LinkKind::Move,
            link: link.clone(),
            valid: true
        })
    );
}

#[test]
fn a_broken_opened_link_keeps_its_kind_and_says_it_is_not_valid() {
    let link = card_of(&mut Identity::generate()).to_link();
    let cut = &link[..link.len() - 20];
    assert_eq!(
        opened_link(cut),
        Some(OpenedLink {
            kind: LinkKind::Add,
            link: cut.to_owned(),
            valid: false
        })
    );
    assert_eq!(
        opened_link("https://flickertalk.com/add#"),
        Some(OpenedLink {
            kind: LinkKind::Add,
            link: "https://flickertalk.com/add#".to_owned(),
            valid: false
        })
    );
    let garbage = "https://flickertalk.com/move#garbage";
    assert_eq!(
        opened_link(garbage),
        Some(OpenedLink {
            kind: LinkKind::Move,
            link: garbage.to_owned(),
            valid: false
        })
    );
    let card_as_move = link.replacen(LINK_PREFIX, MOVE_LINK_PREFIX, 1);
    assert_eq!(
        opened_link(&card_as_move).map(|opened| (opened.kind, opened.valid)),
        Some((LinkKind::Move, false))
    );
    let invite = MoveInvite::new(card_of(&mut Identity::generate())).to_link();
    let invite_as_add = invite.replacen(MOVE_LINK_PREFIX, LINK_PREFIX, 1);
    assert_eq!(
        opened_link(&invite_as_add).map(|opened| (opened.kind, opened.valid)),
        Some((LinkKind::Add, false))
    );
    assert_eq!(
        opened_link(&format!("{link}!x")).map(|opened| opened.valid),
        Some(false)
    );
}

#[test]
fn other_addresses_open_nothing() {
    let link = card_of(&mut Identity::generate()).to_link();
    let payload = link.strip_prefix(LINK_PREFIX).expect("a contact link");
    for url in [
        "https://flickertalk.com/add".to_owned(),
        "https://flickertalk.com/add/".to_owned(),
        "https://flickertalk.com/move".to_owned(),
        "https://flickertalk.com/".to_owned(),
        "https://flickertalk.com/games/chess".to_owned(),
        format!("https://www.flickertalk.com/add#{payload}"),
        format!("https://example.com/add#{payload}"),
        format!("https://flickertalk.com.example.com/add#{payload}"),
        format!("http://flickertalk.com/add#{payload}"),
        format!("https://FlickerTalk.com/add#{payload}"),
        format!("https://flickertalk.com/add?x=1#{payload}"),
        format!("https://flickertalk.com/added#{payload}"),
        format!("flickertalk://add#{payload}"),
        String::new(),
    ] {
        assert_eq!(opened_link(&url), None, "{url}");
    }
}

#[test]
fn an_opened_link_is_never_searched_for_inside_text() {
    let link = card_of(&mut Identity::generate()).to_link();
    for text in pasted_forms(&link) {
        match opened_link(&text) {
            Some(opened) => assert!(text.starts_with(LINK_PREFIX) && !opened.valid, "{text:?}"),
            None => assert!(!text.starts_with(LINK_PREFIX), "{text:?}"),
        }
    }
}

#[test]
fn both_sides_see_the_same_fingerprint() {
    let (alice, bob) = (Identity::generate(), Identity::generate());
    let at_alice = fingerprint(&alice.signing_key(), &bob.signing_key());
    assert_eq!(
        at_alice,
        fingerprint(&bob.signing_key(), &alice.signing_key())
    );
    assert_eq!(at_alice.split(' ').count(), 12);
    assert!(at_alice
        .split(' ')
        .all(|group| group.len() == 4 && group.chars().all(|c| c.is_ascii_hexdigit())));
}

#[test]
fn another_pair_has_another_fingerprint() {
    let (alice, bob, mallory) = (
        Identity::generate(),
        Identity::generate(),
        Identity::generate(),
    );
    assert_ne!(
        fingerprint(&alice.signing_key(), &bob.signing_key()),
        fingerprint(&alice.signing_key(), &mallory.signing_key())
    );
}

#[test]
fn route_capabilities_are_random_and_hashed_for_the_router() {
    let capability = RouteCapability::generate();
    assert_ne!(capability, RouteCapability::generate());
    assert_eq!(
        capability.hash(),
        *blake3::hash(capability.as_bytes()).as_bytes()
    );
    assert_eq!(
        RouteCapability::from_base64(&capability.to_base64()).expect("parses"),
        capability
    );
}
