use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use ft_storage::FileRecord;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_ft_platform::PlatformExt;

use super::{failed, located, Client};

/// The WebView can only name files in these folders, anything else is refused.
pub fn picked_path(dir: &Path, path: &str) -> Result<PathBuf, String> {
    let candidate = Path::new(path);
    let real = candidate.canonicalize().map_err(|_| "that file is no longer there".to_owned())?;
    let allowed = [dir.join("uploads"), dir.join("files").join("uploads"), dir.join("files").join("outgoing"), dir.join("files").join("drive")];
    let inside = allowed.iter().filter_map(|root| root.canonicalize().ok()).any(|root| real.starts_with(&root));
    if inside && real.is_file() {
        Ok(real)
    } else {
        Err("that is not a file the user picked".to_owned())
    }
}

pub fn new_upload_id() -> String {
    rand::random::<[u8; 16]>().iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn upload_path(dir: &Path, id: &str) -> Result<PathBuf, String> {
    let valid = id.len() == 32 && id.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c));
    if !valid {
        return Err("invalid upload".to_owned());
    }
    Ok(dir.join("files").join("outgoing").join(id))
}

pub fn openable(file: Option<FileRecord>, outgoing: bool) -> Result<FileRecord, String> {
    match file {
        Some(file) if outgoing || file.complete => Ok(file),
        Some(_) => Err("the file has not arrived yet".to_owned()),
        None => Err("not a file".to_owned()),
    }
}

#[tauri::command]
pub async fn core_accept_file(message: String, client: State<'_, Client>) -> Result<(), String> {
    let core = client.core().await?;
    tauri::async_runtime::spawn(async move {
        let _ = core.accept_file(&message).await;
    });
    Ok(())
}

#[tauri::command]
pub async fn core_upload_start(client: State<'_, Client>) -> Result<String, String> {
    let id = new_upload_id();
    let path = upload_path(client.dir()?, &id)?;
    let parent = path.parent().expect("uploads live in a directory");
    tokio::fs::create_dir_all(parent).await.map_err(failed)?;
    tokio::fs::File::create(&path).await.map_err(failed)?;
    Ok(id)
}

/// base64 because Android's IPC only carries JSON
#[tauri::command]
pub async fn core_upload_append(upload: String, data: String, client: State<'_, Client>) -> Result<(), String> {
    let bytes = BASE64.decode(data).map_err(failed)?;
    let path = upload_path(client.dir()?, &upload)?;
    let mut file = tokio::fs::OpenOptions::new().append(true).open(&path).await.map_err(failed)?;
    tokio::io::AsyncWriteExt::write_all(&mut file, &bytes).await.map_err(failed)
}

pub(super) async fn file_of(client: &Client, message: &str) -> Result<FileRecord, String> {
    let core = client.core().await?;
    let stored = core.store().message(message).await.map_err(failed)?.ok_or("unknown message")?;
    let file = core.store().file(message).await.map_err(failed)?.map(|file| located(&core, file));
    openable(file, stored.outgoing)
}

#[tauri::command]
pub async fn core_open_file(message: String, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let file = file_of(&client, &message).await?;
    app.platform().open_file(&file.path, &file.mime).map_err(failed)
}

#[derive(Serialize)]
pub struct HandedFile {
    pub name: String,
    pub mime: String,
    pub data: String,
}

#[tauri::command]
pub async fn core_save_file(message: String, app: AppHandle, client: State<'_, Client>) -> Result<(), String> {
    let file = file_of(&client, &message).await?;
    app.platform().save_to_downloads(&file.path, &file.name, &file.mime).map_err(failed)
}

#[tauri::command]
pub async fn core_send_file(contact: String, upload: String, name: String, mime: String, client: State<'_, Client>) -> Result<(), String> {
    let path = upload_path(client.dir()?, &upload)?;
    if !path.exists() {
        return Err("unknown upload".to_owned());
    }
    let core = client.core().await?;
    tauri::async_runtime::spawn(async move {
        let _ = core.send_file(&contact, &path, &name, &mime).await;
    });
    Ok(())
}

pub const PLUGIN_FILE_LIMIT: u64 = 32 * 1024 * 1024;

#[tauri::command]
pub async fn core_pick_for_plugin(accept: Option<String>, app: AppHandle, client: State<'_, Client>) -> Result<Option<HandedFile>, String> {
    let dir = client.dir()?.to_owned();
    let picked = core_pick_files(accept, app).await?;
    hand_over_picked(&dir, &picked)
}

#[tauri::command]
pub async fn core_take_photo_for_plugin(app: AppHandle, client: State<'_, Client>) -> Result<Option<HandedFile>, String> {
    let dir = client.dir()?.to_owned();
    let taken = core_take_photo(app).await?;
    hand_over_picked(&dir, &taken)
}

