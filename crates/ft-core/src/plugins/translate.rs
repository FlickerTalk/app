use anyhow::{ensure, Result};
use async_trait::async_trait;
use serde::Serialize;

use crate::Core;

// per pair because that's all iOS reports
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PairStatus {
    Installed,
    Downloadable,
    Unsupported,
}

// Text never leaves the phone. Language packs download only when the user asks.
#[async_trait]
pub trait Translator: Send + Sync {
    async fn languages(&self) -> Result<Vec<String>>;
    async fn detect(&self, text: &str) -> Result<Option<String>>;
    async fn status(&self, from: &str, to: &str) -> Result<PairStatus>;
    async fn prepare(&self, from: &str, to: &str) -> Result<bool>;
    async fn remove(&self, from: &str, to: &str) -> Result<bool>;
    async fn translate(&self, text: &str, from: &str, to: &str) -> Result<Option<String>>;
}

pub const TRANSLATE_MAX_CHARS: usize = 5_000;

const TRANSLATE_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

// packs are ~30 MB
const PREPARE_WAIT: std::time::Duration = std::time::Duration::from_secs(5 * 60);

const LANGUAGE_TAG_MAX: usize = 35;

// Anything else never reaches native code
pub(crate) fn is_language_tag(tag: &str) -> bool {
    let mut parts = tag.split('-');
    let language = parts.next().unwrap_or_default();
    tag.len() <= LANGUAGE_TAG_MAX
        && (2..=3).contains(&language.len())
        && language.chars().all(|c| c.is_ascii_alphabetic())
        && parts.all(|part| {
            (2..=8).contains(&part.len()) && part.chars().all(|c| c.is_ascii_alphanumeric())
        })
}

fn a_text_to_translate(text: &str) -> Result<()> {
    ensure!(!text.trim().is_empty(), "there is nothing to translate");
    ensure!(
        text.chars().count() <= TRANSLATE_MAX_CHARS,
        "a text to translate is {TRANSLATE_MAX_CHARS} characters at most"
    );
    Ok(())
}

async fn within<T>(
    wait: std::time::Duration,
    asked: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    tokio::time::timeout(wait, asked)
        .await
        .map_err(|_| anyhow::anyhow!("the phone's translator did not answer in time"))?
}

async fn languages_of(translator: &dyn Translator) -> Result<Vec<String>> {
    let mut languages: Vec<String> = Vec::new();
    for tag in within(TRANSLATE_WAIT, translator.languages()).await? {
        if is_language_tag(&tag) && !languages.contains(&tag) {
            languages.push(tag);
        }
    }
    Ok(languages)
}

// BCP-47 "und" = undetermined
async fn detected(translator: &dyn Translator, text: &str) -> Result<Option<String>> {
    a_text_to_translate(text)?;
    let guess = within(TRANSLATE_WAIT, translator.detect(text)).await?;
    Ok(guess.filter(|tag| is_language_tag(tag) && !tag.eq_ignore_ascii_case("und")))
}

async fn status_of(translator: &dyn Translator, from: &str, to: &str) -> Result<PairStatus> {
    if !is_language_tag(from) || !is_language_tag(to) {
        return Ok(PairStatus::Unsupported);
    }
    within(TRANSLATE_WAIT, translator.status(from, to)).await
}

async fn prepared(translator: &dyn Translator, from: &str, to: &str) -> Result<bool> {
    if !is_language_tag(from) || !is_language_tag(to) {
        return Ok(false);
    }
    within(PREPARE_WAIT, translator.prepare(from, to)).await
}

async fn removed(translator: &dyn Translator, from: &str, to: &str) -> Result<bool> {
    if !is_language_tag(from) || !is_language_tag(to) {
        return Ok(false);
    }
    within(TRANSLATE_WAIT, translator.remove(from, to)).await
}

async fn translated(
    translator: &dyn Translator,
    text: &str,
    from: &str,
    to: &str,
) -> Result<Option<String>> {
    a_text_to_translate(text)?;
    if !is_language_tag(from) || !is_language_tag(to) {
        return Ok(None);
    }
    within(TRANSLATE_WAIT, translator.translate(text, from, to)).await
}

impl Core {
    // Pass the text through, never keep or log it

