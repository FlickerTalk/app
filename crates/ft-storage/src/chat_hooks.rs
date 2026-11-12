use anyhow::Result;
use sqlx::Row;

use crate::Store;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageNote {
    pub message_id: String,
    pub plugin: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatHook {
    pub plugin: String,
    pub auto_message: bool,
    pub auto_composer: bool,
    pub lang: Option<String>,
}

impl Store {
    // notes and chat switches hang from no table, so whatever deletes their message or conversation
    // has to call this
    pub(crate) async fn forget_orphans(&self) -> Result<()> {
        sqlx::query(
            "DELETE FROM message_notes WHERE message_id NOT IN (SELECT message_id FROM messages)
               AND message_id NOT IN (SELECT message_id FROM circle_messages)",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query("DELETE FROM chat_hooks WHERE conversation NOT IN (SELECT device_id FROM contacts) AND conversation NOT IN (SELECT id FROM circles)")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_message_note(&self, message_id: &str, plugin: &str, text: &str, at: i64) -> Result<()> {
        sqlx::query(
            "INSERT INTO message_notes (message_id, plugin, text, at) VALUES (?, ?, ?, ?)
             ON CONFLICT (message_id, plugin) DO UPDATE SET text = excluded.text, at = excluded.at",
        )
        .bind(message_id)
        .bind(plugin)
        .bind(text)
        .bind(at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn clear_message_note(&self, message_id: &str, plugin: &str) -> Result<()> {
        sqlx::query("DELETE FROM message_notes WHERE message_id = ? AND plugin = ?").bind(message_id).bind(plugin).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn notes(&self, conversation: &str) -> Result<Vec<MessageNote>> {
        let rows = sqlx::query(
            "SELECT message_id, plugin, text FROM message_notes
             WHERE message_id IN (SELECT message_id FROM messages WHERE contact = ?1 UNION SELECT message_id FROM circle_messages WHERE circle = ?1)
             ORDER BY message_id, plugin",
        )
        .bind(conversation)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|row| MessageNote { message_id: row.get("message_id"), plugin: row.get("plugin"), text: row.get("text") })
            .collect())
    }

    pub async fn set_chat_hooks(&self, conversation: &str, plugin: &str, auto_message: bool, auto_composer: bool, lang: Option<&str>) -> Result<()> {
        if !auto_message && !auto_composer && lang.is_none() {
            sqlx::query("DELETE FROM chat_hooks WHERE conversation = ? AND plugin = ?").bind(conversation).bind(plugin).execute(&self.pool).await?;
            return Ok(());
        }
        let known = sqlx::query("SELECT 1 FROM contacts WHERE device_id = ?1 UNION SELECT 1 FROM circles WHERE id = ?1")
            .bind(conversation)
            .fetch_optional(&self.pool)
            .await?;
        anyhow::ensure!(known.is_some(), "no such conversation");
        sqlx::query(
            "INSERT INTO chat_hooks (conversation, plugin, auto_message, auto_composer, lang) VALUES (?, ?, ?, ?, ?)
             ON CONFLICT (conversation, plugin) DO UPDATE SET auto_message = excluded.auto_message, auto_composer = excluded.auto_composer,
             lang = excluded.lang",
        )
        .bind(conversation)
        .bind(plugin)
        .bind(auto_message)
        .bind(auto_composer)
        .bind(lang)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn chat_hooks(&self, conversation: &str) -> Result<Vec<ChatHook>> {
        let rows = sqlx::query("SELECT plugin, auto_message, auto_composer, lang FROM chat_hooks WHERE conversation = ? ORDER BY plugin")
            .bind(conversation)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(|row| ChatHook {
                plugin: row.get("plugin"),
                auto_message: row.get("auto_message"),
                auto_composer: row.get("auto_composer"),
                lang: row.get("lang"),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    use super::*;
    use crate::tests::{contact, message, store};
    use crate::{CircleMessage, CircleRecord, MessageState, NewContact};

    #[tokio::test]
    async fn a_note_hangs_from_its_message_one_per_tool_and_goes_with_it() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.add_contact(&contact("ft_carol")).await.expect("adds");
        store.insert_message(&message("m1", "ft_bob", false, 1)).await.expect("inserts");
        store.insert_message(&message("m2", "ft_bob", false, 2)).await.expect("inserts");
        store.insert_message(&message("m3", "ft_carol", false, 3)).await.expect("inserts");

        store.set_message_note("m1", "com.example.translate", "hola", 10).await.expect("notes");
        store.set_message_note("m1", "com.example.other", "salut", 11).await.expect("notes");
        store.set_message_note("m3", "com.example.translate", "ciao", 12).await.expect("notes");
        store.set_message_note("m1", "com.example.translate", "hola otra vez", 13).await.expect("replaces");
        let note = |message: &str, plugin: &str, text: &str| MessageNote { message_id: message.to_owned(), plugin: plugin.to_owned(), text: text.to_owned() };
        assert_eq!(
            store.notes("ft_bob").await.expect("reads"),
            vec![note("m1", "com.example.other", "salut"), note("m1", "com.example.translate", "hola otra vez")],
            "this conversation's only, one per tool"
        );

        store.clear_message_note("m1", "com.example.other").await.expect("clears");
        assert_eq!(store.notes("ft_bob").await.unwrap(), vec![note("m1", "com.example.translate", "hola otra vez")]);
        store.forget_message("m1").await.expect("forgets");
        assert!(store.notes("ft_bob").await.unwrap().is_empty(), "the note goes with the message");
    }

    async fn a_circle_with_a_message(store: &Store, id: &str, session: Option<&str>) {
        let circle = CircleRecord {
            id: id.to_owned(),
            name: "Friends".to_owned(),
            card: vec![1],
            revision: 1,
            admins_only: false,
            session: session.map(str::to_owned),
            left: false,
            created_at: 1,
        };
        store.save_circle(&circle).await.expect("saves");
        let said = CircleMessage {
            message_id: format!("{id}-m1"),
            circle: id.to_owned(),
            sender: "ft_bob".to_owned(),
            outgoing: false,
            kind: "text".to_owned(),
            body: "salut".to_owned(),
            sent_at: 1,
            received_at: 1,
            state: MessageState::Delivered,
        };
        store.insert_circle_message(&said).await.expect("inserts");
    }

    #[tokio::test]
    async fn a_circle_message_has_notes_and_a_circle_its_switches() {
        let store = store().await;
        a_circle_with_a_message(&store, "c1", None).await;
        store.set_message_note("c1-m1", "com.example.translate", "hola", 1).await.expect("notes");
        store.set_chat_hooks("c1", "com.example.translate", true, false, Some("fr")).await.expect("sets");
        let notes = store.notes("c1").await.expect("reads");
        assert_eq!((notes.len(), notes[0].message_id.as_str(), notes[0].text.as_str()), (1, "c1-m1", "hola"));
        assert_eq!(store.chat_hooks("c1").await.unwrap()[0].lang.as_deref(), Some("fr"));

        store.remove_circle("c1").await.expect("removes");
        assert!(store.notes("c1").await.unwrap().is_empty());
        assert!(store.chat_hooks("c1").await.unwrap().is_empty(), "its switches went with it");
        assert_eq!(sqlx::query("SELECT COUNT(*) AS n FROM message_notes").fetch_one(&store.pool).await.unwrap().get::<i64, _>("n"), 0, "nothing left behind");
    }

    #[tokio::test]
    async fn notes_and_switches_go_with_their_conversation() {
        let store = store().await;
        let count = |table: &'static str| {
            let store = &store;
            let sql = if table == "message_notes" { "SELECT COUNT(*) AS n FROM message_notes" } else { "SELECT COUNT(*) AS n FROM chat_hooks" };
            async move { sqlx::query(sql).fetch_one(&store.pool).await.unwrap().get::<i64, _>("n") }
        };
        store.add_contact(&contact("ft_bob")).await.unwrap();
        store.insert_message(&message("m1", "ft_bob", false, 1)).await.unwrap();
        store.set_message_note("m1", "com.example.translate", "hola", 1).await.unwrap();
        store.set_chat_hooks("ft_bob", "com.example.translate", true, true, None).await.unwrap();
        store.remove_contact("ft_bob").await.expect("removes");
        assert_eq!((count("message_notes").await, count("chat_hooks").await), (0, 0), "gone with the contact");

        store.add_session("s1", &[1; 32], 3).await.unwrap();
        store.add_contact(&NewContact { session: Some("s1".to_owned()), ..contact("ft_ana") }).await.unwrap();
        store.insert_message(&message("m2", "ft_ana", false, 1)).await.unwrap();
        store.set_message_note("m2", "com.example.translate", "hola", 1).await.unwrap();
        store.set_chat_hooks("ft_ana", "com.example.translate", true, false, None).await.unwrap();
        a_circle_with_a_message(&store, "c2", Some("s1")).await;
        store.set_message_note("c2-m1", "com.example.translate", "hola", 1).await.unwrap();
        store.set_chat_hooks("c2", "com.example.translate", true, false, None).await.unwrap();
        store.remove_session("s1").await.expect("removes");
        assert_eq!((count("message_notes").await, count("chat_hooks").await), (0, 0), "gone with the session");

        store.add_contact(&contact("ft_carol")).await.unwrap();
        store.set_history("ft_carol", 1, 0).await.unwrap();
        store.insert_message(&message("old", "ft_carol", false, 1)).await.unwrap();
        store.set_message_note("old", "com.example.translate", "viejo", 1).await.unwrap();
        assert_eq!(store.sweep(10_000).await.unwrap(), vec!["ft_carol".to_owned()]);
        assert_eq!(count("message_notes").await, 0, "gone with the swept message");
    }

    #[tokio::test]
    async fn editing_or_taking_back_a_message_takes_its_notes() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.insert_message(&message("m1", "ft_bob", false, 1)).await.expect("inserts");
        store.insert_message(&message("m2", "ft_bob", false, 2)).await.expect("inserts");
        store.set_message_note("m1", "com.example.translate", "hola", 1).await.unwrap();
        store.set_message_note("m2", "com.example.translate", "adiós", 1).await.unwrap();

        assert!(store.edit_text("m1", "other words", 50).await.expect("edits"));
        assert!(store.notes("ft_bob").await.unwrap().iter().all(|note| note.message_id != "m1"), "edited: its note went");
        store.set_message_note("m1", "com.example.translate", "otras palabras", 2).await.unwrap();
        assert!(!store.edit_text("m1", "older words", 40).await.expect("an older edit"));
        assert!(store.notes("ft_bob").await.unwrap().iter().any(|note| note.message_id == "m1"), "nothing changed, the note stays");

        assert!(store.mark_deleted("m2").await.expect("takes back"));
        assert!(store.notes("ft_bob").await.unwrap().iter().all(|note| note.message_id != "m2"), "taken back: its note went");
    }

    #[tokio::test]
    async fn a_conversation_remembers_which_tools_run_by_themselves_and_its_language() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.add_contact(&contact("ft_carol")).await.expect("adds");
        let hook = |plugin: &str, auto_message: bool, auto_composer: bool, lang: Option<&str>| ChatHook {
            plugin: plugin.to_owned(),
            auto_message,
            auto_composer,
            lang: lang.map(str::to_owned),
        };

        assert!(store.chat_hooks("ft_bob").await.expect("reads").is_empty(), "nothing runs by itself unless chosen");
        store.set_chat_hooks("ft_bob", "com.example.translate", true, false, None).await.expect("sets");
        store.set_chat_hooks("ft_carol", "com.example.translate", true, true, Some("de")).await.expect("sets");
        assert_eq!(store.chat_hooks("ft_bob").await.unwrap(), vec![hook("com.example.translate", true, false, None)]);
        store.set_chat_hooks("ft_bob", "com.example.translate", true, true, Some("es")).await.expect("changes");
        assert_eq!(store.chat_hooks("ft_bob").await.unwrap(), vec![hook("com.example.translate", true, true, Some("es"))]);
        store.set_chat_hooks("ft_bob", "com.example.translate", false, false, Some("es")).await.expect("switches off");
        assert_eq!(store.chat_hooks("ft_bob").await.unwrap(), vec![hook("com.example.translate", false, false, Some("es"))], "the language stays");
        store.set_chat_hooks("ft_bob", "com.example.translate", false, false, None).await.expect("forgets");
        assert!(store.chat_hooks("ft_bob").await.unwrap().is_empty());
        assert!(store.set_chat_hooks("ft_nobody", "com.example.translate", true, true, None).await.is_err(), "no conversation, no row");

        store.remove_contact("ft_carol").await.expect("removes");
        assert!(store.chat_hooks("ft_carol").await.unwrap().is_empty(), "it goes with the contact");
    }

    #[tokio::test]
    async fn a_removed_plugin_leaves_no_notes_nor_switches_behind() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.insert_message(&message("m1", "ft_bob", false, 1)).await.expect("inserts");
        for plugin in ["com.example.translate", "com.example.other"] {
            store.install_plugin(plugin, "1.0.0", "{}").await.unwrap();
            store.set_message_note("m1", plugin, "note", 1).await.unwrap();
            store.set_chat_hooks("ft_bob", plugin, true, true, Some("es")).await.unwrap();
        }
        store.remove_plugin("com.example.translate").await.expect("removes");
        assert!(store.notes("ft_bob").await.unwrap().iter().all(|note| note.plugin == "com.example.other"));
        assert_eq!(store.chat_hooks("ft_bob").await.unwrap().len(), 1);
        assert_eq!(store.chat_hooks("ft_bob").await.unwrap()[0].plugin, "com.example.other");
    }

    #[tokio::test]
    async fn a_database_from_before_has_no_notes_nor_switches() {
        let options = SqliteConnectOptions::new().in_memory(true).foreign_keys(true);
        let pool = SqlitePoolOptions::new().max_connections(1).connect_with(options).await.expect("opens");
        sqlx::migrate!("./migrations").run_to(22, &pool).await.expect("migrates to 0022");
        sqlx::query("INSERT INTO contacts (device_id, name, card, mailbox, added_at) VALUES ('ft_bob', 'Bob', x'01', 1, 1)").execute(&pool).await.expect("a contact");

        let store = Store::with(pool, None).await.expect("migrates the rest");
        store.insert_message(&message("m1", "ft_bob", false, 1)).await.expect("inserts");
        assert!(store.notes("ft_bob").await.expect("reads").is_empty());
        assert!(store.chat_hooks("ft_bob").await.expect("reads").is_empty());
        store.set_message_note("m1", "com.example.translate", "hola", 1).await.expect("notes");
        store.set_chat_hooks("ft_bob", "com.example.translate", true, false, Some("es")).await.expect("sets");
    }
}
