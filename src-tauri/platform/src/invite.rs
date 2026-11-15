use tauri::Runtime;

use crate::{Platform, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickedContact {
    pub name: String,
    pub number: String,
    pub country: String,
}

#[cfg(any(mobile, test))]
#[derive(serde::Deserialize)]
struct Picked {
    picked: bool,
    #[serde(default)]
    name: String,
    #[serde(default)]
    number: String,
    #[serde(default)]
    country: String,
}

#[cfg(any(mobile, test))]
impl Picked {
    fn contact(self) -> Option<PickedContact> {
        self.picked.then_some(PickedContact {
            name: self.name,
            number: self.number,
            country: self.country,
        })
    }
}

#[cfg(any(mobile, test))]
#[derive(serde::Serialize)]
struct WhatsAppChat<'a> {
    url: &'a str,
}

#[cfg(any(mobile, test))]
#[derive(serde::Deserialize)]
struct Opened {
    opened: bool,
}

impl<R: Runtime> Platform<R> {
    // blocks until the user picks someone or closes the picker, so never on the main thread
    pub fn pick_contact(&self) -> Result<Option<PickedContact>> {
        #[cfg(mobile)]
        {
            Ok(self
                .handle
                .run_mobile_plugin::<Picked>("pickContact", ())?
                .contact())
        }
        #[cfg(not(mobile))]
        {
            Err(crate::Error::Unsupported)
        }
    }

    pub fn open_whatsapp(&self, url: &str) -> Result<bool> {
        #[cfg(mobile)]
        {
            Ok(self
                .handle
                .run_mobile_plugin::<Opened>("openWhatsApp", WhatsAppChat { url })?
                .opened)
        }
        #[cfg(not(mobile))]
        {
            let _ = url;
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picked_contact_comes_back_as_swift_and_kotlin_resolve_it() {
        let picked: Picked = serde_json::from_value(serde_json::json!({
            "picked": true, "name": "Ana", "number": "612 345 678", "country": "ES"
        }))
        .unwrap();
        let ana = PickedContact {
            name: "Ana".to_owned(),
            number: "612 345 678".to_owned(),
            country: "ES".to_owned(),
        };
        assert_eq!(picked.contact(), Some(ana));
        let unnamed: Picked =
            serde_json::from_value(serde_json::json!({ "picked": true, "number": "612 345 678" }))
                .unwrap();
        assert_eq!(
            unnamed.contact().map(|contact| contact.name),
            Some(String::new())
        );
    }

    #[test]
    fn a_closed_picker_comes_back_as_nobody() {
        let closed: Picked =
            serde_json::from_value(serde_json::json!({ "picked": false })).unwrap();
        assert_eq!(closed.contact(), None);
    }

    #[test]
    fn the_whatsapp_chat_travels_as_swift_and_kotlin_read_it() {
        let url = "https://wa.me/34612345678?text=Hi";
        assert_eq!(
            serde_json::to_value(WhatsAppChat { url }).unwrap(),
            serde_json::json!({ "url": url })
        );
        let opened: Opened = serde_json::from_value(serde_json::json!({ "opened": true })).unwrap();
        assert!(opened.opened);
    }

    #[cfg(not(mobile))]
    #[test]
    fn a_desktop_has_no_contact_picker_and_no_whatsapp() {
        let platform = Platform::<tauri::Wry> {
            _runtime: std::marker::PhantomData,
        };
        assert!(platform.pick_contact().is_err());
        assert_eq!(
            platform.open_whatsapp("https://wa.me/34612345678").ok(),
            Some(false)
        );
    }
}