    async fn may_translate(&self, id: &str) -> Result<()> {
        ensure!(
            self.granted_to(id).await?.translate,
            "{id} may not translate"
        );
        Ok(())
    }

    pub async fn plugin_translate_languages(
        &self,
        id: &str,
        translator: &dyn Translator,
    ) -> Result<Vec<String>> {
        self.may_translate(id).await?;
        languages_of(translator).await
    }

    pub async fn plugin_translate_detect(
        &self,
        id: &str,
        translator: &dyn Translator,
        text: &str,
    ) -> Result<Option<String>> {
        self.may_translate(id).await?;
        detected(translator, text).await
    }

    pub async fn plugin_translate_status(
        &self,
        id: &str,
        translator: &dyn Translator,
        from: &str,
        to: &str,
    ) -> Result<PairStatus> {
        self.may_translate(id).await?;
        status_of(translator, from, to).await
    }

    pub async fn plugin_translate_prepare(
        &self,
        id: &str,
        translator: &dyn Translator,
        from: &str,
        to: &str,
    ) -> Result<bool> {
        self.may_translate(id).await?;
        prepared(translator, from, to).await
    }

    pub async fn plugin_translate_remove(
        &self,
        id: &str,
        translator: &dyn Translator,
        from: &str,
        to: &str,
    ) -> Result<bool> {
        self.may_translate(id).await?;
        removed(translator, from, to).await
    }

    pub async fn plugin_translate_text(
        &self,
        id: &str,
        translator: &dyn Translator,
        text: &str,
        from: &str,
        to: &str,
    ) -> Result<Option<String>> {
        self.may_translate(id).await?;
        translated(translator, text, from, to).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Mute;

    #[async_trait]
    impl Translator for Mute {
        async fn languages(&self) -> Result<Vec<String>> {
            std::future::pending().await
        }
        async fn detect(&self, _text: &str) -> Result<Option<String>> {
            std::future::pending().await
        }
        async fn status(&self, _from: &str, _to: &str) -> Result<PairStatus> {
            std::future::pending().await
        }
        async fn prepare(&self, _from: &str, _to: &str) -> Result<bool> {
            std::future::pending().await
        }
        async fn remove(&self, _from: &str, _to: &str) -> Result<bool> {
            std::future::pending().await
        }
        async fn translate(&self, _text: &str, _from: &str, _to: &str) -> Result<Option<String>> {
            std::future::pending().await
        }
    }

    async fn gave_up<T: std::fmt::Debug>(
        asked: impl std::future::Future<Output = Result<T>>,
    ) -> std::time::Duration {
        let started = tokio::time::Instant::now();
        let answer = asked.await;
        assert!(
            answer.is_err(),
            "a translator that never answers is an error, not {answer:?}"
        );
        started.elapsed()
    }

    #[tokio::test(start_paused = true)]
    async fn a_translator_that_never_answers_is_an_error_after_a_while() {
        let half_a_minute = std::time::Duration::from_secs(30);
        for waited in [
            gave_up(translated(&Mute, "hola", "es", "en")).await,
            gave_up(detected(&Mute, "hola")).await,
            gave_up(languages_of(&Mute)).await,
            gave_up(status_of(&Mute, "es", "en")).await,
            gave_up(removed(&Mute, "es", "en")).await,
        ] {
            assert_eq!(waited, half_a_minute);
        }
        assert_eq!(
            gave_up(prepared(&Mute, "es", "en")).await,
            std::time::Duration::from_secs(5 * 60),
            "a download may take a while"
        );
    }

    #[test]
    fn a_language_tag_is_what_bcp_47_looks_like() {
        for tag in [
            "es",
            "pt",
            "fil",
            "zh-Hans",
            "es-419",
            "sr-Latn-RS",
            "EN-us",
            "xx",
        ] {
            assert!(is_language_tag(tag), "{tag} is a tag");
        }
        let long = format!("en-{}", ["abcdefgh"; 5].join("-"));
        for tag in [
            "",
            "e",
            "espa",
            "español",
            "es_ES",
            "es-",
            "-es",
            "es--419",
            "es-x",
            "es-123456789",
            "../es",
            "es en",
            "e1",
            long.as_str(),
        ] {
            assert!(!is_language_tag(tag), "{tag:?} is not a tag");
        }
    }
}
