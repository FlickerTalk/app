use anyhow::Result;
use sqlx::sqlite::SqliteRow;
use sqlx::Row;

use crate::{now, Store};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reminder {
    pub plugin: String,
    pub session: Option<String>,
    pub id: String,
    pub at: i64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRef {
    pub plugin: String,
    pub contact: String,
    pub message_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPlugin {
    pub id: String,
    pub version: String,
    pub granted: String,
    pub installed_at: i64,
}

impl Store {
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
        sqlx::query("DELETE FROM message_notes WHERE plugin = ?").bind(id).execute(&self.pool).await?;
        sqlx::query("DELETE FROM chat_hooks WHERE plugin = ?").bind(id).execute(&self.pool).await?;
        Ok(())
    }

    // records, reminders and refs belong to one place (main list or a hidden session), the same key
    // in two places is two things

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

    pub async fn forget_reminders(&self, plugin: &str) -> Result<bool> {
        let gone = sqlx::query("DELETE FROM reminders WHERE plugin = ?").bind(plugin).execute(&self.pool).await?;
        Ok(gone.rows_affected() > 0)
    }

    // every place, the caller must not show a closed session's
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

#[cfg(test)]
mod tests {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    use super::*;
    use crate::tests::{contact, store};
    use crate::NewContact;

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

        store.install_plugin("com.example.ai", "2.2.0", r#"{"network":[]}"#).await.unwrap();
        let ai = store.plugin("com.example.ai").await.unwrap().expect("installed");
        assert_eq!((ai.version.as_str(), ai.granted.as_str()), ("2.2.0", r#"{"network":["api.openai.com"]}"#));

        store.remove_plugin("com.example.ai").await.unwrap();
        assert_eq!(store.plugins().await.unwrap().len(), 1);
    }

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

        store.install_plugin("com.example.code", "1.0.0", "{}").await.unwrap();
        store.remove_plugin("com.example.code").await.unwrap();
        assert_eq!(store.plugin_keys("com.example.code", None).await.unwrap(), Vec::<String>::new());
    }

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
