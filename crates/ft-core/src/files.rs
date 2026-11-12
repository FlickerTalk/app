//! Files only ever go over a direct connection, never through the mailbox.

use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, ensure, Context, Result};
use ft_protocol::{Body, MessageId, Packet, FILE_CHUNK};
use ft_storage::{Contact, FileRecord, Message, MessageState, OutboxEntry, Store};

use crate::{now, retry_delay, written_while_blocked, Core, Event, RECEIPT_WAIT};

pub const FILE_WINDOW: u32 = 16;
pub const FILE_STALL: Duration = Duration::from_secs(15);
const MAX_CHUNK: u32 = 256 * 1024;
pub const MAX_FILE_SIZE: u64 = 2 * 1024 * 1024 * 1024;
pub const DEFAULT_AUTO_DOWNLOAD: i64 = 10_000_000;
const AUTO_DOWNLOAD: &str = "auto_download";
const PROGRESS_EVERY: i64 = 8;

#[derive(Default)]
pub(crate) struct Transfer {
    // exclusive
    requested_upto: i64,
    in_flight: bool,
    last_activity: Option<Instant>,
    attempts: i64,
    next_try: Option<Instant>,
}

impl Core {
    pub fn set_files_dir(&self, dir: PathBuf) {
        let _ = self.files_dir.set(dir);
    }

    pub(crate) fn files_dir(&self) -> Result<&Path> {
        self.files_dir.get().map(PathBuf::as_path).ok_or_else(|| anyhow!("no directory for files"))
    }

    // Kept relative because iOS can move the app's folder on an update
    pub fn file_path(&self, file: &FileRecord) -> PathBuf {
        match self.files_dir.get() {
            Some(dir) => resolve(Path::new(&file.path), dir),
            None => PathBuf::from(&file.path),
        }
    }

    // A forward shares its original's bytes, so only delete what no message points to any more
    pub(crate) async fn forget_bytes(&self, files: Vec<FileRecord>) {
        if files.is_empty() {
            return;
        }
        let files_dir = self.files_dir.get().map_or(Path::new(""), PathBuf::as_path);
        let Ok(referenced) = referenced_files(&self.store, files_dir).await else { return };
        for file in files {
            let path = self.file_path(&file);
            if !referenced.contains(&canonical(&path)) {
                let _ = std::fs::remove_file(path);
            }
        }
    }

    fn stored_path(&self, path: &Path) -> String {
        match self.files_dir.get().and_then(|dir| path.strip_prefix(dir).ok()) {
            Some(relative) => relative.to_string_lossy().into_owned(),
            None => path.to_string_lossy().into_owned(),
        }
    }

    // 0 = always ask
    pub async fn auto_download_limit(&self) -> Result<i64> {
        Ok(self.store.setting(AUTO_DOWNLOAD).await?.and_then(|value| value.parse().ok()).unwrap_or(DEFAULT_AUTO_DOWNLOAD))
    }

    pub async fn set_auto_download_limit(&self, bytes: i64) -> Result<()> {
        self.store.set_setting(AUTO_DOWNLOAD, &bytes.max(0).to_string()).await
    }

    pub async fn accept_file(&self, message_id: &str) -> Result<()> {
        let message = self.store.message(message_id).await?.context("that message is not here")?;
        let contact = self.chosen(&message.contact).await?;
        let file = self.store.file(message_id).await?.context("that message is not a file")?;
        ensure!(!message.outgoing && !file.complete && !file.failed, "there is nothing to download");
        self.store.set_file_waiting(message_id, false).await?;
        let file = FileRecord { waiting: false, ..file };
        let _ = self.events.send(Event::MessagesChanged { contact: contact.device_id.clone() });
        self.request_chunks(&contact, &file).await
    }

