use serde::{Deserialize, Serialize};
use tauri::Runtime;

use crate::{Platform, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TranslateStatus {
    Installed,
    Downloadable,
    Unsupported,
}

#[derive(Serialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct TranslatePair<'a> {
    from: &'a str,
    to: &'a str,
}

#[derive(Serialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct TranslateSource<'a> {
    text: &'a str,
}

#[derive(Serialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct TranslateRequest<'a> {
    text: &'a str,
    from: &'a str,
    to: &'a str,
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct TranslateLanguages {
    languages: Vec<String>,
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct Detected {
    found: bool,
    #[serde(default)]
    tag: Option<String>,
}

#[cfg_attr(not(mobile), allow(dead_code))]
impl Detected {
    fn tag(self) -> Option<String> {
        self.tag.filter(|tag| self.found && !tag.is_empty())
    }
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct PairState {
    status: TranslateStatus,
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct Prepared {
    ready: bool,
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct Removed {
    removed: bool,
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct Translated {
    found: bool,
    #[serde(default)]
    text: Option<String>,
}

#[cfg_attr(not(mobile), allow(dead_code))]
impl Translated {
    fn text(self) -> Option<String> {
        self.text.filter(|_| self.found)
    }
}

impl<R: Runtime> Platform<R> {
    /// Like every translate_* call this blocks until the phone answers, so not on the main thread.
    pub fn translate_languages(&self) -> Result<Vec<String>> {
        #[cfg(mobile)]
        {
            Ok(self
                .handle
                .run_mobile_plugin::<TranslateLanguages>("translateLanguages", ())?
                .languages)
        }
        #[cfg(not(mobile))]
        {
            Ok(vec![])
        }
    }

    pub fn translate_detect(&self, text: &str) -> Result<Option<String>> {
        #[cfg(mobile)]
        {
            Ok(self
                .handle
                .run_mobile_plugin::<Detected>("translateDetect", TranslateSource { text })?
                .tag())
        }
        #[cfg(not(mobile))]
        {
            let _ = text;
            Ok(None)
        }
    }

    pub fn translate_status(&self, from: &str, to: &str) -> Result<TranslateStatus> {
        #[cfg(mobile)]
        {
            Ok(self
                .handle
                .run_mobile_plugin::<PairState>("translateStatus", TranslatePair { from, to })?
                .status)
        }
        #[cfg(not(mobile))]
        {
            let _ = (from, to);
            Ok(TranslateStatus::Unsupported)
        }
    }

    /// Downloads packs from Google or Apple. Only call it once the user agreed.
    pub fn translate_prepare(&self, from: &str, to: &str) -> Result<bool> {
        #[cfg(mobile)]
        {
            Ok(self
                .handle
                .run_mobile_plugin::<Prepared>("translatePrepare", TranslatePair { from, to })?
                .ready)
        }
        #[cfg(not(mobile))]
        {
            let _ = (from, to);
            Ok(false)
        }
    }

    /// English is the pivot and never removed, so `remove(tag, "en")` frees one language.
    pub fn translate_remove(&self, from: &str, to: &str) -> Result<bool> {
        #[cfg(mobile)]
        {
            Ok(self
                .handle
                .run_mobile_plugin::<Removed>("translateRemove", TranslatePair { from, to })?
                .removed)
        }
        #[cfg(not(mobile))]
        {
            let _ = (from, to);
            Ok(false)
        }
    }

    pub fn translate_text(&self, text: &str, from: &str, to: &str) -> Result<Option<String>> {
        #[cfg(mobile)]
        {
            Ok(self
                .handle
                .run_mobile_plugin::<Translated>(
                    "translateText",
                    TranslateRequest { text, from, to },
                )?
                .text())
        }
        #[cfg(not(mobile))]
        {
            let _ = (text, from, to);
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_translation_travels_as_swift_and_kotlin_read_and_resolve_it() {
        assert_eq!(
            serde_json::to_value(TranslatePair {
                from: "es",
                to: "en"
            })
            .unwrap(),
            serde_json::json!({ "from": "es", "to": "en" })
        );
        assert_eq!(
            serde_json::to_value(TranslateSource { text: "hola" }).unwrap(),
            serde_json::json!({ "text": "hola" })
        );
        assert_eq!(
            serde_json::to_value(TranslateRequest {
                text: "hola",
                from: "es",
                to: "en"
            })
            .unwrap(),
            serde_json::json!({ "text": "hola", "from": "es", "to": "en" })
        );
        let listed: TranslateLanguages =
            serde_json::from_value(serde_json::json!({ "languages": ["en", "es", "zh-TW"] }))
                .unwrap();
        assert_eq!(listed.languages, ["en", "es", "zh-TW"]);
        for (word, status) in [
            ("installed", TranslateStatus::Installed),
            ("downloadable", TranslateStatus::Downloadable),
            ("unsupported", TranslateStatus::Unsupported),
        ] {
            let pair: PairState =
                serde_json::from_value(serde_json::json!({ "status": word })).unwrap();
            assert_eq!(pair.status, status);
        }
        assert!(
            serde_json::from_value::<PairState>(serde_json::json!({ "status": "supported" }))
                .is_err()
        );
        let ready: Prepared = serde_json::from_value(serde_json::json!({ "ready": true })).unwrap();
        assert!(ready.ready);
        let removed: Removed =
            serde_json::from_value(serde_json::json!({ "removed": false })).unwrap();
        assert!(!removed.removed);
    }

    #[test]
    fn a_detected_language_and_a_translation_count_only_when_found() {
        let detected = |json| serde_json::from_value::<Detected>(json).unwrap().tag();
        assert_eq!(
            detected(serde_json::json!({ "found": true, "tag": "es" })),
            Some("es".to_owned())
        );
        assert_eq!(detected(serde_json::json!({ "found": false })), None);
        assert_eq!(
            detected(serde_json::json!({ "found": false, "tag": "es" })),
            None
        );
        assert_eq!(detected(serde_json::json!({ "found": true })), None);
        assert_eq!(
            detected(serde_json::json!({ "found": true, "tag": "" })),
            None
        );
        let translated = |json| serde_json::from_value::<Translated>(json).unwrap().text();
        assert_eq!(
            translated(serde_json::json!({ "found": true, "text": "hello" })),
            Some("hello".to_owned())
        );
        assert_eq!(translated(serde_json::json!({ "found": false })), None);
        assert_eq!(
            translated(serde_json::json!({ "found": true, "text": null })),
            None
        );
    }

    #[cfg(not(mobile))]
    #[test]
    fn a_desktop_cannot_translate() {
        let platform = Platform::<tauri::Wry> {
            _runtime: std::marker::PhantomData,
        };
        let languages: Result<Vec<String>> = platform.translate_languages();
        assert_eq!(languages.ok(), Some(vec![]));
        let detected: Result<Option<String>> = platform.translate_detect("hola");
        assert_eq!(detected.ok(), Some(None));
        let status: Result<TranslateStatus> = platform.translate_status("es", "en");
        assert_eq!(status.ok(), Some(TranslateStatus::Unsupported));
        let prepared: Result<bool> = platform.translate_prepare("es", "en");
        assert_eq!(prepared.ok(), Some(false));
        let removed: Result<bool> = platform.translate_remove("es", "en");
        assert_eq!(removed.ok(), Some(false));
        let translated: Result<Option<String>> = platform.translate_text("hola", "es", "en");
        assert_eq!(translated.ok(), Some(None));
    }
}
