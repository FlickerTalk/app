//! What the drive holds (plan-drive §4.1): folders and files with their names, sizes, kinds and
//! dates, and which blob holds each file. It travels sealed as one file, `index.ftv`; the cloud
//! sees only its size. One phone writes at a time, but a phone that was recovered elsewhere may
//! still write: two indexes merge by id, the newest change of each entry wins, and nothing that
//! was uploaded is ever lost by a merge.

use std::collections::HashSet;

use anyhow::{bail, ensure, Result};
use serde::{Deserialize, Serialize};

/// The format of the index, for the day it changes.
pub const INDEX_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Folder {
    pub id: String,
    pub name: String,
    /// The folder it is in; none at the root.
    pub parent: Option<String>,
    /// When it was last made, renamed or moved (ms since the epoch).
    pub modified: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct File {
    pub id: String,
    pub name: String,
    pub parent: Option<String>,
    pub size: u64,
    pub mime: String,
    pub modified: i64,
    /// The blob in the cloud that holds it, sealed.
    pub blob: String,
}

/// An entry that was removed: kept so a merge does not bring it back from an older copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Removed {
    pub id: String,
    pub at: i64,
}

/// A file of the phone the backup holds (plan-drive §1: what a new phone needs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupFile {
    /// Relative to the app's files folder.
    pub path: String,
    pub size: u64,
    pub blob: String,
}

/// The backup of the phone: its database and the key that seals what is in it, plus its files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Backup {
    pub at: i64,
    /// The phone that wrote it.
    pub writer: String,
    pub db: String,
    pub db_size: u64,
    /// The storage key of that phone, base64. It only ever exists inside the sealed index.
    pub key: String,
    pub files: Vec<BackupFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    pub version: u32,
    /// Grows with every write; a merge takes the larger one and adds one.
    pub revision: u64,
    pub writer: String,
    #[serde(default)]
    pub folders: Vec<Folder>,
    #[serde(default)]
    pub files: Vec<File>,
    #[serde(default)]
    pub removed: Vec<Removed>,
    #[serde(default)]
    pub backup: Option<Backup>,
}

