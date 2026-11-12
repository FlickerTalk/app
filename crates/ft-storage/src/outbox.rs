use std::collections::HashMap;

use anyhow::Result;
use sqlx::sqlite::{SqliteExecutor, SqliteRow};
use sqlx::Row;

use crate::{Message, MessageState, Store};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxEntry {
    pub message_id: String,
    pub contact: String,
    pub created_at: i64,
    pub attempts: i64,
    pub next_attempt: i64,
    pub in_mailbox: bool,
    // ms, None if no copy went to the mailbox yet
    pub mailed_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateEntry {
    pub packet_id: String,
    pub contact: String,
    // encoded once, so a retry sends the same id and time
    pub packet: Vec<u8>,
    pub created_at: i64,
    pub attempts: i64,
    pub next_attempt: i64,
    pub in_mailbox: bool,
}

impl Store {
    pub async fn enqueue(&self, message_id: &str, contact: &str, now: i64) -> Result<()> {
        Self::enqueue_with(&self.pool, message_id, contact, now).await
    }

    async fn enqueue_with<'e>(executor: impl SqliteExecutor<'e>, message_id: &str, contact: &str, now: i64) -> Result<()> {
        sqlx::query("INSERT INTO pending_outbox (message_id, contact, created_at, next_attempt) VALUES (?, ?, ?, ?) ON CONFLICT DO NOTHING")
            .bind(message_id)
            .bind(contact)
            .bind(now)
            .bind(now)
            .execute(executor)
            .await?;
        Ok(())
    }

    pub async fn queue_message(&self, message: &Message, reply_to: Option<&str>, now: i64) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        Self::insert_message_with(&mut *tx, message).await?;
        if let Some(quoted) = reply_to {
            Self::set_reply_with(&mut *tx, &message.message_id, quoted).await?;
        }
        Self::enqueue_with(&mut *tx, &message.message_id, &message.contact, now).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn enqueue_at(&self, message_id: &str, contact: &str, now: i64, send_at: i64) -> Result<()> {
        sqlx::query("INSERT INTO pending_outbox (message_id, contact, created_at, next_attempt) VALUES (?, ?, ?, ?) ON CONFLICT DO NOTHING")
            .bind(message_id)
            .bind(contact)
            .bind(now)
            .bind(send_at)
            .execute(&self.pool)
            .await?;
        sqlx::query("INSERT INTO scheduled (message_id, send_at) VALUES (?, ?) ON CONFLICT (message_id) DO UPDATE SET send_at = excluded.send_at")
            .bind(message_id)
            .bind(send_at)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn unschedule(&self, message_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM scheduled WHERE message_id = ?").bind(message_id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn scheduled(&self, contact: &str) -> Result<HashMap<String, i64>> {
        let rows = sqlx::query("SELECT s.message_id, s.send_at FROM scheduled s JOIN messages m ON m.message_id = s.message_id WHERE m.contact = ?")
            .bind(contact)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|row| (row.get("message_id"), row.get("send_at"))).collect())
    }

    pub async fn scheduled_all(&self) -> Result<HashMap<String, i64>> {
        let rows = sqlx::query("SELECT message_id, send_at FROM scheduled").fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(|row| (row.get("message_id"), row.get("send_at"))).collect())
    }

    pub async fn due(&self, now: i64) -> Result<Vec<OutboxEntry>> {
        let rows = sqlx::query("SELECT * FROM pending_outbox WHERE next_attempt <= ? ORDER BY created_at").bind(now).fetch_all(&self.pool).await?;
        Ok(rows.iter().map(outbox_from).collect())
    }

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

    pub async fn mailed(&self, message_id: &str, at: i64) -> Result<()> {
        sqlx::query("UPDATE pending_outbox SET mailed_at = ? WHERE message_id = ?").bind(at).bind(message_id).execute(&self.pool).await?;
        Ok(())
    }

    // one already shown as sent stays sent, and one that reached the contact is left alone
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

    pub async fn enqueue_update(&self, packet_id: &str, contact: &str, packet: &[u8], now: i64) -> Result<()> {
        sqlx::query("INSERT INTO update_outbox (packet_id, contact, packet, created_at, next_attempt) VALUES (?, ?, ?, ?, ?) ON CONFLICT DO NOTHING")
            .bind(packet_id)
            .bind(contact)
            .bind(packet)
            .bind(now)
            .bind(now)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_outbox(&self) -> Result<Vec<UpdateEntry>> {
        let rows = sqlx::query("SELECT * FROM update_outbox ORDER BY created_at, packet_id").fetch_all(&self.pool).await?;
        Ok(rows.iter().map(update_from).collect())
    }

    pub async fn updates_due(&self, now: i64) -> Result<Vec<UpdateEntry>> {
        let rows = sqlx::query("SELECT * FROM update_outbox WHERE next_attempt <= ? ORDER BY created_at, packet_id").bind(now).fetch_all(&self.pool).await?;
        Ok(rows.iter().map(update_from).collect())
    }

    pub async fn reschedule_update(&self, packet_id: &str, attempts: i64, next_attempt: i64, in_mailbox: bool) -> Result<()> {
        sqlx::query("UPDATE update_outbox SET attempts = ?, next_attempt = ?, in_mailbox = ? WHERE packet_id = ?")
            .bind(attempts)
            .bind(next_attempt)
            .bind(in_mailbox)
            .bind(packet_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn dequeue_update(&self, packet_id: &str) -> Result<bool> {
        Ok(sqlx::query("DELETE FROM update_outbox WHERE packet_id = ?").bind(packet_id).execute(&self.pool).await?.rows_affected() > 0)
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

fn update_from(row: &SqliteRow) -> UpdateEntry {
    UpdateEntry {
        packet_id: row.get("packet_id"),
        contact: row.get("contact"),
        packet: row.get("packet"),
        created_at: row.get("created_at"),
        attempts: row.get("attempts"),
        next_attempt: row.get("next_attempt"),
        in_mailbox: row.get("in_mailbox"),
    }
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    use super::*;
    use crate::migrator;
    use crate::tests::{contact, file, message, store};

    #[tokio::test]
    async fn a_message_for_later_waits_in_the_outbox_until_its_time() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.insert_message(&message("m1", "ft_bob", true, 1)).await.expect("inserts");
        store.enqueue_at("m1", "ft_bob", 1_000, 5_000).await.expect("queues for later");
        assert!(store.due(4_999).await.unwrap().is_empty(), "not yet");
        assert_eq!(store.due(5_000).await.unwrap().len(), 1, "its time has come");
        assert_eq!(store.scheduled("ft_bob").await.unwrap(), HashMap::from([("m1".to_owned(), 5_000)]));
        assert_eq!(store.scheduled_all().await.unwrap().len(), 1);
        store.unschedule("m1").await.expect("sent");
        assert!(store.scheduled("ft_bob").await.unwrap().is_empty());
        store.enqueue_at("m1", "ft_bob", 1_000, 9_000).await.expect("queues again");
        store.forget_message("m1").await.expect("forgets");
        assert!(store.scheduled_all().await.unwrap().is_empty(), "gone with the message");
        assert!(store.outbox().await.unwrap().is_empty(), "and out of the queue");
    }

    #[tokio::test]
    async fn a_message_taken_back_halfway_is_left_as_it_was() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.insert_message(&message("m1", "ft_bob", true, 1)).await.expect("inserts");
        store.insert_file(&file("m1")).await.expect("attaches");
        store.set_reaction("m1", false, Some("👍"), 1).await.expect("reacts");
        store.set_message_note("m1", "com.example.translate", "hola", 1).await.expect("notes");
        store.enqueue_at("m1", "ft_bob", 1_000, 9_000).await.expect("queues for later");
        sqlx::query("CREATE TRIGGER forced_failure BEFORE INSERT ON message_deletions BEGIN SELECT RAISE(ABORT, 'forced'); END")
            .execute(&store.pool)
            .await
            .expect("creates the trigger");

        assert!(store.mark_deleted("m1").await.is_err());
        assert_eq!(store.message("m1").await.unwrap().expect("still here").body, "text m1");
        assert!(store.file("m1").await.unwrap().is_some(), "the file stays");
        assert_eq!(store.reactions("ft_bob").await.unwrap().len(), 1, "the reaction stays");
        assert_eq!(store.notes("ft_bob").await.unwrap().len(), 1, "the note stays");
        assert_eq!(store.outbox().await.unwrap().len(), 1, "it still waits to be sent");
        assert_eq!(store.scheduled_all().await.unwrap(), HashMap::from([("m1".to_owned(), 9_000)]));
        assert!(!store.is_deleted("m1").await.unwrap());
    }

    #[tokio::test]
    async fn a_queued_message_waits_in_the_outbox_with_its_answer() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.insert_message(&message("m1", "ft_bob", false, 1)).await.expect("inserts");

        store.queue_message(&message("m2", "ft_bob", true, 2), Some("m1"), 1_000).await.expect("queues");
        assert_eq!(store.message("m2").await.unwrap().expect("kept").state, MessageState::Pending);
        assert_eq!(store.reply_to("m2").await.unwrap(), Some("m1".to_owned()));
        let outbox = store.outbox().await.unwrap();
        assert_eq!(outbox.len(), 1);
        assert_eq!((outbox[0].message_id.as_str(), outbox[0].contact.as_str(), outbox[0].created_at, outbox[0].next_attempt), ("m2", "ft_bob", 1_000, 1_000));

        store.queue_message(&message("m3", "ft_bob", true, 3), None, 2_000).await.expect("queues");
        assert_eq!(store.reply_to("m3").await.unwrap(), None);
        assert_eq!(store.outbox().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_message_that_cannot_wait_in_the_outbox_is_not_kept() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.insert_message(&message("m1", "ft_bob", false, 1)).await.expect("inserts");
        sqlx::query("CREATE TRIGGER forced_failure BEFORE INSERT ON pending_outbox BEGIN SELECT RAISE(ABORT, 'forced'); END")
            .execute(&store.pool)
            .await
            .expect("creates the trigger");

        assert!(store.queue_message(&message("m2", "ft_bob", true, 2), Some("m1"), 1_000).await.is_err());
        assert!(store.message("m2").await.unwrap().is_none(), "no message left without its place in the outbox");
        assert_eq!(store.reply_to("m2").await.unwrap(), None);
        assert!(store.outbox().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_update_waits_in_its_queue_until_its_receipt() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.enqueue_update("p1", "ft_bob", &[1, 2, 3], 1_000).await.expect("queues");
        store.enqueue_update("p1", "ft_bob", &[9], 1_000).await.expect("twice is once");
        let queued = store.update_outbox().await.unwrap();
        assert_eq!(queued.len(), 1);
        assert_eq!((queued[0].packet_id.as_str(), queued[0].contact.as_str(), queued[0].packet.as_slice()), ("p1", "ft_bob", &[1u8, 2, 3][..]));
        assert_eq!((queued[0].created_at, queued[0].attempts, queued[0].in_mailbox), (1_000, 0, false));
        assert_eq!(store.updates_due(1_000).await.unwrap().len(), 1, "due at once");

        store.reschedule_update("p1", 1, 5_000, true).await.expect("reschedules");
        assert!(store.updates_due(4_999).await.unwrap().is_empty(), "not yet");
        let again = store.updates_due(5_000).await.unwrap();
        assert_eq!((again[0].attempts, again[0].in_mailbox), (1, true));

        assert!(store.dequeue_update("p1").await.expect("its receipt"));
        assert!(!store.dequeue_update("p1").await.expect("again"), "it was not there any more");
        store.enqueue_update("p2", "ft_bob", &[4], 1_000).await.expect("queues");
        store.remove_contact("ft_bob").await.expect("removes");
        assert!(store.update_outbox().await.unwrap().is_empty(), "gone with the contact");
    }

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

    #[tokio::test]
    async fn an_outbox_from_before_has_no_time_of_its_last_copy() {
        let options = SqliteConnectOptions::new().in_memory(true).foreign_keys(true);
        let pool = SqlitePoolOptions::new().max_connections(1).connect_with(options).await.expect("opens");
        migrator().run_to(12, &pool).await.expect("migrates to 0012");
        sqlx::query("INSERT INTO contacts (device_id, name, card, mailbox, added_at) VALUES ('ft_bob', 'Bob', x'01', 1, 1)").execute(&pool).await.expect("a contact");
        sqlx::query("INSERT INTO messages (message_id, contact, outgoing, body, sent_at, state) VALUES ('m1', 'ft_bob', 1, 'hi', 1, 1)")
            .execute(&pool)
            .await
            .expect("a message");
        sqlx::query("INSERT INTO pending_outbox (message_id, contact, created_at, attempts, next_attempt, in_mailbox) VALUES ('m1', 'ft_bob', 1, 3, 9, 1)")
            .execute(&pool)
            .await
            .expect("in the mailbox");

        let store = Store::with(pool, None).await.expect("migrates the rest");
        let entry = &store.outbox().await.expect("lists")[0];
        assert_eq!((entry.attempts, entry.in_mailbox, entry.mailed_at), (3, true, None));
    }
}
