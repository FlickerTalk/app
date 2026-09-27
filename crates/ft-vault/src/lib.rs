//! The user's own cloud (plan-drive, 2026-09-27): a drive of files and a backup of the phone,
//! sealed on the phone before anything leaves it. The provider —Google Drive today— sees sealed
//! blobs with random names, their sizes and their dates, and nothing else; our server sees
//! nothing at all, because it is not there (Plan §100).
//!
//! ```text
//! vault.json    the format version and the drive's random id, in clear (no data in it)
//! key.ftv       the vault key, sealed with the recovery code
//! index.ftv     folders, files, the backup: sealed
//! blob-<id>     a file or the database, sealed, one key each
//! ```
//!
//! The vault key is made on the phone and kept sealed there; the recovery code is shown once. A
//! new phone with the same cloud and the code opens the drive and brings the backup down.

pub mod cipher;
pub mod google;
pub mod index;
pub mod provider;
pub mod recovery;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, ensure, Context, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

pub use index::{Backup, BackupFile, File, Folder, Index};
pub use provider::{quiet, Memory, Progress, Provider, Quota};

const VAULT_FILE: &str = "vault.json";
const KEY_FILE: &str = "key.ftv";
const INDEX_FILE: &str = "index.ftv";
const BLOB_PREFIX: &str = "blob-";
const QUEUE_FILE: &str = "queue.json";
const OUTGOING_DIR: &str = "outgoing";
/// The format of what is in the cloud, for the day it changes.
pub const FORMAT_VERSION: u32 = 1;

/// What `vault.json` says: nothing that means anything to anyone but the app.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct VaultFile {
    format: String,
    version: u32,
    id: String,
    // Nothing about the phone that made it (2026-09-27): a device id here would tie the Google
    // account to the FlickerTalk identity. Old files that still carry `made_by` read fine.
}

/// An upload that did not get through yet (plan-drive §5): the sealed file waits on the phone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    pub blob: String,
    pub name: String,
    pub parent: Option<String>,
    pub size: u64,
    pub mime: String,
    pub attempts: u32,
    pub error: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Queue {
    #[serde(default)]
    pending: Vec<Pending>,
}

/// What the drive is like right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub id: String,
    pub files: usize,
    pub folders: usize,
    /// Bytes of files in the drive, as the user sees them.
    pub used: u64,
    pub pending: usize,
    pub quota: Option<Quota>,
    pub backup_at: Option<i64>,
}

/// A folder, as the drive shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub folders: Vec<Folder>,
    pub files: Vec<File>,
    pub pending: Vec<Pending>,
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|since| since.as_millis() as i64).unwrap_or(0)
}

fn blob_name(id: &str) -> String {
    format!("{BLOB_PREFIX}{id}")
}

/// A drive, open: its key, its index and the cloud behind it.
pub struct Vault {
    key: [u8; 32],
    id: String,
    writer: String,
    provider: Arc<dyn Provider>,
    dir: PathBuf,
    index: Mutex<Index>,
    /// The revision of the index as it was last read from the cloud, to notice another writer.
    seen: Mutex<u64>,
    queue: Mutex<Queue>,
}

impl Vault {
    /// Whether this cloud already has a drive.
    pub async fn exists(provider: &dyn Provider) -> Result<bool> {
        Ok(provider.read(VAULT_FILE).await?.is_some())
    }

    /// Makes a new drive in an empty cloud. Returns it and the recovery code, to show once.
    pub async fn create(provider: Arc<dyn Provider>, dir: PathBuf, writer: &str) -> Result<(Self, String)> {
        ensure!(!Self::exists(provider.as_ref()).await?, "this cloud already has a drive");
        let key: [u8; 32] = rand::random();
        let code = recovery::new_code();
        let id = index::new_id();
        let file = VaultFile { format: "ftvault".to_owned(), version: FORMAT_VERSION, id: id.clone() };
        provider.write(VAULT_FILE, serde_json::to_vec(&file)?).await?;
        provider.write(KEY_FILE, cipher::seal(&recovery::wrap_key(&code)?, "key", &key)?).await?;
        let vault = Self::assemble(provider, dir, writer, key, id, Index::new(writer)).await?;
        vault.write_index(&mut *vault.index.lock().await).await?;
        Ok((vault, code))
    }

