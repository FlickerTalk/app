//! Moving to a new phone (Plan §60, §106 M7): the old phone hands its whole identity — the Olm
//! account, the contacts with their sessions, the history and the settings — to the new one,
//! directly and encrypted. No server sees anything but the usual signalling.
//!
//! - The new phone shows a QR: its own (temporary) card and a one-time secret, never a key.
//! - The old phone scans it, reaches the new phone as it would a contact and offers a consistent
//!   copy of its database, with the key that seals it, proving it read the QR.
//! - The new phone checks the proof, pulls the copy in chunks, checks its hash and says so. The
//!   copy and its key wait in the move folder; the app swaps them in and starts again.
//! - The old phone, once told, is erased by the app: one identity, one phone (§59).
//!
//! Files sent and received do not travel in this version: only the database.

use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use ft_contacts::{move_proof, MoveInvite};
use ft_protocol::{Body, Packet, FILE_CHUNK};
use ft_storage::{Contact, Store};

use crate::files::FILE_WINDOW;
use crate::{Core, Event};

/// The copy the new phone received, ready to be swapped in.
pub const MOVE_DB: &str = "incoming.db";
/// The key that seals it.
pub const MOVE_KEY: &str = "incoming.key";
/// The copy the old phone sends.
const OUTGOING: &str = "outgoing.db";
const PARTIAL: &str = "incoming.db.part";

/// How a move goes, for the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoveUpdate {
    Progress { done: u64, total: u64 },
    /// New phone: the copy is here and checked; the app swaps it in.
    Received,
    /// Old phone: the new one has everything; the app erases this one.
    Sent,
    /// New phone: the copy did not match its hash and was thrown away.
    Failed,
}

struct Incoming {
    from: String,
    key: [u8; 32],
    size: u64,
    hash: [u8; 32],
    next: u64,
    requested_upto: u64,
}

struct Outgoing {
    to: String,
    path: PathBuf,
    size: u64,
}

/// A move in progress, in memory.
#[derive(Default)]
pub(crate) struct MoveState {
    /// New phone: the secret of the invite on screen.
    secret: Option<[u8; 32]>,
    incoming: Option<Incoming>,
    outgoing: Option<Outgoing>,
}

/// The copy and key a new phone received, if both are there.
pub fn received_move(dir: &Path) -> Option<(PathBuf, [u8; 32])> {
    let database = dir.join(MOVE_DB);
    let key: [u8; 32] = std::fs::read(dir.join(MOVE_KEY)).ok()?.try_into().ok()?;
    database.exists().then_some((database, key))
}

impl Core {
    /// Where a move writes its copies; the app sets it once, at start.
    pub fn set_move_dir(&self, dir: PathBuf) {
        let _ = self.move_dir.set(dir);
    }

    fn move_dir(&self) -> Result<&Path> {
        self.move_dir.get().map(PathBuf::as_path).ok_or_else(|| anyhow!("no directory for moving"))
    }

    /// New phone: the invite to show as a QR code.
    pub async fn invite_move(&self) -> Result<String> {
        let invite = MoveInvite::new(self.my_card().await?);
        self.moving.lock().expect("move poisoned").secret = Some(invite.secret);
        Ok(invite.to_link())
    }

    /// Old phone: hands everything to the phone whose invite was scanned.
    pub async fn move_to(&self, link: &str) -> Result<()> {
        let invite = MoveInvite::from_link(link)?;
        let new_phone = invite.card.device_id().to_string();
        if new_phone == self.device_id.as_str() {
            bail!("that is this phone's own invite");
        }
        // The new phone is reached like a contact, which it stops being in the copy.
        self.add_contact(&invite.card.to_link(), None).await?;
        let dir = self.move_dir()?.to_owned();
        tokio::fs::create_dir_all(&dir).await?;
        let path = dir.join(OUTGOING);
        let _ = tokio::fs::remove_file(&path).await;
        self.store.snapshot(&path).await?;
        {
            let copy = Store::open(&path).await?;
            copy.remove_contact(&new_phone).await?;
            copy.close().await;
        }
        let copied = path.clone();
        let (size, hash) = tokio::task::spawn_blocking(move || hash_file(&copied)).await??;
        self.moving.lock().expect("move poisoned").outgoing = Some(Outgoing { to: new_phone.clone(), path, size });

        let proof = move_proof(&invite.secret, self.device_id.as_str(), &new_phone);
        let contact = self.contact(&new_phone).await?;
        let offer = Packet::new(Body::MoveOffer { proof, key: self.key, size, hash });
        if !self.transmit_direct(&contact, &offer).await? {
            bail!("the new phone cannot be reached directly");
        }
        Ok(())
    }

