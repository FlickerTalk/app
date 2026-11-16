use serde::{Deserialize, Serialize};
use tauri::Runtime;

use crate::{Platform, Result};

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct OpenedLinkWire {
    #[serde(default)]
    url: Option<String>,
}

#[cfg_attr(not(mobile), allow(dead_code))]
impl OpenedLinkWire {
    fn url(self) -> Option<String> {
        self.url.filter(|url| !url.is_empty())
    }
}

#[derive(Serialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct LinkEvents<'a> {
    channel: &'a tauri::ipc::Channel<serde_json::Value>,
}

#[cfg_attr(not(mobile), allow(dead_code))]
fn link_opened(body: tauri::ipc::InvokeResponseBody) -> bool {
    #[derive(Deserialize)]
    struct Wire {
        event: String,
    }
    body.deserialize::<Wire>()
        .is_ok_and(|wire| wire.event == "linkOpened")
}

impl<R: Runtime> Platform<R> {
    pub fn opened_link(&self) -> Result<Option<String>> {
        #[cfg(mobile)]
        {
            Ok(self
                .handle
                .run_mobile_plugin::<OpenedLinkWire>("openedLink", ())?
                .url())
        }
        #[cfg(not(mobile))]
        {
            Ok(None)
        }
    }

    pub fn listen_links(&self, handler: impl Fn() + Send + Sync + 'static) {
        #[cfg(mobile)]
        {
            let channel = tauri::ipc::Channel::new(move |body| {
                if link_opened(body) {
                    handler();
                }
                Ok(())
            });
            let _ = self.run("registerLinkEvents", LinkEvents { channel: &channel });
        }
        #[cfg(not(mobile))]
        let _ = handler;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_opened_link_comes_back_as_swift_and_kotlin_resolve_it() {
        let opened: OpenedLinkWire = serde_json::from_value(
            serde_json::json!({ "url": "https://flickertalk.com/add#card" }),
        )
        .unwrap();
        assert_eq!(
            opened.url(),
            Some("https://flickertalk.com/add#card".to_owned())
        );
        for none in [
            serde_json::json!({ "url": "" }),
            serde_json::json!({}),
            serde_json::json!({ "url": null }),
        ] {
            let opened: OpenedLinkWire = serde_json::from_value(none).unwrap();
            assert_eq!(opened.url(), None);
        }
    }

    #[test]
    fn a_link_opened_event_is_read_as_the_native_side_sends_it() {
        let read = |json: &str| link_opened(tauri::ipc::InvokeResponseBody::Json(json.into()));
        assert!(read(r#"{"event":"linkOpened"}"#));
        assert!(!read(r#"{"event":"answer"}"#));
        assert!(!read("not json"));
    }

    #[cfg(not(mobile))]
    #[test]
    fn a_desktop_has_no_opened_link() {
        let platform = Platform::<tauri::Wry> {
            _runtime: std::marker::PhantomData,
        };
        assert_eq!(platform.opened_link().ok(), Some(None));
    }
}
