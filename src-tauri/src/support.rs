use ft_contacts::ContactCard;

// Apps already shipped carry this card, so the support peer has to keep its identity and link.
pub const SUPPORT_CARD: &str = include_str!("../resources/support-card.txt");
pub const SUPPORT_NAME: &str = "FlickerTalk";

pub fn support_link() -> &'static str {
    SUPPORT_CARD.trim()
}

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

    #[test]
    fn the_link_is_the_file_without_its_blank_space() {
        assert_eq!(support_link(), SUPPORT_CARD.trim());
    }

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