    /// New phone: the old one offers its copy. Ignored unless it proves it read our QR.
    pub(crate) async fn move_offered(&self, contact: &Contact, proof: [u8; 32], key: [u8; 32], size: u64, hash: [u8; 32]) -> Result<()> {
        let Some(secret) = self.moving.lock().expect("move poisoned").secret else { return Ok(()) };
        if proof != move_proof(&secret, &contact.device_id, self.device_id.as_str()) || size == 0 {
            return Ok(());
        }
        let dir = self.move_dir()?.to_owned();
        tokio::fs::create_dir_all(&dir).await?;
        tokio::fs::write(dir.join(PARTIAL), b"").await?;
        let _ = tokio::fs::remove_file(dir.join(MOVE_DB)).await;
        let _ = tokio::fs::remove_file(dir.join(MOVE_KEY)).await;
        let count = chunks(size).min(FILE_WINDOW as u64);
        {
            let mut moving = self.moving.lock().expect("move poisoned");
            moving.secret = None;
            moving.incoming = Some(Incoming { from: contact.device_id.clone(), key, size, hash, next: 0, requested_upto: count });
        }
        let request = Packet::new(Body::MoveRequest { from: 0, count: count as u32 });
        self.transmit_direct(contact, &request).await?;
        Ok(())
    }

    /// Old phone: the new one asks for chunks of the copy.
    pub(crate) async fn move_requested(&self, contact: &Contact, from: u64, count: u32) -> Result<()> {
        let (path, size) = {
            let moving = self.moving.lock().expect("move poisoned");
            match &moving.outgoing {
                Some(outgoing) if outgoing.to == contact.device_id => (outgoing.path.clone(), outgoing.size),
                _ => return Ok(()),
            }
        };
        let end = from.saturating_add(count as u64).min(chunks(size));
        for index in from..end {
            let (path, offset) = (path.clone(), index * FILE_CHUNK as u64);
            let length = (size - offset).min(FILE_CHUNK as u64) as usize;
            let data = tokio::task::spawn_blocking(move || read_at(&path, offset, length)).await??;
            if !self.transmit_direct(contact, &Packet::new(Body::MoveChunk { index, data })).await? {
                break;
            }
        }
        Ok(())
    }

    /// New phone: a chunk of the copy, taken only in order.
    pub(crate) async fn move_chunk(&self, contact: &Contact, index: u64, data: Vec<u8>) -> Result<()> {
        let (size, next) = {
            let moving = self.moving.lock().expect("move poisoned");
            match &moving.incoming {
                Some(incoming) if incoming.from == contact.device_id && incoming.next == index => (incoming.size, incoming.next),
                _ => return Ok(()),
            }
        };
        let expected = (size - next * FILE_CHUNK as u64).min(FILE_CHUNK as u64);
        if data.len() as u64 != expected {
            bail!("a move chunk of the wrong size");
        }
        let part = self.move_dir()?.join(PARTIAL);
        let offset = index * FILE_CHUNK as u64;
        tokio::task::spawn_blocking(move || write_at(&part, offset, &data)).await??;

        let total = chunks(size);
        let done = index + 1;
        let ask = {
            let mut moving = self.moving.lock().expect("move poisoned");
            let Some(incoming) = moving.incoming.as_mut() else { return Ok(()) };
            incoming.next = done;
            let half = FILE_WINDOW as u64 / 2;
            (done + half >= incoming.requested_upto && incoming.requested_upto < total).then(|| {
                let from = incoming.requested_upto;
                let count = (total - from).min(half);
                incoming.requested_upto = from + count;
                (from, count)
            })
        };
        let _ = self.events.send(Event::Move(MoveUpdate::Progress { done, total }));
        if done == total {
            return self.finish_move(contact).await;
        }
        if let Some((from, count)) = ask {
            let request = Packet::new(Body::MoveRequest { from, count: count as u32 });
            self.transmit_direct(contact, &request).await?;
        }
        Ok(())
    }

