use std::collections::HashMap;

use anyhow::Result;
use sqlx::Row;

use crate::Store;

impl Store {
    pub async fn set_reply(&self, message_id: &str, reply_to: &str) -> Result<()> {
        sqlx::query("INSERT INTO message_replies (message_id, reply_to) VALUES (?, ?) ON CONFLICT (message_id) DO UPDATE SET reply_to = excluded.reply_to")
            .bind(message_id)
            .bind(reply_to)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn reply_to(&self, message_id: &str) -> Result<Option<String>> {
        let row = sqlx::query("SELECT reply_to FROM message_replies WHERE message_id = ?").bind(message_id).fetch_optional(&self.pool).await?;
        Ok(row.map(|row| row.get("reply_to")))
    }

    pub async fn replies(&self, contact: &str) -> Result<HashMap<String, String>> {
        let rows = sqlx::query(
            "SELECT r.message_id, r.reply_to FROM message_replies r JOIN messages m ON m.message_id = r.message_id WHERE m.contact = ?",
        )
        .bind(contact)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|row| (row.get("message_id"), row.get("reply_to"))).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{contact, message, store};

    #[tokio::test]
    async fn a_message_can_answer_another_and_the_answer_goes_with_it() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.insert_message(&message("m1", "ft_bob", false, 1)).await.expect("inserts");
        store.insert_message(&message("m2", "ft_bob", true, 2)).await.expect("inserts");
        assert_eq!(store.reply_to("m2").await.unwrap(), None);
        store.set_reply("m2", "m1").await.expect("sets");
        assert_eq!(store.reply_to("m2").await.unwrap(), Some("m1".to_owned()));
        assert_eq!(store.replies("ft_bob").await.unwrap(), HashMap::from([("m2".to_owned(), "m1".to_owned())]));
        store.forget_message("m1").await.expect("forgets");
        assert_eq!(store.reply_to("m2").await.unwrap(), Some("m1".to_owned()));
        store.forget_message("m2").await.expect("forgets");
        assert_eq!(store.reply_to("m2").await.unwrap(), None);
        assert!(store.replies("ft_bob").await.unwrap().is_empty());
    }
}
