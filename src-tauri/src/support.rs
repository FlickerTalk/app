//! The contact every install carries (2026-10-28): FlickerTalk's support peer
//! (`crates/ft-support`), always on, which answers texts, files, voice messages and calls. Whoever
//! has a single phone, an app reviewer for one, has someone to talk to from the first start.
//!
//! Its public Contact Card is `resources/support-card.txt`: one `https://flickertalk.com/add#…`
//! link and nothing else, built into the app on every platform (`include_str!`, like the seeds).
//! Empty, or a link that does not read, and the app carries no contact. To change it, write the
//! link the production support peer exports (`crates/ft-support`) into that file, alone; the tests
//! below check that it reads. The card has to stay valid for every app already out: the peer
//! must keep its identity and its link (no new link, no new phone).
//!
//! The contact is added once, when the user starts on a phone whose identity was just made
//! (`Core::add_bundled_contact`), and is then a contact like any other. Nothing goes to it until
//! the user writes to it (§100).

use ft_contacts::ContactCard;

/// The support peer's card as a link; empty when this build carries none.
pub const SUPPORT_CARD: &str = include_str!("../resources/support-card.txt");
/// What the contact is called on the phone: the app's name, the same in every language.
pub const SUPPORT_NAME: &str = "FlickerTalk";

/// The link of the card this build carries; empty when it carries none.
pub fn support_link() -> &'static str {
    SUPPORT_CARD.trim()
}

/// The support peer's id, when this build carries a card that reads.
pub fn support_id() -> Option<String> {
    support_id_of(support_link())
}

fn support_id_of(link: &str) -> Option<String> {
    ContactCard::from_link(link).ok().map(|card| card.device_id().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ft_contacts::{ContactCard, LINK_PREFIX};

    /// The card of a throwaway identity, made for these tests.
    const CARD: &str = "https://flickertalk.com/add#omRib2R5WPGoZ3ZlcnNpb24BZG5hbWVrRmxpY2tlclRhbGtrc2lnbmluZ19rZXlYIJx0llmfCd6MOP3e5KQn8nEDlBb8HTofjjBjK4K4R1v7bGV4Y2hhbmdlX2tleVggp14Lbmx060Ij7rbZyYsuIsMtHOQTQz0nG7ksSnC4CBFsZmFsbGJhY2tfa2V5WCBNnGa8Q029eaCHtub05tOvjfUpXzI_MaMUQyLde9Y5dnByb3V0ZV9jYXBhYmlsaXR5WCCNsdIDqnWwkDyFUh4To6bG0CqQSC3c34AAtWllOcxQr2dtYWlsYm949WxlbnZlbG9wZV9rZXn2aXNpZ25hdHVyZVhAt3b0dQoIRtVcfX2t3QoZ0qNI0WAwO8n2QnUmaFvPKBu_rN-XDUDVNtqpD3Boh_owbgpQ873uBAecFcGCaXo9CQ";
    const CARD_ID: &str = "ft_2sKh26wC2SsswH43HkJB7SiDEHWS3s7zWfuJ25qYuQJJ";

    #[test]
    fn a_card_that_reads_names_the_support_peer() {
        assert_eq!(support_id_of(CARD).as_deref(), Some(CARD_ID));
    }

    #[test]
    fn no_card_or_a_broken_one_names_nobody() {
        for link in ["", "not a link", &CARD[..CARD.len() - 6], "https://flickertalk.com/add#AAAA"] {
            assert_eq!(support_id_of(link), None, "{link:?}");
        }
    }

    // Whoever drops the production card in the file may leave a line break after it.
    #[test]
    fn the_link_is_the_file_without_its_blank_space() {
        assert_eq!(support_link(), SUPPORT_CARD.trim());
    }

    // The file holds the link alone, or nothing: a paste that is not a card fails here, not on
    // the phones.
    #[test]
    fn the_card_the_app_carries_is_empty_or_one_link_that_reads() {
        let link = support_link();
        if link.is_empty() {
            return;
        }
        assert!(link.starts_with(LINK_PREFIX), "the file holds a contact link and nothing else");
        assert!(!link.contains(char::is_whitespace), "one link, on one line");
        let card = ContactCard::from_link(link).expect("the support peer's card reads");
        assert_eq!(support_id().as_deref(), Some(card.device_id().as_str()));
    }
}
