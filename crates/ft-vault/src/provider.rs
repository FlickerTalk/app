//! Where the sealed files live (plan-drive §4.2): the user's own cloud, behind a trait so the
//! vault and its tests never care which. Names are flat (`vault.json`, `key.ftv`, `index.ftv`,
//! `blob-<id>`) inside the app's own folder there; a provider maps them however it likes.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{bail, Result};
use async_trait::async_trait;

/// How far a transfer is: (done, total), in bytes.
pub type Progress = Arc<dyn Fn(u64, u64) + Send + Sync>;

/// A progress callback that says nothing.
pub fn quiet() -> Progress {
    Arc::new(|_, _| {})
}

/// The user's room in the cloud, when the provider says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quota {
    pub used: u64,
    pub total: u64,
}

#[async_trait]
pub trait Provider: Send + Sync {
    /// A small file, whole; None if it is not there.
    async fn read(&self, name: &str) -> Result<Option<Vec<u8>>>;
    /// Writes a small file, replacing it if it is there.
    async fn write(&self, name: &str, bytes: Vec<u8>) -> Result<()>;
    /// Uploads a file from disk, however big, replacing what has the same name.
    async fn upload(&self, name: &str, path: &Path, progress: Progress) -> Result<()>;
    /// Downloads a file to disk, however big.
    async fn download(&self, name: &str, path: &Path, progress: Progress) -> Result<()>;
    /// Removes a file; nothing happens if it is not there.
    async fn remove(&self, name: &str) -> Result<()>;
    /// The names and sizes of every file in the app's folder.
    async fn list(&self) -> Result<Vec<(String, u64)>>;
    /// The room the user has, if the provider tells.
    async fn quota(&self) -> Result<Option<Quota>>;
}

/// A cloud in memory, for the tests: what was written, and a switch to make it fail.
#[derive(Default)]
pub struct Memory {
    files: Mutex<BTreeMap<String, Vec<u8>>>,
    failing: AtomicBool,
    writes: AtomicUsize,
    quota: Mutex<Option<Quota>>,
}

impl Memory {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// From now on, every write and upload fails, as a cloud out of reach would.
    pub fn set_failing(&self, failing: bool) {
        self.failing.store(failing, Ordering::SeqCst);
    }

    pub fn set_quota(&self, quota: Option<Quota>) {
        *self.quota.lock().unwrap() = quota;
    }

    /// How many writes and uploads got through.
    pub fn writes(&self) -> usize {
        self.writes.load(Ordering::SeqCst)
    }

    pub fn names(&self) -> Vec<String> {
        self.files.lock().unwrap().keys().cloned().collect()
    }

    pub fn bytes_of(&self, name: &str) -> Option<Vec<u8>> {
        self.files.lock().unwrap().get(name).cloned()
    }

    /// Changes a byte of a file, as a cloud that lies or a disk that rots would.
    pub fn corrupt(&self, name: &str, at: usize) {
        if let Some(bytes) = self.files.lock().unwrap().get_mut(name) {
            if let Some(byte) = bytes.get_mut(at) {
                *byte ^= 0x55;
            }
        }
    }

    fn check(&self) -> Result<()> {
        if self.failing.load(Ordering::SeqCst) {
            bail!("the cloud is out of reach");
        }
        Ok(())
    }
}

#[async_trait]
impl Provider for Memory {
    async fn read(&self, name: &str) -> Result<Option<Vec<u8>>> {
        self.check()?;
        Ok(self.files.lock().unwrap().get(name).cloned())
    }

    async fn write(&self, name: &str, bytes: Vec<u8>) -> Result<()> {
        self.check()?;
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.files.lock().unwrap().insert(name.to_owned(), bytes);
        Ok(())
    }

    async fn upload(&self, name: &str, path: &Path, progress: Progress) -> Result<()> {
        self.check()?;
        let bytes = tokio::fs::read(path).await?;
        progress(bytes.len() as u64, bytes.len() as u64);
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.files.lock().unwrap().insert(name.to_owned(), bytes);
        Ok(())
    }

    async fn download(&self, name: &str, path: &Path, progress: Progress) -> Result<()> {
        self.check()?;
        let Some(bytes) = self.files.lock().unwrap().get(name).cloned() else { bail!("{name} is not in the cloud") };
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(path, &bytes).await?;
        progress(bytes.len() as u64, bytes.len() as u64);
        Ok(())
    }

    async fn remove(&self, name: &str) -> Result<()> {
        self.check()?;
        self.files.lock().unwrap().remove(name);
        Ok(())
    }

    async fn list(&self) -> Result<Vec<(String, u64)>> {
        self.check()?;
        Ok(self.files.lock().unwrap().iter().map(|(name, bytes)| (name.clone(), bytes.len() as u64)).collect())
    }

    async fn quota(&self) -> Result<Option<Quota>> {
        self.check()?;
        Ok(*self.quota.lock().unwrap())
    }
}
