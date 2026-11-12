use anyhow::Result;
use sqlx::Row;

use crate::Store;

pub struct StoredIdentity {
    pub sealed: String,
    pub route_capability: [u8; 32],
    pub envelope: Option<String>,
}

impl Store {
    pub async fn identity(&self) -> Result<Option<StoredIdentity>> {
        let row = sqlx::query("SELECT sealed, route_capability, envelope FROM identity WHERE id = 1").fetch_optional(&self.pool).await?;
        row.map(|row| {
            let capability: Vec<u8> = row.get("route_capability");
            Ok(StoredIdentity {
                sealed: row.get("sealed"),
                route_capability: capability.try_into().map_err(|_| anyhow::anyhow!("corrupt route capability"))?,
                envelope: row.get("envelope"),
            })
        })
        .transpose()
    }

    pub async fn save_envelope(&self, sealed: &str) -> Result<()> {
        sqlx::query("UPDATE identity SET envelope = ? WHERE id = 1").bind(sealed).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn forget_envelope(&self) -> Result<()> {
        sqlx::query("UPDATE identity SET envelope = NULL WHERE id = 1").execute(&self.pool).await?;
        Ok(())
    }

    pub async fn set_route_capability(&self, route_capability: &[u8; 32]) -> Result<()> {
        sqlx::query("UPDATE identity SET route_capability = ? WHERE id = 1")
            .bind(route_capability.as_slice())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn save_identity(&self, sealed: &str, route_capability: &[u8; 32]) -> Result<()> {
        sqlx::query(
            "INSERT INTO identity (id, sealed, route_capability, created_at) VALUES (1, ?, ?, unixepoch())
             ON CONFLICT (id) DO UPDATE SET sealed = excluded.sealed",
        )
        .bind(sealed)
        .bind(route_capability.as_slice())
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::store;

    #[tokio::test]
    async fn the_identity_is_stored_once_created() {
        let store = store().await;
        assert!(store.identity().await.expect("reads").is_none());
        store.save_identity("sealed", &[5; 32]).await.expect("saves");
        let saved = store.identity().await.expect("reads").expect("present");
        assert_eq!(saved.sealed, "sealed");
        assert_eq!(saved.route_capability, [5; 32]);
    }
}
