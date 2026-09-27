//! The drive end to end, against the cloud in memory (plan-drive §6): make it, fill it, come
//! back to it from a new phone with the recovery code, lose the network, back the phone up and
//! bring the backup down.

use std::path::{Path, PathBuf};
use ft_vault::{quiet, Memory, Provider, Quota, Vault};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ft-vault-{name}-{}", ft_vault::index::new_id()));
    std::fs::create_dir_all(&dir).expect("creates the directory");
    dir
}

fn file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, bytes).unwrap();
    path
}

#[tokio::test]
async fn a_drive_is_made_filled_and_read_back_and_the_cloud_sees_nothing_of_it() {
    let cloud = Memory::new();
    let home = scratch("home");
    let (vault, code) = Vault::create(cloud.clone(), home.join("vault"), "phone-a").await.unwrap();
    assert!(Vault::exists(cloud.as_ref()).await.unwrap());
    assert!(Vault::create(cloud.clone(), home.join("again"), "phone-a").await.is_err(), "one drive per cloud");

    let docs = vault.mkdir("Docs", None).await.unwrap();
    let big: Vec<u8> = (0..3_000_000u32).map(|at| (at % 253) as u8).collect();
    let tax = file(&home, "tax.pdf", &big);
    let photo = file(&home, "photo.jpg", b"jpeg bytes");
    let tax_id = vault.upload(&tax, "tax 2026.pdf", "application/pdf", Some(&docs), quiet()).await.unwrap().expect("uploaded");
    vault.upload(&photo, "photo.jpg", "image/jpeg", None, quiet()).await.unwrap().expect("uploaded");

    let root = vault.list(None).await.unwrap();
    assert_eq!(root.folders.iter().map(|one| one.name.as_str()).collect::<Vec<_>>(), ["Docs"]);
    assert_eq!(root.files.iter().map(|one| one.name.as_str()).collect::<Vec<_>>(), ["photo.jpg"]);
    let inside = vault.list(Some(&docs)).await.unwrap();
    assert_eq!(inside.files[0].size, big.len() as u64);
    assert!(vault.list(Some("nowhere")).await.is_err());

    // The cloud holds sealed blobs with random names, and not one byte of what was uploaded.
    let names = cloud.names();
    assert!(names.contains(&"vault.json".to_owned()) && names.contains(&"key.ftv".to_owned()) && names.contains(&"index.ftv".to_owned()));
    assert_eq!(names.iter().filter(|name| name.starts_with("blob-")).count(), 2);
    for name in &names {
        let bytes = cloud.bytes_of(name).unwrap();
        assert!(!contains(&bytes, b"tax 2026"), "{name} carries a name");
        // Found on a real Drive (2026-09-27): vault.json named the phone that made it, which ties
        // the Google account to the FlickerTalk identity. It says nothing of who wrote it.
        assert!(!contains(&bytes, b"phone-a"), "{name} names the phone");
        assert!(!contains(&bytes, b"jpeg bytes"), "{name} carries content");
        assert!(!contains(&bytes, &big[1000..1100]), "{name} carries content");
    }
    let clear: serde_json::Value = serde_json::from_slice(&cloud.bytes_of("vault.json").unwrap()).unwrap();
    assert_eq!(clear["format"], "ftvault");

    let down = home.join("down.pdf");
    let got = vault.download(&tax_id, &down, quiet()).await.unwrap();
    assert_eq!(got.name, "tax 2026.pdf");
    assert_eq!(std::fs::read(&down).unwrap(), big);

    // A blob the cloud changed does not open, and nothing is left on the phone.
    let blob = format!("blob-{}", got.blob);
    cloud.corrupt(&blob, 30);
    let bad = home.join("bad.pdf");
    assert!(vault.download(&tax_id, &bad, quiet()).await.is_err());
    assert!(!bad.exists());

    let status = vault.status().await.unwrap();
    assert_eq!((status.files, status.folders, status.pending), (2, 1, 0));
    assert_eq!(status.used, big.len() as u64 + 10);
    cloud.set_quota(Some(Quota { used: 5, total: 100 }));
    assert_eq!(vault.status().await.unwrap().quota, Some(Quota { used: 5, total: 100 }));

    // Rename, move, remove: the index follows, and a removed file's blob goes with it.
    vault.rename(&tax_id, "renamed.pdf").await.unwrap();
    vault.move_to(&tax_id, None).await.unwrap();
    assert_eq!(vault.list(None).await.unwrap().files.len(), 2);
    vault.remove(&docs).await.unwrap();
    vault.remove(&tax_id).await.unwrap();
    assert!(!cloud.names().contains(&blob));
    assert_eq!(cloud.names().iter().filter(|name| name.starts_with("blob-")).count(), 1);
    let _ = code;
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

#[tokio::test]
async fn a_new_phone_opens_the_drive_with_the_recovery_code_and_only_with_it() {
    let cloud = Memory::new();
    let old = scratch("old");
    let (vault, code) = Vault::create(cloud.clone(), old.join("vault"), "phone-a").await.unwrap();
    let note = file(&old, "note.txt", b"remember this");
    let id = vault.upload(&note, "note.txt", "text/plain", None, quiet()).await.unwrap().unwrap();

    let new = scratch("new");
    assert!(Vault::recover(cloud.clone(), new.join("vault"), "phone-b", "ABCDE-FGHJK-MNPQR-STVWX-YZ012-34567").await.is_err(), "a wrong code");
    assert!(Vault::recover(cloud.clone(), new.join("vault"), "phone-b", "nonsense").await.is_err());
    let typed = code.to_lowercase().replace('-', " ");
    let recovered = Vault::recover(cloud.clone(), new.join("vault"), "phone-b", &typed).await.unwrap();
    assert_eq!(recovered.id(), vault.id());
    assert_eq!(recovered.key(), vault.key());
    let listing = recovered.list(None).await.unwrap();
    assert_eq!(listing.files.iter().map(|one| one.id.as_str()).collect::<Vec<_>>(), [id.as_str()]);
    let down = new.join("note.txt");
    recovered.download(&id, &down, quiet()).await.unwrap();
    assert_eq!(std::fs::read(down).unwrap(), b"remember this");

    // The phone that has the key opens it without the code; an empty cloud is not a drive.
    let reopened = Vault::open(cloud.clone(), old.join("vault"), "phone-a", vault.key()).await.unwrap();
    assert_eq!(reopened.list(None).await.unwrap().files.len(), 1);
    assert!(Vault::open(Memory::new(), old.join("x"), "phone-a", vault.key()).await.is_err());
    assert!(Vault::recover(Memory::new(), old.join("x"), "phone-a", &code).await.is_err());
}

#[tokio::test]
async fn two_phones_writing_the_same_drive_lose_nothing() {
    let cloud = Memory::new();
    let (a, code) = Vault::create(cloud.clone(), scratch("a").join("vault"), "phone-a").await.unwrap();
    let b = Vault::recover(cloud.clone(), scratch("b").join("vault"), "phone-b", &code).await.unwrap();
    let home = scratch("files");
    let one = file(&home, "one.txt", b"1");
    let two = file(&home, "two.txt", b"2");
    a.upload(&one, "one.txt", "text/plain", None, quiet()).await.unwrap();
    b.upload(&two, "two.txt", "text/plain", None, quiet()).await.unwrap();
    a.mkdir("From A", None).await.unwrap();
    // Each phone wrote without knowing of the other; each write took the other's changes in.
    let mut names: Vec<String> = a.list(None).await.unwrap().files.into_iter().map(|file| file.name).collect();
    names.sort();
    assert_eq!(names, ["one.txt", "two.txt"]);
    b.refresh().await.unwrap();
    let listing = b.list(None).await.unwrap();
    assert_eq!(listing.files.len(), 2);
    assert_eq!(listing.folders[0].name, "From A");
}

#[tokio::test]
async fn an_upload_without_network_waits_on_the_phone_and_goes_when_it_can() {
    let cloud = Memory::new();
    let home = scratch("queue");
    let (vault, _) = Vault::create(cloud.clone(), home.join("vault"), "phone-a").await.unwrap();
    let writes = cloud.writes();
    let note = file(&home, "note.txt", b"later");
    cloud.set_failing(true);
    assert!(vault.upload(&note, "note.txt", "text/plain", None, quiet()).await.unwrap().is_none(), "not uploaded yet");
    std::fs::remove_file(&note).unwrap();
    let listing = vault.list(None).await.unwrap();
    assert!(listing.files.is_empty());
    assert_eq!(listing.pending.len(), 1);
    assert_eq!(listing.pending[0].name, "note.txt");
    assert!(!listing.pending[0].error.is_empty(), "the reason is shown, never a fake success");
    assert_eq!(cloud.writes(), writes);
    assert_eq!(vault.run_queue(quiet()).await.unwrap(), 1, "still out of reach");

    // The queue survives the app being closed: it is on disk with the sealed copy.
    let key = vault.key();
    drop(vault);
    cloud.set_failing(false);
    let vault = Vault::open(cloud.clone(), home.join("vault"), "phone-a", key).await.unwrap();
    assert_eq!(vault.list(None).await.unwrap().pending.len(), 1);
    assert_eq!(vault.run_queue(quiet()).await.unwrap(), 0);
    let listing = vault.list(None).await.unwrap();
    assert_eq!(listing.files[0].name, "note.txt");
    assert!(listing.pending.is_empty());
    let down = home.join("down.txt");
    vault.download(&listing.files[0].id, &down, quiet()).await.unwrap();
    assert_eq!(std::fs::read(down).unwrap(), b"later");
    assert!(std::fs::read_dir(home.join("vault").join("outgoing")).unwrap().next().is_none(), "nothing sealed left behind");
}

#[tokio::test]
async fn the_phone_is_backed_up_and_a_new_one_brings_it_down() {
    let cloud = Memory::new();
    let old = scratch("backup-old");
    let (vault, code) = Vault::create(cloud.clone(), old.join("vault"), "phone-a").await.unwrap();
    let db = file(&old, "snapshot.db", b"sqlite bytes of the whole history");
    let files = old.join("files");
    file(&files, "uploads/photo.jpg", b"a photo");
    file(&files, "incoming/ft_x/doc.pdf", &vec![7u8; 100_000]);
    let storage_key = [42u8; 32];

    let backup = vault.backup(&db, &storage_key, &files, quiet()).await.unwrap();
    assert_eq!(backup.files.len(), 2);
    assert_eq!(backup.db_size, 33);
    assert!(vault.status().await.unwrap().backup_at.is_some());
    let blobs_after_first = cloud.names().iter().filter(|name| name.starts_with("blob-")).count();
    assert_eq!(blobs_after_first, 3);
    // Nothing of the phone is readable in the cloud: not the key, not the database.
    for name in cloud.names() {
        let bytes = cloud.bytes_of(&name).unwrap();
        assert!(!contains(&bytes, b"sqlite bytes"), "{name}");
        assert!(!contains(&bytes, &storage_key), "{name}");
        assert!(!contains(&bytes, b"photo.jpg"), "{name}");
    }

    // A second backup sends only what changed, and drops the blobs of the first.
    file(&files, "uploads/photo.jpg", b"a photo");
    file(&files, "uploads/new.txt", b"new");
    let second = vault.backup(&db, &storage_key, &files, quiet()).await.unwrap();
    assert_eq!(second.files.len(), 3);
    let kept = second.files.iter().find(|one| one.path == "uploads/photo.jpg").unwrap();
    assert_eq!(kept.blob, backup.files.iter().find(|one| one.path == "uploads/photo.jpg").unwrap().blob, "same file, same blob");
    assert_ne!(second.db, backup.db);
    assert!(!cloud.names().contains(&format!("blob-{}", backup.db)), "the old database blob went");
    assert_eq!(cloud.names().iter().filter(|name| name.starts_with("blob-")).count(), 4);

    let new = scratch("backup-new");
    let recovered = Vault::recover(cloud.clone(), new.join("vault"), "phone-b", &code).await.unwrap();
    let restored = recovered.restore(&new.join("move"), &new.join("files"), quiet()).await.unwrap();
    assert_eq!(restored.at, second.at);
    assert_eq!(std::fs::read(new.join("move").join("incoming.db")).unwrap(), b"sqlite bytes of the whole history");
    assert_eq!(std::fs::read(new.join("move").join("incoming.key")).unwrap(), storage_key);
    assert_eq!(std::fs::read(new.join("files").join("uploads/photo.jpg")).unwrap(), b"a photo");
    assert_eq!(std::fs::read(new.join("files").join("incoming/ft_x/doc.pdf")).unwrap().len(), 100_000);
    assert_eq!(std::fs::read(new.join("files").join("uploads/new.txt")).unwrap(), b"new");
    assert!(recovered.list(None).await.unwrap().files.is_empty(), "the backup is not in the drive's folders");

    // A drive with no backup says so; leftovers nothing names are collected.
    let (empty, _) = Vault::create(Memory::new(), scratch("e").join("vault"), "x").await.unwrap();
    assert!(empty.restore(&new.join("m"), &new.join("f"), quiet()).await.is_err());
    cloud.write("blob-orphan", b"left behind".to_vec()).await.unwrap();
    assert_eq!(vault.collect_garbage().await.unwrap(), 1);
    assert_eq!(cloud.names().iter().filter(|name| name.starts_with("blob-")).count(), 4);
}
