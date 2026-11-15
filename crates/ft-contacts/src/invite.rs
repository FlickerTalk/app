use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use phonenumber::{country, Mode};

const UNRESERVED: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

const LONGEST_NUMBER: usize = 15;

pub fn whatsapp_number(number: &str, country: &str) -> Option<String> {
    let number = number.trim();
    let parsed = match number
        .strip_prefix('+')
        .or_else(|| number.strip_prefix("00"))
    {
        Some(international) => phonenumber::parse(None, format!("+{}", digits(international))),
        None => {
            let country: country::Id = country.trim().to_ascii_uppercase().parse().ok()?;
            phonenumber::parse(Some(country), number)
        }
    }
    .ok()?;
    phonenumber::is_valid(&parsed).then(|| digits(&parsed.format().mode(Mode::E164).to_string()))
}

pub fn whatsapp_link(number: &str, text: &str) -> Option<String> {
    let digits_only = !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit());
    (digits_only && number.len() <= LONGEST_NUMBER).then(|| {
        format!(
            "https://wa.me/{number}?text={}",
            utf8_percent_encode(text, UNRESERVED)
        )
    })
}

pub fn greeting_name(name: &str) -> String {
    name.split_whitespace()
        .next()
        .filter(|word| word.chars().any(char::is_alphabetic))
        .unwrap_or_default()
        .to_owned()
}

fn digits(text: &str) -> String {
    text.chars().filter(char::is_ascii_digit).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_international_number_is_kept_whatever_the_phone_country() {
        assert_eq!(
            whatsapp_number("+34 612 34 56 78", "US").as_deref(),
            Some("34612345678")
        );
        assert_eq!(
            whatsapp_number("0034612345678", "GB").as_deref(),
            Some("34612345678")
        );
        assert_eq!(
            whatsapp_number("00 44 7400 123456", "").as_deref(),
            Some("447400123456")
        );
    }

    #[test]
    fn a_national_number_gets_the_calling_code_of_the_phone_country() {
        assert_eq!(
            whatsapp_number("612 345 678", "ES").as_deref(),
            Some("34612345678")
        );
        assert_eq!(
            whatsapp_number("612 345 678", "es").as_deref(),
            Some("34612345678")
        );
        assert_eq!(
            whatsapp_number("(415) 555-2671", "US").as_deref(),
            Some("14155552671")
        );
    }

    #[test]
    fn the_trunk_zero_goes_where_the_country_drops_it_and_stays_where_it_dials_it() {
        assert_eq!(
            whatsapp_number("07400 123456", "GB").as_deref(),
            Some("447400123456")
        );
        assert_eq!(
            whatsapp_number("06 1234 5678", "IT").as_deref(),
            Some("390612345678")
        );
        assert_eq!(
            whatsapp_number("312 345 6789", "IT").as_deref(),
            Some("393123456789")
        );
    }

    #[test]
    fn what_is_not_a_valid_number_gives_no_number() {
        assert_eq!(whatsapp_number("call me", "ES"), None);
        assert_eq!(whatsapp_number("", "ES"), None);
        assert_eq!(whatsapp_number("   ", "ES"), None);
        assert_eq!(whatsapp_number("12", "ES"), None);
        assert_eq!(whatsapp_number("+34 1", "ES"), None);
        assert_eq!(whatsapp_number("+", "ES"), None);
    }

    #[test]
    fn a_national_number_without_a_known_country_gives_no_number() {
        assert_eq!(whatsapp_number("612 345 678", ""), None);
        assert_eq!(whatsapp_number("612 345 678", "XX"), None);
        assert_eq!(whatsapp_number("612 345 678", "001"), None);
    }

    #[test]
    fn the_chat_link_carries_the_number_and_the_whole_text_encoded() {
        assert_eq!(
            whatsapp_link("34612345678", "Hi Ana, I'm on FlickerTalk: https://flickertalk.com/add#a-b_c").as_deref(),
            Some("https://wa.me/34612345678?text=Hi%20Ana%2C%20I%27m%20on%20FlickerTalk%3A%20https%3A%2F%2Fflickertalk.com%2Fadd%23a-b_c")
        );
        assert_eq!(
            whatsapp_link("34612345678", "¿Qué tal? & más").as_deref(),
            Some("https://wa.me/34612345678?text=%C2%BFQu%C3%A9%20tal%3F%20%26%20m%C3%A1s")
        );
    }

    #[test]
    fn a_chat_link_needs_a_number_of_digits_only() {
        assert_eq!(whatsapp_link("", "Hi"), None);
        assert_eq!(whatsapp_link("+34612345678", "Hi"), None);
        assert_eq!(whatsapp_link("34612345678/../x", "Hi"), None);
        assert_eq!(whatsapp_link("1234567890123456", "Hi"), None);
    }

    #[test]
    fn the_greeting_uses_the_first_name() {
        assert_eq!(greeting_name("María García López"), "María");
        assert_eq!(greeting_name("  Ana  "), "Ana");
        assert_eq!(greeting_name("王小明"), "王小明");
    }

    #[test]
    fn a_contact_saved_without_a_name_gets_no_greeting_name() {
        assert_eq!(greeting_name(""), "");
        assert_eq!(greeting_name("   "), "");
        assert_eq!(greeting_name("+34 612 34 56 78"), "");
        assert_eq!(greeting_name("612345678"), "");
    }
}
