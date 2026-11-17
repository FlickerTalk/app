use anyhow::{ensure, Context, Result};
use ft_plugins::{CatalogueEntry, Manifest, Permissions};
use vodozemac::Ed25519PublicKey;

use super::{CATALOGUE_HOME, CORE_VERSION};
use crate::web::Fetch;
use crate::Core;

const PACKAGE_LIMIT: u64 = 8 * 1024 * 1024;
const INDEX_LIMIT: u64 = 512 * 1024;

impl Core {
    // Nothing is read until the index signature checks out
    pub async fn catalogue(&self, fetch: &dyn Fetch, catalogue: &Ed25519PublicKey) -> Result<Vec<CatalogueEntry>> {
        // index.json is a cut-down copy for app 1.0.0
        let index = fetch.get(&format!("{CATALOGUE_HOME}/{}", ft_plugins::INDEX), INDEX_LIMIT).await?;
        let signature = fetch.get(&format!("{CATALOGUE_HOME}/{}.sig", ft_plugins::INDEX), 1024).await?;
        let index = String::from_utf8(index).context("the index is not text")?;
        let signature = String::from_utf8(signature).context("the signature is not text")?;
        Ok(ft_plugins::catalogue_entries(&index, signature.trim(), catalogue)?
            .into_iter()
            .filter(|entry| entry.runs_on(CORE_VERSION) && entry.kind != ft_plugins::Kind::Unknown)
            .collect())
    }

    // Never open a package that isn't byte for byte what the index listed
    pub async fn add_plugin(
        &self,
        entry: &CatalogueEntry,
        fetch: &dyn Fetch,
        catalogue: &Ed25519PublicKey,
    ) -> Result<Manifest> {
        ensure!(entry.url.starts_with(&format!("{CATALOGUE_HOME}/")), "that plugin is not in our catalogue");
        ensure!(entry.size <= PACKAGE_LIMIT, "that plugin is too big");
        if self.store.plugin(&entry.id).await?.is_none() {
            self.usable(entry.kind).await?;
        }
        let package = fetch.get(&entry.url, PACKAGE_LIMIT).await?;
        ft_plugins::download(entry, &package, catalogue)?;
        self.install_plugin(&package, catalogue, Permissions::default()).await
    }

