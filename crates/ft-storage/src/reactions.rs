use std::collections::HashMap;

use anyhow::Result;
use sqlx::Row;

use crate::Store;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reactions {
    pub mine: Option<String>,
    pub theirs: Option<String>,
}

impl Store {
    pub async fn set_reaction(&self, message_id: &str, mine: bool, emoji: Option<&str>, at: i64) -> Result<()> {
        sqlx::query(
            "INSERT INTO reactions (message_id, mine, emoji, reacted_at) VALUES (?, ?, ?, ?)
             ON CONFLICT (message_id, mine) DO UPDATE SET emoji = excluded.emoji, reacted_at = excluded.reacted_at
             WHERE excluded.reacted_at >= reactions.reacted_at",
        )
        .bind(message_id)
        .bind(mine)
        .bind(emoji.unwrap_or(""))
        .bind(at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn reactions(&self, contact: &str) -> Result<HashMap<String, Reactions>> {
        let rows = sqlx::query("SELECT r.message_id, r.mine, r.emoji FROM reactions r JOIN messages m ON m.message_id = r.message_id WHERE m.contact = ? AND r.emoji <> ''")
            .bind(contact)
            .fetch_all(&self.pool)
            .await?;
        let mut all: HashMap<String, Reactions> = HashMap::new();
        for row in rows {
            let entry = all.entry(row.get("message_id")).or_default();
            let emoji: String = row.get("emoji");
            if row.get::<bool, _>("mine") {
                entry.mine = Some(emoji);
            } else {
                entry.theirs = Some(emoji);
            }
        }
        Ok(all)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{contact, message, store};

    #[tokio::test]
    async fn an_older_edit_or_reaction_never_replaces_a_newer_one() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.insert_message(&message("m1", "ft_bob", false, 1)).await.expect("inserts");

        assert!(store.edit_text("m1", "newer", 200).await.expect("edits"));
        assert!(!store.edit_text("m1", "older", 100).await.expect("comes late"));
        assert_eq!(store.message("m1").await.unwrap().unwrap().body, "newer");
        assert!(store.edit_text("m1", "newest", 300).await.expect("edits"));
        assert_eq!(store.message("m1").await.unwrap().unwrap().body, "newest");

        async fn theirs(store: &Store) -> Option<String> {
            store.reactions("ft_bob").await.unwrap().get("m1").and_then(|r| r.theirs.clone())
        }
        store.set_reaction("m1", false, Some("👍"), 200).await.expect("reacts");
        store.set_reaction("m1", false, Some("😂"), 100).await.expect("comes late");
        assert_eq!(theirs(&store).await.as_deref(), Some("👍"));
        store.set_reaction("m1", false, None, 300).await.expect("takes back");
        store.set_reaction("m1", false, Some("❤️"), 250).await.expect("comes late");
        assert_eq!(theirs(&store).await, None, "taken back is newer");
        assert!(store.reactions("ft_bob").await.unwrap().is_empty(), "nothing listed for it");
        store.set_reaction("m1", false, Some("🎉"), 400).await.expect("reacts again");
        assert_eq!(theirs(&store).await.as_deref(), Some("🎉"));
        store.set_reaction("m1", true, Some("👍"), 50).await.expect("ours is apart");
        assert_eq!(store.reactions("ft_bob").await.unwrap()["m1"].mine.as_deref(), Some("👍"));
    }

    #[tokio::test]
    async fn each_side_puts_one_emoji_on_a_message() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.insert_message(&message("m1", "ft_bob", false, 1)).await.expect("inserts");
        store.set_reaction("m1", true, Some("👍"), 1).await.expect("reacts");
        store.set_reaction("m1", false, Some("❤️"), 1).await.expect("reacts");
        let all = store.reactions("ft_bob").await.unwrap();
        assert_eq!(all.get("m1"), Some(&Reactions { mine: Some("👍".to_owned()), theirs: Some("❤️".to_owned()) }));
        store.set_reaction("m1", true, Some("😂"), 2).await.expect("changes");
        store.set_reaction("m1", false, None, 2).await.expect("takes back");
        let all = store.reactions("ft_bob").await.unwrap();
        assert_eq!(all.get("m1"), Some(&Reactions { mine: Some("😂".to_owned()), theirs: None }));
        store.forget_message("m1").await.expect("forgets");
        assert!(store.reactions("ft_bob").await.unwrap().is_empty(), "gone with the message");
    }
}
