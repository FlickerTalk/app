use anyhow::Result;
use sqlx::sqlite::SqliteRow;
use sqlx::Row;

use crate::{now, MessageState, Store};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CircleRecord {
    pub id: String,
    pub name: String,
    pub card: Vec<u8>,
    pub revision: i64,
    pub admins_only: bool,
    pub session: Option<String>,
    pub left: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CircleMember {
    pub device_id: String,
    pub name: String,
    pub admin: bool,
}

// text, an event (created, joined, left, removed, renamed) or a control packet (card, leave) that
// is never shown
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CircleMessage {
    pub message_id: String,
    pub circle: String,
    pub sender: String,
    pub outgoing: bool,
    pub kind: String,
    pub body: String,
    pub sent_at: i64,
    pub received_at: i64,
    pub state: MessageState,
}

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

impl Store {
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

    pub async fn circles_with(&self, device_id: &str) -> Result<Vec<String>> {
        let rows = sqlx::query("SELECT m.circle FROM circle_members m JOIN circles c ON c.id = m.circle WHERE m.device_id = ? AND c.left = 0")
            .bind(device_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(|row| row.get("circle")).collect())
    }

    pub async fn remove_circle(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM circles WHERE id = ?").bind(id).execute(&self.pool).await?;
        self.forget_orphans().await
    }

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

    pub async fn circle_unread(&self, circle: &str) -> Result<Vec<String>> {
        let rows = sqlx::query("SELECT message_id FROM circle_messages WHERE circle = ? AND outgoing = 0 AND kind = 'text' AND state < 3 ORDER BY received_at")
            .bind(circle)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(|row| row.get("message_id")).collect())
    }

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

    pub async fn circle_due(&self, now: i64) -> Result<Vec<CircleOutboxEntry>> {
        let rows = sqlx::query("SELECT * FROM circle_outbox WHERE next_attempt <= ? ORDER BY created_at").bind(now).fetch_all(&self.pool).await?;
        Ok(rows.iter().map(circle_outbox_from).collect())
    }

    pub async fn circle_outbox(&self) -> Result<Vec<CircleOutboxEntry>> {
        let rows = sqlx::query("SELECT * FROM circle_outbox ORDER BY created_at").fetch_all(&self.pool).await?;
        Ok(rows.iter().map(circle_outbox_from).collect())
    }

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

    pub async fn circle_dequeue(&self, message_id: &str, contact: &str) -> Result<bool> {
        let gone = sqlx::query("DELETE FROM circle_outbox WHERE message_id = ? AND contact = ?")
            .bind(message_id)
            .bind(contact)
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok(gone > 0)
    }

    pub async fn circle_dequeue_member(&self, circle: &str, contact: &str) -> Result<()> {
        sqlx::query("DELETE FROM circle_outbox WHERE contact = ? AND message_id IN (SELECT message_id FROM circle_messages WHERE circle = ?)")
            .bind(contact)
            .bind(circle)
            .execute(&self.pool)
            .await?;
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{contact, store};

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
}
