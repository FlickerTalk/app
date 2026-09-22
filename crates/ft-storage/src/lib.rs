//! Local SQLite of the device (Plan §25–27): the whole history lives here and nowhere else.
//!
//! Messages are idempotent by `message_id` (§27), their state only moves forward
//! (pending → sent → delivered → read, §38) and an outgoing message stays in `pending_outbox`
//! until the DELIVERED receipt arrives (§26).

use std::path::Path;

use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions, SqliteRow};
use sqlx::Row;

/// Outgoing: pending → sent → delivered → read. Incoming messages start as delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MessageState {
    Pending = 0,
    /// In the recipient's mailbox or handed to the DataChannel.
    Sent = 1,
    Delivered = 2,
    Read = 3,
}

impl MessageState {
    fn from_rank(rank: i64) -> Self {
        match rank {
            0 => Self::Pending,
            1 => Self::Sent,
            2 => Self::Delivered,
            _ => Self::Read,
        }
    }
}

pub struct StoredIdentity {
    pub sealed: String,
    pub route_capability: [u8; 32],
}

pub struct NewContact {
    pub device_id: String,
    pub name: String,
    pub card: Vec<u8>,
    pub mailbox: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contact {
    pub device_id: String,
    pub name: String,
    pub card: Vec<u8>,
    pub mailbox: bool,
    pub blocked: bool,
    /// They have written back, so they have our Contact Card.
    pub introduced: bool,
    pub added_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub message_id: String,
    pub contact: String,
    pub outgoing: bool,
    pub body: String,
    pub sent_at: i64,
    pub state: MessageState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxEntry {
    pub message_id: String,
    pub contact: String,
    pub created_at: i64,
    pub attempts: i64,
    pub next_attempt: i64,
    pub in_mailbox: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversation {
    pub contact: Contact,
    pub last: Option<Message>,
    pub unread: i64,
}

pub struct Store {
    pool: SqlitePool,
}

impl Store {
    pub async fn open(path: &Path) -> Result<Self> {
        let options = SqliteConnectOptions::new().filename(path).create_if_missing(true).foreign_keys(true);
        Self::with(SqlitePoolOptions::new().max_connections(4).connect_with(options).await?).await
    }

    /// For tests: one connection, so every query sees the same in-memory database.
    pub async fn open_in_memory() -> Result<Self> {
        let options = SqliteConnectOptions::new().in_memory(true).foreign_keys(true);
        Self::with(SqlitePoolOptions::new().max_connections(1).connect_with(options).await?).await
    }

    async fn with(pool: SqlitePool) -> Result<Self> {
        sqlx::migrate!("./migrations").run(&pool).await.context("cannot migrate the local database")?;
        Ok(Self { pool })
    }

    pub async fn identity(&self) -> Result<Option<StoredIdentity>> {
        let row = sqlx::query("SELECT sealed, route_capability FROM identity WHERE id = 1").fetch_optional(&self.pool).await?;
        row.map(|row| {
            let capability: Vec<u8> = row.get("route_capability");
            Ok(StoredIdentity {
                sealed: row.get("sealed"),
                route_capability: capability.try_into().map_err(|_| anyhow::anyhow!("corrupt route capability"))?,
            })
        })
        .transpose()
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

    /// Adds a contact or, if already known, refreshes its card (keys, capability, mailbox).
    pub async fn add_contact(&self, contact: &NewContact) -> Result<()> {
        sqlx::query(
            "INSERT INTO contacts (device_id, name, card, mailbox, added_at) VALUES (?, ?, ?, ?, unixepoch())
             ON CONFLICT (device_id) DO UPDATE SET card = excluded.card, mailbox = excluded.mailbox",
        )
        .bind(&contact.device_id)
        .bind(&contact.name)
        .bind(&contact.card)
        .bind(contact.mailbox)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn contact(&self, device_id: &str) -> Result<Option<Contact>> {
        let row = sqlx::query("SELECT * FROM contacts WHERE device_id = ?").bind(device_id).fetch_optional(&self.pool).await?;
        Ok(row.as_ref().map(contact_from))
    }

    pub async fn contacts(&self) -> Result<Vec<Contact>> {
        let rows = sqlx::query("SELECT * FROM contacts ORDER BY name").fetch_all(&self.pool).await?;
        Ok(rows.iter().map(contact_from).collect())
    }

    pub async fn rename_contact(&self, device_id: &str, name: &str) -> Result<()> {
        self.update_contact("UPDATE contacts SET name = ? WHERE device_id = ?", name, device_id).await
    }

    pub async fn set_blocked(&self, device_id: &str, blocked: bool) -> Result<()> {
        sqlx::query("UPDATE contacts SET blocked = ? WHERE device_id = ?").bind(blocked).bind(device_id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn set_introduced(&self, device_id: &str) -> Result<()> {
        sqlx::query("UPDATE contacts SET introduced = 1 WHERE device_id = ?").bind(device_id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn set_contact_mailbox(&self, device_id: &str, mailbox: bool) -> Result<()> {
        sqlx::query("UPDATE contacts SET mailbox = ? WHERE device_id = ?").bind(mailbox).bind(device_id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn channel(&self, device_id: &str) -> Result<Option<String>> {
        let row = sqlx::query("SELECT channel FROM contacts WHERE device_id = ?").bind(device_id).fetch_optional(&self.pool).await?;
        Ok(row.and_then(|row| row.get("channel")))
    }

    pub async fn save_channel(&self, device_id: &str, sealed: &str) -> Result<()> {
        self.update_contact("UPDATE contacts SET channel = ? WHERE device_id = ?", sealed, device_id).await
    }

    async fn update_contact(&self, sql: &'static str, value: &str, device_id: &str) -> Result<()> {
        sqlx::query(sql).bind(value).bind(device_id).execute(&self.pool).await?;
        Ok(())
    }

    /// Returns false when the message was already there (§27): acknowledge, do not store twice.
    pub async fn insert_message(&self, message: &Message) -> Result<bool> {
        let result = sqlx::query(
            "INSERT INTO messages (message_id, contact, outgoing, body, sent_at, state) VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT (message_id) DO NOTHING",
        )
        .bind(&message.message_id)
        .bind(&message.contact)
        .bind(message.outgoing)
        .bind(&message.body)
        .bind(message.sent_at)
        .bind(message.state as i64)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn message(&self, message_id: &str) -> Result<Option<Message>> {
        let row = sqlx::query("SELECT * FROM messages WHERE message_id = ?").bind(message_id).fetch_optional(&self.pool).await?;
        Ok(row.as_ref().map(message_from))
    }

    /// The last `limit` messages with a contact, oldest first.
    pub async fn messages(&self, contact: &str, limit: i64) -> Result<Vec<Message>> {
        let rows = sqlx::query(
            "SELECT * FROM (SELECT * FROM messages WHERE contact = ? ORDER BY sent_at DESC, message_id DESC LIMIT ?)
             ORDER BY sent_at, message_id",
        )
        .bind(contact)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(message_from).collect())
    }

    /// Moves messages to `state`, never backwards.
    pub async fn advance(&self, message_ids: &[String], state: MessageState) -> Result<()> {
        for id in message_ids {
            sqlx::query("UPDATE messages SET state = ? WHERE message_id = ? AND state < ?")
                .bind(state as i64)
                .bind(id)
                .bind(state as i64)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    /// Incoming messages from `contact` not yet shown to the user.
    pub async fn unread(&self, contact: &str) -> Result<Vec<String>> {
        let rows = sqlx::query("SELECT message_id FROM messages WHERE contact = ? AND outgoing = 0 AND state < 3 ORDER BY sent_at")
            .bind(contact)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(|row| row.get("message_id")).collect())
    }

    /// Every contact with its last message and unread count, most recent first.
    pub async fn conversations(&self) -> Result<Vec<Conversation>> {
        let mut conversations = Vec::new();
        for contact in self.contacts().await? {
            let last = sqlx::query("SELECT * FROM messages WHERE contact = ? ORDER BY sent_at DESC, message_id DESC LIMIT 1")
                .bind(&contact.device_id)
                .fetch_optional(&self.pool)
                .await?
                .as_ref()
                .map(message_from);
            let unread: i64 = sqlx::query("SELECT COUNT(*) AS n FROM messages WHERE contact = ? AND outgoing = 0 AND state < 3")
                .bind(&contact.device_id)
                .fetch_one(&self.pool)
                .await?
                .get("n");
            conversations.push(Conversation { contact, last, unread });
        }
        conversations.sort_by_key(|c| std::cmp::Reverse(c.last.as_ref().map_or(i64::MIN, |m| m.sent_at)));
        Ok(conversations)
    }

    pub async fn enqueue(&self, message_id: &str, contact: &str, now: i64) -> Result<()> {
        sqlx::query("INSERT INTO pending_outbox (message_id, contact, created_at, next_attempt) VALUES (?, ?, ?, ?) ON CONFLICT DO NOTHING")
            .bind(message_id)
            .bind(contact)
            .bind(now)
            .bind(now)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Entries whose next attempt is due at `now`.
    pub async fn due(&self, now: i64) -> Result<Vec<OutboxEntry>> {
        let rows = sqlx::query("SELECT * FROM pending_outbox WHERE next_attempt <= ? ORDER BY created_at").bind(now).fetch_all(&self.pool).await?;
        Ok(rows.iter().map(outbox_from).collect())
    }

    /// Every entry still waiting for a DELIVERED receipt.
    pub async fn outbox(&self) -> Result<Vec<OutboxEntry>> {
        let rows = sqlx::query("SELECT * FROM pending_outbox ORDER BY created_at").fetch_all(&self.pool).await?;
        Ok(rows.iter().map(outbox_from).collect())
    }

    pub async fn reschedule(&self, message_id: &str, attempts: i64, next_attempt: i64, in_mailbox: bool) -> Result<()> {
        sqlx::query("UPDATE pending_outbox SET attempts = ?, next_attempt = ?, in_mailbox = ? WHERE message_id = ?")
            .bind(attempts)
            .bind(next_attempt)
            .bind(in_mailbox)
            .bind(message_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn dequeue(&self, message_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM pending_outbox WHERE message_id = ?").bind(message_id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn setting(&self, key: &str) -> Result<Option<String>> {
        let row = sqlx::query("SELECT value FROM settings WHERE key = ?").bind(key).fetch_optional(&self.pool).await?;
        Ok(row.map(|row| row.get("value")))
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

fn contact_from(row: &SqliteRow) -> Contact {
    Contact {
        device_id: row.get("device_id"),
        name: row.get("name"),
        card: row.get("card"),
        mailbox: row.get("mailbox"),
        blocked: row.get("blocked"),
        introduced: row.get("introduced"),
        added_at: row.get("added_at"),
    }
}

fn message_from(row: &SqliteRow) -> Message {
    Message {
        message_id: row.get("message_id"),
        contact: row.get("contact"),
        outgoing: row.get("outgoing"),
        body: row.get("body"),
        sent_at: row.get("sent_at"),
        state: MessageState::from_rank(row.get("state")),
    }
}

fn outbox_from(row: &SqliteRow) -> OutboxEntry {
    OutboxEntry {
        message_id: row.get("message_id"),
        contact: row.get("contact"),
        created_at: row.get("created_at"),
        attempts: row.get("attempts"),
        next_attempt: row.get("next_attempt"),
        in_mailbox: row.get("in_mailbox"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn store() -> Store {
        Store::open_in_memory().await.expect("opens")
    }

    fn contact(id: &str) -> NewContact {
        NewContact { device_id: id.to_owned(), name: "Bob".to_owned(), card: vec![1, 2], mailbox: true }
    }

    fn message(id: &str, contact: &str, outgoing: bool, sent_at: i64) -> Message {
        Message {
            message_id: id.to_owned(),
            contact: contact.to_owned(),
            outgoing,
            body: format!("text {id}"),
            sent_at,
            state: if outgoing { MessageState::Pending } else { MessageState::Delivered },
        }
    }

    #[tokio::test]
    async fn the_identity_is_stored_once_created() {
        let store = store().await;
        assert!(store.identity().await.expect("reads").is_none());
        store.save_identity("sealed", &[5; 32]).await.expect("saves");
        let saved = store.identity().await.expect("reads").expect("present");
        assert_eq!(saved.sealed, "sealed");
        assert_eq!(saved.route_capability, [5; 32]);
    }

    #[tokio::test]
    async fn contacts_are_listed_renamed_and_blocked() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.rename_contact("ft_bob", "Robert").await.expect("renames");
        store.set_blocked("ft_bob", true).await.expect("blocks");

        let bob = store.contact("ft_bob").await.expect("reads").expect("present");
        assert_eq!(bob.name, "Robert");
        assert!(bob.blocked);
        assert_eq!(store.contacts().await.expect("lists").len(), 1);
    }

    // Scanning the same card again refreshes it without losing the conversation.
    #[tokio::test]
    async fn adding_a_known_contact_again_updates_the_card() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.insert_message(&message("m1", "ft_bob", true, 1)).await.expect("inserts");
        let newer = NewContact { card: vec![9], mailbox: false, ..contact("ft_bob") };
        store.add_contact(&newer).await.expect("updates");

        let bob = store.contact("ft_bob").await.expect("reads").expect("present");
        assert_eq!(bob.card, vec![9]);
        assert!(!bob.mailbox);
        assert_eq!(store.messages("ft_bob", 10).await.expect("lists").len(), 1);
    }

    // Until a contact has written back, our Contact Card keeps travelling with our messages.
    #[tokio::test]
    async fn a_contact_is_introduced_once_it_writes_back() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        assert!(!store.contact("ft_bob").await.unwrap().unwrap().introduced);
        store.set_introduced("ft_bob").await.expect("marks");
        assert!(store.contact("ft_bob").await.unwrap().unwrap().introduced);
        store.add_contact(&contact("ft_bob")).await.expect("updates the card");
        assert!(store.contact("ft_bob").await.unwrap().unwrap().introduced, "a new card keeps the mark");
    }

    #[tokio::test]
    async fn a_message_is_found_by_its_id() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.insert_message(&message("m1", "ft_bob", true, 1)).await.expect("inserts");
        assert_eq!(store.message("m1").await.unwrap().expect("found").body, "text m1");
        assert!(store.message("nope").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn the_encrypted_channel_is_kept_per_contact() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        assert!(store.channel("ft_bob").await.expect("reads").is_none());
        store.save_channel("ft_bob", "sessions").await.expect("saves");
        assert_eq!(store.channel("ft_bob").await.expect("reads").as_deref(), Some("sessions"));
    }

    // §27: a retried message is acknowledged but stored once.
    #[tokio::test]
    async fn the_same_message_is_stored_once() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        assert!(store.insert_message(&message("m1", "ft_bob", false, 1)).await.expect("inserts"));
        assert!(!store.insert_message(&message("m1", "ft_bob", false, 1)).await.expect("ignores"));
        assert_eq!(store.messages("ft_bob", 10).await.expect("lists").len(), 1);
    }

    #[tokio::test]
    async fn messages_come_back_in_conversation_order() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        for (id, at) in [("b", 2), ("a", 1), ("c", 3)] {
            store.insert_message(&message(id, "ft_bob", true, at)).await.expect("inserts");
        }
        let ids: Vec<_> = store.messages("ft_bob", 10).await.expect("lists").into_iter().map(|m| m.message_id).collect();
        assert_eq!(ids, ["a", "b", "c"]);
    }

    // A late "delivered" receipt must not undo a "read".
    #[tokio::test]
    async fn states_only_move_forward() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.insert_message(&message("m1", "ft_bob", true, 1)).await.expect("inserts");
        store.advance(&["m1".to_owned()], MessageState::Read).await.expect("advances");
        store.advance(&["m1".to_owned()], MessageState::Delivered).await.expect("ignored");
        let state = store.messages("ft_bob", 10).await.expect("lists")[0].state;
        assert_eq!(state, MessageState::Read);
    }

    #[tokio::test]
    async fn the_outbox_retries_until_delivered() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.insert_message(&message("m1", "ft_bob", true, 1)).await.expect("inserts");
        store.enqueue("m1", "ft_bob", 100).await.expect("queues");

        let due = store.due(100).await.expect("lists");
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].attempts, 0);

        store.reschedule("m1", 1, 500, true).await.expect("reschedules");
        assert!(store.due(200).await.expect("lists").is_empty());
        let later = store.due(500).await.expect("lists");
        assert_eq!((later[0].attempts, later[0].in_mailbox), (1, true));

        store.dequeue("m1").await.expect("dequeues");
        assert!(store.due(10_000).await.expect("lists").is_empty());
    }

    #[tokio::test]
    async fn conversations_show_the_last_message_and_the_unread_count() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.add_contact(&NewContact { name: "Carol".to_owned(), ..contact("ft_carol") }).await.expect("adds");
        store.insert_message(&message("m1", "ft_bob", false, 1)).await.expect("inserts");
        store.insert_message(&message("m2", "ft_bob", false, 2)).await.expect("inserts");
        store.insert_message(&message("m3", "ft_bob", true, 3)).await.expect("inserts");
        store.advance(&["m1".to_owned()], MessageState::Read).await.expect("reads");

        let conversations = store.conversations().await.expect("lists");
        let bob = conversations.iter().find(|c| c.contact.device_id == "ft_bob").expect("bob");
        assert_eq!(bob.last.as_ref().map(|m| m.message_id.as_str()), Some("m3"));
        assert_eq!(bob.unread, 1);
        let carol = conversations.iter().find(|c| c.contact.device_id == "ft_carol").expect("carol");
        assert!(carol.last.is_none());
        assert_eq!(conversations[0].contact.device_id, "ft_bob", "most recent first");
    }

    #[tokio::test]
    async fn unread_incoming_messages_can_be_listed_for_read_receipts() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.insert_message(&message("m1", "ft_bob", false, 1)).await.expect("inserts");
        store.insert_message(&message("m2", "ft_bob", true, 2)).await.expect("inserts");
        assert_eq!(store.unread("ft_bob").await.expect("lists"), ["m1"]);
    }

    #[tokio::test]
    async fn settings_are_remembered() {
        let store = store().await;
        assert!(store.setting("mailbox").await.expect("reads").is_none());
        store.set_setting("mailbox", "0").await.expect("saves");
        assert_eq!(store.setting("mailbox").await.expect("reads").as_deref(), Some("0"));
    }

    #[tokio::test]
    async fn the_database_survives_a_restart() {
        let path = std::env::temp_dir().join(format!("ft-storage-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let store = Store::open(&path).await.expect("opens");
            store.add_contact(&contact("ft_bob")).await.expect("adds");
        }
        let store = Store::open(&path).await.expect("reopens");
        assert!(store.contact("ft_bob").await.expect("reads").is_some());
        let _ = std::fs::remove_file(&path);
    }
}