    // The file has to stay at `path`, chunks are read from it on demand
    pub async fn send_file(&self, contact: &str, path: &Path, name: &str, mime: &str) -> Result<String> {
        self.chosen(contact).await?;
        let name = safe_file_name(name);
        let source = path.to_owned();
        let (size, hash) = tokio::task::spawn_blocking(move || hash_file(&source)).await??;
        let packet = Packet::new(Body::File { name: name.clone(), size, mime: mime.to_owned(), hash, chunk: FILE_CHUNK });
        let message_id = packet.id.to_string();
        self.store
            .insert_message(&Message {
                message_id: message_id.clone(),
                contact: contact.to_owned(),
                outgoing: true,
                body: name.clone(),
                sent_at: packet.sent_at as i64,
                received_at: packet.sent_at as i64,
                state: MessageState::Pending,
            })
            .await?;
        self.store
            .insert_file(&FileRecord {
                message_id: message_id.clone(),
                name,
                size: size as i64,
                mime: mime.to_owned(),
                hash,
                chunk: FILE_CHUNK as i64,
                path: self.stored_path(path),
                chunks_done: 0,
                complete: false,
                failed: false,
                waiting: false,
            })
            .await?;
        self.store.enqueue(&message_id, contact, now()).await?;
        let _ = self.events.send(Event::MessagesChanged { contact: contact.to_owned() });

        if let Some(entry) = self.store.outbox().await?.into_iter().find(|e| e.message_id == message_id) {
            self.attempt(&entry).await;
        }
        Ok(message_id)
    }

    pub async fn resume_files(&self) -> Result<()> {
        self.resume(None, FILE_STALL, true).await
    }

    pub async fn resume_files_from(&self, contact: &str, quiet: Duration) -> Result<()> {
        self.resume(Some(contact), quiet, false).await
    }

    async fn resume(&self, only: Option<&str>, quiet: Duration, backoff: bool) -> Result<()> {
        for (contact, file) in self.store.incomplete_incoming_files().await? {
            if only.is_some_and(|only| only != contact) {
                continue;
            }
            let due = {
                let transfers = self.transfers.lock().expect("transfers poisoned");
                match transfers.get(&file.message_id) {
                    None => true,
                    Some(transfer) if transfer.in_flight || !backoff => {
                        transfer.last_activity.is_none_or(|at| at.elapsed() >= quiet)
                    }
                    Some(transfer) => transfer.next_try.is_none_or(|at| Instant::now() >= at),
                }
            };
            if due {
                let contact = self.contact(&contact).await?;
                self.request_chunks(&contact, &file).await?;
            }
        }
        Ok(())
    }