    /// Opens the drive this phone already has the key of.
    pub async fn open(provider: Arc<dyn Provider>, dir: PathBuf, writer: &str, key: [u8; 32]) -> Result<Self> {
        let file = Self::vault_file(provider.as_ref()).await?;
        let index = Self::read_index(provider.as_ref(), &key).await?.unwrap_or_else(|| Index::new(writer));
        let seen = index.revision;
        let vault = Self::assemble(provider, dir, writer, key, file.id, index).await?;
        *vault.seen.lock().await = seen;
        Ok(vault)
    }

    /// Opens the drive on a new phone: the cloud and the recovery code are all it has.
    pub async fn recover(provider: Arc<dyn Provider>, dir: PathBuf, writer: &str, code: &str) -> Result<Self> {
        let wrap = recovery::wrap_key(code)?;
        let Some(sealed) = provider.read(KEY_FILE).await? else { bail!("this cloud has no drive to recover") };
        let key: [u8; 32] = cipher::open(&wrap, "key", &sealed)
            .map_err(|_| anyhow::anyhow!("that is not the recovery code of this drive"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("the drive's key is corrupt"))?;
        Self::open(provider, dir, writer, key).await
    }

    async fn vault_file(provider: &dyn Provider) -> Result<VaultFile> {
        let Some(bytes) = provider.read(VAULT_FILE).await? else { bail!("this cloud has no drive") };
        let file: VaultFile = serde_json::from_slice(&bytes).context("vault.json is not ours")?;
        ensure!(file.format == "ftvault", "vault.json is not ours");
        ensure!(file.version <= FORMAT_VERSION, "the drive was made by a newer app");
        Ok(file)
    }

    async fn assemble(provider: Arc<dyn Provider>, dir: PathBuf, writer: &str, key: [u8; 32], id: String, index: Index) -> Result<Self> {
        tokio::fs::create_dir_all(dir.join(OUTGOING_DIR)).await?;
        let queue = match tokio::fs::read(dir.join(QUEUE_FILE)).await {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            Err(_) => Queue::default(),
        };
        Ok(Self { key, id, writer: writer.to_owned(), provider, dir, index: Mutex::new(index), seen: Mutex::new(0), queue: Mutex::new(queue) })
    }

    /// The vault key, for the phone to keep sealed. It never goes anywhere else.
    pub fn key(&self) -> [u8; 32] {
        self.key
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    async fn read_index(provider: &dyn Provider, key: &[u8; 32]) -> Result<Option<Index>> {
        let Some(sealed) = provider.read(INDEX_FILE).await? else { return Ok(None) };
        let json = cipher::open(key, "index", &sealed).context("the drive's index does not open")?;
        Ok(Some(Index::parse(&json)?))
    }

    /// Writes the index, first taking in what another phone may have written since we read it.
    async fn write_index(&self, index: &mut Index) -> Result<()> {
        let mut seen = self.seen.lock().await;
        if let Some(remote) = Self::read_index(self.provider.as_ref(), &self.key).await? {
            if remote.revision != *seen {
                index.merge(&remote);
            }
        }
        index.revision += 1;
        index.writer = self.writer.clone();
        self.provider.write(INDEX_FILE, cipher::seal(&self.key, "index", &index.to_json())?).await?;
        *seen = index.revision;
        Ok(())
    }

    /// Brings the index up to date with the cloud, for a phone that was recovered elsewhere.
    pub async fn refresh(&self) -> Result<()> {
        let mut index = self.index.lock().await;
        if let Some(remote) = Self::read_index(self.provider.as_ref(), &self.key).await? {
            let mut seen = self.seen.lock().await;
            if remote.revision != *seen {
                index.merge(&remote);
                *seen = index.revision;
            }
        }
        Ok(())
    }

    async fn save_queue(&self, queue: &Queue) -> Result<()> {
        tokio::fs::write(self.dir.join(QUEUE_FILE), serde_json::to_vec(queue)?).await?;
        Ok(())
    }

    pub async fn status(&self) -> Result<Status> {
        let index = self.index.lock().await;
        let quota = self.provider.quota().await.unwrap_or(None);
        Ok(Status {
            id: self.id.clone(),
            files: index.files.len(),
            folders: index.folders.len(),
            used: index.files.iter().map(|file| file.size).sum(),
            pending: self.queue.lock().await.pending.len(),
            quota,
            backup_at: index.backup.as_ref().map(|backup| backup.at),
        })
    }

    /// What a folder holds (the root when none), and what is still waiting to be uploaded there.
    pub async fn list(&self, parent: Option<&str>) -> Result<Listing> {
        let index = self.index.lock().await;
        if let Some(id) = parent {
            ensure!(index.folder(id).is_some(), "no such folder");
        }
        let (folders, files) = index.list(parent);
        let pending = self.queue.lock().await.pending.iter().filter(|one| one.parent.as_deref() == parent).cloned().collect();
        Ok(Listing { folders: folders.into_iter().cloned().collect(), files: files.into_iter().cloned().collect(), pending })
    }

    pub async fn file(&self, id: &str) -> Option<File> {
        self.index.lock().await.file(id).cloned()
    }

    pub async fn mkdir(&self, name: &str, parent: Option<&str>) -> Result<String> {
        let mut index = self.index.lock().await;
        let id = index.mkdir(name, parent, now())?;
        self.write_index(&mut index).await?;
        Ok(id)
    }

    pub async fn rename(&self, id: &str, name: &str) -> Result<()> {
        let mut index = self.index.lock().await;
        index.rename(id, name, now())?;
        self.write_index(&mut index).await
    }

    pub async fn move_to(&self, id: &str, parent: Option<&str>) -> Result<()> {
        let mut index = self.index.lock().await;
        index.move_to(id, parent, now())?;
        self.write_index(&mut index).await
    }

    /// Removes a file or a folder from the drive and, after the index says so, its blobs.
    pub async fn remove(&self, id: &str) -> Result<()> {
        let blobs = {
            let mut index = self.index.lock().await;
            let blobs = index.remove(id, now())?;
            self.write_index(&mut index).await?;
            blobs
        };
        for blob in blobs {
            let _ = self.provider.remove(&blob_name(&blob)).await;
        }
        Ok(())
    }

    /// Seals a file of the phone and uploads it. If the cloud is out of reach the sealed copy
    /// waits on the phone and the file shows as pending; `run_queue` tries again.
    pub async fn upload(&self, path: &Path, name: &str, mime: &str, parent: Option<&str>, progress: Progress) -> Result<Option<String>> {
        let name = index::clean_name(name)?;
        {
            let index = self.index.lock().await;
            ensure!(parent.is_none_or(|id| index.folder(id).is_some()), "no such folder");
        }
        let blob = index::new_id();
        let sealed = self.dir.join(OUTGOING_DIR).join(&blob);
        let size = seal_file(&self.key, &blob, path, &sealed).await?;
        let pending = Pending { blob, name, parent: parent.map(str::to_owned), size, mime: mime.to_owned(), attempts: 0, error: String::new() };
        match self.push(&pending, progress).await {
            Ok(id) => Ok(Some(id)),
            Err(error) => {
                let mut queue = self.queue.lock().await;
                queue.pending.push(Pending { attempts: 1, error: error.to_string(), ..pending });
                self.save_queue(&queue).await?;
                Ok(None)
            }
        }
    }

    /// Uploads a sealed file that waits on the phone and puts it in the index.
    async fn push(&self, pending: &Pending, progress: Progress) -> Result<String> {
        let sealed = self.dir.join(OUTGOING_DIR).join(&pending.blob);
        self.provider.upload(&blob_name(&pending.blob), &sealed, progress).await?;
        let id = {
            let mut index = self.index.lock().await;
            let parent = pending.parent.as_deref().filter(|id| index.folder(id).is_some());
            let id = index.add_file(&pending.name, parent, pending.size, &pending.mime, &pending.blob, now())?;
            self.write_index(&mut index).await?;
            id
        };
        let _ = tokio::fs::remove_file(&sealed).await;
        Ok(id)
    }

    /// Tries every upload that waits. Returns how many still wait.
    pub async fn run_queue(&self, progress: Progress) -> Result<usize> {
        let waiting = self.queue.lock().await.pending.clone();
        let mut still = vec![];
        for pending in waiting {
            match self.push(&pending, progress.clone()).await {
                Ok(_) => {}
                Err(error) => still.push(Pending { attempts: pending.attempts + 1, error: error.to_string(), ..pending }),
            }
        }
        let mut queue = self.queue.lock().await;
        queue.pending = still;
        self.save_queue(&queue).await?;
        Ok(queue.pending.len())
    }

    /// Forgets an upload that waits, and its sealed copy.
    pub async fn cancel_pending(&self, blob: &str) -> Result<()> {
        let mut queue = self.queue.lock().await;
        queue.pending.retain(|one| one.blob != blob);
        self.save_queue(&queue).await?;
        let _ = tokio::fs::remove_file(self.dir.join(OUTGOING_DIR).join(blob)).await;
        Ok(())
    }

    /// Brings a file of the drive down to `to`, opened. Fails, and writes nothing, if the blob
    /// was changed or is not the one the index names.
    pub async fn download(&self, id: &str, to: &Path, progress: Progress) -> Result<File> {
        let file = self.file(id).await.context("no such file")?;
        self.fetch_blob(&file.blob, to, progress).await?;
        Ok(file)
    }

    async fn fetch_blob(&self, blob: &str, to: &Path, progress: Progress) -> Result<()> {
        let sealed = self.dir.join(OUTGOING_DIR).join(format!("{blob}.down"));
        self.provider.download(&blob_name(blob), &sealed, progress).await?;
        let opened = open_file(&self.key, blob, &sealed, to).await;
        let _ = tokio::fs::remove_file(&sealed).await;
        opened.map(|_| ())
    }

    // ---- Backup (plan-drive §1, §61): what a new phone needs ----

    /// Puts the phone's database, its storage key and its files in the cloud, sealed. `db` is a
    /// consistent copy of the database (the store's snapshot); `files_dir` the app's files folder.
    pub async fn backup(&self, db: &Path, storage_key: &[u8; 32], files_dir: &Path, progress: Progress) -> Result<Backup> {
        let previous = self.index.lock().await.backup.clone();
        let db_blob = index::new_id();
        let sealed = self.dir.join(OUTGOING_DIR).join(&db_blob);
        let db_size = seal_file(&self.key, &db_blob, db, &sealed).await?;
        self.provider.upload(&blob_name(&db_blob), &sealed, progress.clone()).await?;
        let _ = tokio::fs::remove_file(&sealed).await;

        // A file already in the last backup, same path and size, is not sent again.
        let mut files = vec![];
        for (path, size) in walk(files_dir).await? {
            let kept = previous.as_ref().and_then(|backup| backup.files.iter().find(|one| one.path == path && one.size == size));
            if let Some(kept) = kept {
                files.push(kept.clone());
                continue;
            }
            let blob = index::new_id();
            let sealed = self.dir.join(OUTGOING_DIR).join(&blob);
            seal_file(&self.key, &blob, &files_dir.join(&path), &sealed).await?;
            self.provider.upload(&blob_name(&blob), &sealed, progress.clone()).await?;
            let _ = tokio::fs::remove_file(&sealed).await;
            files.push(BackupFile { path, size, blob });
        }
        let backup = Backup { at: now(), writer: self.writer.clone(), db: db_blob, db_size, key: STANDARD.encode(storage_key), files };
        {
            let mut index = self.index.lock().await;
            index.backup = Some(backup.clone());
            self.write_index(&mut index).await?;
        }
        // The blobs of the backup before are no longer needed.
        if let Some(previous) = previous {
            let kept: HashSet<&str> = backup.files.iter().map(|one| one.blob.as_str()).collect();
            let _ = self.provider.remove(&blob_name(&previous.db)).await;
            for old in previous.files.iter().filter(|one| !kept.contains(one.blob.as_str())) {
                let _ = self.provider.remove(&blob_name(&old.blob)).await;
            }
        }
        Ok(backup)
    }

    /// The backup, if the drive holds one.
    pub async fn backup_info(&self) -> Option<Backup> {
        self.index.lock().await.backup.clone()
    }

    /// Brings the backup down: the database and key into `move_dir`, as a move from another
    /// phone would leave them (the app swaps them in at the next start), and the files into
    /// `files_dir`. Nothing is swapped in here.
    pub async fn restore(&self, move_dir: &Path, files_dir: &Path, progress: Progress) -> Result<Backup> {
        let backup = self.backup_info().await.context("this drive holds no backup")?;
        let key: [u8; 32] = STANDARD.decode(&backup.key)?.try_into().map_err(|_| anyhow::anyhow!("the backup's key is corrupt"))?;
        tokio::fs::create_dir_all(move_dir).await?;
        let db = move_dir.join("incoming.db.part");
        self.fetch_blob(&backup.db, &db, progress.clone()).await?;
        for file in &backup.files {
            let to = files_dir.join(&file.path);
            ensure!(!file.path.contains("..") && !file.path.starts_with('/'), "the backup names a path outside the files folder");
            self.fetch_blob(&file.blob, &to, progress.clone()).await?;
        }
        tokio::fs::write(move_dir.join("incoming.key"), key).await?;
        tokio::fs::rename(&db, move_dir.join("incoming.db")).await?;
        Ok(backup)
    }

    /// Removes the blobs nothing in the index names: what an upload left half done, or a removal
    /// could not delete at the time.
    pub async fn collect_garbage(&self) -> Result<usize> {
        let named = self.index.lock().await.blobs();
        let waiting: HashSet<String> = self.queue.lock().await.pending.iter().map(|one| one.blob.clone()).collect();
        let mut removed = 0;
        for (name, _) in self.provider.list().await? {
            let Some(id) = name.strip_prefix(BLOB_PREFIX) else { continue };
            if !named.contains(id) && !waiting.contains(id) {
                self.provider.remove(&name).await?;
                removed += 1;
            }
        }
        Ok(removed)
    }
}

/// Seals a file of the phone into `sealed`; returns the plaintext size.
async fn seal_file(key: &[u8; 32], blob: &str, from: &Path, sealed: &Path) -> Result<u64> {
    let (key, blob, from, sealed) = (*key, blob.to_owned(), from.to_owned(), sealed.to_owned());
    tokio::task::spawn_blocking(move || {
        let source = std::fs::File::open(&from).with_context(|| format!("cannot read {}", from.display()))?;
        let size = source.metadata()?.len();
        let target = std::fs::File::create(&sealed)?;
        cipher::encrypt(&key, &blob, std::io::BufReader::new(source), std::io::BufWriter::new(target))?;
        Ok::<u64, anyhow::Error>(size)
    })
    .await?
}

/// Opens a sealed file into `to`; nothing is left there if it does not open.
async fn open_file(key: &[u8; 32], blob: &str, sealed: &Path, to: &Path) -> Result<u64> {
    if let Some(parent) = to.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let (key, blob, sealed, to) = (*key, blob.to_owned(), sealed.to_owned(), to.to_owned());
    tokio::task::spawn_blocking(move || {
        let part = to.with_extension("part");
        let source = std::fs::File::open(&sealed)?;
        let target = std::fs::File::create(&part)?;
        let opened = cipher::decrypt(&key, &blob, std::io::BufReader::new(source), std::io::BufWriter::new(target));
        match opened {
            Ok(size) => {
                std::fs::rename(&part, &to)?;
                Ok(size)
            }
            Err(error) => {
                let _ = std::fs::remove_file(&part);
                Err(error)
            }
        }
    })
    .await?
}

/// Every file under a folder, as relative paths with `/`, and its size.
async fn walk(root: &Path) -> Result<Vec<(String, u64)>> {
    let mut found = vec![];
    if !root.exists() {
        return Ok(found);
    }
    let mut stack = vec![root.to_owned()];
    while let Some(dir) = stack.pop() {
        let mut entries = tokio::fs::read_dir(&dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let kind = entry.file_type().await?;
            if kind.is_dir() {
                stack.push(path);
            } else if kind.is_file() {
                let relative = path.strip_prefix(root)?.components().map(|part| part.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/");
                found.push((relative, entry.metadata().await?.len()));
            }
        }
    }
    found.sort();
    Ok(found)
}
