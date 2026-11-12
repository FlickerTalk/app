use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{bail, Result};
use async_trait::async_trait;

// (done, total) in bytes
pub type Progress = Arc<dyn Fn(u64, u64) + Send + Sync>;

pub fn quiet() -> Progress {
    Arc::new(|_, _| {})
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quota {
    pub used: u64,
    pub total: u64,
}

#[async_trait]
pub trait Provider: Send + Sync {
    // small files only
    async fn read(&self, name: &str) -> Result<Option<Vec<u8>>>;
    async fn write(&self, name: &str, bytes: Vec<u8>) -> Result<()>;
    async fn upload(&self, name: &str, path: &Path, progress: Progress) -> Result<()>;
    async fn download(&self, name: &str, path: &Path, progress: Progress) -> Result<()>;
    // a missing file isn't an error
    async fn remove(&self, name: &str) -> Result<()>;
    async fn list(&self) -> Result<Vec<(String, u64)>>;
    async fn quota(&self) -> Result<Option<Quota>>;
}

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

    pub fn set_failing(&self, failing: bool) {
        self.failing.store(failing, Ordering::SeqCst);
    }

    pub fn set_quota(&self, quota: Option<Quota>) {
        *self.quota.lock().unwrap() = quota;
    }

    pub fn writes(&self) -> usize {
        self.writes.load(Ordering::SeqCst)
    }

    pub fn names(&self) -> Vec<String> {
        self.files.lock().unwrap().keys().cloned().collect()
    }

    pub fn bytes_of(&self, name: &str) -> Option<Vec<u8>> {
        self.files.lock().unwrap().get(name).cloned()
    }

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
