use anyhow::Result;
use sqlx::Row;

use crate::Store;

impl Store {
    pub async fn set_spare_capability(&self, slot: u8, capability: &[u8; 32]) -> Result<()> {
        sqlx::query("UPDATE spare_capabilities SET capability = ? WHERE slot = ?")
            .bind(capability.as_slice())
            .bind(slot)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn add_session(&self, id: &str, pin_hash: &[u8; 32], slot: u8) -> Result<()> {
        sqlx::query("INSERT INTO sessions (id, pin_hash, created_at, slot) VALUES (?, ?, unixepoch(), ?)")
            .bind(id)
            .bind(pin_hash.as_slice())
            .bind(slot)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // None for a session made before slots
    pub async fn session_slot(&self, id: &str) -> Result<Option<u8>> {
        let row = sqlx::query("SELECT slot FROM sessions WHERE id = ?").bind(id).fetch_optional(&self.pool).await?;
        Ok(row.and_then(|row| row.get::<Option<u8>, _>("slot")))
    }

    pub async fn set_session_slot(&self, id: &str, slot: u8) -> Result<()> {
        sqlx::query("UPDATE sessions SET slot = ? WHERE id = ?").bind(slot).bind(id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn session_with_slot(&self, slot: u8) -> Result<Option<String>> {
        let row = sqlx::query("SELECT id FROM sessions WHERE slot = ?").bind(slot).fetch_optional(&self.pool).await?;
        Ok(row.map(|row| row.get("id")))
    }

    pub async fn used_slots(&self) -> Result<Vec<u8>> {
        let rows = sqlx::query("SELECT slot FROM sessions WHERE slot IS NOT NULL ORDER BY slot").fetch_all(&self.pool).await?;
        Ok(rows.iter().map(|row| row.get::<u8, _>("slot")).collect())
    }

    pub async fn spare_capabilities(&self) -> Result<Vec<(u8, [u8; 32])>> {
        let rows = sqlx::query("SELECT slot, capability FROM spare_capabilities ORDER BY slot").fetch_all(&self.pool).await?;
        rows.iter()
            .map(|row| {
                let capability: Vec<u8> = row.get("capability");
                Ok((row.get::<u8, _>("slot"), capability.try_into().map_err(|_| anyhow::anyhow!("corrupt capability"))?))
            })
            .collect()
    }

    pub async fn add_spare_capability(&self, slot: u8, capability: &[u8; 32]) -> Result<()> {
        sqlx::query("INSERT INTO spare_capabilities (slot, capability) VALUES (?, ?)")
            .bind(slot)
            .bind(capability.as_slice())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn session_by_pin(&self, pin_hash: &[u8; 32]) -> Result<Option<String>> {
        let row = sqlx::query("SELECT id FROM sessions WHERE pin_hash = ?").bind(pin_hash.as_slice()).fetch_optional(&self.pool).await?;
        Ok(row.map(|row| row.get("id")))
    }

    // the files on disk are the caller's to delete
    pub async fn remove_session(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM plugin_refs WHERE contact IN (SELECT device_id FROM contacts WHERE session = ?)")
            .bind(id)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM pending_outbox WHERE contact IN (SELECT device_id FROM contacts WHERE session = ?)")
            .bind(id)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM contacts WHERE session = ?").bind(id).execute(&self.pool).await?;
        sqlx::query("DELETE FROM sessions WHERE id = ?").bind(id).execute(&self.pool).await?;
        self.forget_orphans().await
    }

    pub async fn empty_sessions(&self) -> Result<Vec<String>> {
        let rows = sqlx::query(
            "SELECT id FROM sessions
             WHERE id NOT IN (SELECT session FROM contacts WHERE session IS NOT NULL)
               AND id NOT IN (SELECT session FROM circles WHERE session IS NOT NULL)
             ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(|row| row.get("id")).collect())
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::{contact, message, store};
    use crate::{CircleRecord, NewContact};

    #[tokio::test]
    async fn a_removed_session_leaves_nothing_behind() {
        let store = store().await;
        store.add_session("s1", &[1; 32], 3).await.expect("creates");
        store.add_contact(&NewContact { session: Some("s1".to_owned()), ..contact("ft_bob") }).await.expect("adds inside");
        store.insert_message(&message("m1", "ft_bob", true, 1)).await.expect("inserts");
        store.enqueue("m1", "ft_bob", 1).await.expect("queues");
        store.remove_session("s1").await.expect("removes");
        assert!(store.session_by_pin(&[1; 32]).await.unwrap().is_none());
        assert!(store.contact("ft_bob").await.unwrap().is_none());
        assert!(store.message("m1").await.unwrap().is_none());
        assert!(store.outbox().await.unwrap().is_empty());
        assert!(store.used_slots().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn only_sessions_with_nobody_in_them_are_empty() {
        let store = store().await;
        store.add_session("s1", &[1; 32], 1).await.expect("creates");
        store.add_session("s2", &[2; 32], 2).await.expect("creates");
        store.add_session("s3", &[3; 32], 3).await.expect("creates");
        store.add_contact(&NewContact { session: Some("s2".to_owned()), ..contact("ft_bob") }).await.expect("adds inside");
        store
            .add_contact(&NewContact { session: Some("s3".to_owned()), accepted: false, ..contact("ft_eve") })
            .await
            .expect("a request inside");
        store.add_contact(&contact("ft_carol")).await.expect("adds to the main list");
        assert_eq!(store.empty_sessions().await.expect("lists"), vec!["s1".to_owned()]);

        store
            .save_circle(&CircleRecord {
                id: "c1".to_owned(),
                name: "Friends".to_owned(),
                card: vec![1],
                revision: 1,
                admins_only: false,
                session: Some("s1".to_owned()),
                left: false,
                created_at: 1,
            })
            .await
            .expect("saves");
        assert!(store.empty_sessions().await.expect("lists").is_empty());
    }

    #[tokio::test]
    async fn a_session_is_found_by_its_pin_hash_and_owns_its_contacts() {
        let store = store().await;
        store.add_session("s1", &[7; 32], 1).await.expect("adds");
        assert_eq!(store.session_by_pin(&[7; 32]).await.expect("reads").as_deref(), Some("s1"));
        assert_eq!(store.session_by_pin(&[8; 32]).await.expect("reads"), None);

        store.add_contact(&contact("ft_bob")).await.expect("adds bob");
        store.add_contact(&NewContact { session: Some("s1".to_owned()), ..contact("ft_pablo") }).await.expect("adds pablo");
        store.insert_message(&message("m1", "ft_pablo", false, 5)).await.expect("inserts");

        let main: Vec<String> = store.conversations().await.expect("lists").into_iter().map(|c| c.contact.device_id).collect();
        assert_eq!(main, vec!["ft_bob"], "the main list never shows a session's contacts");
        let hidden = store.session_conversations("s1").await.expect("lists");
        assert_eq!(hidden.len(), 1);
        assert_eq!(hidden[0].contact.device_id, "ft_pablo");
        assert_eq!(hidden[0].unread, 1);
        assert_eq!(store.contact("ft_pablo").await.unwrap().unwrap().session.as_deref(), Some("s1"));
        assert_eq!(store.contact("ft_bob").await.unwrap().unwrap().session, None);
    }

    #[tokio::test]
    async fn scanning_a_known_contact_inside_a_session_does_not_move_them() {
        let store = store().await;
        store.add_session("s1", &[7; 32], 1).await.expect("adds");
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.add_contact(&NewContact { session: Some("s1".to_owned()), ..contact("ft_bob") }).await.expect("updates");
        assert_eq!(store.contact("ft_bob").await.unwrap().unwrap().session, None);
    }

    #[tokio::test]
    async fn two_sessions_cannot_share_a_pin() {
        let store = store().await;
        store.add_session("s1", &[7; 32], 1).await.expect("adds");
        assert!(store.add_session("s2", &[7; 32], 2).await.is_err());
    }

    #[tokio::test]
    async fn spare_capabilities_are_kept_and_each_session_has_its_slot() {
        let store = store().await;
        assert!(store.spare_capabilities().await.expect("reads").is_empty());
        for slot in 1..=7u8 {
            store.add_spare_capability(slot, &[slot; 32]).await.expect("keeps");
        }
        let spares = store.spare_capabilities().await.expect("reads");
        assert_eq!(spares.len(), 7);
        assert_eq!(spares[2], (3, [3; 32]));

        store.add_session("s1", &[7; 32], 1).await.expect("adds");
        store.add_session("s2", &[8; 32], 4).await.expect("adds");
        assert_eq!(store.session_slot("s2").await.expect("reads"), Some(4));
        assert_eq!(store.used_slots().await.expect("reads"), vec![1, 4]);
        assert!(store.add_session("s3", &[9; 32], 4).await.is_err(), "one session per slot");
    }
}
