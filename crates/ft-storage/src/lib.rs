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
    /// How long this phone keeps their messages, in seconds; 0 keeps them forever (issue app#1).
    pub keep_for: i64,
    /// How long a read message stays, in seconds after it was read; 0 never burns them.
    pub burn_after_read: i64,
}

/// A plugin installed on this phone, with what the user granted it (issue app#3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPlugin {
    pub id: String,
    pub version: String,
    /// The granted permissions, as the manifest writes them (JSON); ft-plugins reads it.
    pub granted: String,
    pub installed_at: i64,
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

/// A file sent or received (§62–63); its message carries the name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRecord {
    pub message_id: String,
    pub name: String,
    pub size: i64,
    pub mime: String,
    /// BLAKE3 of the whole file.
    pub hash: [u8; 32],
    /// Bytes per chunk.
    pub chunk: i64,
    /// Where the bytes are on this device.
    pub path: String,
    /// Outgoing: chunks sent; incoming: chunks received in order.
    pub chunks_done: i64,
    /// The receiver has the whole file and its hash matches.
    pub complete: bool,
    /// The bytes did not match the hash: the transfer was given up.
    pub failed: bool,
}

impl FileRecord {
    pub fn chunks(&self) -> i64 {
        if self.chunk <= 0 {
            return 0;
        }
        (self.size + self.chunk - 1) / self.chunk
    }
}

/// How a call ended (§66).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallOutcome {
    Answered,
    /// Incoming, and nobody answered.
    Missed,
    Declined,
    Busy,
    /// Outgoing, and the caller gave up before an answer.
    Cancelled,
    /// The contact could not be reached directly.
    Unreachable,
    /// The media could not connect.
    Failed,
}

impl CallOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Answered => "answered",
            Self::Missed => "missed",
            Self::Declined => "declined",
            Self::Busy => "busy",
            Self::Cancelled => "cancelled",
            Self::Unreachable => "unreachable",
            Self::Failed => "failed",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        [Self::Answered, Self::Missed, Self::Declined, Self::Busy, Self::Cancelled, Self::Unreachable, Self::Failed]
            .into_iter()
            .find(|outcome| outcome.as_str() == name)
    }
}