    /// New phone: the whole copy is here; check it, keep it with its key and say so.
    async fn finish_move(&self, contact: &Contact) -> Result<()> {
        let Some(incoming) = self.moving.lock().expect("move poisoned").incoming.take() else { return Ok(()) };
        let dir = self.move_dir()?.to_owned();
        let part = dir.join(PARTIAL);
        let checked = part.clone();
        let (size, hash) = tokio::task::spawn_blocking(move || hash_file(&checked)).await??;
        if size != incoming.size || hash != incoming.hash {
            let _ = tokio::fs::remove_file(&part).await;
            let _ = self.events.send(Event::Move(MoveUpdate::Failed));
            return Ok(());
        }
        write_key(&dir.join(MOVE_KEY), &incoming.key)?;
        tokio::fs::rename(&part, dir.join(MOVE_DB)).await?;
        self.transmit_direct(contact, &Packet::new(Body::MoveDone)).await?;
        let _ = self.events.send(Event::Move(MoveUpdate::Received));
        Ok(())
    }

    /// Old phone: the new one has it all.
    pub(crate) async fn move_finished(&self, contact: &Contact) -> Result<()> {
        let outgoing = {
            let mut moving = self.moving.lock().expect("move poisoned");
            match &moving.outgoing {
                Some(outgoing) if outgoing.to == contact.device_id => moving.outgoing.take(),
                _ => None,
            }
        };
        if let Some(outgoing) = outgoing {
            let _ = tokio::fs::remove_file(&outgoing.path).await;
            let _ = self.events.send(Event::Move(MoveUpdate::Sent));
        }
        Ok(())
    }
}

fn chunks(size: u64) -> u64 {
    size.div_ceil(FILE_CHUNK as u64)
}

fn hash_file(path: &Path) -> std::io::Result<(u64, [u8; 32])> {
    let mut hasher = blake3::Hasher::new();
    hasher.update_reader(std::fs::File::open(path)?)?;
    Ok((hasher.count(), *hasher.finalize().as_bytes()))
}

fn read_at(path: &Path, offset: u64, length: usize) -> std::io::Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    let mut data = vec![0; length];
    file.read_exact(&mut data)?;
    Ok(data)
}

fn write_at(path: &Path, offset: u64, data: &[u8]) -> std::io::Result<()> {
    let mut file = OpenOptions::new().write(true).open(path)?;
    file.seek(SeekFrom::Start(offset))?;
    file.write_all(data)
}

/// The key goes to a file only this app can read.
fn write_key(path: &Path, key: &[u8; 32]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)?.write_all(key).context("cannot keep the moved key")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_and_its_key_make_a_received_move() {
        let dir = std::env::temp_dir().join(format!("ft-move-{}", ft_protocol::MessageId::new()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(received_move(&dir).is_none());
        std::fs::write(dir.join(MOVE_DB), b"db").unwrap();
        assert!(received_move(&dir).is_none(), "not without its key");
        write_key(&dir.join(MOVE_KEY), &[9; 32]).unwrap();
        assert_eq!(received_move(&dir), Some((dir.join(MOVE_DB), [9; 32])));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_copy_is_cut_in_file_sized_chunks() {
        assert_eq!(chunks(1), 1);
        assert_eq!(chunks(FILE_CHUNK as u64), 1);
        assert_eq!(chunks(FILE_CHUNK as u64 + 1), 2);
    }
}
