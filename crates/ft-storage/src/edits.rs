use std::collections::HashSet;

use anyhow::Result;
use sqlx::Row;

use crate::{now, Store};

impl Store {
    // an edit older than the one already here changes nothing, whatever order they arrive in
    pub async fn edit_text(&self, message_id: &str, text: &str, at: i64) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let newer = sqlx::query("SELECT 1 FROM message_edits WHERE message_id = ? AND edited_at > ?").bind(message_id).bind(at).fetch_optional(&mut *tx).await?;
        if newer.is_some() {
            return Ok(false);
        }
        let changed = sqlx::query("UPDATE messages SET body = ? WHERE message_id = ?").bind(text).bind(message_id).execute(&mut *tx).await?.rows_affected();
        if changed == 1 {
            sqlx::query("INSERT INTO message_edits (message_id, edited_at) VALUES (?, ?) ON CONFLICT (message_id) DO UPDATE SET edited_at = excluded.edited_at")
                .bind(message_id)
                .bind(at)
                .execute(&mut *tx)
                .await?;
            // a note on the old words (a translation) is wrong for the new ones
            sqlx::query("DELETE FROM message_notes WHERE message_id = ?").bind(message_id).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(changed == 1)
    }

    pub async fn edited(&self, contact: &str) -> Result<HashSet<String>> {
        let rows = sqlx::query("SELECT e.message_id FROM message_edits e JOIN messages m ON m.message_id = e.message_id WHERE m.contact = ?")
            .bind(contact)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|row| row.get("message_id")).collect())
    }

    pub async fn mark_deleted(&self, message_id: &str) -> Result<bool> {
        let changed = sqlx::query("UPDATE messages SET body = '' WHERE message_id = ?").bind(message_id).execute(&self.pool).await?.rows_affected();
        if changed == 1 {
            sqlx::query("DELETE FROM files WHERE message_id = ?").bind(message_id).execute(&self.pool).await?;
            sqlx::query("DELETE FROM reactions WHERE message_id = ?").bind(message_id).execute(&self.pool).await?;
            sqlx::query("DELETE FROM message_notes WHERE message_id = ?").bind(message_id).execute(&self.pool).await?;
            sqlx::query("DELETE FROM pending_outbox WHERE message_id = ?").bind(message_id).execute(&self.pool).await?;
            sqlx::query("DELETE FROM scheduled WHERE message_id = ?").bind(message_id).execute(&self.pool).await?;
            sqlx::query("INSERT INTO message_deletions (message_id, deleted_at) VALUES (?, ?) ON CONFLICT (message_id) DO NOTHING")
                .bind(message_id)
                .bind(now())
                .execute(&self.pool)
                .await?;
        }
        Ok(changed == 1)
    }

    pub async fn deleted(&self, contact: &str) -> Result<HashSet<String>> {
        let rows = sqlx::query("SELECT d.message_id FROM message_deletions d JOIN messages m ON m.message_id = d.message_id WHERE m.contact = ?")
            .bind(contact)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|row| row.get("message_id")).collect())
    }

    pub async fn is_deleted(&self, message_id: &str) -> Result<bool> {
        Ok(sqlx::query("SELECT 1 FROM message_deletions WHERE message_id = ?").bind(message_id).fetch_optional(&self.pool).await?.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{contact, message, store};

    #[tokio::test]
    async fn a_message_is_edited_in_place_or_emptied_when_taken_back() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.insert_message(&message("m1", "ft_bob", true, 1)).await.expect("inserts");
        store.insert_message(&message("m2", "ft_bob", true, 2)).await.expect("inserts");
        store.set_reaction("m2", false, Some("👍"), 1).await.expect("reacts");

        assert!(store.edit_text("m1", "other words", 1).await.expect("edits"));
        assert!(!store.edit_text("nope", "x", 1).await.expect("edits nothing"));
        assert_eq!(store.message("m1").await.unwrap().unwrap().body, "other words");
        assert_eq!(store.edited("ft_bob").await.unwrap(), HashSet::from(["m1".to_owned()]));

        assert!(store.mark_deleted("m2").await.expect("takes back"));
        assert_eq!(store.message("m2").await.unwrap().unwrap().body, "");
        assert!(store.is_deleted("m2").await.unwrap());
        assert!(!store.is_deleted("m1").await.unwrap());
        assert_eq!(store.deleted("ft_bob").await.unwrap(), HashSet::from(["m2".to_owned()]));
        assert!(store.reactions("ft_bob").await.unwrap().is_empty(), "its reactions went with it");
        store.forget_message("m1").await.expect("forgets");
        assert!(store.edited("ft_bob").await.unwrap().is_empty(), "the mark goes with the message");
    }

    #[tokio::test]
    async fn a_message_taken_back_before_it_went_out_leaves_the_outbox() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.insert_message(&message("m1", "ft_bob", true, 1)).await.expect("inserts");
        store.insert_message(&message("m2", "ft_bob", true, 2)).await.expect("inserts");
        store.enqueue("m1", "ft_bob", 1_000).await.expect("queues");
        store.enqueue_at("m2", "ft_bob", 1_000, 9_000).await.expect("queues for later");

        assert!(store.mark_deleted("m1").await.expect("takes back"));
        assert!(store.mark_deleted("m2").await.expect("takes back"));
        assert!(store.outbox().await.unwrap().is_empty(), "neither waits to be sent");
        assert!(store.scheduled_all().await.unwrap().is_empty(), "nor is for later");
    }
}
