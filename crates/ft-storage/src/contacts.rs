use anyhow::Result;
use sqlx::sqlite::SqliteRow;
use sqlx::Row;

use crate::{now, Store};

pub struct NewContact {
    pub device_id: String,
    pub name: String,
    pub card: Vec<u8>,
    pub mailbox: bool,
    pub session: Option<String>,
    pub receipts: bool,
    pub accepted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContactRules {
    pub muted: bool,
    pub accepts_chat: bool,
    pub accepts_calls: bool,
    pub receipts: bool,
    // only ever sent over the direct connection
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
    pub introduced: bool,
    pub added_at: i64,
    // seconds, 0 = forever
    pub keep_for: i64,
    // seconds after reading, 0 = never
    pub burn_after_read: i64,
    pub session: Option<String>,
    pub rules: ContactRules,
    pub accepted: bool,
    pub via_circle: bool,
    pub unblocked_at: Option<i64>,
}

impl Store {
    // a contact the user accepted stays accepted
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

    pub async fn refresh_card(&self, device_id: &str, card: &[u8]) -> Result<()> {
        sqlx::query("UPDATE contacts SET card = ? WHERE device_id = ?").bind(card).bind(device_id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn set_accepted(&self, device_id: &str, accepted: bool) -> Result<()> {
        sqlx::query("UPDATE contacts SET accepted = ? WHERE device_id = ?").bind(accepted).bind(device_id).execute(&self.pool).await?;
        Ok(())
    }

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

    pub async fn contact(&self, device_id: &str) -> Result<Option<Contact>> {
        let row = sqlx::query("SELECT * FROM contacts WHERE device_id = ?").bind(device_id).fetch_optional(&self.pool).await?;
        Ok(row.as_ref().map(contact_from))
    }

    pub async fn contacts(&self) -> Result<Vec<Contact>> {
        let rows = sqlx::query("SELECT * FROM contacts WHERE session IS NULL AND accepted = 1 ORDER BY name").fetch_all(&self.pool).await?;
        Ok(rows.iter().map(contact_from).collect())
    }

    pub async fn all_contacts(&self) -> Result<Vec<Contact>> {
        let rows = sqlx::query("SELECT * FROM contacts ORDER BY name").fetch_all(&self.pool).await?;
        Ok(rows.iter().map(contact_from).collect())
    }

    pub async fn session_contacts(&self, session: &str) -> Result<Vec<Contact>> {
        let rows = sqlx::query("SELECT * FROM contacts WHERE session = ? AND accepted = 1 ORDER BY name")
            .bind(session)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(contact_from).collect())
    }

    pub async fn remove_contact(&self, device_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM pending_outbox WHERE contact = ?").bind(device_id).execute(&self.pool).await?;
        sqlx::query("DELETE FROM contacts WHERE device_id = ?").bind(device_id).execute(&self.pool).await?;
        self.forget_orphans().await
    }

    pub async fn rename_contact(&self, device_id: &str, name: &str) -> Result<()> {
        self.update_contact("UPDATE contacts SET name = ? WHERE device_id = ?", name, device_id).await
    }

    pub async fn set_blocked(&self, device_id: &str, blocked: bool) -> Result<()> {
        sqlx::query(
            "UPDATE contacts SET unblocked_at = CASE WHEN blocked AND NOT ? THEN ? ELSE unblocked_at END, blocked = ?
             WHERE device_id = ?",
        )
        .bind(blocked)
        .bind(now())
        .bind(blocked)
        .bind(device_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_introduced(&self, device_id: &str) -> Result<()> {
        sqlx::query("UPDATE contacts SET introduced = 1 WHERE device_id = ?").bind(device_id).execute(&self.pool).await?;
        Ok(())
    }

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

    pub async fn set_via_circle(&self, device_id: &str, via_circle: bool) -> Result<()> {
        sqlx::query("UPDATE contacts SET via_circle = ? WHERE device_id = ?").bind(via_circle).bind(device_id).execute(&self.pool).await?;
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
        unblocked_at: row.get("unblocked_at"),
    }
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    use super::*;
    use crate::tests::{contact, message, store};

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

    #[tokio::test]
    async fn unblocking_a_contact_remembers_when() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        async fn unblocked_at(store: &Store) -> Option<i64> {
            store.contact("ft_bob").await.expect("reads").expect("present").unblocked_at
        }
        assert_eq!(unblocked_at(&store).await, None, "never blocked");
        store.set_blocked("ft_bob", false).await.expect("not blocked to begin with");
        assert_eq!(unblocked_at(&store).await, None, "nothing was unblocked");

        store.set_blocked("ft_bob", true).await.expect("blocks");
        assert_eq!(unblocked_at(&store).await, None, "blocking leaves it");
        let before = now();
        store.set_blocked("ft_bob", false).await.expect("unblocks");
        let at = unblocked_at(&store).await.expect("remembered");
        assert!((before..=now()).contains(&at), "this phone's clock at the unblock");

        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        store.set_blocked("ft_bob", false).await.expect("unblocks again");
        assert_eq!(unblocked_at(&store).await, Some(at), "already unblocked: kept");
        store.set_blocked("ft_bob", true).await.expect("blocks again");
        assert_eq!(unblocked_at(&store).await, Some(at), "the last unblock stays while blocked");
    }

    #[tokio::test]
    async fn a_contact_from_before_has_no_unblock() {
        let options = SqliteConnectOptions::new().in_memory(true).foreign_keys(true);
        let pool = SqlitePoolOptions::new().max_connections(1).connect_with(options).await.expect("opens");
        sqlx::migrate!("./migrations").run_to(21, &pool).await.expect("migrates to 0021");
        sqlx::query("INSERT INTO contacts (device_id, name, card, mailbox, added_at) VALUES ('ft_bob', 'Bob', x'01', 1, 1)").execute(&pool).await.expect("a contact");

        let store = Store::with(pool, None).await.expect("migrates the rest");
        assert_eq!(store.contact("ft_bob").await.expect("reads").expect("present").unblocked_at, None);
    }

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
    async fn the_encrypted_channel_is_kept_per_contact() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        assert!(store.channel("ft_bob").await.expect("reads").is_none());
        store.save_channel("ft_bob", "sessions").await.expect("saves");
        assert_eq!(store.channel("ft_bob").await.expect("reads").as_deref(), Some("sessions"));
    }

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

    #[tokio::test]
    async fn a_new_contact_takes_the_receipts_default_it_is_given() {
        let store = store().await;
        store.add_contact(&NewContact { receipts: false, ..contact("ft_bob") }).await.expect("adds");
        assert!(!store.contact("ft_bob").await.unwrap().unwrap().rules.receipts);
    }

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
}
