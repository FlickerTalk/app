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
    /// The router refused it for good (a 403): out of the outbox until the user sends it again
    /// (§84). Below `Pending`, so sending again and any late receipt move it forward; the column
    /// already held integers, so rows from before keep their state.
    NotSent = -1,
    Pending = 0,
    /// In the recipient's mailbox or handed to the DataChannel.
    Sent = 1,
    Delivered = 2,
    Read = 3,
}

impl MessageState {
    fn from_rank(rank: i64) -> Self {
        match rank {
            -1 => Self::NotSent,
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
    /// The envelope key, sealed at rest (A1); `None` on a database from before it.
    pub envelope: Option<String>,
}

pub struct NewContact {
    pub device_id: String,
    pub name: String,
    pub card: Vec<u8>,
    pub mailbox: bool,
    /// The hidden session the contact is added to; `None` for the main list.
    pub session: Option<String>,
    /// Whether they are told their messages arrived and were read (the Settings default).
    pub receipts: bool,
    /// Whether the user chose them (scanned, or accepted a request). Someone who wrote first
    /// with our link waits in the requests until then (A5).
    pub accepted: bool,
}

/// What this phone takes from a contact and what it tells them (issues app#4–#6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContactRules {
    /// Their calls show but neither ring nor vibrate.
    pub muted: bool,
    /// Their messages and files are kept.
    pub accepts_chat: bool,
    /// Their calls come in.
    pub accepts_calls: bool,
    /// They see their messages as delivered and read.
    pub receipts: bool,
    /// They see when the user is writing to them (2026-10-05); only over the direct connection.
    pub typing: bool,
}

impl Default for ContactRules {
    fn default() -> Self {
        Self { muted: false, accepts_chat: true, accepts_calls: true, receipts: true, typing: true }
    }
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
    /// The hidden session this contact belongs to; `None` for the main list.
    pub session: Option<String>,
    pub rules: ContactRules,
    /// `false` while they wait in the requests (A5): hidden, silent, no files, no calls.
    pub accepted: bool,
    /// Known only because they are in a circle with this phone: in no list, in no requests
    /// unless they write on their own.
    pub via_circle: bool,
}

/// A local reminder a plugin set (2026-09-27).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reminder {
    pub plugin: String,
    /// The hidden session it was set in (2026-10-01, §108); `None` for the main list.
    pub session: Option<String>,
    pub id: String,
    /// Milliseconds since the epoch.
    pub at: i64,
    /// What the notification says; empty for the generic one.
    pub text: String,
}

/// Where a plugin's opaque `ref` points (2026-09-27).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRef {
    pub plugin: String,
    pub contact: String,
    pub message_id: String,
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
    /// The sender's clock, as it said (M5): shown, never trusted for keeping or ordering.
    pub sent_at: i64,
    /// This phone's clock when the message was stored: what history and order go by (M5).
    pub received_at: i64,
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
    /// When the last copy went to the mailbox (ms); `None` if none did since this was kept.
    pub mailed_at: Option<i64>,
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
    /// Incoming and bigger than the user downloads on their own: not a byte is asked for until
    /// they say so (A4).
    pub waiting: bool,
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

/// A circle as this phone holds it: its current signed card and what the lists need of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CircleRecord {
    pub id: String,
    pub name: String,
    pub card: Vec<u8>,
    pub revision: i64,
    pub admins_only: bool,
    /// The hidden session it belongs to; `None` for the main list.
    pub session: Option<String>,
    /// This phone left, or was taken out: what was said stays, nothing more comes or goes.
    pub left: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CircleMember {
    pub device_id: String,
    /// As their own card names them.
    pub name: String,
    pub admin: bool,
}

/// Something said or done in a circle. `kind` is `text`, an event (`created`, `joined`, `left`,
/// `removed`, `renamed`) whose body names who or what, or a control packet (`card`, `leave`)
/// that only exists to go through the outbox and is never shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CircleMessage {
    pub message_id: String,
    pub circle: String,
    /// The device that said it; this phone's own id when outgoing.
    pub sender: String,
    pub outgoing: bool,
    pub kind: String,
    pub body: String,
    pub sent_at: i64,
    pub received_at: i64,
    pub state: MessageState,
}

/// What still has to reach one member of a circle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CircleOutboxEntry {
    pub message_id: String,
    pub contact: String,
    pub created_at: i64,
    pub attempts: i64,
    pub next_attempt: i64,
    pub in_mailbox: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CircleConversation {
    pub circle: CircleRecord,
    pub members: i64,
    pub last: Option<CircleMessage>,
    pub unread: i64,
}


