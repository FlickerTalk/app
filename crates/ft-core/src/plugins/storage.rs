use anyhow::{ensure, Result};

use super::RECORD_KEY;
use crate::Core;

const MEMORY_KEY: usize = 64;
const MEMORY_VALUE: usize = 64 * 1024;
const MEMORY_KEYS: usize = 64;

pub const RECORD_VALUE: usize = 16 * 1024 * 1024;

impl Core {
    // The plugin frame has an opaque origin, so the browser gives it no storage
    pub async fn plugin_remembers(&self, id: &str, session: Option<&str>, key: &str) -> Result<Option<String>> {
        self.store.plugin_value(id, self.place(session)?, key).await
    }

    pub async fn plugin_remember(&self, id: &str, session: Option<&str>, key: &str, value: &str) -> Result<()> {
        let session = self.place(session)?;
        ensure!(!key.is_empty() && key.len() <= MEMORY_KEY, "that key is too long");
        ensure!(value.len() <= MEMORY_VALUE, "a plugin may not keep that much");
        ensure!(self.store.plugin(id).await?.is_some(), "{id} is not installed here");
        let keys = self.store.plugin_keys(id, session).await?;
        ensure!(
            keys.len() < MEMORY_KEYS || keys.iter().any(|known| known == key),
            "a plugin may not keep that many things"
        );
        self.store.set_plugin_value(id, session, key, value).await
    }

    pub async fn plugin_forget(&self, id: &str, session: Option<&str>, key: &str) -> Result<()> {
        self.store.forget_plugin_value(id, self.place(session)?, key).await
    }

    pub async fn plugin_memory_keys(&self, id: &str, session: Option<&str>) -> Result<Vec<String>> {
        self.store.plugin_keys(id, self.place(session)?).await
    }

    pub async fn plugin_record(&self, id: &str, session: Option<&str>, key: &str) -> Result<Option<Vec<u8>>> {
        self.store.plugin_record(id, self.place(session)?, key).await
    }

    pub async fn plugin_record_set(&self, id: &str, session: Option<&str>, key: &str, value: &[u8]) -> Result<()> {
        let session = self.place(session)?;
        ensure!(!key.is_empty() && key.len() <= RECORD_KEY, "that key is too long");
        ensure!(value.len() <= RECORD_VALUE, "a record may not hold that much");
        let quota = self.granted_to(id).await?.storage.quota();
        let held = self.store.plugin_record(id, session, key).await?.map_or(0, |old| old.len() as i64);
        let used = self.store.plugin_records_size(id, session).await? - held;
        ensure!(used + value.len() as i64 <= quota as i64, "the plugin has no room left for that");
        self.store.set_plugin_record(id, session, key, value).await
    }

    pub async fn plugin_record_forget(&self, id: &str, session: Option<&str>, key: &str) -> Result<()> {
        self.store.forget_plugin_record(id, self.place(session)?, key).await
    }

    pub async fn plugin_record_keys(&self, id: &str, session: Option<&str>, prefix: &str) -> Result<Vec<String>> {
        self.store.plugin_record_keys(id, self.place(session)?, prefix).await
    }

    pub async fn plugin_records_usage(&self, id: &str, session: Option<&str>) -> Result<(u64, u64)> {
        let session = self.place(session)?;
        let quota = self.granted_to(id).await?.storage.quota();
        Ok((self.store.plugin_records_size(id, session).await? as u64, quota))
    }
}