/// Every picker copy is deleted, handed over or not, so the original (with the place Clean would
/// strip from a photo) doesn't stay in the app.
pub fn hand_over_picked(dir: &Path, picked: &[PickedView]) -> Result<Option<HandedFile>, String> {
    let mut files = picked.iter();
    let first = files.next().map(|file| {
        let bytes = take_picked(dir, &file.path)?;
        Ok(HandedFile { name: file.name.clone(), mime: file.mime.clone(), data: BASE64.encode(bytes) })
    });
    for file in files {
        forget_picked(dir, &file.path);
    }
    first.transpose()
}

/// Picker folders only. Those copies are fresh and no message points to them.
fn picker_copy(dir: &Path, path: &str) -> Result<PathBuf, String> {
    let path = picked_path(dir, path)?;
    let pickers = [dir.join("uploads"), dir.join("files").join("uploads")];
    if pickers.iter().filter_map(|root| root.canonicalize().ok()).any(|root| path.starts_with(&root)) {
        Ok(path)
    } else {
        Err("that is not a file the user picked".to_owned())
    }
}

fn forget_picked(dir: &Path, path: &str) {
    if let Ok(path) = picker_copy(dir, path) {
        let _ = std::fs::remove_file(path);
    }
}

fn take_picked(dir: &Path, path: &str) -> Result<Vec<u8>, String> {
    let path = picker_copy(dir, path)?;
    let read = match std::fs::metadata(&path) {
        Ok(meta) if meta.len() > PLUGIN_FILE_LIMIT => Err("that file is too big to hand over".to_owned()),
        Ok(_) => std::fs::read(&path).map_err(failed),
        Err(error) => Err(failed(error)),
    };
    let _ = std::fs::remove_file(&path);
    read
}

#[tauri::command]
pub async fn core_pick_files(accept: Option<String>, app: AppHandle) -> Result<Vec<PickedView>, String> {
    let accept = accept.unwrap_or_default();
    let picked = tauri::async_runtime::spawn_blocking(move || app.platform().pick_files(&accept))
        .await
        .map_err(failed)?
        .map_err(failed)?;
    Ok(picked
        .into_iter()
        .map(|file| PickedView { path: file.path, name: file.name, mime: file.mime, size: file.size })
        .collect())
}

#[tauri::command]
pub async fn core_take_photo(app: AppHandle) -> Result<Vec<PickedView>, String> {
    let taken = tauri::async_runtime::spawn_blocking(move || app.platform().take_photo()).await.map_err(failed)?.map_err(failed)?;
    Ok(taken.into_iter().map(|file| PickedView { path: file.path, name: file.name, mime: file.mime, size: file.size }).collect())
}

#[derive(Serialize, Deserialize, Clone)]
pub struct PickedView {
    pub path: String,
    pub name: String,
    pub mime: String,
    pub size: u64,
}

/// Keeps the picker's copy, the contact pulls the chunks after `send_file` returns.
#[tauri::command]
pub async fn core_send_picked(contact: String, file: PickedView, client: State<'_, Client>) -> Result<(), String> {
    let path = picked_path(client.dir()?, &file.path)?;
    let core = client.core().await?;
    tauri::async_runtime::spawn(async move {
        let _ = core.send_file(&contact, &path, &file.name, &file.mime).await;
    });
    Ok(())
}

