use anyhow::Result;
use sqlx::Row;

use crate::{now, Store};

impl Store {
    pub async fn set_pinned(&self, message_id: &str, pinned: bool) -> Result<()> {
        if pinned {
            sqlx::query("INSERT INTO pinned (message_id, pinned_at) VALUES (?, ?) ON CONFLICT (message_id) DO NOTHING")
                .bind(message_id)
                .bind(now())
                .execute(&self.pool)
                .await?;
        } else {
            sqlx::query("DELETE FROM pinned WHERE message_id = ?").bind(message_id).execute(&self.pool).await?;
        }
        Ok(())
    }

    pub async fn pinned(&self, contact: &str) -> Result<Vec<String>> {
        let rows = sqlx::query("SELECT p.message_id FROM pinned p JOIN messages m ON m.message_id = p.message_id WHERE m.contact = ? ORDER BY p.pinned_at DESC, p.message_id DESC")
            .bind(contact)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|row| row.get("message_id")).collect())
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::{contact, message, store};

    #[tokio::test]
    async fn messages_are_pinned_on_this_phone_latest_first() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.insert_message(&message("m1", "ft_bob", false, 1)).await.expect("inserts");
        store.insert_message(&message("m2", "ft_bob", true, 2)).await.expect("inserts");
        assert!(store.pinned("ft_bob").await.unwrap().is_empty());
        store.set_pinned("m1", true).await.expect("pins");
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        store.set_pinned("m2", true).await.expect("pins");
        store.set_pinned("m2", true).await.expect("again changes nothing");
        assert_eq!(store.pinned("ft_bob").await.unwrap(), vec!["m2".to_owned(), "m1".to_owned()]);
        store.set_pinned("m2", false).await.expect("unpins");
        assert_eq!(store.pinned("ft_bob").await.unwrap(), vec!["m1".to_owned()]);
        store.forget_message("m1").await.expect("forgets");
        assert!(store.pinned("ft_bob").await.unwrap().is_empty(), "gone with the message");
    }
}
