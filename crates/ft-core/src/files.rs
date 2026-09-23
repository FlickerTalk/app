//! Files (Plan §62–64, §106 M5): only ever sent directly, in chunks the receiver pulls.
//!
//! - Sending: the offer (name, size, type and BLAKE3 of the file) is a message in the outbox, like
//!   a text, but it only travels over a direct connection: nothing of a file reaches the mailbox.
//!   The bytes stay on the sender's phone until the transfer completes.
//! - Receiving: the offer is stored and acknowledged, and the chunks are asked for a window at a
//!   time (flow control). Each chunk is written at its place; once all are in, the hash is
//!   checked and the sender is told (`FileDone`). Bytes that do not match are thrown away.
//! - Resuming (§63): a transfer that stalls, because the connection dropped, is asked again from
//!   the first missing chunk once the sender can be reached.

use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use ft_protocol::{Body, MessageId, Packet, FILE_CHUNK};
use ft_storage::{Contact, FileRecord, Message, MessageState, OutboxEntry};

use crate::{now, retry_delay, Core, Event, RECEIPT_WAIT};

/// Chunks asked for at once; the next half is asked for when half of them are in.
pub const FILE_WINDOW: u32 = 16;
/// A transfer with no chunk for this long is asked again.
pub const FILE_STALL: Duration = Duration::from_secs(15);
/// The largest chunk accepted from an offer.
const MAX_CHUNK: u32 = 256 * 1024;
/// The UI hears about a transfer's progress every so many chunks.
const PROGRESS_EVERY: i64 = 8;

/// An incoming transfer in progress, in memory.
#[derive(Default)]
pub(crate) struct Transfer {
    /// Chunks asked for so far: [0, requested_upto).
    requested_upto: i64,
    /// A request reached the sender and its chunks are expected.
    in_flight: bool,
    last_activity: Option<Instant>,
    attempts: i64,
    next_try: Option<Instant>,
}

impl Core {
    /// Where received files are kept; the app sets it once, at start.
    pub fn set_files_dir(&self, dir: PathBuf) {
        let _ = self.files_dir.set(dir);
    }

    fn files_dir(&self) -> Result<&Path> {
        self.files_dir.get().map(PathBuf::as_path).ok_or_else(|| anyhow!("no directory for files"))
    }

    /// Where a file's bytes are on this device. Paths are kept relative to the files folder,
    /// because that folder moves: iOS may change the app's folder on an update, and a move takes
    /// the database to another phone (§60).
    pub fn file_path(&self, file: &FileRecord) -> PathBuf {
        match self.files_dir.get() {
            Some(dir) => resolve(Path::new(&file.path), dir),
            None => PathBuf::from(&file.path),
        }
    }

    /// How a path is kept: relative when it is inside the files folder.
    fn stored_path(&self, path: &Path) -> String {
        match self.files_dir.get().and_then(|dir| path.strip_prefix(dir).ok()) {
            Some(relative) => relative.to_string_lossy().into_owned(),
            None => path.to_string_lossy().into_owned(),
        }
    }