/// A call in the history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallRecord {
    pub call_id: String,
    pub contact: String,
    pub outgoing: bool,
    pub video: bool,
    pub started_at: i64,
    pub answered_at: Option<i64>,
    pub ended_at: Option<i64>,
    /// `None` while the call goes on.
    pub outcome: Option<CallOutcome>,
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

    /// Closes the database; the file can be moved or deleted afterwards.
    pub async fn close(&self) {
        self.pool.close().await;
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

    /// Removes the contact with its conversation, files, calls and pending messages.
    pub async fn remove_contact(&self, device_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM pending_outbox WHERE contact = ?").bind(device_id).execute(&self.pool).await?;
        sqlx::query("DELETE FROM contacts WHERE device_id = ?").bind(device_id).execute(&self.pool).await?;
        Ok(())
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

    /// How long this phone keeps the conversation with them, and how long a read message stays
    /// after being read (issue app#1). Both in seconds; 0 means forever and never.
    pub async fn set_history(&self, device_id: &str, keep_for: i64, burn_after_read: i64) -> Result<()> {
        sqlx::query("UPDATE contacts SET keep_for = ?, burn_after_read = ? WHERE device_id = ?")
            .bind(keep_for)
            .bind(burn_after_read)
            .bind(device_id)
            .execute(&self.pool)
            .await?;
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
        self.advance_at(message_ids, state, now()).await
    }

    /// Like `advance`, with the clock given: reading stamps `read_at`, and only the first time.
    pub async fn advance_at(&self, message_ids: &[String], state: MessageState, at: i64) -> Result<()> {
        for id in message_ids {
            sqlx::query("UPDATE messages SET state = ? WHERE message_id = ? AND state < ?")
                .bind(state as i64)
                .bind(id)
                .bind(state as i64)
                .execute(&self.pool)
                .await?;
            if state == MessageState::Read {
                sqlx::query("UPDATE messages SET read_at = ? WHERE message_id = ? AND read_at IS NULL")
                    .bind(at)
                    .bind(id)
                    .execute(&self.pool)
                    .await?;
            }
        }
        Ok(())
    }

    /// When the message was first read, in milliseconds.
    pub async fn read_at(&self, message_id: &str) -> Result<Option<i64>> {
        let row = sqlx::query("SELECT read_at FROM messages WHERE message_id = ?")
            .bind(message_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.and_then(|row| row.get("read_at")))
    }

    /// Applies the history rules (issue app#1) and returns the contacts whose conversation
    /// changed. Files of the deleted messages go with them (their rows cascade); the bytes on
    /// disk are the caller's to remove.
    pub async fn sweep(&self, now: i64) -> Result<Vec<String>> {
        let stale = sqlx::query(
            "SELECT DISTINCT m.contact FROM messages m JOIN contacts c ON c.device_id = m.contact
             WHERE (c.keep_for > 0 AND m.sent_at < ? - c.keep_for * 1000)
                OR (c.burn_after_read > 0 AND m.read_at IS NOT NULL AND m.read_at < ? - c.burn_after_read * 1000)",
        )
        .bind(now)
        .bind(now)
        .fetch_all(&self.pool)
        .await?;
        let contacts: Vec<String> = stale.iter().map(|row| row.get("contact")).collect();
        if contacts.is_empty() {
            return Ok(contacts);
        }
        sqlx::query(
            "DELETE FROM messages WHERE message_id IN (
                 SELECT m.message_id FROM messages m JOIN contacts c ON c.device_id = m.contact
                 WHERE (c.keep_for > 0 AND m.sent_at < ? - c.keep_for * 1000)
                    OR (c.burn_after_read > 0 AND m.read_at IS NOT NULL AND m.read_at < ? - c.burn_after_read * 1000)
             )",
        )
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(contacts)
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

    /// Returns false when the file was already there (a retried offer).
    pub async fn insert_file(&self, file: &FileRecord) -> Result<bool> {
        let result = sqlx::query(
            "INSERT INTO files (message_id, name, size, mime, hash, chunk, path, chunks_done, complete)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT (message_id) DO NOTHING",
        )
        .bind(&file.message_id)
        .bind(&file.name)
        .bind(file.size)
        .bind(&file.mime)
        .bind(file.hash.as_slice())
        .bind(file.chunk)
        .bind(&file.path)
        .bind(file.chunks_done)
        .bind(file.complete)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn file(&self, message_id: &str) -> Result<Option<FileRecord>> {
        let row = sqlx::query("SELECT * FROM files WHERE message_id = ?").bind(message_id).fetch_optional(&self.pool).await?;
        row.as_ref().map(file_from).transpose()
    }

    /// The files exchanged with a contact.
    pub async fn files(&self, contact: &str) -> Result<Vec<FileRecord>> {
        let rows = sqlx::query("SELECT files.* FROM files JOIN messages USING (message_id) WHERE messages.contact = ? ORDER BY sent_at")
            .bind(contact)
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(file_from).collect()
    }

    pub async fn set_file_progress(&self, message_id: &str, chunks_done: i64, complete: bool) -> Result<()> {
        sqlx::query("UPDATE files SET chunks_done = ?, complete = ? WHERE message_id = ?")
            .bind(chunks_done)
            .bind(complete)
            .bind(message_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// The received bytes did not match the hash: the transfer is given up.
    pub async fn set_file_failed(&self, message_id: &str) -> Result<()> {
        sqlx::query("UPDATE files SET failed = 1, complete = 0, chunks_done = 0 WHERE message_id = ?")
            .bind(message_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Incoming files still missing chunks, with their sender: (contact, file).
    pub async fn incomplete_incoming_files(&self) -> Result<Vec<(String, FileRecord)>> {
        let rows = sqlx::query(
            "SELECT files.*, messages.contact FROM files JOIN messages USING (message_id)
             WHERE messages.outgoing = 0 AND files.complete = 0 AND files.failed = 0 ORDER BY sent_at",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(|row| Ok((row.get("contact"), file_from(row)?))).collect()
    }

    /// Returns false when the call was already logged (a repeated offer).
    pub async fn insert_call(&self, call: &CallRecord) -> Result<bool> {
        let result = sqlx::query(
            "INSERT INTO calls (call_id, contact, outgoing, video, started_at, answered_at, ended_at, outcome)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT (call_id) DO NOTHING",
        )
        .bind(&call.call_id)
        .bind(&call.contact)
        .bind(call.outgoing)
        .bind(call.video)
        .bind(call.started_at)
        .bind(call.answered_at)
        .bind(call.ended_at)
        .bind(call.outcome.map(CallOutcome::as_str))
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn call(&self, call_id: &str) -> Result<Option<CallRecord>> {
        let row = sqlx::query("SELECT * FROM calls WHERE call_id = ?").bind(call_id).fetch_optional(&self.pool).await?;
        Ok(row.as_ref().map(call_from))
    }

    /// The last `limit` calls, newest first.
    pub async fn calls(&self, limit: i64) -> Result<Vec<CallRecord>> {
        let rows = sqlx::query("SELECT * FROM calls ORDER BY started_at DESC, call_id DESC LIMIT ?").bind(limit).fetch_all(&self.pool).await?;
        Ok(rows.iter().map(call_from).collect())
    }

    pub async fn answer_call(&self, call_id: &str, at: i64) -> Result<()> {
        sqlx::query("UPDATE calls SET answered_at = ? WHERE call_id = ? AND answered_at IS NULL")
            .bind(at)
            .bind(call_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn finish_call(&self, call_id: &str, at: i64, outcome: CallOutcome) -> Result<()> {
        sqlx::query("UPDATE calls SET ended_at = ?, outcome = ? WHERE call_id = ? AND ended_at IS NULL")
            .bind(at)
            .bind(outcome.as_str())
            .bind(call_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Closes the calls a stopped app left open, as what they were by then.
    pub async fn finish_open_calls(&self, at: i64) -> Result<()> {
        sqlx::query(
            "UPDATE calls SET ended_at = ?, outcome = CASE
                 WHEN answered_at IS NOT NULL THEN 'answered'
                 WHEN outgoing = 1 THEN 'cancelled'
                 ELSE 'missed' END
             WHERE ended_at IS NULL",
        )
        .bind(at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Writes a consistent copy of the whole database to `path`, which must not exist (§60).
    /// Installs a plugin, or moves it to a new version. An update keeps what was granted: the
    /// user said yes to this plugin, not to this build of it (§53).
    pub async fn install_plugin(&self, id: &str, version: &str, granted: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO plugins (id, version, granted, installed_at) VALUES (?, ?, ?, ?)
             ON CONFLICT (id) DO UPDATE SET version = excluded.version",
        )
        .bind(id)
        .bind(version)
        .bind(granted)
        .bind(now())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// What the user grants, and revokes, at any time.
    pub async fn grant_plugin(&self, id: &str, granted: &str) -> Result<()> {
        sqlx::query("UPDATE plugins SET granted = ? WHERE id = ?").bind(granted).bind(id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn plugin(&self, id: &str) -> Result<Option<InstalledPlugin>> {
        let row = sqlx::query("SELECT * FROM plugins WHERE id = ?").bind(id).fetch_optional(&self.pool).await?;
        Ok(row.as_ref().map(plugin_from))
    }

    pub async fn plugins(&self) -> Result<Vec<InstalledPlugin>> {
        let rows = sqlx::query("SELECT * FROM plugins ORDER BY id").fetch_all(&self.pool).await?;
        Ok(rows.iter().map(plugin_from).collect())
    }

    pub async fn remove_plugin(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM plugins WHERE id = ?").bind(id).execute(&self.pool).await?;
        sqlx::query("DELETE FROM plugin_memory WHERE plugin = ?").bind(id).execute(&self.pool).await?;
        Ok(())
    }

    /// What a plugin left under that key, if anything (§53).
    pub async fn plugin_value(&self, plugin: &str, key: &str) -> Result<Option<String>> {
        let row = sqlx::query("SELECT value FROM plugin_memory WHERE plugin = ? AND key = ?")
            .bind(plugin)
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| row.get::<String, _>("value")))
    }

    pub async fn set_plugin_value(&self, plugin: &str, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO plugin_memory (plugin, key, value) VALUES (?, ?, ?)
             ON CONFLICT (plugin, key) DO UPDATE SET value = excluded.value",
        )
        .bind(plugin)
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Everything that plugin remembers, by key, in a stable order.
    pub async fn plugin_keys(&self, plugin: &str) -> Result<Vec<String>> {
        let rows = sqlx::query("SELECT key FROM plugin_memory WHERE plugin = ? ORDER BY key")
            .bind(plugin)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(|row| row.get::<String, _>("key")).collect())
    }

    pub async fn forget_plugin_value(&self, plugin: &str, key: &str) -> Result<()> {
        sqlx::query("DELETE FROM plugin_memory WHERE plugin = ? AND key = ?")
            .bind(plugin)
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn snapshot(&self, path: &Path) -> Result<()> {
        let target = path.to_str().context("the snapshot path is not valid text")?;
        sqlx::query("VACUUM INTO ?").bind(target).execute(&self.pool).await.context("cannot copy the database")?;
        if !path.exists() {
            anyhow::bail!("the database copy was not written");
        }
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

/// Milliseconds since the epoch, this device's clock.
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis() as i64)
        .unwrap_or_default()
}

fn plugin_from(row: &SqliteRow) -> InstalledPlugin {
    InstalledPlugin {
        id: row.get("id"),
        version: row.get("version"),
        granted: row.get("granted"),
        installed_at: row.get("installed_at"),
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
        keep_for: row.get("keep_for"),
        burn_after_read: row.get("burn_after_read"),
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

fn file_from(row: &SqliteRow) -> Result<FileRecord> {
    let hash: Vec<u8> = row.get("hash");
    Ok(FileRecord {
        message_id: row.get("message_id"),
        name: row.get("name"),
        size: row.get("size"),
        mime: row.get("mime"),
        hash: hash.try_into().map_err(|_| anyhow::anyhow!("a stored file hash is not 32 bytes"))?,
        chunk: row.get("chunk"),
        path: row.get("path"),
        chunks_done: row.get("chunks_done"),
        complete: row.get("complete"),
        failed: row.get("failed"),
    })
}

fn call_from(row: &SqliteRow) -> CallRecord {
    let outcome: Option<String> = row.get("outcome");
    CallRecord {
        call_id: row.get("call_id"),
        contact: row.get("contact"),
        outgoing: row.get("outgoing"),
        video: row.get("video"),
        started_at: row.get("started_at"),
        answered_at: row.get("answered_at"),
        ended_at: row.get("ended_at"),
        outcome: outcome.as_deref().and_then(CallOutcome::parse),
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

    fn file(id: &str) -> FileRecord {
        FileRecord {
            message_id: id.to_owned(),
            name: "photo.jpg".to_owned(),
            size: 100_000,
            mime: "image/jpeg".to_owned(),
            hash: [3; 32],
            chunk: 49_152,
            path: format!("/files/{id}"),
            chunks_done: 0,
            complete: false,
            failed: false,
        }
    }

    // §62–63: a file message keeps its metadata, where its bytes are and how far it got.
    #[tokio::test]
    async fn a_file_travels_with_its_message() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.insert_message(&message("f1", "ft_bob", false, 1)).await.expect("inserts");
        store.insert_message(&message("m2", "ft_bob", false, 2)).await.expect("inserts");
        assert!(store.insert_file(&file("f1")).await.expect("inserts the file"));
        assert!(!store.insert_file(&file("f1")).await.expect("ignores it again"), "a retried offer is stored once");

        let stored = store.file("f1").await.expect("reads").expect("present");
        assert_eq!(stored, file("f1"));
        assert_eq!(stored.chunks(), 3, "100 000 bytes in chunks of 48 KiB");
        assert!(store.file("m2").await.expect("reads").is_none(), "a text is not a file");

        store.set_file_progress("f1", 2, false).await.expect("updates");
        assert_eq!(store.file("f1").await.unwrap().unwrap().chunks_done, 2);
        assert_eq!(store.files("ft_bob").await.expect("lists").len(), 1);
    }

    // Incoming files still missing chunks are resumed when the sender is back (§63).
    #[tokio::test]
    async fn incomplete_incoming_files_are_listed_for_resuming() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.insert_message(&message("in", "ft_bob", false, 1)).await.unwrap();
        store.insert_message(&message("out", "ft_bob", true, 2)).await.unwrap();
        store.insert_message(&message("done", "ft_bob", false, 3)).await.unwrap();
        for id in ["in", "out", "done"] {
            store.insert_file(&file(id)).await.unwrap();
        }
        store.set_file_progress("done", 3, true).await.unwrap();
        store.insert_message(&message("bad", "ft_bob", false, 4)).await.unwrap();
        store.insert_file(&file("bad")).await.unwrap();
        store.set_file_failed("bad").await.expect("marks it failed");
        assert!(store.file("bad").await.unwrap().unwrap().failed);

        let waiting = store.incomplete_incoming_files().await.expect("lists");
        assert_eq!(waiting.iter().map(|(contact, file)| (contact.as_str(), file.message_id.as_str())).collect::<Vec<_>>(), [("ft_bob", "in")]);
    }

    #[test]
    fn an_empty_file_has_no_chunks() {
        assert_eq!(FileRecord { size: 0, ..file("x") }.chunks(), 0);
        assert_eq!(FileRecord { size: 49_152, ..file("x") }.chunks(), 1);
    }

    fn call(id: &str, contact: &str, outgoing: bool, started_at: i64) -> CallRecord {
        CallRecord {
            call_id: id.to_owned(),
            contact: contact.to_owned(),
            outgoing,
            video: true,
            started_at,
            answered_at: None,
            ended_at: None,
            outcome: None,
        }
    }

    // §66: the call history lives only on the phone.
    #[tokio::test]
    async fn calls_are_logged_from_ringing_to_their_end() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        assert!(store.insert_call(&call("c1", "ft_bob", true, 10)).await.expect("logs"));
        assert!(!store.insert_call(&call("c1", "ft_bob", true, 10)).await.expect("once"), "a repeated offer is logged once");
        store.answer_call("c1", 12).await.expect("answers");
        store.finish_call("c1", 70, CallOutcome::Answered).await.expect("ends");

        let logged = store.call("c1").await.expect("reads").expect("present");
        assert_eq!((logged.answered_at, logged.ended_at, logged.outcome), (Some(12), Some(70), Some(CallOutcome::Answered)));
        assert!(store.call("nope").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn the_call_history_is_newest_first() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        for (id, at) in [("old", 1), ("new", 3), ("mid", 2)] {
            store.insert_call(&call(id, "ft_bob", false, at)).await.unwrap();
        }
        let ids: Vec<_> = store.calls(2).await.expect("lists").into_iter().map(|c| c.call_id).collect();
        assert_eq!(ids, ["new", "mid"]);
    }

    // A call cut short by the app stopping is closed at the next start, as what it was by then.
    #[tokio::test]
    async fn calls_left_open_are_closed() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.insert_call(&call("talking", "ft_bob", true, 1)).await.unwrap();
        store.answer_call("talking", 2).await.unwrap();
        store.insert_call(&call("calling", "ft_bob", true, 3)).await.unwrap();
        store.insert_call(&call("ringing", "ft_bob", false, 4)).await.unwrap();
        store.insert_call(&call("done", "ft_bob", false, 5)).await.unwrap();
        store.finish_call("done", 6, CallOutcome::Declined).await.unwrap();

        store.finish_open_calls(9).await.expect("closes");
        let outcome = |id: &'static str| {
            let store = &store;
            async move { store.call(id).await.unwrap().unwrap().outcome }
        };
        assert_eq!(outcome("talking").await, Some(CallOutcome::Answered));
        assert_eq!(outcome("calling").await, Some(CallOutcome::Cancelled));
        assert_eq!(outcome("ringing").await, Some(CallOutcome::Missed));
        assert_eq!(outcome("done").await, Some(CallOutcome::Declined), "finished calls are left alone");
    }

    #[test]
    fn call_outcomes_have_stable_names() {
        for outcome in [
            CallOutcome::Answered,
            CallOutcome::Missed,
            CallOutcome::Declined,
            CallOutcome::Busy,
            CallOutcome::Cancelled,
            CallOutcome::Unreachable,
            CallOutcome::Failed,
        ] {
            assert_eq!(CallOutcome::parse(outcome.as_str()), Some(outcome));
        }
        assert_eq!(CallOutcome::Unreachable.as_str(), "unreachable");
    }

    // §60: moving to a new phone sends a consistent copy of the whole database.
    #[tokio::test]
    async fn a_snapshot_is_a_complete_copy() {
        let path = std::env::temp_dir().join(format!("ft-storage-snapshot-{}.db", std::process::id()));
        let source = path.with_extension("source.db");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&source);
        // On disk, as in the app: an in-memory database copies into memory too.
        let store = Store::open(&source).await.expect("opens");
        store.add_contact(&contact("ft_bob")).await.unwrap();
        store.insert_message(&message("m1", "ft_bob", true, 1)).await.unwrap();
        store.set_setting("name", "Alice").await.unwrap();

        store.snapshot(&path).await.expect("copies");
        let copy = Store::open(&path).await.expect("opens the copy");
        assert_eq!(copy.contacts().await.unwrap().len(), 1);
        assert_eq!(copy.messages("ft_bob", 10).await.unwrap()[0].message_id, "m1");
        assert_eq!(copy.setting("name").await.unwrap().as_deref(), Some("Alice"));
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&source);
    }

    // Removing a contact takes its conversation with it.
    #[tokio::test]
    async fn a_removed_contact_leaves_nothing_behind() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.unwrap();
        store.add_contact(&contact("ft_carol")).await.unwrap();
        store.insert_message(&message("m1", "ft_bob", true, 1)).await.unwrap();
        store.enqueue("m1", "ft_bob", 1).await.unwrap();
        store.remove_contact("ft_bob").await.expect("removes");
        assert!(store.contact("ft_bob").await.unwrap().is_none());
        assert!(store.message("m1").await.unwrap().is_none());
        assert!(store.outbox().await.unwrap().is_empty());
        assert!(store.contact("ft_carol").await.unwrap().is_some());
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

    // Issue #1: each contact can have its history expire, and read messages can disappear a few
    // minutes later. Both are choices of this phone: nothing of it travels.
    #[tokio::test]
    async fn keeps_a_history_rule_for_each_contact() {
        let store = store().await;
        store.add_contact(&contact("bob")).await.unwrap();
        let bob = store.contact("bob").await.unwrap().unwrap();
        assert_eq!((bob.keep_for, bob.burn_after_read), (0, 0), "forever, and no burning, by default");

        store.set_history("bob", 7 * 86_400, 300).await.unwrap();
        let bob = store.contact("bob").await.unwrap().unwrap();
        assert_eq!((bob.keep_for, bob.burn_after_read), (7 * 86_400, 300));
    }

    #[tokio::test]
    async fn stamps_when_a_message_was_read() {
        let store = store().await;
        store.add_contact(&contact("bob")).await.unwrap();
        store.insert_message(&message("m1", "bob", false, 1_000)).await.unwrap();
        assert_eq!(store.read_at("m1").await.unwrap(), None);

        store.advance_at(&["m1".to_owned()], MessageState::Read, 5_000).await.unwrap();
        assert_eq!(store.read_at("m1").await.unwrap(), Some(5_000));
        store.advance_at(&["m1".to_owned()], MessageState::Read, 9_000).await.unwrap();
        assert_eq!(store.read_at("m1").await.unwrap(), Some(5_000), "the first reading is the one");
    }

    #[tokio::test]
    async fn sweeps_old_messages_and_read_ones() {
        let store = store().await;
        store.add_contact(&contact("bob")).await.unwrap();
        store.add_contact(&NewContact { device_id: "carol".to_owned(), ..contact("carol") }).await.unwrap();
        // Bob: a week of history, and read messages gone five minutes later.
        store.set_history("bob", 7 * 86_400, 300).await.unwrap();

        let day = 86_400_000;
        let now = 30 * day;
        store.insert_message(&message("old", "bob", false, now - 8 * day)).await.unwrap();
        store.insert_message(&message("recent", "bob", false, now - day)).await.unwrap();
        store.insert_message(&message("read", "bob", false, now - 1000)).await.unwrap();
        store.insert_file(&file("old")).await.unwrap();
        store.insert_message(&message("ancient", "carol", false, now - 400 * day)).await.unwrap();
        store.advance_at(&["read".to_owned()], MessageState::Read, now - 600_000).await.unwrap();

        let gone = store.sweep(now).await.unwrap();
        assert_eq!(gone, vec!["bob".to_owned()], "only bob has a rule");
        let left: Vec<String> = store.messages("bob", 50).await.unwrap().into_iter().map(|m| m.message_id).collect();
        assert_eq!(left, vec!["recent".to_owned()], "the old one and the read one are gone");
        assert!(store.file("old").await.unwrap().is_none(), "its file row goes with it");
        assert_eq!(store.messages("carol", 50).await.unwrap().len(), 1, "carol keeps everything");
    }

    // Issue app#3: what the user granted each plugin lives on the phone, next to the plugin.
    // Installing grants nothing: the grants start as the plugin was installed with (§53).
    #[tokio::test]
    async fn remembers_which_plugins_are_installed_and_what_they_were_granted() {
        let store = store().await;
        assert!(store.plugins().await.unwrap().is_empty());

        store.install_plugin("com.example.code", "1.0.0", "{}").await.unwrap();
        store.install_plugin("com.example.ai", "2.1.0", r#"{"network":[]}"#).await.unwrap();
        let listed = store.plugins().await.unwrap();
        assert_eq!(
            listed.iter().map(|p| (p.id.as_str(), p.version.as_str())).collect::<Vec<_>>(),
            [("com.example.ai", "2.1.0"), ("com.example.code", "1.0.0")],
            "in a stable order"
        );
        assert!(listed[0].installed_at > 0);

        store.grant_plugin("com.example.ai", r#"{"network":["api.openai.com"]}"#).await.unwrap();
        let ai = store.plugin("com.example.ai").await.unwrap().expect("installed");
        assert_eq!(ai.granted, r#"{"network":["api.openai.com"]}"#);
        assert_eq!(ai.version, "2.1.0", "granting does not touch the version");

        // Installing again is an update: the version moves, what was granted stays.
        store.install_plugin("com.example.ai", "2.2.0", r#"{"network":[]}"#).await.unwrap();
        let ai = store.plugin("com.example.ai").await.unwrap().expect("installed");
        assert_eq!((ai.version.as_str(), ai.granted.as_str()), ("2.2.0", r#"{"network":["api.openai.com"]}"#));

        store.remove_plugin("com.example.ai").await.unwrap();
        assert_eq!(store.plugins().await.unwrap().len(), 1);
    }

    // A plugin runs in a frame with no origin of its own, so it has no storage of its own either:
    // what it wants to remember it leaves here, apart from every other plugin (§53).
    #[tokio::test]
    async fn keeps_what_each_plugin_remembers_apart_from_the_others() {
        let store = store().await;
        assert_eq!(store.plugin_value("com.example.code", "pen").await.unwrap(), None);

        store.set_plugin_value("com.example.code", "pen", "black").await.unwrap();
        store.set_plugin_value("com.example.ai", "pen", "blue").await.unwrap();
        assert_eq!(store.plugin_value("com.example.code", "pen").await.unwrap().as_deref(), Some("black"));
        assert_eq!(store.plugin_value("com.example.ai", "pen").await.unwrap().as_deref(), Some("blue"));

        store.set_plugin_value("com.example.code", "pen", "red").await.unwrap();
        store.set_plugin_value("com.example.code", "size", "3").await.unwrap();
        assert_eq!(store.plugin_value("com.example.code", "pen").await.unwrap().as_deref(), Some("red"));
        assert_eq!(store.plugin_keys("com.example.code").await.unwrap(), ["pen", "size"]);

        // Taking a plugin out takes what it remembered with it.
        store.install_plugin("com.example.code", "1.0.0", "{}").await.unwrap();
        store.remove_plugin("com.example.code").await.unwrap();
        assert_eq!(store.plugin_keys("com.example.code").await.unwrap(), Vec::<String>::new());
    }
}
