use serde::{Deserialize, Serialize};
use tauri::Runtime;

use crate::{Platform, Result};

#[derive(Serialize)]
struct OpenFile<'a> {
    path: &'a str,
    mime: &'a str,
}

#[derive(Serialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct Pick<'a> {
    accept: &'a str,
}

#[derive(Serialize)]
struct SaveFile<'a> {
    path: &'a str,
    name: &'a str,
    mime: &'a str,
}

#[derive(Serialize)]
struct ShareText<'a> {
    text: &'a str,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
pub struct PickedFile {
    pub path: String,
    pub name: String,
    pub mime: String,
    pub size: u64,
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct Picked {
    files: Vec<PickedFile>,
}

impl<R: Runtime> Platform<R> {
    /// Only works for files under `files/`, the FileProvider shares nothing else.
    pub fn open_file(&self, path: &str, mime: &str) -> Result<()> {
        self.run("openFile", OpenFile { path, mime })
    }

    pub fn save_to_downloads(&self, path: &str, name: &str, mime: &str) -> Result<()> {
        self.run("saveToDownloads", SaveFile { path, name, mime })
    }

    pub fn print_file(&self, path: &str, name: &str, mime: &str) -> Result<()> {
        self.run("printFile", SaveFile { path, name, mime })
    }

    pub fn share_text(&self, text: &str) -> Result<()> {
        self.run("shareText", ShareText { text })
    }

    pub fn share_file(&self, path: &str, name: &str, mime: &str) -> Result<()> {
        self.run("shareFile", SaveFile { path, name, mime })
    }

    // Used instead of the WebView's file input, which strands the user outside the app until they
    // pick something.
    pub fn pick_files(&self, accept: &str) -> Result<Vec<PickedFile>> {
        #[cfg(mobile)]
        {
            Ok(self
                .handle
                .run_mobile_plugin::<Picked>("pickFiles", Pick { accept })?
                .files)
        }
        #[cfg(not(mobile))]
        let _ = accept;
        #[cfg(not(mobile))]
        {
            Err(crate::Error::Unsupported)
        }
    }

    pub fn take_photo(&self) -> Result<Vec<PickedFile>> {
        #[cfg(mobile)]
        {
            Ok(self
                .handle
                .run_mobile_plugin::<Picked>("takePhoto", ())?
                .files)
        }
        #[cfg(not(mobile))]
        {
            Err(crate::Error::Unsupported)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kotlin_side_gets_the_file_names_it_expects() {
        let open = serde_json::to_value(OpenFile {
            path: "/files/a.jpg",
            mime: "image/jpeg",
        })
        .unwrap();
        assert_eq!(
            open,
            serde_json::json!({ "path": "/files/a.jpg", "mime": "image/jpeg" })
        );
        let save = serde_json::to_value(SaveFile {
            path: "/files/a.jpg",
            name: "a.jpg",
            mime: "image/jpeg",
        })
        .unwrap();
        assert_eq!(
            save,
            serde_json::json!({ "path": "/files/a.jpg", "name": "a.jpg", "mime": "image/jpeg" })
        );
        let picked: Picked = serde_json::from_value(serde_json::json!({
            "files": [{ "path": "/data/uploads/1-a.jpg", "name": "a.jpg", "mime": "image/jpeg", "size": 12 }]
        }))
        .unwrap();
        assert_eq!(picked.files[0].name, "a.jpg");
        assert_eq!(picked.files[0].size, 12);
        let share = serde_json::to_value(ShareText {
            text: "Add me: https://flickertalk.com/add#card",
        })
        .unwrap();
        assert_eq!(
            share,
            serde_json::json!({ "text": "Add me: https://flickertalk.com/add#card" })
        );
        let pick = serde_json::to_value(Pick { accept: "image/*" }).unwrap();
        assert_eq!(pick, serde_json::json!({ "accept": "image/*" }));
        let print = serde_json::to_value(SaveFile {
            path: "/files/a.pdf",
            name: "a.pdf",
            mime: "application/pdf",
        })
        .unwrap();
        assert_eq!(
            print,
            serde_json::json!({ "path": "/files/a.pdf", "name": "a.pdf", "mime": "application/pdf" })
        );
    }
}