    pub async fn update_plugins(
        &self,
        listed: &[CatalogueEntry],
        fetch: &dyn Fetch,
        catalogue: &Ed25519PublicKey,
        open: &std::collections::HashSet<String>,
    ) -> Vec<String> {
        let mut updated = Vec::new();
        for plugin in self.plugins().await.unwrap_or_default() {
            let here = &plugin.manifest;
            let Some(entry) = listed.iter().find(|entry| entry.id == here.id) else { continue };
            let higher = !ft_plugins::version_at_least(&here.version, &entry.version);
            if !higher
                || open.contains(&here.id)
                || !entry.runs_on(CORE_VERSION)
                || entry.kind != here.kind
                || entry.size > PACKAGE_LIMIT
            {
                continue;
            }
            if self.add_plugin(entry, fetch, catalogue).await.is_ok() {
                updated.push(here.id.clone());
            }
        }
        updated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn served_index(languages: impl Fn(usize) -> std::collections::BTreeMap<String, ft_plugins::Localized>, image: &str) -> String {
        const SERVED: [(&str, &str, usize); 16] = [
            ("game.chess", "Chess", 88), ("game.fourinarow", "Four in a Row", 96), ("game.tictactoe", "Tic-Tac-Toe", 94),
            ("board", "Board", 81), ("clean", "Clean", 123), ("drive", "My drive", 58), ("images", "Image", 91),
            ("list", "List", 84), ("markdown", "Markdown", 76), ("notes", "Notes", 56), ("pdf", "PDF", 70),
            ("poll", "Poll", 96), ("redact", "Cover", 78), ("sign", "Sign", 122), ("sketch", "Sketch", 53),
            ("split", "Split", 124),
        ];
        let entries: Vec<CatalogueEntry> = SERVED
            .iter()
            .map(|(id, name, summary)| CatalogueEntry {
                id: format!("com.flickertalk.{id}"),
                name: (*name).to_owned(),
                version: "1.0.1".to_owned(),
                min_core_version: "1.3.0".to_owned(),
                size: 123_456,
                hash: "ab".repeat(32),
                url: format!("{CATALOGUE_HOME}/com.flickertalk.{id}/1.0.1.ftplugin"),
                summary: "x".repeat(*summary),
                kind: if id.starts_with("game.") { ft_plugins::Kind::Game } else { ft_plugins::Kind::Tool },
                locales: languages(*summary),
                icon: "radio-button-on-outline".to_owned(),
                image: image.to_owned(),
            })
            .collect();
        serde_json::to_string_pretty(&serde_json::json!({ "plugins": entries })).unwrap()
    }

    const LANGUAGES: [(&str, &str); 20] = [
        ("es", "dibuja "), ("pt", "desenhe "), ("fr", "dessinez "), ("de", "zeichnen "), ("it", "disegna "),
        ("ro", "desenează "), ("ru", "рисуйте "), ("uk", "малюйте "), ("pl", "rysuj "), ("tr", "çizin "),
        ("ar", "ارسم "), ("hi", "चित्र बनाएं "), ("bn", "আঁকুন "), ("id", "gambar "), ("vi", "vẽ hình "),
        ("th", "วาดภาพ "), ("ja", "指で描く"), ("ko", "그리기 "), ("zh-CN", "用手指画"), ("zh-TW", "用手指畫"),
    ];

    #[test]
    fn a_fully_translated_index_fits_what_the_core_downloads() {
        let text = |word: &str, chars: usize| word.chars().cycle().take(chars).collect::<String>();
        let svg = |bytes: usize, filler: char| {
            let bare = r#"<svg viewBox="0 0 64 64"><desc></desc></svg>"#;
            format!(r#"<svg viewBox="0 0 64 64"><desc>{}</desc></svg>"#, filler.to_string().repeat(bytes - bare.len()))
        };
        let realistic = served_index(|summary| {
            LANGUAGES
                .iter()
                .map(|(code, word)| {
                    let dense = matches!(*code, "ja" | "ko" | "zh-CN" | "zh-TW");
                    let (name, summary) = if dense { (6, summary * 6 / 10) } else { (18, summary * 13 / 10) };
                    let said = ft_plugins::Localized { name: Some(text(word, name)), summary: Some(text(word, summary.min(200))), ..Default::default() };
                    ((*code).to_owned(), said)
                })
                .collect()
        }, &svg(1500, 'x'));
        let longest = served_index(|_| {
            LANGUAGES
                .iter()
                .map(|(code, _)| ((*code).to_owned(), ft_plugins::Localized { name: Some("न".repeat(64)), summary: Some("न".repeat(200)), ..Default::default() }))
                .collect()
        }, &svg(ft_plugins::icon::IMAGE_LIMIT, '"'));
        eprintln!("catalogue.json: {} bytes translated, {} bytes at the limits", realistic.len(), longest.len());
        assert!((realistic.len() as u64) < INDEX_LIMIT / 4, "{} bytes leaves little room to grow", realistic.len());
        assert!((longest.len() as u64) < INDEX_LIMIT, "{} bytes is more than the core downloads", longest.len());

        let catalogue = vodozemac::Ed25519SecretKey::new();
        let signature = catalogue.sign(realistic.as_bytes()).to_base64();
        let listed = ft_plugins::catalogue_entries(&realistic, &signature, &catalogue.public_key()).expect("signed");
        assert_eq!(listed.len(), 16);
        assert!(listed.iter().all(|entry| entry.locales.len() == 20));
    }

    #[test]
    fn this_core_is_offered_games() {
        assert!(ft_plugins::version_at_least(CORE_VERSION, ft_plugins::GAMES_SINCE), "{CORE_VERSION}");
    }
}