    pub(crate) async fn deliver_file(&self, entry: &OutboxEntry, contact: &Contact, message: &Message, file: &FileRecord) -> Result<()> {
        let body = Body::File {
            name: file.name.clone(),
            size: file.size as u64,
            mime: file.mime.clone(),
            hash: file.hash,
            chunk: file.chunk as u32,
        };
        let packet = Packet::resend(MessageId::parse(&message.message_id)?, message.sent_at as u64, body);
        let attempts = entry.attempts + 1;
        let next = if self.transmit_direct(contact, &packet).await? {
            self.store.advance(std::slice::from_ref(&entry.message_id), MessageState::Sent).await?;
            let _ = self.events.send(Event::MessagesChanged { contact: contact.device_id.clone() });
            RECEIPT_WAIT
        } else {
            retry_delay(attempts)
        };
        if self.store.outbox().await?.iter().any(|e| e.message_id == entry.message_id) {
            self.store.reschedule(&entry.message_id, attempts, now() + next.as_millis() as i64, false).await?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn offered(
        &self,
        contact: &Contact,
        id: MessageId,
        sent_at: u64,
        name: String,
        size: u64,
        mime: String,
        hash: [u8; 32],
        chunk: u32,
    ) -> Result<()> {
        if chunk == 0 || chunk > MAX_CHUNK || size > MAX_FILE_SIZE {
            let _ = self.send_control(contact, Body::FileFailed { file: id }).await;
            bail!("an offer with an impossible size");
        }
        if !contact.rules.accepts_chat || written_while_blocked(contact, sent_at) {
            let _ = self.send_control(contact, Body::Received { ids: vec![id] }).await;
            return Ok(());
        }
        let message_id = id.to_string();
        let name = safe_file_name(&name);
        let dir = self.files_dir()?.join(&message_id);
        tokio::fs::create_dir_all(&dir).await.context("cannot create the file's directory")?;
        let waiting = size as i64 > self.auto_download_limit().await?;
        let stored = self
            .store
            .insert_message(&Message {
                message_id: message_id.clone(),
                contact: contact.device_id.clone(),
                outgoing: false,
                body: name.clone(),
                sent_at: sent_at as i64,
                received_at: now(),
                state: MessageState::Delivered,
            })
            .await?;
        self.store
            .insert_file(&FileRecord {
                message_id: message_id.clone(),
                path: self.stored_path(&dir.join(&name)),
                name,
                size: size as i64,
                mime,
                hash,
                chunk: chunk as i64,
                chunks_done: 0,
                complete: false,
                failed: false,
                waiting,
            })
            .await?;
        if stored {
            self.announce_messages(contact);
        }
        // ack even a repeated offer, the sender is waiting for it
        let _ = self.send_control(contact, self.acknowledgement(contact, id)).await;

        let Some(file) = self.incoming_file(contact, &message_id).await? else { return Ok(()) };
        let busy = self.transfers.lock().expect("transfers poisoned").get(&message_id).is_some_and(|t| t.in_flight);
        // never pull a stranger's file
        if !file.complete && !file.failed && !file.waiting && contact.accepted && !busy {
            self.request_chunks(contact, &file).await?;
        }
        Ok(())
    }

    async fn request_chunks(&self, contact: &Contact, file: &FileRecord) -> Result<()> {
        let (from, chunks) = (file.chunks_done, file.chunks());
        if from >= chunks {
            return self.finish(contact, file).await;
        }
        let count = (chunks - from).min(FILE_WINDOW as i64);
        let reached = self.ask(contact, file, from, count).await?;
        let mut transfers = self.transfers.lock().expect("transfers poisoned");
        let transfer = transfers.entry(file.message_id.clone()).or_default();
        transfer.requested_upto = from + count;
        self.note_request(transfer, reached);
        Ok(())
    }

    async fn ask(&self, contact: &Contact, file: &FileRecord, from: i64, count: i64) -> Result<bool> {
        let body = Body::FileRequest { file: MessageId::parse(&file.message_id)?, from: from as u64, count: count as u32 };
        self.transmit_direct(contact, &Packet::new(body)).await
    }

    fn note_request(&self, transfer: &mut Transfer, reached: bool) {
        transfer.in_flight = reached;
        transfer.last_activity = Some(Instant::now());
        if reached {
            transfer.attempts = 0;
            transfer.next_try = None;
        } else {
            transfer.attempts += 1;
            transfer.next_try = Some(Instant::now() + retry_delay(transfer.attempts));
        }
    }

    pub(crate) async fn serve_chunks(&self, contact: &Contact, file: MessageId, from: u64, count: u32) -> Result<()> {
        let message_id = file.to_string();
        let Some(record) = self.outgoing_file(contact, &message_id).await? else { return Ok(()) };
        let end = (from.saturating_add(count as u64)).min(record.chunks() as u64);
        let mut sent_upto = from as i64;
        for index in from..end {
            let (path, offset, length) = (self.file_path(&record), index * record.chunk as u64, chunk_length(&record, index));
            let Ok(data) = tokio::task::spawn_blocking(move || read_chunk(&path, offset, length)).await? else {
                // The receiver has to hear about it too, or it keeps asking for ever and sits at 0 %
                self.store.set_file_failed(&message_id).await?;
                let _ = self.send_control(contact, Body::FileFailed { file }).await;
                let _ = self.events.send(Event::MessagesChanged { contact: contact.device_id.clone() });
                return Ok(());
            };
            if !self.transmit_direct(contact, &Packet::new(Body::FileChunk { file, index, data })).await? {
                break;
            }
            sent_upto = index as i64 + 1;
        }
        if sent_upto > record.chunks_done && !record.complete {
            self.store.set_file_progress(&message_id, sent_upto, false).await?;
            let _ = self.events.send(Event::MessagesChanged { contact: contact.device_id.clone() });
        }
        Ok(())
    }

    pub(crate) async fn take_chunk(&self, contact: &Contact, file: MessageId, index: u64, data: Vec<u8>) -> Result<()> {
        let message_id = file.to_string();
        let Some(record) = self.incoming_file(contact, &message_id).await? else { return Ok(()) };
        if record.complete || record.failed || record.waiting || !contact.accepted || index as i64 != record.chunks_done {
            return Ok(());
        }
        if data.len() as u64 != chunk_length(&record, index) as u64 {
            bail!("a chunk of the wrong size");
        }
        let (path, offset) = (self.file_path(&record), index * record.chunk as u64);
        tokio::task::spawn_blocking(move || write_chunk(&path, offset, &data)).await??;
        let done = index as i64 + 1;
        self.store.set_file_progress(&message_id, done, false).await?;
        let record = FileRecord { chunks_done: done, ..record };
        if done == record.chunks() {
            return self.finish(contact, &record).await;
        }
        if done % PROGRESS_EVERY == 0 {
            let _ = self.events.send(Event::MessagesChanged { contact: contact.device_id.clone() });
        }

        let next = {
            let mut transfers = self.transfers.lock().expect("transfers poisoned");
            let transfer = transfers.entry(message_id.clone()).or_insert_with(|| Transfer { requested_upto: done, ..Transfer::default() });
            transfer.in_flight = true;
            transfer.last_activity = Some(Instant::now());
            let half = FILE_WINDOW as i64 / 2;
            let upto = transfer.requested_upto.max(done);
            (done + half >= upto && upto < record.chunks()).then(|| {
                let count = (record.chunks() - upto).min(half);
                transfer.requested_upto = upto + count;
                (upto, count)
            })
        };
        if let Some((from, count)) = next {
            let reached = self.ask(contact, &record, from, count).await?;
            if !reached {
                let mut transfers = self.transfers.lock().expect("transfers poisoned");
                if let Some(transfer) = transfers.get_mut(&message_id) {
                    self.note_request(transfer, false);
                }
            }
        }
        Ok(())
    }

    async fn finish(&self, contact: &Contact, file: &FileRecord) -> Result<()> {
        self.transfers.lock().expect("transfers poisoned").remove(&file.message_id);
        let path = self.file_path(file);
        let checked = path.clone();
        let (size, hash) = tokio::task::spawn_blocking(move || {
            // An empty file has no chunk to create it.
            OpenOptions::new().create(true).append(true).open(&checked)?;
            hash_file(&checked)
        })
        .await??;
        if size as i64 == file.size && hash == file.hash {
            self.store.set_file_progress(&file.message_id, file.chunks(), true).await?;
            let _ = self.send_control(contact, Body::FileDone { file: MessageId::parse(&file.message_id)? }).await;
        } else {
            let _ = tokio::fs::remove_file(&path).await;
            self.store.set_file_failed(&file.message_id).await?;
        }
        let _ = self.events.send(Event::MessagesChanged { contact: contact.device_id.clone() });
        Ok(())
    }

    pub(crate) async fn file_done(&self, contact: &Contact, file: MessageId) -> Result<()> {
        let message_id = file.to_string();
        if let Some(record) = self.outgoing_file(contact, &message_id).await? {
            self.store.set_file_progress(&message_id, record.chunks(), true).await?;
            let _ = self.events.send(Event::MessagesChanged { contact: contact.device_id.clone() });
        }
        Ok(())
    }

    pub(crate) async fn file_failed(&self, contact: &Contact, file: MessageId) -> Result<()> {
        let message_id = file.to_string();
        let Some(record) = self.incoming_file(contact, &message_id).await? else { return Ok(()) };
        if record.complete || record.failed {
            return Ok(());
        }
        self.transfers.lock().expect("transfers poisoned").remove(&message_id);
        let _ = tokio::fs::remove_file(self.file_path(&record)).await;
        self.store.set_file_failed(&message_id).await?;
        let _ = self.events.send(Event::MessagesChanged { contact: contact.device_id.clone() });
        Ok(())
    }

    async fn incoming_file(&self, contact: &Contact, message_id: &str) -> Result<Option<FileRecord>> {
        self.file_with(contact, message_id, false).await
    }

    async fn outgoing_file(&self, contact: &Contact, message_id: &str) -> Result<Option<FileRecord>> {
        self.file_with(contact, message_id, true).await
    }

    async fn file_with(&self, contact: &Contact, message_id: &str, outgoing: bool) -> Result<Option<FileRecord>> {
        let Some(message) = self.store.message(message_id).await? else { return Ok(None) };
        if message.contact != contact.device_id || message.outgoing != outgoing {
            return Ok(None);
        }
        self.store.file(message_id).await
    }
}

// Older versions stored absolute paths, re-root a missing one from its `files` folder
fn resolve(stored: &Path, files_dir: &Path) -> PathBuf {
    if !stored.is_absolute() {
        return files_dir.join(stored);
    }
    if stored.exists() {
        return stored.to_owned();
    }
    let Some(folder) = files_dir.file_name() else { return stored.to_owned() };
    let parts: Vec<_> = stored.components().collect();
    match parts.iter().rposition(|part| part.as_os_str() == folder) {
        Some(at) => parts[at + 1..].iter().fold(files_dir.to_owned(), |path, part| path.join(part)),
        None => stored.to_owned(),
    }
}

pub const TRANSIT_FOLDERS: [&str; 3] = ["uploads", "outgoing", "drive"];

pub async fn referenced_files(store: &Store, files_dir: &Path) -> Result<HashSet<PathBuf>> {
    Ok(store.file_paths().await?.iter().map(|stored| canonical(&resolve(Path::new(stored), files_dir))).collect())
}

pub(crate) fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_owned())
}

