//! The whole history lives here and nowhere else.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use sqlx::migrate::Migrator;
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

// Installed phones recorded these checksums; only the comments in those files have changed since.
const RELEASED_CHECKSUMS: [(i64, &str); 23] = [
    (1, "7ebca7399c3d3bc8d6127ab852f2963fce161afa66dee6b73e0131e1baf6b5a28b5bb1b5f41e2761eb3e41894cd327c8"),
    (2, "ca571259d5660a8ed995ae64274c57247b79db151598d288c4a51dd6160f28a3274cde5e2e46bdd05427b6142ee17cd6"),
    (3, "2e88aa77a12b66f795477253f16b3218056f3d580bc7b6fb620172f5102af3c344e3ed83a419860bf8330d809fdf908c"),
    (4, "d917061b0103fe8a09bb0552ce9ef99938f092f91e0df4e5e1c3355aea4ebfc5b7306d11def45b6e8ce0a76f90fb6898"),
    (5, "4d43f2f4450c28f0599c75697287cb209ebba6d692513bb68235695a069e436350c3ca3d97913f5588d3cf9b562bfe51"),
    (6, "02b3fe9e5ad0e9fe1f792ae81aa4d5f1969e0f63f41caf2ea672263dc91c4de2dac709e0a7f66964fa0477c9f3138a0b"),
    (7, "b21b87c7d721d9f3e43c7930c1607f016e1649e7b7119cf707ed093a1be0b6a414af92e1d7963cdcf928815799e3ae72"),
    (8, "f3679c442035acdd7ab4cd71dd63cde5375e246972e3ae216631e2bfbcb64a4742bdf1d02bfdaf967551266d44dcc6e2"),
    (9, "eb0ec40ce92129dcf1f4f37978f00b616ed589a292252c3b4487943c0444feb7a1811f79ec2f0269994bdfeb25e65f14"),
    (10, "c078f4e0ff4c722b4a77c1288d50f0154209bc752bdc6f665bb7c755fa0e7e9ff367448729f86938aadfe28f8eb5c7bb"),
    (11, "cc53af2af49c4aded952ca4509814a7109bb2f4955707f1a543e636d6baec6ad0dc40e9e5d8f664a034ba9b185bf87e2"),
    (12, "1b87f05b5d54495c4fdc84c596ec1fbfeeb355eaea51db22077b083bb203e3de2dbe4ee9fee6d6f215288e6fc24cf752"),
    (13, "85d9da94a7a7c8b9ecd2af65fd7791f987f38523771a7ef54186d2e9f36c54aef078aa81f4e48e84a774375ad0fadecb"),
    (14, "3d98858108e97afee3ad98238f56e8ec12ae53504a99a78385510b6c29c14a32e58d80608859813a4b5303eca0f904e4"),
    (15, "9f0b8682ef7c7d631712b4d1929381884464c148de4b5abe621d7871d77761be50298cae526e584441e93b47fd4659f3"),
    (16, "b392606dd3d137ea46adc8631bcd09ea4bf57cb4905c0290232915f7d5ebe054d81c6f381454ee25ec9d025c64e78faf"),
    (17, "18fa999c9ff920a8a70ac60fe7e711eb06b5da68d0f41ab5ef63582e5d3c9c3fcf335b1de2629b59a33da526f9ce1712"),
    (18, "4108ef4c5890c2969f55f0ca40a89fe6bd30d4b8ee8aafe1d23bc1233711f3cce07382724728baf126046905a0a4793c"),
    (19, "bbc64f64741ff1025bcd13875f9affb5e3f92cc82b6b1f64a129ad774bdbffd9b2ab26e6573432f7d55bc19e59b50578"),
    (20, "bf67f9e4ca5108fa16238b27ac78c4606a6e589e72a71f612d77de9a65b285e99f962633b7596e6b01078f57befca5ff"),
    (21, "02c14a9f7cdb64743d963cbea812152639ed479b8aca9d82abd2de938618264e37e6bd8addc336d2f0e79a8a52339c6b"),
    (22, "b9d2c852b5bcd4dc159add5ed56fffe62232771fad30401d12216e0649da88d93bccc2679c70443f202437b8e457af53"),
    (23, "27f01a5d109768cb60e46d0dea480149912f7bd8a13a3b4fb1eeb8dc64b6fab82d13161ec1ecc4ae1656d27973d91894"),
];

pub(crate) fn migrator() -> Migrator {
    let mut migrator = sqlx::migrate!("./migrations");
    migrator.migrations = migrator
        .migrations
        .iter()
        .cloned()
        .map(|mut migration| {
            if let Some((_, hex)) = RELEASED_CHECKSUMS.iter().find(|(version, _)| *version == migration.version) {
                migration.checksum = (0..hex.len())
                    .step_by(2)
                    .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).expect("a released checksum is hex"))
                    .collect::<Vec<_>>()
                    .into();
            }
            migration
        })
        .collect::<Vec<_>>()
        .into();
    migrator
}

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
        migrator().run(&pool).await.context("cannot migrate the local database")?;
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
    use std::collections::HashMap;

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

    #[tokio::test]
    async fn opens_a_database_that_a_released_version_migrated() {
        let path = std::env::temp_dir().join(format!("ft-storage-released-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let store = Store::open(&path).await.expect("creates");
            store.add_contact(&contact("ft_bob")).await.expect("adds");
            for (version, checksum) in RELEASED_CHECKSUMS {
                sqlx::query("UPDATE _sqlx_migrations SET checksum = unhex(?) WHERE version = ?")
                    .bind(checksum)
                    .bind(version)
                    .execute(&store.pool)
                    .await
                    .expect("records the released checksum");
            }
            store.close().await;
        }
        let store = Store::open(&path).await.expect("opens what a released version migrated");
        assert!(store.contact("ft_bob").await.expect("reads").is_some());
        store.close().await;
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn a_new_database_records_the_released_checksums() {
        let store = store().await;
        let recorded: HashMap<i64, String> = sqlx::query_as("SELECT version, lower(hex(checksum)) FROM _sqlx_migrations")
            .fetch_all(&store.pool)
            .await
            .expect("reads")
            .into_iter()
            .collect();
        for (version, checksum) in RELEASED_CHECKSUMS {
            assert_eq!(recorded.get(&version).map(String::as_str), Some(checksum), "migration {version}");
        }
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