/// The pool is shared: a clone is the same database.
#[derive(Clone)]
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

    /// Keeps the envelope key, sealed (A1).
    pub async fn save_envelope(&self, sealed: &str) -> Result<()> {
        sqlx::query("UPDATE identity SET envelope = ? WHERE id = 1").bind(sealed).execute(&self.pool).await?;
        Ok(())
    }

    /// Drops the envelope key, as a database from before it has none: for the tests of the
    /// upgrade, which must hand every contact the new card.
    pub async fn forget_envelope(&self) -> Result<()> {
        sqlx::query("UPDATE identity SET envelope = NULL WHERE id = 1").execute(&self.pool).await?;
        Ok(())
    }

    /// A renewed link (A5): the device's own route capability changes.
    pub async fn set_route_capability(&self, route_capability: &[u8; 32]) -> Result<()> {
        sqlx::query("UPDATE identity SET route_capability = ? WHERE id = 1")
            .bind(route_capability.as_slice())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// A renewed link of a hidden session (A5): its slot gets a fresh capability.
    pub async fn set_spare_capability(&self, slot: u8, capability: &[u8; 32]) -> Result<()> {
        sqlx::query("UPDATE spare_capabilities SET capability = ? WHERE slot = ?")
            .bind(capability.as_slice())
            .bind(slot)
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

    /// Adds a contact or, if already known, refreshes its card (keys, capability, mailbox).
    /// Adds the contact, or refreshes the card of a known one. A known contact keeps its name and
    /// stays in the list or session where it was.
    /// Accepting is one way: a known contact that the user chose stays chosen (A5).
    pub async fn add_contact(&self, contact: &NewContact) -> Result<()> {
        sqlx::query(
            "INSERT INTO contacts (device_id, name, card, mailbox, added_at, session, receipts, accepted) VALUES (?, ?, ?, ?, unixepoch(), ?, ?, ?)
             ON CONFLICT (device_id) DO UPDATE SET card = excluded.card, mailbox = excluded.mailbox,
             accepted = MAX(contacts.accepted, excluded.accepted)",
        )
        .bind(&contact.device_id)
        .bind(&contact.name)
        .bind(&contact.card)
        .bind(contact.mailbox)
        .bind(&contact.session)
        .bind(contact.receipts)
        .bind(contact.accepted)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Only the card of a known contact, as they send it in a `ContactCard` packet.
    pub async fn refresh_card(&self, device_id: &str, card: &[u8]) -> Result<()> {
        sqlx::query("UPDATE contacts SET card = ? WHERE device_id = ?").bind(card).bind(device_id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn set_accepted(&self, device_id: &str, accepted: bool) -> Result<()> {
        sqlx::query("UPDATE contacts SET accepted = ? WHERE device_id = ?").bind(accepted).bind(device_id).execute(&self.pool).await?;
        Ok(())
    }

    /// The strangers who wrote first and wait for a yes (A5): of the main list, or of a session.
    pub async fn requests(&self, session: Option<&str>) -> Result<Vec<Contact>> {
        let rows = match session {
            None => sqlx::query(
                "SELECT * FROM contacts WHERE accepted = 0 AND blocked = 0 AND session IS NULL
                   AND (via_circle = 0 OR EXISTS (SELECT 1 FROM messages WHERE contact = device_id)) ORDER BY added_at DESC",
            )
            .fetch_all(&self.pool)
            .await?,
            Some(session) => sqlx::query(
                "SELECT * FROM contacts WHERE accepted = 0 AND blocked = 0 AND session = ?
                   AND (via_circle = 0 OR EXISTS (SELECT 1 FROM messages WHERE contact = device_id)) ORDER BY added_at DESC",
            )
            .bind(session)
            .fetch_all(&self.pool)
            .await?,
        };
        Ok(rows.iter().map(contact_from).collect())
    }

    /// The conversations waiting in the requests, newest first.
    pub async fn request_conversations(&self, session: Option<&str>) -> Result<Vec<Conversation>> {
        self.conversations_of(self.requests(session).await?).await
    }

    pub async fn set_rules(&self, device_id: &str, rules: &ContactRules) -> Result<()> {
        sqlx::query("UPDATE contacts SET muted = ?, accepts_chat = ?, accepts_calls = ?, receipts = ?, typing = ? WHERE device_id = ?")
            .bind(rules.muted)
            .bind(rules.accepts_chat)
            .bind(rules.accepts_calls)
            .bind(rules.receipts)
            .bind(rules.typing)
            .bind(device_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Creates a hidden session under the keyed hash of its PIN; fails if another has that hash.
    pub async fn add_session(&self, id: &str, pin_hash: &[u8; 32], slot: u8) -> Result<()> {
        sqlx::query("INSERT INTO sessions (id, pin_hash, created_at, slot) VALUES (?, ?, unixepoch(), ?)")
            .bind(id)
            .bind(pin_hash.as_slice())
            .bind(slot)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// The slot of a session's route capability (app#9); `None` for one made before slots.
    pub async fn session_slot(&self, id: &str) -> Result<Option<u8>> {
        let row = sqlx::query("SELECT slot FROM sessions WHERE id = ?").bind(id).fetch_optional(&self.pool).await?;
        Ok(row.and_then(|row| row.get::<Option<u8>, _>("slot")))
    }

    pub async fn set_session_slot(&self, id: &str, slot: u8) -> Result<()> {
        sqlx::query("UPDATE sessions SET slot = ? WHERE id = ?").bind(slot).bind(id).execute(&self.pool).await?;
        Ok(())
    }

    /// The session holding a slot, if any.
    pub async fn session_with_slot(&self, slot: u8) -> Result<Option<String>> {
        let row = sqlx::query("SELECT id FROM sessions WHERE slot = ?").bind(slot).fetch_optional(&self.pool).await?;
        Ok(row.map(|row| row.get("id")))
    }

    /// The slots sessions have taken, in order.
    pub async fn used_slots(&self) -> Result<Vec<u8>> {
        let rows = sqlx::query("SELECT slot FROM sessions WHERE slot IS NOT NULL ORDER BY slot").fetch_all(&self.pool).await?;
        Ok(rows.iter().map(|row| row.get::<u8, _>("slot")).collect())
    }

    /// The seven spare route capabilities (app#9), by slot.
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

    /// The session whose PIN has this hash, if any.
    pub async fn session_by_pin(&self, pin_hash: &[u8; 32]) -> Result<Option<String>> {
        let row = sqlx::query("SELECT id FROM sessions WHERE pin_hash = ?").bind(pin_hash.as_slice()).fetch_optional(&self.pool).await?;
        Ok(row.map(|row| row.get("id")))
    }

    pub async fn contact(&self, device_id: &str) -> Result<Option<Contact>> {
        let row = sqlx::query("SELECT * FROM contacts WHERE device_id = ?").bind(device_id).fetch_optional(&self.pool).await?;
        Ok(row.as_ref().map(contact_from))
    }

    /// The contacts of the main list: a session's contacts are listed through it, and the
    /// strangers waiting in the requests through `requests`.
    pub async fn contacts(&self) -> Result<Vec<Contact>> {
        let rows = sqlx::query("SELECT * FROM contacts WHERE session IS NULL AND accepted = 1 ORDER BY name").fetch_all(&self.pool).await?;
        Ok(rows.iter().map(contact_from).collect())
    }

    /// Every contact there is, chosen or not, in every session: what a broadcast reaches.
    pub async fn all_contacts(&self) -> Result<Vec<Contact>> {
        let rows = sqlx::query("SELECT * FROM contacts ORDER BY name").fetch_all(&self.pool).await?;
        Ok(rows.iter().map(contact_from).collect())
    }

    /// The contacts of a hidden session.
    pub async fn session_contacts(&self, session: &str) -> Result<Vec<Contact>> {
        let rows = sqlx::query("SELECT * FROM contacts WHERE session = ? AND accepted = 1 ORDER BY name")
            .bind(session)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(contact_from).collect())
    }

    /// Takes a hidden session away with everything in it: its contacts (and so their messages,
    /// files and calls, which cascade), what plugins kept in it (cascades too), the refs plugins
    /// were handed to its messages, and its slot (A3). The bytes of the files are the caller's.
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
        Ok(())
    }

    /// The sessions with nothing in them (A3): no contact, not even a stranger waiting for a
    /// yes, and no circle.
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
    /// `received_at` is stamped by this phone's clock, now (M5).
    pub async fn insert_message(&self, message: &Message) -> Result<bool> {
        let result = sqlx::query(
            "INSERT INTO messages (message_id, contact, outgoing, body, sent_at, received_at, state) VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (message_id) DO NOTHING",
        )
        .bind(&message.message_id)
        .bind(&message.contact)
        .bind(message.outgoing)
        .bind(&message.body)
        .bind(message.sent_at)
        .bind(if message.received_at > 0 { message.received_at } else { now() })
        .bind(message.state as i64)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn message(&self, message_id: &str) -> Result<Option<Message>> {
        let row = sqlx::query("SELECT * FROM messages WHERE message_id = ?").bind(message_id).fetch_optional(&self.pool).await?;
        Ok(row.as_ref().map(message_from))
    }

    /// The last `limit` messages with a contact, oldest first, in the order they reached this
    /// phone (M5): a sender's clock cannot push a message into the past.
    pub async fn messages(&self, contact: &str, limit: i64) -> Result<Vec<Message>> {
        let rows = sqlx::query(
            "SELECT * FROM (SELECT * FROM messages WHERE contact = ? ORDER BY received_at DESC, message_id DESC LIMIT ?)
             ORDER BY received_at, message_id",
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
    /// disk are the caller's to remove. Age counts from when the message reached this phone
    /// (M5), and a message still waiting to be delivered is never swept (M6).
    pub async fn sweep(&self, now: i64) -> Result<Vec<String>> {
        let stale = sqlx::query(
            "SELECT DISTINCT m.contact FROM messages m JOIN contacts c ON c.device_id = m.contact
             WHERE m.message_id NOT IN (SELECT message_id FROM pending_outbox)
               AND ((c.keep_for > 0 AND m.received_at < ? - c.keep_for * 1000)
                OR (c.burn_after_read > 0 AND m.read_at IS NOT NULL AND m.read_at < ? - c.burn_after_read * 1000))",
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
                 WHERE m.message_id NOT IN (SELECT message_id FROM pending_outbox)
                   AND ((c.keep_for > 0 AND m.received_at < ? - c.keep_for * 1000)
                    OR (c.burn_after_read > 0 AND m.read_at IS NOT NULL AND m.read_at < ? - c.burn_after_read * 1000))
             )",
        )
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(contacts)
    }

    /// Erases one message from this phone, with its file and its place in the outbox (§61). Says
    /// whether there was one to erase.
    pub async fn forget_message(&self, message_id: &str) -> Result<bool> {
        let gone = sqlx::query("DELETE FROM messages WHERE message_id = ?")
            .bind(message_id)
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok(gone > 0)
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
    /// The conversations of the main list, newest first.
    pub async fn conversations(&self) -> Result<Vec<Conversation>> {
        self.conversations_of(self.contacts().await?).await
    }

    /// The conversations of a hidden session, newest first.
    pub async fn session_conversations(&self, session: &str) -> Result<Vec<Conversation>> {
        self.conversations_of(self.session_contacts(session).await?).await
    }

    async fn conversations_of(&self, contacts: Vec<Contact>) -> Result<Vec<Conversation>> {
        let mut conversations = Vec::new();
        for contact in contacts {
            let last = sqlx::query("SELECT * FROM messages WHERE contact = ? ORDER BY received_at DESC, message_id DESC LIMIT 1")
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
        conversations.sort_by_key(|c| std::cmp::Reverse(c.last.as_ref().map_or(i64::MIN, |m| m.received_at)));
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

    /// A copy of the message went to the mailbox `at` (ms).
    pub async fn mailed(&self, message_id: &str, at: i64) -> Result<()> {
        sqlx::query("UPDATE pending_outbox SET mailed_at = ? WHERE message_id = ?").bind(at).bind(message_id).execute(&self.pool).await?;
        Ok(())
    }

    /// The router refused the message for good: it leaves the outbox and, if it was still
    /// pending, is shown as not sent (§84). A message already shown as sent stays sent (it was
    /// true and never claimed delivery, 2026-10-01), and one that reached the contact is left
    /// alone. Returns whether the state changed to not sent.
    pub async fn mark_not_sent(&self, message_id: &str) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let marked = sqlx::query("UPDATE messages SET state = ? WHERE message_id = ? AND state = ?")
            .bind(MessageState::NotSent as i64)
            .bind(message_id)
            .bind(MessageState::Pending as i64)
            .execute(&mut *tx)
            .await?
            .rows_affected()
            > 0;
        sqlx::query("DELETE FROM pending_outbox WHERE message_id = ?").bind(message_id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(marked)
    }

    pub async fn dequeue(&self, message_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM pending_outbox WHERE message_id = ?").bind(message_id).execute(&self.pool).await?;
        Ok(())
    }

    /// Returns false when the file was already there (a retried offer).
    pub async fn insert_file(&self, file: &FileRecord) -> Result<bool> {
        let result = sqlx::query(
            "INSERT INTO files (message_id, name, size, mime, hash, chunk, path, chunks_done, complete, waiting)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT (message_id) DO NOTHING",
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
        .bind(file.waiting)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// The user asked for a file that was waiting (A4): from now on it is pulled like any other.
    pub async fn set_file_waiting(&self, message_id: &str, waiting: bool) -> Result<()> {
        sqlx::query("UPDATE files SET waiting = ? WHERE message_id = ?").bind(waiting).bind(message_id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn file(&self, message_id: &str) -> Result<Option<FileRecord>> {
        let row = sqlx::query("SELECT * FROM files WHERE message_id = ?").bind(message_id).fetch_optional(&self.pool).await?;
        row.as_ref().map(file_from).transpose()
    }

    /// Where the bytes of every file any message points to are, as stored, each place once: the
    /// sweep of what nothing points to and two messages sharing bytes need all of them (2026-10-02).
    pub async fn file_paths(&self) -> Result<Vec<String>> {
        let rows = sqlx::query("SELECT DISTINCT path FROM files").fetch_all(&self.pool).await?;
        Ok(rows.iter().map(|row| row.get("path")).collect())
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

    /// Incoming files still missing chunks, with their sender: (contact, file). Not the ones
    /// waiting for the user (A4), nor those of a stranger still in the requests (A5).
    pub async fn incomplete_incoming_files(&self) -> Result<Vec<(String, FileRecord)>> {
        let rows = sqlx::query(
            "SELECT files.*, messages.contact FROM files JOIN messages USING (message_id)
             JOIN contacts ON contacts.device_id = messages.contact
             WHERE messages.outgoing = 0 AND files.complete = 0 AND files.failed = 0 AND files.waiting = 0
               AND contacts.accepted = 1 ORDER BY received_at",
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
        sqlx::query("DELETE FROM plugin_records WHERE plugin = ?").bind(id).execute(&self.pool).await?;
        sqlx::query("DELETE FROM reminders WHERE plugin = ?").bind(id).execute(&self.pool).await?;
        sqlx::query("DELETE FROM plugin_refs WHERE plugin = ?").bind(id).execute(&self.pool).await?;
        Ok(())
    }

    // ---- Plugin records, reminders and refs (2026-09-27) ----
    // Since 2026-10-01 (§108) each belongs to a place: the main list (`session` `None`) or one
    // hidden session. The same key in two places is two things.

    pub async fn plugin_record(&self, plugin: &str, session: Option<&str>, key: &str) -> Result<Option<Vec<u8>>> {
        let row = sqlx::query("SELECT value FROM plugin_records WHERE plugin = ? AND session IS ? AND key = ?")
            .bind(plugin)
            .bind(session)
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| row.get::<Vec<u8>, _>("value")))
    }

    pub async fn set_plugin_record(&self, plugin: &str, session: Option<&str>, key: &str, value: &[u8]) -> Result<()> {
        sqlx::query(
            "INSERT INTO plugin_records (plugin, session, key, value, updated_at) VALUES (?, ?, ?, ?, ?)
             ON CONFLICT (plugin, ifnull(session, ''), key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        )
        .bind(plugin)
        .bind(session)
        .bind(key)
        .bind(value)
        .bind(now())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn forget_plugin_record(&self, plugin: &str, session: Option<&str>, key: &str) -> Result<()> {
        sqlx::query("DELETE FROM plugin_records WHERE plugin = ? AND session IS ? AND key = ?")
            .bind(plugin)
            .bind(session)
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// The keys of a plugin's records that start with `prefix`, in order.
    pub async fn plugin_record_keys(&self, plugin: &str, session: Option<&str>, prefix: &str) -> Result<Vec<String>> {
        let rows = sqlx::query("SELECT key FROM plugin_records WHERE plugin = ? AND session IS ? AND substr(key, 1, ?) = ? ORDER BY key")
            .bind(plugin)
            .bind(session)
            .bind(prefix.len() as i64)
            .bind(prefix)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(|row| row.get::<String, _>("key")).collect())
    }

    /// How many bytes a plugin keeps in its records, in that place.
    pub async fn plugin_records_size(&self, plugin: &str, session: Option<&str>) -> Result<i64> {
        let row = sqlx::query("SELECT COALESCE(SUM(length(value)), 0) AS n FROM plugin_records WHERE plugin = ? AND session IS ?")
            .bind(plugin)
            .bind(session)
            .fetch_one(&self.pool)
            .await?;
        Ok(row.get("n"))
    }

    pub async fn set_reminder(&self, reminder: &Reminder) -> Result<()> {
        sqlx::query(
            "INSERT INTO reminders (plugin, session, id, at, text) VALUES (?, ?, ?, ?, ?)
             ON CONFLICT (plugin, ifnull(session, ''), id) DO UPDATE SET at = excluded.at, text = excluded.text",
        )
        .bind(&reminder.plugin)
        .bind(&reminder.session)
        .bind(&reminder.id)
        .bind(reminder.at)
        .bind(&reminder.text)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn cancel_reminder(&self, plugin: &str, session: Option<&str>, id: &str) -> Result<bool> {
        let gone = sqlx::query("DELETE FROM reminders WHERE plugin = ? AND session IS ? AND id = ?")
            .bind(plugin)
            .bind(session)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(gone.rows_affected() > 0)
    }

    /// Drops every reminder of a plugin: whether there was any.
    pub async fn forget_reminders(&self, plugin: &str) -> Result<bool> {
        let gone = sqlx::query("DELETE FROM reminders WHERE plugin = ?").bind(plugin).execute(&self.pool).await?;
        Ok(gone.rows_affected() > 0)
    }

    /// A plugin's reminders, soonest first; every plugin's when `plugin` is `None`. Of every place:
    /// which of them may be shown is the caller's to decide (a closed session's may not).
    pub async fn reminders(&self, plugin: Option<&str>) -> Result<Vec<Reminder>> {
        let rows = match plugin {
            Some(plugin) => sqlx::query("SELECT * FROM reminders WHERE plugin = ? ORDER BY at, id, ifnull(session, '')").bind(plugin).fetch_all(&self.pool).await?,
            None => sqlx::query("SELECT * FROM reminders ORDER BY at, id, ifnull(session, '')").fetch_all(&self.pool).await?,
        };
        Ok(rows.iter().map(reminder_from).collect())
    }

    pub async fn add_plugin_ref(&self, reference: &str, plugin: &str, contact: &str, message_id: &str) -> Result<()> {
        sqlx::query("INSERT INTO plugin_refs (ref, plugin, contact, message_id, created_at) VALUES (?, ?, ?, ?, ?) ON CONFLICT DO NOTHING")
            .bind(reference)
            .bind(plugin)
            .bind(contact)
            .bind(message_id)
            .bind(now())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// The ref a plugin already has for this message, if any.
    pub async fn plugin_ref_for(&self, plugin: &str, message_id: &str) -> Result<Option<String>> {
        let row = sqlx::query("SELECT ref FROM plugin_refs WHERE plugin = ? AND message_id = ?")
            .bind(plugin)
            .bind(message_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| row.get::<String, _>("ref")))
    }

    pub async fn plugin_ref(&self, reference: &str) -> Result<Option<PluginRef>> {
        let row = sqlx::query("SELECT * FROM plugin_refs WHERE ref = ?").bind(reference).fetch_optional(&self.pool).await?;
        Ok(row.map(|row| PluginRef { plugin: row.get("plugin"), contact: row.get("contact"), message_id: row.get("message_id") }))
    }

    /// What a plugin left under that key, if anything (§53).
    pub async fn plugin_value(&self, plugin: &str, session: Option<&str>, key: &str) -> Result<Option<String>> {
        let row = sqlx::query("SELECT value FROM plugin_memory WHERE plugin = ? AND session IS ? AND key = ?")
            .bind(plugin)
            .bind(session)
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| row.get::<String, _>("value")))
    }

    pub async fn set_plugin_value(&self, plugin: &str, session: Option<&str>, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO plugin_memory (plugin, session, key, value) VALUES (?, ?, ?, ?)
             ON CONFLICT (plugin, ifnull(session, ''), key) DO UPDATE SET value = excluded.value",
        )
        .bind(plugin)
        .bind(session)
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Everything that plugin remembers in that place, by key, in a stable order.
    pub async fn plugin_keys(&self, plugin: &str, session: Option<&str>) -> Result<Vec<String>> {
        let rows = sqlx::query("SELECT key FROM plugin_memory WHERE plugin = ? AND session IS ? ORDER BY key")
            .bind(plugin)
            .bind(session)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(|row| row.get::<String, _>("key")).collect())
    }

    pub async fn forget_plugin_value(&self, plugin: &str, session: Option<&str>, key: &str) -> Result<()> {
        sqlx::query("DELETE FROM plugin_memory WHERE plugin = ? AND session IS ? AND key = ?")
            .bind(plugin)
            .bind(session)
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

    // ---- Circles (2026-09-27) ----

    /// Whether a contact is known only through a circle.
    pub async fn set_via_circle(&self, device_id: &str, via_circle: bool) -> Result<()> {
        sqlx::query("UPDATE contacts SET via_circle = ? WHERE device_id = ?").bind(via_circle).bind(device_id).execute(&self.pool).await?;
        Ok(())
    }

    /// Keeps a circle as its card says now; a new one, or a later revision of a known one.
    pub async fn save_circle(&self, circle: &CircleRecord) -> Result<()> {
        sqlx::query(
            "INSERT INTO circles (id, name, card, revision, admins_only, session, left, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (id) DO UPDATE SET name = excluded.name, card = excluded.card, revision = excluded.revision,
             admins_only = excluded.admins_only, left = excluded.left",
        )
        .bind(&circle.id)
        .bind(&circle.name)
        .bind(&circle.card)
        .bind(circle.revision)
        .bind(circle.admins_only)
        .bind(&circle.session)
        .bind(circle.left)
        .bind(circle.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Replaces the members of a circle with what its card says.
    pub async fn set_circle_members(&self, circle: &str, members: &[CircleMember]) -> Result<()> {
        sqlx::query("DELETE FROM circle_members WHERE circle = ?").bind(circle).execute(&self.pool).await?;
        for member in members {
            sqlx::query("INSERT INTO circle_members (circle, device_id, name, admin) VALUES (?, ?, ?, ?)")
                .bind(circle)
                .bind(&member.device_id)
                .bind(&member.name)
                .bind(member.admin)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    pub async fn circle(&self, id: &str) -> Result<Option<CircleRecord>> {
        let row = sqlx::query("SELECT * FROM circles WHERE id = ?").bind(id).fetch_optional(&self.pool).await?;
        Ok(row.as_ref().map(circle_from))
    }

    /// The circles of the main list, or of a hidden session, by name.
    pub async fn circles(&self, session: Option<&str>) -> Result<Vec<CircleRecord>> {
        let rows = match session {
            None => sqlx::query("SELECT * FROM circles WHERE session IS NULL ORDER BY name").fetch_all(&self.pool).await?,
            Some(session) => sqlx::query("SELECT * FROM circles WHERE session = ? ORDER BY name").bind(session).fetch_all(&self.pool).await?,
        };
        Ok(rows.iter().map(circle_from).collect())
    }

    pub async fn circle_members(&self, circle: &str) -> Result<Vec<CircleMember>> {
        let rows = sqlx::query("SELECT * FROM circle_members WHERE circle = ? ORDER BY admin DESC, name").bind(circle).fetch_all(&self.pool).await?;
        Ok(rows
            .iter()
            .map(|row| CircleMember { device_id: row.get("device_id"), name: row.get("name"), admin: row.get("admin") })
            .collect())
    }

    /// The circles a device is in with this phone, left ones aside.
    pub async fn circles_with(&self, device_id: &str) -> Result<Vec<String>> {
        let rows = sqlx::query("SELECT m.circle FROM circle_members m JOIN circles c ON c.id = m.circle WHERE m.device_id = ? AND c.left = 0")
            .bind(device_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(|row| row.get("circle")).collect())
    }

    /// Takes a circle away with everything said in it.
    pub async fn remove_circle(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM circles WHERE id = ?").bind(id).execute(&self.pool).await?;
        Ok(())
    }

    /// Returns false when the message was already there (a retry from another member).
    pub async fn insert_circle_message(&self, message: &CircleMessage) -> Result<bool> {
        let result = sqlx::query(
            "INSERT INTO circle_messages (message_id, circle, sender, outgoing, kind, body, sent_at, received_at, state) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (message_id) DO NOTHING",
        )
        .bind(&message.message_id)
        .bind(&message.circle)
        .bind(&message.sender)
        .bind(message.outgoing)
        .bind(&message.kind)
        .bind(&message.body)
        .bind(message.sent_at)
        .bind(if message.received_at > 0 { message.received_at } else { now() })
        .bind(message.state as i64)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn circle_message(&self, message_id: &str) -> Result<Option<CircleMessage>> {
        let row = sqlx::query("SELECT * FROM circle_messages WHERE message_id = ?").bind(message_id).fetch_optional(&self.pool).await?;
        Ok(row.as_ref().map(circle_message_from))
    }

    /// The last `limit` things said or done in a circle, oldest first, as they reached this
    /// phone (M5). Control packets are not among them.
    pub async fn circle_messages(&self, circle: &str, limit: i64) -> Result<Vec<CircleMessage>> {
        let rows = sqlx::query(
            "SELECT * FROM (SELECT * FROM circle_messages WHERE circle = ? AND kind IN ('text', 'created', 'joined', 'left', 'removed', 'renamed')
             ORDER BY received_at DESC, message_id DESC LIMIT ?) ORDER BY received_at, message_id",
        )
        .bind(circle)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(circle_message_from).collect())
    }

    /// What others said in a circle and this phone has not shown yet.
    pub async fn circle_unread(&self, circle: &str) -> Result<Vec<String>> {
        let rows = sqlx::query("SELECT message_id FROM circle_messages WHERE circle = ? AND outgoing = 0 AND kind = 'text' AND state < 3 ORDER BY received_at")
            .bind(circle)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(|row| row.get("message_id")).collect())
    }

    /// Moves circle messages to `state`, never backwards.
    pub async fn advance_circle(&self, message_ids: &[String], state: MessageState) -> Result<()> {
        for id in message_ids {
            sqlx::query("UPDATE circle_messages SET state = ? WHERE message_id = ? AND state < ?")
                .bind(state as i64)
                .bind(id)
                .bind(state as i64)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    /// Every circle of the main list or of a session with its last message and unread count,
    /// newest first.
    pub async fn circle_conversations(&self, session: Option<&str>) -> Result<Vec<CircleConversation>> {
        let mut conversations = Vec::new();
        for circle in self.circles(session).await? {
            let last = sqlx::query(
                "SELECT * FROM circle_messages WHERE circle = ? AND kind IN ('text', 'created', 'joined', 'left', 'removed', 'renamed')
                 ORDER BY received_at DESC, message_id DESC LIMIT 1",
            )
            .bind(&circle.id)
            .fetch_optional(&self.pool)
            .await?
            .as_ref()
            .map(circle_message_from);
            let unread: i64 = sqlx::query("SELECT COUNT(*) AS n FROM circle_messages WHERE circle = ? AND outgoing = 0 AND kind = 'text' AND state < 3")
                .bind(&circle.id)
                .fetch_one(&self.pool)
                .await?
                .get("n");
            let members: i64 = sqlx::query("SELECT COUNT(*) AS n FROM circle_members WHERE circle = ?")
                .bind(&circle.id)
                .fetch_one(&self.pool)
                .await?
                .get("n");
            conversations.push(CircleConversation { circle, members, last, unread });
        }
        conversations.sort_by_key(|c| std::cmp::Reverse(c.last.as_ref().map_or(i64::MIN, |m| m.received_at)));
        Ok(conversations)
    }

    pub async fn circle_enqueue(&self, message_id: &str, contact: &str, now: i64) -> Result<()> {
        sqlx::query("INSERT INTO circle_outbox (message_id, contact, created_at, next_attempt) VALUES (?, ?, ?, ?) ON CONFLICT DO NOTHING")
            .bind(message_id)
            .bind(contact)
            .bind(now)
            .bind(now)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Circle entries whose next attempt is due at `now`.
    pub async fn circle_due(&self, now: i64) -> Result<Vec<CircleOutboxEntry>> {
        let rows = sqlx::query("SELECT * FROM circle_outbox WHERE next_attempt <= ? ORDER BY created_at").bind(now).fetch_all(&self.pool).await?;
        Ok(rows.iter().map(circle_outbox_from).collect())
    }

    /// Every circle entry still waiting for its member's receipt.
    pub async fn circle_outbox(&self) -> Result<Vec<CircleOutboxEntry>> {
        let rows = sqlx::query("SELECT * FROM circle_outbox ORDER BY created_at").fetch_all(&self.pool).await?;
        Ok(rows.iter().map(circle_outbox_from).collect())
    }

    /// The entries of one circle message: who it still has to reach.
    pub async fn circle_pending(&self, message_id: &str) -> Result<Vec<CircleOutboxEntry>> {
        let rows = sqlx::query("SELECT * FROM circle_outbox WHERE message_id = ? ORDER BY contact").bind(message_id).fetch_all(&self.pool).await?;
        Ok(rows.iter().map(circle_outbox_from).collect())
    }

    pub async fn circle_reschedule(&self, message_id: &str, contact: &str, attempts: i64, next_attempt: i64, in_mailbox: bool) -> Result<()> {
        sqlx::query("UPDATE circle_outbox SET attempts = ?, next_attempt = ?, in_mailbox = ? WHERE message_id = ? AND contact = ?")
            .bind(attempts)
            .bind(next_attempt)
            .bind(in_mailbox)
            .bind(message_id)
            .bind(contact)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// One member got it: their entry goes. Says whether there was one.
    pub async fn circle_dequeue(&self, message_id: &str, contact: &str) -> Result<bool> {
        let gone = sqlx::query("DELETE FROM circle_outbox WHERE message_id = ? AND contact = ?")
            .bind(message_id)
            .bind(contact)
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok(gone > 0)
    }

    /// Nothing more for this member: they left, or were taken out.
    pub async fn circle_dequeue_member(&self, circle: &str, contact: &str) -> Result<()> {
        sqlx::query("DELETE FROM circle_outbox WHERE contact = ? AND message_id IN (SELECT message_id FROM circle_messages WHERE circle = ?)")
            .bind(contact)
            .bind(circle)
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

fn reminder_from(row: &SqliteRow) -> Reminder {
    Reminder { plugin: row.get("plugin"), session: row.get("session"), id: row.get("id"), at: row.get("at"), text: row.get("text") }
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
        session: row.get("session"),
        rules: ContactRules {
            muted: row.get("muted"),
            accepts_chat: row.get("accepts_chat"),
            accepts_calls: row.get("accepts_calls"),
            receipts: row.get("receipts"),
            typing: row.get("typing"),
        },
        accepted: row.get("accepted"),
        via_circle: row.get("via_circle"),
    }
}

fn message_from(row: &SqliteRow) -> Message {
    Message {
        message_id: row.get("message_id"),
        contact: row.get("contact"),
        outgoing: row.get("outgoing"),
        body: row.get("body"),
        sent_at: row.get("sent_at"),
        received_at: row.get("received_at"),
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
        waiting: row.get("waiting"),
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

fn circle_from(row: &SqliteRow) -> CircleRecord {
    CircleRecord {
        id: row.get("id"),
        name: row.get("name"),
        card: row.get("card"),
        revision: row.get("revision"),
        admins_only: row.get("admins_only"),
        session: row.get("session"),
        left: row.get("left"),
        created_at: row.get("created_at"),
    }
}

fn circle_message_from(row: &SqliteRow) -> CircleMessage {
    CircleMessage {
        message_id: row.get("message_id"),
        circle: row.get("circle"),
        sender: row.get("sender"),
        outgoing: row.get("outgoing"),
        kind: row.get("kind"),
        body: row.get("body"),
        sent_at: row.get("sent_at"),
        received_at: row.get("received_at"),
        state: MessageState::from_rank(row.get("state")),
    }
}

fn circle_outbox_from(row: &SqliteRow) -> CircleOutboxEntry {
    CircleOutboxEntry {
        message_id: row.get("message_id"),
        contact: row.get("contact"),
        created_at: row.get("created_at"),
        attempts: row.get("attempts"),
        next_attempt: row.get("next_attempt"),
        in_mailbox: row.get("in_mailbox"),
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
        mailed_at: row.get("mailed_at"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn store() -> Store {
        Store::open_in_memory().await.expect("opens")
    }

    fn contact(id: &str) -> NewContact {
        NewContact { device_id: id.to_owned(), name: "Bob".to_owned(), card: vec![1, 2], mailbox: true, session: None, receipts: true, accepted: true }
    }

    fn message(id: &str, contact: &str, outgoing: bool, sent_at: i64) -> Message {
        Message {
            message_id: id.to_owned(),
            contact: contact.to_owned(),
            outgoing,
            body: format!("text {id}"),
            sent_at,
            received_at: sent_at,
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
            waiting: false,
        }
    }

    // A5: a stranger who wrote first waits in the requests, out of the main list, until the user
    // says yes; scanning them says yes too, and a yes is never taken back by a new card.
    #[tokio::test]
    async fn a_stranger_waits_in_the_requests_until_chosen() {
        let store = store().await;
        store.add_contact(&NewContact { accepted: false, ..contact("ft_mallory") }).await.expect("adds");
        assert!(store.contacts().await.unwrap().is_empty());
        assert_eq!(store.requests(None).await.unwrap().len(), 1);
        store.set_accepted("ft_mallory", true).await.expect("accepts");
        assert_eq!(store.contacts().await.unwrap().len(), 1);
        assert!(store.requests(None).await.unwrap().is_empty());
        store.add_contact(&NewContact { accepted: false, ..contact("ft_mallory") }).await.expect("a new card");
        assert!(store.contact("ft_mallory").await.unwrap().unwrap().accepted, "still chosen");
    }

    // A3: a session goes with everything in it, and its slot is free again.
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

    // A3: a session with nobody in it, not even a stranger waiting for a yes, is empty.
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

        // A circle is something in the session too, even with nobody else in it any more.
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

    // M5: what the sender's clock says is kept, but the order and the age are this phone's.
    #[tokio::test]
    async fn messages_are_ordered_and_aged_by_arrival() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        // "late" says it was sent long ago and arrives first; "early" claims the far future.
        store.insert_message(&Message { received_at: 1_000_000, ..message("late", "ft_bob", false, 1_000) }).await.unwrap();
        store.insert_message(&Message { received_at: 2_000_000, ..message("early", "ft_bob", false, 999_999_999_999) }).await.unwrap();
        let ids: Vec<_> = store.messages("ft_bob", 10).await.unwrap().into_iter().map(|m| m.message_id).collect();
        assert_eq!(ids, ["late", "early"], "in the order they arrived, whatever their clocks said");
        assert_eq!(store.message("late").await.unwrap().unwrap().sent_at, 1_000, "the sender's time is kept");
        // Without an arrival time, the store stamps now.
        store.insert_message(&Message { received_at: 0, ..message("now", "ft_bob", false, 5) }).await.unwrap();
        let stamped = store.message("now").await.unwrap().unwrap();
        assert!(stamped.received_at > 2_000_000, "stamped on arrival: {}", stamped.received_at);
        assert_eq!(store.messages("ft_bob", 10).await.unwrap().last().unwrap().message_id, "now");
    }

    // M6: a message still waiting to go is never swept, however old.
    #[tokio::test]
    async fn the_sweep_spares_what_is_still_in_the_outbox() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.set_history("ft_bob", 1, 0).await.expect("keeps a second");
        store.insert_message(&message("pending", "ft_bob", true, 1)).await.unwrap();
        store.enqueue("pending", "ft_bob", 1).await.unwrap();
        store.insert_message(&message("old", "ft_bob", true, 1)).await.unwrap();
        let far_future = now() + 10 * 24 * 3600 * 1000;
        store.sweep(far_future).await.expect("sweeps");
        assert!(store.message("pending").await.unwrap().is_some(), "still to be delivered");
        assert!(store.message("old").await.unwrap().is_none());
    }

    // A4: a waiting file is not among what the resume loop pulls.
    #[tokio::test]
    async fn a_waiting_file_is_not_resumed() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.insert_message(&message("big", "ft_bob", false, 1)).await.unwrap();
        store.insert_file(&FileRecord { waiting: true, ..file("big") }).await.unwrap();
        assert!(store.incomplete_incoming_files().await.unwrap().is_empty());
        store.set_file_waiting("big", false).await.expect("the user asks for it");
        assert_eq!(store.incomplete_incoming_files().await.unwrap().len(), 1);
    }

    // The user erases one message on this phone (§61): the row goes, and with it its file and
    // its place in the outbox. Nothing is told to the other side: this is our copy.
    #[tokio::test]
    async fn a_message_is_erased_with_everything_that_hangs_from_it() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.insert_message(&message("m1", "ft_bob", true, 1)).await.expect("inserts");
        store.insert_message(&message("m2", "ft_bob", true, 2)).await.expect("inserts");
        store.insert_file(&file("m1")).await.expect("inserts the file");
        store.enqueue("m1", "ft_bob", 1).await.expect("queues");

        assert!(store.forget_message("m1").await.expect("erases"));
        assert!(store.message("m1").await.expect("reads").is_none());
        assert!(store.file("m1").await.expect("reads").is_none(), "its file goes with it");
        assert!(store.outbox().await.expect("reads").is_empty(), "and its place in the outbox");
        assert!(store.message("m2").await.expect("reads").is_some(), "the others stay");
        assert!(!store.forget_message("m1").await.expect("says so"), "erasing it twice erases nothing");
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

    // 2026-10-02: where every file any message points to is, whoever it is with — the main list,
    // a hidden session, a stranger still in the requests — once per place, for the sweep of what
    // nothing points to and for keeping bytes two messages share.
    #[tokio::test]
    async fn every_path_a_message_points_to_is_listed_once() {
        let store = store().await;
        store.add_session("s1", &[1; 32], 1).await.expect("creates");
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.add_contact(&NewContact { session: Some("s1".to_owned()), ..contact("ft_carol") }).await.expect("adds inside");
        store.add_contact(&NewContact { accepted: false, ..contact("ft_eve") }).await.expect("a request");
        assert!(store.file_paths().await.expect("lists").is_empty());

        for (id, contact, path) in [
            ("m1", "ft_bob", "uploads/1-a.jpg"),
            ("m2", "ft_bob", "uploads/1-a.jpg"), // forwarded: the same bytes
            ("m3", "ft_carol", "drive/x1/doc.pdf"),
            ("m4", "ft_eve", "m4/photo.jpg"),
            ("m5", "ft_bob", "/old/container/files/outgoing/u1"),
        ] {
            store.insert_message(&message(id, contact, true, 1)).await.unwrap();
            store.insert_file(&FileRecord { path: path.to_owned(), ..file(id) }).await.unwrap();
        }
        store.insert_message(&message("t1", "ft_bob", true, 2)).await.unwrap();

        let mut paths = store.file_paths().await.expect("lists");
        paths.sort();
        assert_eq!(paths, ["/old/container/files/outgoing/u1", "drive/x1/doc.pdf", "m4/photo.jpg", "uploads/1-a.jpg"]);
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

    // The router refused it (§84): out of the outbox, and a pending message is shown as not sent.
    // One already shown as sent stays sent (it was true and never claimed delivery, 2026-10-01),
    // and what was delivered stays so. Sending it again makes it pending, and a receipt that comes
    // late still moves it on.
    #[tokio::test]
    async fn a_message_not_sent_leaves_the_outbox_and_may_go_again() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        for id in ["m1", "m2", "m3"] {
            store.insert_message(&message(id, "ft_bob", true, 1)).await.expect("inserts");
            store.enqueue(id, "ft_bob", 100).await.expect("queues");
        }
        store.advance(&["m2".to_owned()], MessageState::Delivered).await.expect("advances");
        store.advance(&["m3".to_owned()], MessageState::Sent).await.expect("advances");

        assert!(store.mark_not_sent("m1").await.expect("marks"));
        assert!(!store.mark_not_sent("m2").await.expect("leaves it"), "a delivered message was sent");
        assert!(!store.mark_not_sent("m3").await.expect("leaves it"), "a sent message stays sent");
        let state = |id: &'static str| {
            let store = store.clone();
            async move { store.message(id).await.expect("reads").expect("exists").state }
        };
        assert_eq!(state("m1").await, MessageState::NotSent);
        assert_eq!(state("m2").await, MessageState::Delivered);
        assert_eq!(state("m3").await, MessageState::Sent);
        assert!(store.outbox().await.expect("lists").iter().all(|entry| entry.message_id != "m1" && entry.message_id != "m3"));

        store.advance(&["m1".to_owned()], MessageState::Pending).await.expect("sent again");
        assert_eq!(state("m1").await, MessageState::Pending);
        store.mark_not_sent("m1").await.expect("marks");
        store.advance(&["m1".to_owned()], MessageState::Delivered).await.expect("late receipt");
        assert_eq!(state("m1").await, MessageState::Delivered);
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

    // 2026-10-01: when the last copy went to the mailbox, so that the next goes a day later.
    #[tokio::test]
    async fn the_outbox_keeps_when_the_last_copy_went_to_the_mailbox() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.insert_message(&message("m1", "ft_bob", true, 1)).await.expect("inserts");
        store.enqueue("m1", "ft_bob", 100).await.expect("queues");
        assert_eq!(store.outbox().await.expect("lists")[0].mailed_at, None);

        store.mailed("m1", 700).await.expect("keeps it");
        store.reschedule("m1", 1, 500, true).await.expect("reschedules");
        assert_eq!(store.outbox().await.expect("lists")[0].mailed_at, Some(700), "a reschedule leaves it");
    }

    // A database from before the column: what was in the mailbox stays there, with no time.
    #[tokio::test]
    async fn an_outbox_from_before_has_no_time_of_its_last_copy() {
        let options = SqliteConnectOptions::new().in_memory(true).foreign_keys(true);
        let pool = SqlitePoolOptions::new().max_connections(1).connect_with(options).await.expect("opens");
        sqlx::migrate!("./migrations").run_to(12, &pool).await.expect("migrates to 0012");
        sqlx::query("INSERT INTO contacts (device_id, name, card, mailbox, added_at) VALUES ('ft_bob', 'Bob', x'01', 1, 1)").execute(&pool).await.expect("a contact");
        sqlx::query("INSERT INTO messages (message_id, contact, outgoing, body, sent_at, state) VALUES ('m1', 'ft_bob', 1, 'hi', 1, 1)")
            .execute(&pool)
            .await
            .expect("a message");
        sqlx::query("INSERT INTO pending_outbox (message_id, contact, created_at, attempts, next_attempt, in_mailbox) VALUES ('m1', 'ft_bob', 1, 3, 9, 1)")
            .execute(&pool)
            .await
            .expect("in the mailbox");

        let store = Store::with(pool).await.expect("migrates the rest");
        let entry = &store.outbox().await.expect("lists")[0];
        assert_eq!((entry.attempts, entry.in_mailbox, entry.mailed_at), (3, true, None));
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
        assert_eq!(store.plugin_value("com.example.code", None, "pen").await.unwrap(), None);

        store.set_plugin_value("com.example.code", None, "pen", "black").await.unwrap();
        store.set_plugin_value("com.example.ai", None, "pen", "blue").await.unwrap();
        assert_eq!(store.plugin_value("com.example.code", None, "pen").await.unwrap().as_deref(), Some("black"));
        assert_eq!(store.plugin_value("com.example.ai", None, "pen").await.unwrap().as_deref(), Some("blue"));

        store.set_plugin_value("com.example.code", None, "pen", "red").await.unwrap();
        store.set_plugin_value("com.example.code", None, "size", "3").await.unwrap();
        assert_eq!(store.plugin_value("com.example.code", None, "pen").await.unwrap().as_deref(), Some("red"));
        assert_eq!(store.plugin_keys("com.example.code", None).await.unwrap(), ["pen", "size"]);

        // Taking a plugin out takes what it remembered with it.
        store.install_plugin("com.example.code", "1.0.0", "{}").await.unwrap();
        store.remove_plugin("com.example.code").await.unwrap();
        assert_eq!(store.plugin_keys("com.example.code", None).await.unwrap(), Vec::<String>::new());
    }

    // Hidden sessions (Plan, 2026-09-23): a contact belongs to the main list or to one session,
    // and only the main list is what the phone shows by default.
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

    // The same person scanned again stays where they were: a card refresh never moves a contact.
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

    // Issues app#4–#6: what this phone takes from a contact and what it tells them. Everything
    // is on by default except muting, and none of it travels.
    #[tokio::test]
    async fn a_contact_starts_accepting_everything_and_its_rules_can_change() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        let bob = store.contact("ft_bob").await.unwrap().unwrap();
        assert_eq!(bob.rules, ContactRules::default());
        assert_eq!(
            ContactRules::default(),
            ContactRules { muted: false, accepts_chat: true, accepts_calls: true, receipts: true, typing: true }
        );

        let rules = ContactRules { muted: true, accepts_chat: false, accepts_calls: true, receipts: false, typing: false };
        store.set_rules("ft_bob", &rules).await.expect("sets");
        assert_eq!(store.contact("ft_bob").await.unwrap().unwrap().rules, rules);
    }

    // The receipts switch in Settings is the default for contacts added later.
    #[tokio::test]
    async fn a_new_contact_takes_the_receipts_default_it_is_given() {
        let store = store().await;
        store.add_contact(&NewContact { receipts: false, ..contact("ft_bob") }).await.expect("adds");
        assert!(!store.contact("ft_bob").await.unwrap().unwrap().rules.receipts);
    }

    // Hidden sessions, phase 2 (app#9): seven spare route capabilities, kept once; each session
    // takes one slot of its own.
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

    // Circles (2026-09-27): a circle is kept as its card says, what is said in it is listed in
    // the order it arrived, and each member's entry in the outbox goes with their own receipt.
    #[tokio::test]
    async fn circles_are_kept_listed_and_delivered_member_by_member() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.add_contact(&contact("ft_carol")).await.expect("adds");
        let circle = CircleRecord {
            id: "c1".to_owned(),
            name: "Friends".to_owned(),
            card: vec![1, 2, 3],
            revision: 1,
            admins_only: false,
            session: None,
            left: false,
            created_at: 5,
        };
        store.save_circle(&circle).await.expect("saves");
        store
            .set_circle_members(
                "c1",
                &[
                    CircleMember { device_id: "ft_me".to_owned(), name: "Me".to_owned(), admin: true },
                    CircleMember { device_id: "ft_bob".to_owned(), name: "Bob".to_owned(), admin: false },
                    CircleMember { device_id: "ft_carol".to_owned(), name: "Carol".to_owned(), admin: false },
                ],
            )
            .await
            .expect("members");
        assert_eq!(store.circle("c1").await.expect("reads"), Some(circle.clone()));
        assert_eq!(store.circles(None).await.expect("lists").len(), 1);
        assert!(store.circles(Some("s1")).await.expect("lists").is_empty());
        assert_eq!(store.circle_members("c1").await.expect("members").len(), 3);
        assert_eq!(store.circles_with("ft_bob").await.expect("with"), vec!["c1".to_owned()]);

        let said = |id: &str, sender: &str, outgoing: bool, kind: &str, at: i64| CircleMessage {
            message_id: id.to_owned(),
            circle: "c1".to_owned(),
            sender: sender.to_owned(),
            outgoing,
            kind: kind.to_owned(),
            body: format!("{kind} {id}"),
            sent_at: at,
            received_at: at,
            state: if outgoing { MessageState::Pending } else { MessageState::Delivered },
        };
        assert!(store.insert_circle_message(&said("m1", "ft_me", true, "text", 10)).await.expect("inserts"));
        assert!(!store.insert_circle_message(&said("m1", "ft_me", true, "text", 10)).await.expect("again"), "once per id");
        assert!(store.insert_circle_message(&said("m2", "ft_bob", false, "text", 20)).await.expect("inserts"));
        assert!(store.insert_circle_message(&said("k1", "ft_me", true, "card", 30)).await.expect("inserts"));
        assert!(store.insert_circle_message(&said("e1", "ft_carol", false, "joined", 40)).await.expect("inserts"));

        let shown: Vec<String> = store.circle_messages("c1", 10).await.expect("lists").into_iter().map(|m| m.message_id).collect();
        assert_eq!(shown, ["m1", "m2", "e1"], "control packets are not shown; events are");
        assert_eq!(store.circle_unread("c1").await.expect("unread"), vec!["m2".to_owned()], "only texts of others count");
        let list = store.circle_conversations(None).await.expect("conversations");
        assert_eq!(list[0].unread, 1);
        assert_eq!(list[0].members, 3);
        assert_eq!(list[0].last.as_ref().map(|m| m.message_id.as_str()), Some("e1"));

        // m1 goes to both; Bob's receipt clears only Bob's entry.
        store.circle_enqueue("m1", "ft_bob", 1).await.expect("queues");
        store.circle_enqueue("m1", "ft_carol", 1).await.expect("queues");
        store.circle_enqueue("m1", "ft_carol", 1).await.expect("queues twice is once");
        assert_eq!(store.circle_outbox().await.expect("outbox").len(), 2);
        assert_eq!(store.circle_due(1).await.expect("due").len(), 2);
        store.circle_reschedule("m1", "ft_carol", 1, 999, true).await.expect("reschedules");
        assert_eq!(store.circle_due(1).await.expect("due").len(), 1);
        assert!(store.circle_dequeue("m1", "ft_bob").await.expect("dequeues"));
        assert!(!store.circle_dequeue("m1", "ft_bob").await.expect("dequeues"), "gone already");
        assert_eq!(store.circle_pending("m1").await.expect("pending").len(), 1);
        assert!(store.circle_pending("m1").await.expect("pending")[0].in_mailbox);
        store.circle_dequeue_member("c1", "ft_carol").await.expect("member out");
        assert!(store.circle_pending("m1").await.expect("pending").is_empty());

        store.advance_circle(&["m2".to_owned()], MessageState::Read).await.expect("reads");
        assert!(store.circle_unread("c1").await.expect("unread").is_empty());

        // A later revision keeps the id and the session; leaving keeps the history.
        store.save_circle(&CircleRecord { revision: 2, name: "Old friends".to_owned(), left: true, ..circle }).await.expect("saves");
        let held = store.circle("c1").await.expect("reads").expect("there");
        assert_eq!((held.revision, held.name.as_str(), held.left), (2, "Old friends", true));
        assert!(store.circles_with("ft_bob").await.expect("with").is_empty(), "a left circle counts for nobody");
        assert_eq!(store.circle_messages("c1", 10).await.expect("lists").len(), 3);

        store.remove_circle("c1").await.expect("removes");
        assert!(store.circle("c1").await.expect("reads").is_none());
        assert!(store.circle_message("m1").await.expect("reads").is_none(), "what was said goes with it");
        assert!(store.circle_outbox().await.expect("outbox").is_empty());
    }

    // A contact known only through a circle waits in no requests until they write on their own.
    #[tokio::test]
    async fn a_circle_contact_is_no_request_until_they_write() {
        let store = store().await;
        store.add_contact(&NewContact { accepted: false, ..contact("ft_dave") }).await.expect("adds");
        store.set_via_circle("ft_dave", true).await.expect("marks");
        assert!(store.contact("ft_dave").await.expect("reads").expect("there").via_circle);
        assert!(store.requests(None).await.expect("requests").is_empty());
        assert!(store.contacts().await.expect("contacts").is_empty());
        store.insert_message(&message("m1", "ft_dave", false, 1)).await.expect("writes");
        assert_eq!(store.requests(None).await.expect("requests").len(), 1, "now they wait for a yes");
    }

    // 2026-09-27: a plugin's records are bytes, apart from every other plugin's, counted for the
    // quota, and gone with the plugin; so are its reminders and the refs it was handed.
    #[tokio::test]
    async fn keeps_records_reminders_and_refs_for_each_plugin_and_takes_them_with_it() {
        let store = store().await;
        store.install_plugin("com.example.notes", "1.0.0", "{}").await.unwrap();
        assert_eq!(store.plugin_record("com.example.notes", None, "note/1").await.unwrap(), None);
        store.set_plugin_record("com.example.notes", None, "note/1", b"milk").await.unwrap();
        store.set_plugin_record("com.example.notes", None, "note/2", b"eggs and bread").await.unwrap();
        store.set_plugin_record("com.example.notes", None, "settings", b"{}").await.unwrap();
        store.set_plugin_record("com.example.other", None, "note/1", b"theirs").await.unwrap();
        assert_eq!(store.plugin_record("com.example.notes", None, "note/1").await.unwrap().as_deref(), Some(&b"milk"[..]));
        assert_eq!(store.plugin_record_keys("com.example.notes", None, "note/").await.unwrap(), ["note/1", "note/2"]);
        assert_eq!(store.plugin_record_keys("com.example.notes", None, "").await.unwrap().len(), 3);
        assert_eq!(store.plugin_records_size("com.example.notes", None).await.unwrap(), 4 + 14 + 2);
        store.set_plugin_record("com.example.notes", None, "note/1", b"oat milk").await.unwrap();
        assert_eq!(store.plugin_records_size("com.example.notes", None).await.unwrap(), 8 + 14 + 2, "a changed record counts once");
        store.forget_plugin_record("com.example.notes", None, "note/2").await.unwrap();
        assert_eq!(store.plugin_record_keys("com.example.notes", None, "note/").await.unwrap(), ["note/1"]);

        let soon = Reminder { plugin: "com.example.notes".to_owned(), session: None, id: "r1".to_owned(), at: 2_000, text: String::new() };
        let later = Reminder { plugin: "com.example.notes".to_owned(), session: None, id: "r2".to_owned(), at: 5_000, text: "call mum".to_owned() };
        store.set_reminder(&later).await.unwrap();
        store.set_reminder(&soon).await.unwrap();
        store.set_reminder(&Reminder { plugin: "com.example.other".to_owned(), session: None, id: "x".to_owned(), at: 1, text: String::new() }).await.unwrap();
        assert_eq!(store.reminders(Some("com.example.notes")).await.unwrap(), vec![soon.clone(), later.clone()]);
        assert_eq!(store.reminders(None).await.unwrap().len(), 3);
        store.set_reminder(&Reminder { at: 9_000, ..soon.clone() }).await.unwrap();
        assert_eq!(store.reminders(Some("com.example.notes")).await.unwrap()[1].id, "r1", "moved later");
        assert!(store.cancel_reminder("com.example.notes", None, "r2").await.unwrap());
        assert!(!store.cancel_reminder("com.example.notes", None, "r2").await.unwrap());

        store.add_plugin_ref("ref-1", "com.example.notes", "ft_bob", "m1").await.unwrap();
        assert_eq!(store.plugin_ref_for("com.example.notes", "m1").await.unwrap().as_deref(), Some("ref-1"));
        assert_eq!(store.plugin_ref("ref-1").await.unwrap().map(|r| r.contact), Some("ft_bob".to_owned()));
        assert_eq!(store.plugin_ref("ref-9").await.unwrap(), None);

        store.remove_plugin("com.example.notes").await.unwrap();
        assert!(store.plugin_record_keys("com.example.notes", None, "").await.unwrap().is_empty());
        assert!(store.reminders(Some("com.example.notes")).await.unwrap().is_empty());
        assert_eq!(store.plugin_ref("ref-1").await.unwrap(), None);
        assert_eq!(store.plugin_record("com.example.other", None, "note/1").await.unwrap().as_deref(), Some(&b"theirs"[..]), "another plugin's stay");
    }

    // 2026-10-01 (§108): what plugins kept before records said where they came from stays where
    // everyone saw it, in the main list.
    #[tokio::test]
    async fn what_plugins_kept_before_sessions_counted_stays_in_the_main_list() {
        let path = std::env::temp_dir().join(format!("ft-storage-plugin-places-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let options = SqliteConnectOptions::new().filename(&path).create_if_missing(true).foreign_keys(true);
            let pool = SqlitePoolOptions::new().max_connections(1).connect_with(options).await.unwrap();
            let mut before = sqlx::migrate!("./migrations");
            before.migrations = before.migrations.iter().filter(|migration| migration.version <= 12).cloned().collect::<Vec<_>>().into();
            before.run(&pool).await.unwrap();
            sqlx::query("INSERT INTO plugin_records (plugin, key, value, updated_at) VALUES ('com.example.notes', 'board/1', x'01', 1)").execute(&pool).await.unwrap();
            sqlx::query("INSERT INTO plugin_memory (plugin, key, value) VALUES ('com.example.notes', 'showText', '1')").execute(&pool).await.unwrap();
            sqlx::query("INSERT INTO reminders (plugin, id, at, text) VALUES ('com.example.notes', 'r1', 5, 'milk')").execute(&pool).await.unwrap();
            pool.close().await;
        }
        let store = Store::open(&path).await.expect("migrates");
        assert_eq!(store.plugin_record("com.example.notes", None, "board/1").await.unwrap().as_deref(), Some(&[1u8][..]));
        assert_eq!(store.plugin_value("com.example.notes", None, "showText").await.unwrap().as_deref(), Some("1"));
        assert_eq!(store.reminders(None).await.unwrap()[0].session, None);
        let _ = std::fs::remove_file(&path);
    }

    // 2026-10-01 (§108): what a plugin keeps inside a hidden session is that session's alone: the
    // same key elsewhere is another thing, each place counts its own size, and it all goes with
    // the session, the refs to its messages too.
    #[tokio::test]
    async fn keeps_a_plugins_data_apart_for_each_session_and_drops_it_with_the_session() {
        let store = store().await;
        store.install_plugin("com.example.notes", "1.0.0", "{}").await.unwrap();
        store.add_session("s1", &[1; 32], 1).await.unwrap();
        store.add_session("s2", &[2; 32], 2).await.unwrap();
        let notes = "com.example.notes";

        store.set_plugin_record(notes, None, "board/1", b"main").await.unwrap();
        store.set_plugin_record(notes, Some("s1"), "board/1", b"hidden one").await.unwrap();
        store.set_plugin_record(notes, Some("s1"), "board/1", b"hidden").await.unwrap();
        assert_eq!(store.plugin_record(notes, None, "board/1").await.unwrap().as_deref(), Some(&b"main"[..]));
        assert_eq!(store.plugin_record(notes, Some("s1"), "board/1").await.unwrap().as_deref(), Some(&b"hidden"[..]));
        assert_eq!(store.plugin_record(notes, Some("s2"), "board/1").await.unwrap(), None);
        assert_eq!(store.plugin_record_keys(notes, Some("s2"), "").await.unwrap(), Vec::<String>::new());
        assert_eq!(store.plugin_records_size(notes, None).await.unwrap(), 4);
        assert_eq!(store.plugin_records_size(notes, Some("s1")).await.unwrap(), 6, "a changed record counts once");

        store.set_plugin_value(notes, None, "showText", "0").await.unwrap();
        store.set_plugin_value(notes, Some("s1"), "showText", "1").await.unwrap();
        assert_eq!(store.plugin_value(notes, None, "showText").await.unwrap().as_deref(), Some("0"));
        assert_eq!(store.plugin_value(notes, Some("s1"), "showText").await.unwrap().as_deref(), Some("1"));
        assert!(store.plugin_keys(notes, Some("s2")).await.unwrap().is_empty());

        let main = Reminder { plugin: notes.to_owned(), session: None, id: "r1".to_owned(), at: 1_000, text: "milk".to_owned() };
        let hidden = Reminder { session: Some("s1".to_owned()), text: "the secret".to_owned(), ..main.clone() };
        store.set_reminder(&main).await.unwrap();
        store.set_reminder(&hidden).await.unwrap();
        assert_eq!(store.reminders(Some(notes)).await.unwrap(), vec![main.clone(), hidden.clone()], "the same id in two places is two reminders");
        assert!(store.cancel_reminder(notes, Some("s1"), "r1").await.unwrap());
        assert_eq!(store.reminders(Some(notes)).await.unwrap(), vec![main.clone()], "the main list's stays");
        store.set_reminder(&hidden).await.unwrap();

        store.add_contact(&NewContact { session: Some("s1".to_owned()), ..contact("ft_bob") }).await.unwrap();
        store.add_contact(&contact("ft_carol")).await.unwrap();
        store.add_plugin_ref("ref-hidden", notes, "ft_bob", "m1").await.unwrap();
        store.add_plugin_ref("ref-main", notes, "ft_carol", "m2").await.unwrap();

        store.remove_session("s1").await.unwrap();
        assert!(store.plugin_record_keys(notes, Some("s1"), "").await.unwrap().is_empty());
        assert!(store.plugin_keys(notes, Some("s1")).await.unwrap().is_empty());
        assert_eq!(store.reminders(None).await.unwrap(), vec![main], "its reminders went with it");
        assert_eq!(store.plugin_ref("ref-hidden").await.unwrap(), None, "and the refs to its messages");
        assert!(store.plugin_ref("ref-main").await.unwrap().is_some());
        assert_eq!(store.plugin_record(notes, None, "board/1").await.unwrap().as_deref(), Some(&b"main"[..]));
        assert_eq!(store.plugin_value(notes, None, "showText").await.unwrap().as_deref(), Some("0"));
    }
}