// If the store can't say what is referenced, delete nothing
pub async fn sweep_orphans(store: &Store, files_dir: &Path, folders: Vec<PathBuf>) -> Result<usize> {
    let referenced = referenced_files(store, files_dir).await?;
    Ok(tokio::task::spawn_blocking(move || folders.iter().map(|folder| sweep_folder(folder, &referenced, false)).sum()).await?)
}

fn sweep_folder(folder: &Path, referenced: &HashSet<PathBuf>, inner: bool) -> usize {
    let Ok(entries) = std::fs::read_dir(folder) else { return 0 };
    let mut gone = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => gone += sweep_folder(&path, referenced, true),
            // a symlink goes as itself, whatever it points to
            Ok(_) if !referenced.contains(&canonical(&path)) && std::fs::remove_file(&path).is_ok() => gone += 1,
            _ => {}
        }
    }
    if inner {
        let _ = std::fs::remove_dir(folder);
    }
    gone
}

fn chunk_length(file: &FileRecord, index: u64) -> usize {
    let start = index as i64 * file.chunk;
    (file.size - start).clamp(0, file.chunk) as usize
}

fn hash_file(path: &Path) -> std::io::Result<(u64, [u8; 32])> {
    let mut hasher = blake3::Hasher::new();
    hasher.update_reader(std::fs::File::open(path)?)?;
    Ok((hasher.count(), *hasher.finalize().as_bytes()))
}

