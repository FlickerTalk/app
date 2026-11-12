//! The whole history lives here and nowhere else.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqliteConnection, SqlitePool, SqlitePoolOptions};
use sqlx::ConnectOptions;

mod calls;
mod chat_hooks;
mod circles;
mod contacts;
mod edits;
mod files;
mod identity;
mod messages;
mod outbox;
mod pinned;
mod plugins;
mod reactions;
mod replies;
mod sessions;
mod settings;

pub use calls::{CallOutcome, CallRecord};
pub use chat_hooks::{ChatHook, MessageNote};
pub use circles::{CircleConversation, CircleMember, CircleMessage, CircleOutboxEntry, CircleRecord};
pub use contacts::{Contact, ContactRules, NewContact};
pub use files::FileRecord;
pub use identity::StoredIdentity;
pub use messages::{Conversation, Message, MessageState};
pub use outbox::{OutboxEntry, UpdateEntry};
pub use plugins::{InstalledPlugin, PluginRef, Reminder};
pub use reactions::Reactions;

#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
    // an in-memory database dies with its last connection, and the pool swaps its connection when a
    // query future is dropped
    _keep_alive: Option<Arc<Mutex<SqliteConnection>>>,
}

static IN_MEMORY_DATABASES: AtomicUsize = AtomicUsize::new(0);

impl Store {
    pub async fn open(path: &Path) -> Result<Self> {
        let options = SqliteConnectOptions::new().filename(path).create_if_missing(true).foreign_keys(true);
        Self::with(SqlitePoolOptions::new().max_connections(4).connect_with(options).await?, None).await
    }

    pub async fn open_in_memory() -> Result<Self> {
        let name = format!("file:ft-memory-{}-{}", std::process::id(), IN_MEMORY_DATABASES.fetch_add(1, Ordering::Relaxed));
        let options = SqliteConnectOptions::new().filename(name).in_memory(true).shared_cache(true).foreign_keys(true);
        let keep_alive = options.connect().await?;
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .min_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect_with(options)
            .await?;
        Self::with(pool, Some(Arc::new(Mutex::new(keep_alive)))).await
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    async fn with(pool: SqlitePool, keep_alive: Option<Arc<Mutex<SqliteConnection>>>) -> Result<Self> {
        sqlx::migrate!("./migrations").run(&pool).await.context("cannot migrate the local database")?;
        Ok(Self { pool, _keep_alive: keep_alive })
    }

    pub async fn snapshot(&self, path: &Path) -> Result<()> {
        let target = path.to_str().context("the snapshot path is not valid text")?;
        sqlx::query("VACUUM INTO ?").bind(target).execute(&self.pool).await.context("cannot copy the database")?;
        if !path.exists() {
            anyhow::bail!("the database copy was not written");
        }
        Ok(())
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) async fn store() -> Store {
        Store::open_in_memory().await.expect("opens")
    }

    #[tokio::test]
    async fn an_in_memory_store_survives_its_connection_being_replaced() {
        let store = store().await;
        store.add_contact(&contact("ft_bob")).await.expect("adds");
        store.pool.acquire().await.expect("acquires").close().await.expect("closes");
        assert!(store.contact("ft_bob").await.expect("still has its tables").is_some(), "still has its data");
    }

    #[tokio::test]
    async fn two_in_memory_stores_are_two_databases() {
        let (first, second) = (store().await, store().await);
        first.add_contact(&contact("ft_bob")).await.expect("adds");
        assert!(second.contact("ft_bob").await.expect("lists").is_none());
    }

    pub(crate) fn contact(id: &str) -> NewContact {
        NewContact { device_id: id.to_owned(), name: "Bob".to_owned(), card: vec![1, 2], mailbox: true, session: None, receipts: true, accepted: true }
    }

    pub(crate) fn message(id: &str, contact: &str, outgoing: bool, sent_at: i64) -> Message {
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

    pub(crate) fn file(id: &str) -> FileRecord {
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

    #[tokio::test]
    async fn a_snapshot_is_a_complete_copy() {
        let path = std::env::temp_dir().join(format!("ft-storage-snapshot-{}.db", std::process::id()));
        let source = path.with_extension("source.db");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&source);
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
