use anyhow::Result;
use sqlx::Row;

use crate::Store;

impl Store {
    pub async fn setting(&self, key: &str) -> Result<Option<String>> {
        let row = sqlx::query("SELECT value FROM settings WHERE key = ?").bind(key).fetch_optional(&self.pool).await?;
        Ok(row.map(|row| row.get("value")))
    }

    pub async fn forget_setting(&self, key: &str) -> Result<()> {
        sqlx::query("DELETE FROM settings WHERE key = ?").bind(key).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT (key) DO UPDATE SET value = excluded.value")
            .bind(key)
            .bind(value)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::store;

    #[tokio::test]
    async fn settings_are_remembered() {
        let store = store().await;
        assert!(store.setting("mailbox").await.expect("reads").is_none());
        store.set_setting("mailbox", "0").await.expect("saves");
        assert_eq!(store.setting("mailbox").await.expect("reads").as_deref(), Some("0"));
    }
}
