use anyhow::Result;
use sqlx::sqlite::{SqliteExecutor, SqliteRow};
use sqlx::Row;

use crate::{now, Contact, Store};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MessageState {
    // below Pending on purpose, so resending or a late receipt still moves it forward
    NotSent = -1,
    Pending = 0,
    Sent = 1,
    Delivered = 2,
    Read = 3,
}

impl MessageState {
    pub(crate) fn from_rank(rank: i64) -> Self {
        match rank {
            -1 => Self::NotSent,
            0 => Self::Pending,
            1 => Self::Sent,
            2 => Self::Delivered,
            _ => Self::Read,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub message_id: String,
    pub contact: String,
    pub outgoing: bool,
    pub body: String,
    // sender's clock, shown but never used for keeping or ordering
    pub sent_at: i64,
    pub received_at: i64,
    pub state: MessageState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversation {
    pub contact: Contact,
    pub last: Option<Message>,
    pub unread: i64,
}

impl Store {
    pub async fn insert_message(&self, message: &Message) -> Result<bool> {
        Self::insert_message_with(&self.pool, message).await
    }

    pub(crate) async fn insert_message_with<'e>(executor: impl SqliteExecutor<'e>, message: &Message) -> Result<bool> {
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
        .execute(executor)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn message(&self, message_id: &str) -> Result<Option<Message>> {
        let row = sqlx::query("SELECT * FROM messages WHERE message_id = ?").bind(message_id).fetch_optional(&self.pool).await?;
        Ok(row.as_ref().map(message_from))
    }

    // ordered by arrival, so a sender's clock can't push a message into the past
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

    // case-insensitive for Latin letters only
    pub async fn search_messages(&self, contact: &str, query: &str, limit: i64) -> Result<Vec<Message>> {
        let needle = query.trim().to_lowercase().replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
        if needle.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query(
            "SELECT * FROM messages WHERE contact = ? AND lower(body) LIKE ? ESCAPE '\\' ORDER BY received_at DESC, message_id DESC LIMIT ?",
        )
        .bind(contact)
        .bind(format!("%{needle}%"))
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(message_from).collect())
    }

    // never moves a message backwards
    pub async fn advance(&self, message_ids: &[String], state: MessageState) -> Result<()> {
        self.advance_at(message_ids, state, now()).await
    }

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

    pub async fn read_at(&self, message_id: &str) -> Result<Option<i64>> {
        let row = sqlx::query("SELECT read_at FROM messages WHERE message_id = ?")
            .bind(message_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.and_then(|row| row.get("read_at")))
    }

    // a message still waiting to be delivered is never swept, and age counts from when it got here
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
        self.forget_orphans().await?;
        Ok(contacts)
    }

    pub async fn forget_message(&self, message_id: &str) -> Result<bool> {
        let gone = sqlx::query("DELETE FROM messages WHERE message_id = ?")
            .bind(message_id)
            .execute(&self.pool)
            .await?
            .rows_affected();
        sqlx::query("DELETE FROM message_notes WHERE message_id = ?").bind(message_id).execute(&self.pool).await?;
        Ok(gone > 0)
    }

    pub async fn unread(&self, contact: &str) -> Result<Vec<String>> {
        let rows = sqlx::query("SELECT message_id FROM messages WHERE contact = ? AND outgoing = 0 AND state < 3 ORDER BY sent_at")
            .bind(contact)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(|row| row.get("message_id")).collect())
    }

    pub async fn request_conversations(&self, session: Option<&str>) -> Result<Vec<Conversation>> {
        self.conversations_of(self.requests(session).await?).await
    }

    pub async fn conversations(&self) -> Result<Vec<Conversation>> {
        self.conversations_of(self.contacts().await?).await
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{contact, file, message, store};
    use crate::NewContact;

    #[tokio::test]
    async fn messages_are_ordered_and_aged_by_arrival() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.insert_message(&Message { received_at: 1_000_000, ..message("late", "ft_bob", false, 1_000) }).await.unwrap();
        store.insert_message(&Message { received_at: 2_000_000, ..message("early", "ft_bob", false, 999_999_999_999) }).await.unwrap();
        let ids: Vec<_> = store.messages("ft_bob", 10).await.unwrap().into_iter().map(|m| m.message_id).collect();
        assert_eq!(ids, ["late", "early"], "in the order they arrived, whatever their clocks said");
        assert_eq!(store.message("late").await.unwrap().unwrap().sent_at, 1_000, "the sender's time is kept");
        store.insert_message(&Message { received_at: 0, ..message("now", "ft_bob", false, 5) }).await.unwrap();
        let stamped = store.message("now").await.unwrap().unwrap();
        assert!(stamped.received_at > 2_000_000, "stamped on arrival: {}", stamped.received_at);
        assert_eq!(store.messages("ft_bob", 10).await.unwrap().last().unwrap().message_id, "now");
    }

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

    #[tokio::test]
    async fn messages_are_searched_by_their_words_in_one_conversation() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.add_contact(&contact("ft_carol")).await.expect("adds");
        store.insert_message(&Message { body: "Dinner on Friday?".into(), ..message("m1", "ft_bob", false, 1) }).await.expect("inserts");
        store.insert_message(&Message { body: "100% sure, friday it is".into(), ..message("m2", "ft_bob", true, 2) }).await.expect("inserts");
        store.insert_message(&Message { body: "friday works".into(), ..message("m3", "ft_carol", false, 3) }).await.expect("inserts");
        let ids = |found: Vec<Message>| found.into_iter().map(|m| m.message_id).collect::<Vec<_>>();
        assert_eq!(ids(store.search_messages("ft_bob", "FRIDAY", 10).await.unwrap()), vec!["m2".to_owned(), "m1".to_owned()], "newest first, any case, this conversation only");
        assert_eq!(ids(store.search_messages("ft_bob", "100%", 10).await.unwrap()), vec!["m2".to_owned()], "a percent sign is a letter");
        assert_eq!(ids(store.search_messages("ft_bob", "_", 10).await.unwrap()), Vec::<String>::new(), "an underscore too");
        assert_eq!(ids(store.search_messages("ft_bob", "friday", 1).await.unwrap()), vec!["m2".to_owned()], "at most the limit");
        assert!(store.search_messages("ft_bob", "   ", 10).await.unwrap().is_empty(), "nothing for nothing");
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
}