    /// Offers the file at `path` to the contact and returns its message id. The file must stay
    /// there: its chunks are read from it when the contact asks for them.
    pub async fn send_file(&self, contact: &str, path: &Path, name: &str, mime: &str) -> Result<String> {
        self.contact(contact).await?;
        self.allowed(ft_billing::Doing::SendFile).await?;
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
            })
            .await?;
        self.store.enqueue(&message_id, contact, now()).await?;
        let _ = self.events.send(Event::MessagesChanged { contact: contact.to_owned() });

        if let Some(entry) = self.store.outbox().await?.into_iter().find(|e| e.message_id == message_id) {
            self.deliver(&entry).await?;
        }
        Ok(message_id)
    }

    /// Asks again for the missing chunks of incoming files that have stalled (§63); a transfer
    /// that could not reach its sender waits for its next try.
    pub async fn resume_files(&self) -> Result<()> {
        self.resume(None, FILE_STALL, true).await
    }

    /// A direct connection with the contact opened: asks at once for what is missing from every
    /// transfer of theirs that has been quiet for `quiet`, whatever its next try.
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

    /// One delivery attempt for a file offer: direct only.
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

    /// A contact offers a file: store it, acknowledge it and start pulling its chunks.
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
        if chunk == 0 || chunk > MAX_CHUNK || size > i64::MAX as u64 {
            bail!("an offer with an impossible size");
        }
        if !contact.rules.accepts_chat {
            // Chat off (app#5): the file is not taken; they stop offering it and see only sent.
            let _ = self.send_control(contact, Body::Received { ids: vec![id] }).await;
            return Ok(());
        }
        let message_id = id.to_string();
        let name = safe_file_name(&name);
        let dir = self.files_dir()?.join(&message_id);
        tokio::fs::create_dir_all(&dir).await.context("cannot create the file's directory")?;
        let stored = self
            .store
            .insert_message(&Message {
                message_id: message_id.clone(),
                contact: contact.device_id.clone(),
                outgoing: false,
                body: name.clone(),
                sent_at: sent_at as i64,
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
            })
            .await?;
        if stored {
            self.announce_messages(contact);
        }
        // Always acknowledged, even a repeated offer: the sender is waiting for it (§27).
        let _ = self.send_control(contact, self.acknowledgement(contact, id)).await;

        let Some(file) = self.incoming_file(contact, &message_id).await? else { return Ok(()) };
        let busy = self.transfers.lock().expect("transfers poisoned").get(&message_id).is_some_and(|t| t.in_flight);
        if !file.complete && !file.failed && !busy {
            self.request_chunks(contact, &file).await?;
        }
        Ok(())
    }

    /// Asks the sender for the next window of chunks, from the first missing one.
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

    /// The contact asks for chunks of a file we offered them.
    pub(crate) async fn serve_chunks(&self, contact: &Contact, file: MessageId, from: u64, count: u32) -> Result<()> {
        let message_id = file.to_string();
        let Some(record) = self.outgoing_file(contact, &message_id).await? else { return Ok(()) };
        let end = (from.saturating_add(count as u64)).min(record.chunks() as u64);
        let mut sent_upto = from as i64;
        for index in from..end {
            let (path, offset, length) = (self.file_path(&record), index * record.chunk as u64, chunk_length(&record, index));
            let Ok(data) = tokio::task::spawn_blocking(move || read_chunk(&path, offset, length)).await? else {
                // The bytes are gone (or unreadable): the transfer can never finish. Both sides
                // must know, or the receiver keeps asking for ever and the UI stays at 0 %.
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

    /// A chunk of a file the contact is sending us. Only the next one in order is taken.
    pub(crate) async fn take_chunk(&self, contact: &Contact, file: MessageId, index: u64, data: Vec<u8>) -> Result<()> {
        let message_id = file.to_string();
        let Some(record) = self.incoming_file(contact, &message_id).await? else { return Ok(()) };
        if record.complete || record.failed || index as i64 != record.chunks_done {
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

        // Keep half a window ahead of what has arrived.
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

    /// Every chunk is in: check the hash, keep or throw away the bytes, tell the sender.
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

    /// The contact has the whole file we sent.
    pub(crate) async fn file_done(&self, contact: &Contact, file: MessageId) -> Result<()> {
        let message_id = file.to_string();
        if let Some(record) = self.outgoing_file(contact, &message_id).await? {
            self.store.set_file_progress(&message_id, record.chunks(), true).await?;
            let _ = self.events.send(Event::MessagesChanged { contact: contact.device_id.clone() });
        }
        Ok(())
    }

    /// The contact no longer has the bytes of a file they offered us: what came is thrown away
    /// and the transfer is marked failed, so it is not asked for again.
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

    /// The file, if it belongs to the conversation with this contact and goes the given way.
    async fn file_with(&self, contact: &Contact, message_id: &str, outgoing: bool) -> Result<Option<FileRecord>> {
        let Some(message) = self.store.message(message_id).await? else { return Ok(None) };
        if message.contact != contact.device_id || message.outgoing != outgoing {
            return Ok(None);
        }
        self.store.file(message_id).await
    }
}

/// A stored path on this device: relative ones under the files folder; an absolute one from an
/// older version, if it is gone, from its `files` folder on, under the current one.
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

/// Bytes of chunk `index`: all are full but the last.
fn chunk_length(file: &FileRecord, index: u64) -> usize {
    let start = index as i64 * file.chunk;
    (file.size - start).clamp(0, file.chunk) as usize
}

/// Size and BLAKE3 of a file.
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

/// A name that is safe to create on this device: no directories, no special characters, not
/// hidden and not too long (the extension is kept).
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
        // An absolute path from before the folder moved (iOS update).
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
        };
        assert_eq!([0, 1, 2].map(|index| chunk_length(&file, index)), [48, 48, 4]);
    }
}
