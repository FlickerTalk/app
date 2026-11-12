use anyhow::Result;
use sqlx::sqlite::SqliteRow;
use sqlx::Row;

use crate::Store;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRecord {
    pub message_id: String,
    pub name: String,
    pub size: i64,
    pub mime: String,
    pub hash: [u8; 32],
    pub chunk: i64,
    pub path: String,
    pub chunks_done: i64,
    pub complete: bool,
    pub failed: bool,
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

impl Store {
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

    pub async fn set_file_waiting(&self, message_id: &str, waiting: bool) -> Result<()> {
        sqlx::query("UPDATE files SET waiting = ? WHERE message_id = ?").bind(waiting).bind(message_id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn file(&self, message_id: &str) -> Result<Option<FileRecord>> {
        let row = sqlx::query("SELECT * FROM files WHERE message_id = ?").bind(message_id).fetch_optional(&self.pool).await?;
        row.as_ref().map(file_from).transpose()
    }

    pub async fn file_paths(&self) -> Result<Vec<String>> {
        let rows = sqlx::query("SELECT DISTINCT path FROM files").fetch_all(&self.pool).await?;
        Ok(rows.iter().map(|row| row.get("path")).collect())
    }

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

    pub async fn set_file_failed(&self, message_id: &str) -> Result<()> {
        sqlx::query("UPDATE files SET failed = 1, complete = 0, chunks_done = 0 WHERE message_id = ?")
            .bind(message_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // not the ones waiting for the user, nor a stranger's still in the requests
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{contact, file, message, store};
    use crate::NewContact;

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
}