/// Every picker copy gets deleted, uploaded or not. The drive queue keeps a sealed copy, never the
/// path.
pub async fn upload_all_picked<Upload, Uploading>(dir: &Path, picked: &[PickedView], mut upload: Upload) -> Result<usize, String>
where
    Upload: FnMut(PickedView, PathBuf) -> Uploading,
    Uploading: std::future::Future<Output = anyhow::Result<Option<String>>>,
{
    let mut went = Ok(0);
    for file in picked {
        if let Ok(count) = went {
            went = match picker_copy(dir, &file.path) {
                Ok(path) => upload(file.clone(), path).await.map(|_| count + 1).map_err(failed),
                Err(error) => Err(error),
            };
        }
        forget_picked(dir, &file.path);
    }
    went
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::tests::{record, scratch};

    #[test]
    fn only_whole_files_can_be_opened() {
        assert!(openable(Some(record(4, true, false)), false).is_ok());
        assert!(openable(Some(record(2, false, false)), false).is_err());
        assert!(openable(Some(record(0, false, true)), false).is_err());
        assert!(openable(Some(record(1, false, false)), true).is_ok());
        assert!(openable(None, true).is_err());
    }

    #[test]
    fn picked_paths_stay_in_the_pickers_folders() {
        let dir = scratch("picked");
        std::fs::create_dir_all(dir.join("uploads")).unwrap();
        std::fs::create_dir_all(dir.join("files").join("outgoing")).unwrap();
        std::fs::write(dir.join("uploads").join("photo.jpg"), b"jpg").unwrap();
        std::fs::write(dir.join("files").join("outgoing").join("made.pdf"), b"pdf").unwrap();
        std::fs::write(dir.join("flickertalk.db"), b"secret").unwrap();
        std::fs::write(dir.join("storage.key.sealed"), b"secret").unwrap();

        assert!(picked_path(&dir, &dir.join("uploads").join("photo.jpg").to_string_lossy()).is_ok());
        assert!(picked_path(&dir, &dir.join("files").join("outgoing").join("made.pdf").to_string_lossy()).is_ok());
        std::fs::create_dir_all(dir.join("files").join("drive").join("f1")).unwrap();
        std::fs::write(dir.join("files").join("drive").join("f1").join("doc.pdf"), b"pdf").unwrap();
        assert!(picked_path(&dir, &dir.join("files").join("drive").join("f1").join("doc.pdf").to_string_lossy()).is_ok());
        for refused in [
            dir.join("flickertalk.db"),
            dir.join("storage.key.sealed"),
            dir.join("uploads").join("..").join("flickertalk.db"),
            dir.join("uploads"),
            PathBuf::from("/etc/passwd"),
            dir.join("uploads").join("missing.jpg"),
        ] {
            assert!(picked_path(&dir, &refused.to_string_lossy()).is_err(), "{}", refused.display());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_picked_copy_is_gone_once_a_plugin_has_it() {
        let dir = scratch("taken");
        for folder in [dir.join("files").join("uploads"), dir.join("uploads")] {
            std::fs::create_dir_all(&folder).unwrap();
            let copy = folder.join("1759400000000-photo.jpg");
            std::fs::write(&copy, b"jpg with gps").unwrap();

            assert_eq!(take_picked(&dir, &copy.to_string_lossy()).unwrap(), b"jpg with gps");
            assert!(!copy.exists(), "{}", copy.display());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_pickers_copies_can_be_taken() {
        let dir = scratch("not-taken");
        std::fs::create_dir_all(dir.join("files").join("outgoing")).unwrap();
        std::fs::create_dir_all(dir.join("files").join("drive").join("f1")).unwrap();
        std::fs::create_dir_all(dir.join("files").join("uploads")).unwrap();
        let kept = [
            dir.join("files").join("outgoing").join("1-made.pdf"),
            dir.join("files").join("drive").join("f1").join("doc.pdf"),
            dir.join("flickertalk.db"),
        ];
        for file in &kept {
            std::fs::write(file, b"keep").unwrap();
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&kept[0], dir.join("files").join("uploads").join("link.pdf")).unwrap();

        for refused in kept.iter().cloned().chain([dir.join("files").join("uploads").join("link.pdf")]) {
            assert!(take_picked(&dir, &refused.to_string_lossy()).is_err(), "{}", refused.display());
        }
        for file in &kept {
            assert_eq!(std::fs::read(file).unwrap(), b"keep", "{}", file.display());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn picked(path: &Path, name: &str) -> PickedView {
        PickedView { path: path.to_string_lossy().into_owned(), name: name.to_owned(), mime: "image/jpeg".to_owned(), size: 1 }
    }

    #[test]
    fn a_plugin_gets_the_first_picked_file_and_no_copy_stays() {
        let dir = scratch("hand-over");
        let uploads = dir.join("files").join("uploads");
        std::fs::create_dir_all(&uploads).unwrap();
        let (first, second) = (uploads.join("1759400000000-a.jpg"), uploads.join("1759400000000-b.jpg"));
        std::fs::write(&first, b"first with gps").unwrap();
        std::fs::write(&second, b"second with gps").unwrap();

        let handed = hand_over_picked(&dir, &[picked(&first, "a.jpg"), picked(&second, "b.jpg")]).unwrap().expect("a file");
        assert_eq!((handed.name.as_str(), handed.mime.as_str()), ("a.jpg", "image/jpeg"));
        assert_eq!(BASE64.decode(&handed.data).unwrap(), b"first with gps");
        assert!(!first.exists() && !second.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_first_pick_too_big_for_a_plugin_leaves_no_copy_either() {
        let dir = scratch("hand-over-big");
        let uploads = dir.join("files").join("uploads");
        std::fs::create_dir_all(&uploads).unwrap();
        let (first, second) = (uploads.join("1759400000000-video.mp4"), uploads.join("1759400000000-b.jpg"));
        std::fs::File::create(&first).unwrap().set_len(PLUGIN_FILE_LIMIT + 1).unwrap();
        std::fs::write(&second, b"second").unwrap();

        assert!(hand_over_picked(&dir, &[picked(&first, "video.mp4"), picked(&second, "b.jpg")]).is_err());
        assert!(!first.exists() && !second.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn handing_over_deletes_only_what_the_pick_returned_in_the_pickers_folder() {
        let dir = scratch("hand-over-none");
        let uploads = dir.join("files").join("uploads");
        std::fs::create_dir_all(&uploads).unwrap();
        std::fs::create_dir_all(dir.join("files").join("outgoing")).unwrap();
        let sent = uploads.join("1759300000000-sent.jpg");
        let made = dir.join("files").join("outgoing").join("1-made.pdf");
        std::fs::write(&sent, b"a sent message's file").unwrap();
        std::fs::write(&made, b"made").unwrap();

        assert!(hand_over_picked(&dir, &[]).unwrap().is_none());
        assert!(sent.exists());

        let first = uploads.join("1759400000000-a.jpg");
        std::fs::write(&first, b"a").unwrap();
        hand_over_picked(&dir, &[picked(&first, "a.jpg"), picked(&made, "made.pdf")]).unwrap();
        assert!(!first.exists());
        assert_eq!(std::fs::read(&made).unwrap(), b"made");
        assert!(sent.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_copy_a_drive_upload_picked_is_gone() {
        let dir = scratch("drive-up");
        let uploads = dir.join("files").join("uploads");
        std::fs::create_dir_all(&uploads).unwrap();
        let copies: Vec<_> = (0..3).map(|index| uploads.join(format!("1759400000000-{index}-a.jpg"))).collect();
        for copy in &copies {
            std::fs::write(copy, b"plain with gps").unwrap();
        }
        let picks: Vec<_> = copies.iter().map(|copy| picked(copy, "a.jpg")).collect();
        let mut answers = vec![Ok(Some("x1".to_owned())), Ok(None), Ok(Some("x3".to_owned()))].into_iter();

        let went = tauri::async_runtime::block_on(upload_all_picked(&dir, &picks, |file, path| {
            assert_eq!(file.name, "a.jpg");
            let answer = answers.next().unwrap();
            async move {
                assert_eq!(std::fs::read(path).unwrap(), b"plain with gps", "sealed from the copy, before it goes");
                answer
            }
        }));
        assert_eq!(went, Ok(3));
        assert!(copies.iter().all(|copy| !copy.exists()));
        assert_eq!(tauri::async_runtime::block_on(upload_all_picked(&dir, &[], |_, _| async { panic!("nothing to upload") })), Ok(0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_drive_upload_leaves_no_copy_either() {
        let dir = scratch("drive-fails");
        let uploads = dir.join("files").join("uploads");
        std::fs::create_dir_all(&uploads).unwrap();
        let copies: Vec<_> = (0..3).map(|index| uploads.join(format!("1759400000000-{index}-a.jpg"))).collect();
        for copy in &copies {
            std::fs::write(copy, b"plain").unwrap();
        }
        let picks: Vec<_> = copies.iter().map(|copy| picked(copy, "a.jpg")).collect();
        let mut tries = 0;

        let went = tauri::async_runtime::block_on(upload_all_picked(&dir, &picks, |_, _| {
            tries += 1;
            let answer = if tries == 1 { Ok(Some("x1".to_owned())) } else { Err(anyhow::anyhow!("no such folder")) };
            async move { answer }
        }));
        assert!(went.is_err());
        assert_eq!(tries, 2, "the third is not tried");
        assert!(copies.iter().all(|copy| !copy.exists()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_drive_upload_takes_only_the_pickers_copies() {
        let dir = scratch("drive-keep");
        std::fs::create_dir_all(dir.join("files").join("outgoing")).unwrap();
        let made = dir.join("files").join("outgoing").join("1-made.pdf");
        std::fs::write(&made, b"keep").unwrap();

        let went = tauri::async_runtime::block_on(upload_all_picked(&dir, &[picked(&made, "made.pdf")], |_, _| async { panic!("never uploaded") }));
        assert!(went.is_err());
        assert_eq!(std::fs::read(&made).unwrap(), b"keep");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_picked_copy_too_big_for_a_plugin_is_gone_too() {
        let dir = scratch("too-big");
        std::fs::create_dir_all(dir.join("files").join("uploads")).unwrap();
        let copy = dir.join("files").join("uploads").join("1759400000000-video.mp4");
        std::fs::File::create(&copy).unwrap().set_len(PLUGIN_FILE_LIMIT + 1).unwrap();

        assert!(take_picked(&dir, &copy.to_string_lossy()).is_err());
        assert!(!copy.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn upload_ids_cannot_point_elsewhere() {
        let dir = Path::new("/data");
        let id = new_upload_id();
        assert_eq!(upload_path(dir, &id).unwrap(), dir.join("files").join("outgoing").join(&id));
        assert_ne!(new_upload_id(), id);
        for bad in ["../identity", "", "abc", "/etc/passwd", "0123456789abcdef0123456789abcdeg"] {
            assert!(upload_path(dir, bad).is_err(), "{bad}");
        }
    }
}