/// A fresh id for a folder, a file or a blob: random, so the cloud learns nothing from names.
pub fn new_id() -> String {
    let bytes: [u8; 16] = rand::random();
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A name as the drive keeps it: trimmed, one line, not empty, not a path.
pub fn clean_name(name: &str) -> Result<String> {
    let clean: String = name.trim().chars().filter(|symbol| !symbol.is_control()).take(200).collect();
    ensure!(!clean.is_empty(), "a name cannot be empty");
    ensure!(!clean.contains('/') && !clean.contains('\\'), "a name cannot contain slashes");
    ensure!(clean != "." && clean != "..", "that is not a name");
    Ok(clean)
}

impl Index {
    pub fn new(writer: &str) -> Self {
        Self { version: INDEX_VERSION, revision: 0, writer: writer.to_owned(), folders: vec![], files: vec![], removed: vec![], backup: None }
    }

    pub fn parse(json: &[u8]) -> Result<Self> {
        let index: Self = serde_json::from_slice(json)?;
        ensure!(index.version <= INDEX_VERSION, "the drive was written by a newer app");
        Ok(index)
    }

    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("an index serializes")
    }

    pub fn folder(&self, id: &str) -> Option<&Folder> {
        self.folders.iter().find(|folder| folder.id == id)
    }

    pub fn file(&self, id: &str) -> Option<&File> {
        self.files.iter().find(|file| file.id == id)
    }

    /// Whether `parent` names a folder that exists, or the root.
    fn parent_exists(&self, parent: Option<&str>) -> bool {
        parent.is_none_or(|id| self.folder(id).is_some())
    }

    /// Whether `folder` is `ancestor` or lies inside it.
    fn within(&self, folder: &str, ancestor: &str) -> bool {
        let mut at = Some(folder.to_owned());
        let mut steps = 0;
        while let Some(id) = at {
            if id == ancestor {
                return true;
            }
            at = self.folder(&id).and_then(|one| one.parent.clone());
            steps += 1;
            if steps > 10_000 {
                return true;
            }
        }
        false
    }

    /// The folders and files inside `parent` (the root when none), by name.
    pub fn list(&self, parent: Option<&str>) -> (Vec<&Folder>, Vec<&File>) {
        let mut folders: Vec<&Folder> = self.folders.iter().filter(|one| one.parent.as_deref() == parent).collect();
        let mut files: Vec<&File> = self.files.iter().filter(|one| one.parent.as_deref() == parent).collect();
        folders.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        (folders, files)
    }

    pub fn mkdir(&mut self, name: &str, parent: Option<&str>, now: i64) -> Result<String> {
        ensure!(self.parent_exists(parent), "no such folder");
        let folder = Folder { id: new_id(), name: clean_name(name)?, parent: parent.map(str::to_owned), modified: now };
        let id = folder.id.clone();
        self.folders.push(folder);
        Ok(id)
    }

    pub fn add_file(&mut self, name: &str, parent: Option<&str>, size: u64, mime: &str, blob: &str, now: i64) -> Result<String> {
        ensure!(self.parent_exists(parent), "no such folder");
        let file = File {
            id: new_id(),
            name: clean_name(name)?,
            parent: parent.map(str::to_owned),
            size,
            mime: mime.to_owned(),
            modified: now,
            blob: blob.to_owned(),
        };
        let id = file.id.clone();
        self.files.push(file);
        Ok(id)
    }

    pub fn rename(&mut self, id: &str, name: &str, now: i64) -> Result<()> {
        let name = clean_name(name)?;
        if let Some(folder) = self.folders.iter_mut().find(|one| one.id == id) {
            folder.name = name;
            folder.modified = now;
            return Ok(());
        }
        if let Some(file) = self.files.iter_mut().find(|one| one.id == id) {
            file.name = name;
            file.modified = now;
            return Ok(());
        }
        bail!("no such file or folder")
    }

    /// Moves a file or a folder into `parent`; a folder never goes inside itself.
    pub fn move_to(&mut self, id: &str, parent: Option<&str>, now: i64) -> Result<()> {
        ensure!(self.parent_exists(parent), "no such folder");
        if self.folder(id).is_some() {
            if let Some(target) = parent {
                ensure!(!self.within(target, id), "a folder cannot go inside itself");
            }
            let folder = self.folders.iter_mut().find(|one| one.id == id).expect("checked");
            folder.parent = parent.map(str::to_owned);
            folder.modified = now;
            return Ok(());
        }
        if let Some(file) = self.files.iter_mut().find(|one| one.id == id) {
            file.parent = parent.map(str::to_owned);
            file.modified = now;
            return Ok(());
        }
        bail!("no such file or folder")
    }

    /// Removes a file, or a folder with everything in it. Returns the blobs that are now unused.
    pub fn remove(&mut self, id: &str, now: i64) -> Result<Vec<String>> {
        let mut gone: HashSet<String> = HashSet::new();
        if self.folder(id).is_some() {
            gone.insert(id.to_owned());
            loop {
                let more: Vec<String> = self
                    .folders
                    .iter()
                    .filter(|one| one.parent.as_ref().is_some_and(|parent| gone.contains(parent)) && !gone.contains(&one.id))
                    .map(|one| one.id.clone())
                    .collect();
                if more.is_empty() {
                    break;
                }
                gone.extend(more);
            }
        } else if self.file(id).is_some() {
            gone.insert(id.to_owned());
        } else {
            bail!("no such file or folder");
        }
        let mut blobs = vec![];
        self.files.retain(|file| {
            let goes = gone.contains(&file.id) || file.parent.as_ref().is_some_and(|parent| gone.contains(parent));
            if goes {
                blobs.push(file.blob.clone());
                gone.insert(file.id.clone());
            }
            !goes
        });
        self.folders.retain(|folder| !gone.contains(&folder.id));
        for id in gone {
            self.removed.push(Removed { id, at: now });
        }
        Ok(blobs)
    }

    /// Every blob the index refers to, files and backup alike.
    pub fn blobs(&self) -> HashSet<String> {
        let mut all: HashSet<String> = self.files.iter().map(|file| file.blob.clone()).collect();
        if let Some(backup) = &self.backup {
            all.insert(backup.db.clone());
            all.extend(backup.files.iter().map(|file| file.blob.clone()));
        }
        all
    }

    /// Takes in what another copy of the index has (plan-drive §4.1, two writers): every entry by
    /// id, the newest change wins, and a removal beats anything older than it. The backup is the
    /// newest of the two.
    pub fn merge(&mut self, other: &Index) {
        let mut removed: Vec<Removed> = self.removed.clone();
        for one in &other.removed {
            match removed.iter_mut().find(|mine| mine.id == one.id) {
                Some(mine) => mine.at = mine.at.max(one.at),
                None => removed.push(one.clone()),
            }
        }
        let removed_at = |id: &str| removed.iter().find(|one| one.id == id).map(|one| one.at);

        let mut folders = self.folders.clone();
        for theirs in &other.folders {
            match folders.iter_mut().find(|mine| mine.id == theirs.id) {
                Some(mine) => {
                    if theirs.modified > mine.modified {
                        *mine = theirs.clone();
                    }
                }
                None => folders.push(theirs.clone()),
            }
        }
        folders.retain(|folder| removed_at(&folder.id).is_none_or(|at| folder.modified > at));

        let mut files = self.files.clone();
        for theirs in &other.files {
            match files.iter_mut().find(|mine| mine.id == theirs.id) {
                Some(mine) => {
                    if theirs.modified > mine.modified {
                        *mine = theirs.clone();
                    }
                }
                None => files.push(theirs.clone()),
            }
        }
        files.retain(|file| removed_at(&file.id).is_none_or(|at| file.modified > at));
        // A file whose folder is gone goes to the root rather than vanishing.
        let folder_ids: HashSet<&str> = folders.iter().map(|one| one.id.as_str()).collect();
        for file in &mut files {
            if file.parent.as_deref().is_some_and(|parent| !folder_ids.contains(parent)) {
                file.parent = None;
            }
        }
        let folder_ids: HashSet<String> = folder_ids.into_iter().map(str::to_owned).collect();
        for folder in &mut folders {
            if folder.parent.as_deref().is_some_and(|parent| !folder_ids.contains(parent)) {
                folder.parent = None;
            }
        }

        if other.backup.as_ref().is_some_and(|theirs| self.backup.as_ref().is_none_or(|mine| theirs.at > mine.at)) {
            self.backup = other.backup.clone();
        }
        self.folders = folders;
        self.files = files;
        self.removed = removed;
        self.revision = self.revision.max(other.revision);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_and_files_are_made_named_moved_and_removed() {
        let mut index = Index::new("phone-a");
        let docs = index.mkdir("Docs", None, 1).unwrap();
        let inner = index.mkdir("2026", Some(&docs), 2).unwrap();
        let file = index.add_file("tax.pdf", Some(&inner), 1000, "application/pdf", "blob-1", 3).unwrap();
        let loose = index.add_file("photo.jpg", None, 2000, "image/jpeg", "blob-2", 4).unwrap();
        let (folders, files) = index.list(None);
        assert_eq!(folders.iter().map(|one| one.name.as_str()).collect::<Vec<_>>(), ["Docs"]);
        assert_eq!(files.iter().map(|one| one.name.as_str()).collect::<Vec<_>>(), ["photo.jpg"]);
        assert_eq!(index.list(Some(&inner)).1[0].id, file);

        index.rename(&file, "  taxes 2026.pdf ", 5).unwrap();
        assert_eq!(index.file(&file).unwrap().name, "taxes 2026.pdf");
        assert!(index.rename(&file, "a/b", 5).is_err());
        assert!(index.rename("nobody", "x", 5).is_err());
        assert!(index.mkdir("", None, 5).is_err());
        assert!(index.mkdir("x", Some("nowhere"), 5).is_err());

        index.move_to(&loose, Some(&inner), 6).unwrap();
        assert_eq!(index.list(None).1.len(), 0);
        assert!(index.move_to(&docs, Some(&inner), 7).is_err(), "a folder cannot go inside itself");
        assert!(index.move_to(&docs, Some(&docs), 7).is_err());

        let blobs = index.remove(&docs, 8).unwrap();
        assert_eq!(blobs.len(), 2, "every file inside, however deep");
        assert!(index.folders.is_empty() && index.files.is_empty());
        assert_eq!(index.removed.len(), 4, "the two folders and the two files");
        assert!(index.remove(&docs, 9).is_err());
        assert!(index.blobs().is_empty());
    }

    #[test]
    fn two_phones_that_both_wrote_end_up_with_everything_and_nothing_removed_comes_back() {
        let mut base = Index::new("a");
        let shared = base.add_file("shared.txt", None, 10, "text/plain", "blob-s", 1).unwrap();
        let doomed = base.add_file("doomed.txt", None, 10, "text/plain", "blob-d", 1).unwrap();
        base.revision = 3;

        let mut a = base.clone();
        let mut b = base.clone();
        b.writer = "b".to_owned();
        a.add_file("from-a.txt", None, 1, "text/plain", "blob-a", 10).unwrap();
        a.rename(&shared, "renamed-by-a.txt", 20).unwrap();
        a.revision = 4;
        b.add_file("from-b.txt", None, 1, "text/plain", "blob-b", 11).unwrap();
        b.remove(&doomed, 12).unwrap();
        b.rename(&shared, "renamed-by-b.txt", 15).unwrap();
        b.revision = 4;

        let mut merged = a.clone();
        merged.merge(&b);
        let names: Vec<&str> = merged.files.iter().map(|one| one.name.as_str()).collect();
        assert!(names.contains(&"from-a.txt") && names.contains(&"from-b.txt"));
        assert!(!names.contains(&"doomed.txt"), "removed on b, gone for both");
        assert_eq!(merged.file(&shared).unwrap().name, "renamed-by-a.txt", "the newest change wins");
        assert_eq!(merged.revision, 4);

        // The other way round gives the same drive.
        let mut other = b.clone();
        other.merge(&a);
        let mut left: Vec<_> = merged.files.clone();
        let mut right: Vec<_> = other.files.clone();
        left.sort_by(|x, y| x.id.cmp(&y.id));
        right.sort_by(|x, y| x.id.cmp(&y.id));
        assert_eq!(left, right);

        // A file put in a folder the other side removed is not lost: it goes to the root.
        let mut c = merged.clone();
        let folder = c.mkdir("Later", None, 30).unwrap();
        let mut d = c.clone();
        let inside = c.add_file("inside.txt", Some(&folder), 1, "text/plain", "blob-i", 31).unwrap();
        d.remove(&folder, 32).unwrap();
        c.merge(&d);
        assert!(c.folder(&folder).is_none());
        assert_eq!(c.file(&inside).unwrap().parent, None);
    }

    #[test]
    fn the_backup_is_the_newest_one_and_its_blobs_count() {
        let mut a = Index::new("a");
        a.backup = Some(Backup { at: 1, writer: "a".into(), db: "db-1".into(), db_size: 5, key: "k".into(), files: vec![BackupFile { path: "x".into(), size: 1, blob: "f-1".into() }] });
        let mut b = Index::new("b");
        b.backup = Some(Backup { at: 2, writer: "b".into(), db: "db-2".into(), db_size: 5, key: "k".into(), files: vec![] });
        let mut merged = a.clone();
        merged.merge(&b);
        assert_eq!(merged.backup.as_ref().unwrap().db, "db-2");
        assert!(a.blobs().contains("db-1") && a.blobs().contains("f-1"));
        let json = a.to_json();
        assert_eq!(Index::parse(&json).unwrap(), a);
        let mut newer = a.clone();
        newer.version = 99;
        assert!(Index::parse(&newer.to_json()).is_err());
        assert!(clean_name("..").is_err());
        assert_eq!(new_id().len(), 32);
    }
}