fn read_chunk(path: &Path, offset: u64, length: usize) -> std::io::Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut data = vec![0; length];
    file.read_exact(&mut data)?;
    Ok(data)
}

fn write_chunk(path: &Path, offset: u64, data: &[u8]) -> std::io::Result<()> {
    let mut file = OpenOptions::new().create(true).write(true).truncate(false).open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    file.write_all(data)
}

pub fn safe_file_name(name: &str) -> String {
    const LIMIT: usize = 120;
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String =
        base.chars().map(|c| if c.is_control() || matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c }).collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim();
    if cleaned.is_empty() {
        return "file".to_owned();
    }
    if cleaned.chars().count() <= LIMIT {
        return cleaned.to_owned();
    }
    let extension = cleaned.rsplit_once('.').map(|(_, ext)| ext).filter(|ext| ext.chars().count() <= 10).unwrap_or("");
    let stem: String = cleaned.chars().take(LIMIT - extension.chars().count() - 1).collect();
    if extension.is_empty() {
        cleaned.chars().take(LIMIT).collect()
    } else {
        format!("{stem}.{extension}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ft_storage::NewContact;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ft-core-{name}-{}", rand::random::<u64>()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn put(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"bytes").unwrap();
    }

    async fn pointing_to(paths: &[&str]) -> Store {
        let store = Store::open_in_memory().await.unwrap();
        store
            .add_contact(&NewContact { device_id: "ft_bob".into(), name: "Bob".into(), card: vec![1], mailbox: true, session: None, receipts: true, accepted: true })
            .await
            .unwrap();
        for (index, path) in paths.iter().enumerate() {
            let id = format!("m{index}");
            store
                .insert_message(&Message { message_id: id.clone(), contact: "ft_bob".into(), outgoing: true, body: "f".into(), sent_at: 1, received_at: 1, state: MessageState::Sent })
                .await
                .unwrap();
            let file = FileRecord {
                message_id: id,
                name: "f".into(),
                size: 5,
                mime: "image/jpeg".into(),
                hash: [0; 32],
                chunk: 5,
                path: (*path).to_owned(),
                chunks_done: 0,
                complete: false,
                failed: false,
                waiting: false,
            };
            store.insert_file(&file).await.unwrap();
        }
        store
    }

    #[tokio::test]
    async fn the_sweep_keeps_what_messages_point_to_and_nothing_else() {
        let dir = scratch("sweep");
        let files = dir.join("files");
        let legacy = dir.join("uploads");
        let kept = [
            files.join("uploads").join("1-a.jpg"),
            files.join("drive").join("x1").join("doc.pdf"),
            files.join("m9").join("photo.jpg"),
            files.join("outgoing").join("u1"),
            legacy.join("old.jpg"),
        ];
        let gone = [
            files.join("uploads").join("2-b.jpg"),
            files.join("outgoing").join("3-made.pdf"),
            files.join("drive").join("x2").join("tax.pdf"),
            files.join("drive").join("x3").join("deep").join("a.part"),
            legacy.join("stale.jpg"),
        ];
        for file in kept.iter().chain(&gone) {
            put(file);
        }
        let old_outgoing = "/old/container/files/outgoing/u1";
        let legacy_kept = legacy.join("old.jpg").to_string_lossy().into_owned();
        let store = pointing_to(&["uploads/1-a.jpg", "uploads/1-a.jpg", "drive/x1/doc.pdf", "m9/photo.jpg", old_outgoing, &legacy_kept]).await;

        let folders = vec![files.join("uploads"), files.join("outgoing"), files.join("drive"), legacy.clone()];
        assert_eq!(sweep_orphans(&store, &files, folders).await.unwrap(), gone.len());
        for file in &kept {
            assert!(file.exists(), "{}", file.display());
        }
        for file in &gone {
            assert!(!file.exists(), "{}", file.display());
        }
        assert!(!files.join("drive").join("x2").exists() && !files.join("drive").join("x3").exists());
        assert!(files.join("drive").join("x1").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_sweep_that_cannot_ask_the_store_deletes_nothing() {
        let dir = scratch("sweep-fails");
        let files = dir.join("files");
        let waiting = files.join("uploads").join("2-b.jpg");
        put(&waiting);
        let store = pointing_to(&[]).await;
        store.close().await;

        assert!(sweep_orphans(&store, &files, vec![files.join("uploads")]).await.is_err());
        assert!(waiting.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_names_cannot_escape_their_directory() {
        assert_eq!(safe_file_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_file_name("a/b\\c.txt"), "c.txt");
        assert_eq!(safe_file_name(".."), "file");
        assert_eq!(safe_file_name(""), "file");
        assert_eq!(safe_file_name(".hidden"), "hidden");
        assert_eq!(safe_file_name("what?.txt"), "what_.txt");
    }

    #[test]
    fn ordinary_names_are_kept() {
        assert_eq!(safe_file_name("holiday photo.jpg"), "holiday photo.jpg");
        assert_eq!(safe_file_name("Informe año 2026.pdf"), "Informe año 2026.pdf");
    }

    #[test]
    fn long_names_keep_their_extension() {
        let name = safe_file_name(&format!("{}.pdf", "x".repeat(300)));
        assert_eq!(name.chars().count(), 120);
        assert!(name.ends_with(".pdf"));
    }

    #[test]
    fn stored_paths_follow_the_files_folder() {
        let files = Path::new("/new/container/files");
        assert_eq!(resolve(Path::new("m1/photo.jpg"), files), files.join("m1/photo.jpg"));
        assert_eq!(resolve(Path::new("/old/container/files/outgoing/u1"), files), files.join("outgoing/u1"));
        assert_eq!(resolve(Path::new("/elsewhere/x.bin"), files), Path::new("/elsewhere/x.bin"));
    }

    #[test]
    fn the_last_chunk_is_the_rest() {
        let file = FileRecord {
            message_id: String::new(),
            name: String::new(),
            size: 100,
            mime: String::new(),
            hash: [0; 32],
            chunk: 48,
            path: String::new(),
            chunks_done: 0,
            complete: false,
            failed: false,
            waiting: false,
        };
        assert_eq!([0, 1, 2].map(|index| chunk_length(&file, index)), [48, 48, 4]);
    }
}
