use tauri::{AppHandle, State};
use tauri_plugin_ft_platform::PlatformExt;

use super::{failed, Client};

struct PlatformTranslator(AppHandle);

impl PlatformTranslator {
    async fn ask<T: Send + 'static>(
        &self,
        call: impl FnOnce(&AppHandle) -> tauri_plugin_ft_platform::Result<T> + Send + 'static,
    ) -> anyhow::Result<T> {
        let app = self.0.clone();
        Ok(tauri::async_runtime::spawn_blocking(move || call(&app)).await??)
    }
}

fn pair_status(status: tauri_plugin_ft_platform::TranslateStatus) -> ft_core::plugins::PairStatus {
    match status {
        tauri_plugin_ft_platform::TranslateStatus::Installed => ft_core::plugins::PairStatus::Installed,
        tauri_plugin_ft_platform::TranslateStatus::Downloadable => ft_core::plugins::PairStatus::Downloadable,
        tauri_plugin_ft_platform::TranslateStatus::Unsupported => ft_core::plugins::PairStatus::Unsupported,
    }
}

#[async_trait::async_trait]
impl ft_core::plugins::Translator for PlatformTranslator {
    async fn languages(&self) -> anyhow::Result<Vec<String>> {
        self.ask(|app| app.platform().translate_languages()).await
    }

    async fn detect(&self, text: &str) -> anyhow::Result<Option<String>> {
        let text = text.to_owned();
        self.ask(move |app| app.platform().translate_detect(&text)).await
    }

    async fn status(&self, from: &str, to: &str) -> anyhow::Result<ft_core::plugins::PairStatus> {
        let (from, to) = (from.to_owned(), to.to_owned());
        Ok(pair_status(self.ask(move |app| app.platform().translate_status(&from, &to)).await?))
    }

    async fn prepare(&self, from: &str, to: &str) -> anyhow::Result<bool> {
        let (from, to) = (from.to_owned(), to.to_owned());
        self.ask(move |app| app.platform().translate_prepare(&from, &to)).await
    }

    async fn remove(&self, from: &str, to: &str) -> anyhow::Result<bool> {
        let (from, to) = (from.to_owned(), to.to_owned());
        self.ask(move |app| app.platform().translate_remove(&from, &to)).await
    }

    async fn translate(&self, text: &str, from: &str, to: &str) -> anyhow::Result<Option<String>> {
        let (text, from, to) = (text.to_owned(), from.to_owned(), to.to_owned());
        self.ask(move |app| app.platform().translate_text(&text, &from, &to)).await
    }
}

#[tauri::command]
pub async fn core_translate_languages(plugin: String, app: AppHandle, client: State<'_, Client>) -> Result<Vec<String>, String> {
    let core = client.core().await?;
    core.plugin_translate_languages(&plugin, &PlatformTranslator(app)).await.map_err(failed)
}

#[tauri::command]
pub async fn core_translate_detect(plugin: String, text: String, app: AppHandle, client: State<'_, Client>) -> Result<Option<String>, String> {
    let core = client.core().await?;
    core.plugin_translate_detect(&plugin, &PlatformTranslator(app), &text).await.map_err(failed)
}

#[tauri::command]
pub async fn core_translate_status(
    plugin: String,
    from: String,
    to: String,
    app: AppHandle,
    client: State<'_, Client>,
) -> Result<ft_core::plugins::PairStatus, String> {
    let core = client.core().await?;
    core.plugin_translate_status(&plugin, &PlatformTranslator(app), &from, &to).await.map_err(failed)
}

#[tauri::command]
pub async fn core_translate_prepare(plugin: String, from: String, to: String, app: AppHandle, client: State<'_, Client>) -> Result<bool, String> {
    let core = client.core().await?;
    core.plugin_translate_prepare(&plugin, &PlatformTranslator(app), &from, &to).await.map_err(failed)
}

#[tauri::command]
pub async fn core_translate_remove(plugin: String, from: String, to: String, app: AppHandle, client: State<'_, Client>) -> Result<bool, String> {
    let core = client.core().await?;
    core.plugin_translate_remove(&plugin, &PlatformTranslator(app), &from, &to).await.map_err(failed)
}

/// Don't keep or log the text, it never leaves the phone.
#[tauri::command]
pub async fn core_translate_text(
    plugin: String,
    text: String,
    from: String,
    to: String,
    app: AppHandle,
    client: State<'_, Client>,
) -> Result<Option<String>, String> {
    let core = client.core().await?;
    core.plugin_translate_text(&plugin, &PlatformTranslator(app), &text, &from, &to).await.map_err(failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pair_reaches_the_screen_as_one_of_three_words() {
        use tauri_plugin_ft_platform::TranslateStatus;
        for (bridge, word) in [
            (TranslateStatus::Installed, "installed"),
            (TranslateStatus::Downloadable, "downloadable"),
            (TranslateStatus::Unsupported, "unsupported"),
        ] {
            assert_eq!(serde_json::to_value(pair_status(bridge)).unwrap(), serde_json::json!(word), "{bridge:?}");
        }
    }
}
