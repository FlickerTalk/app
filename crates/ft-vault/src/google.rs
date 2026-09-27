//! Google Drive (plan-drive §4.2): the app's own folder, made with the `drive.file` scope, which
//! only ever sees what the app made. Small files go whole; big ones by the resumable upload, in
//! parts, so a phone that loses the network for a moment does not start again. The login is
//! OAuth with PKCE in the system browser: the core builds the address, the platform bridge opens
//! it and brings the redirect back, and the tokens never leave the core.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, ensure, Context, Result};
use async_trait::async_trait;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Mutex;

use crate::provider::{Progress, Provider, Quota};

/// The folder the app makes in the user's Drive. The user sees it, and only sealed files in it.
pub const FOLDER: &str = "FlickerTalk";
/// What the app asks for: only the files it makes itself. Not sensitive, no audit (plan §4.2).
pub const SCOPE: &str = "https://www.googleapis.com/auth/drive.file";
pub const API: &str = "https://www.googleapis.com";
pub const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const FOLDER_MIME: &str = "application/vnd.google-apps.folder";
/// A part of a resumable upload: a multiple of 256 KiB, as Google asks.
const PART: u64 = 8 * 1024 * 1024;
/// How long one request may take. An upload part is 8 MiB; a slow phone still makes it.
const PATIENCE: Duration = Duration::from_secs(120);

// ---- OAuth ----

/// The tokens of a login. The refresh token lasts; the access token an hour.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// When the access token expires, ms since the epoch.
    pub expires_at: i64,
}

/// A PKCE verifier and its challenge, fresh.
pub fn pkce() -> (String, String) {
    let bytes: [u8; 32] = rand::random();
    let verifier = URL_SAFE_NO_PAD.encode(bytes);
    let challenge = URL_SAFE_NO_PAD.encode(sha256(verifier.as_bytes()));
    (verifier, challenge)
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    // SHA-256 is what PKCE names; `ring` carries it, and the HTTPS stack already builds `ring`.
    let digest = ring::digest::digest(&ring::digest::SHA256, bytes);
    digest.as_ref().try_into().expect("32 bytes")
}

fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// The address the browser opens to log in. `redirect` is the app's own scheme with a path.
pub fn auth_url(auth_base: &str, client_id: &str, redirect: &str, challenge: &str, state: &str) -> String {
    format!(
        "{auth_base}?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={}&code_challenge_method=S256&state={}&access_type=offline&prompt=consent",
        encode(client_id),
        encode(redirect),
        encode(SCOPE),
        encode(challenge),
        encode(state)
    )
}

/// The code and state a redirect brought back, or why there is none.
pub fn code_from_redirect(url: &str, state: &str) -> Result<String> {
    let query = url.split_once('?').map(|(_, query)| query).unwrap_or_default();
    let mut code = None;
    let mut got_state = None;
    for pair in query.split('&') {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = decode(value);
        match name {
            "code" => code = Some(value),
            "state" => got_state = Some(value),
            "error" => bail!("the login was refused: {value}"),
            _ => {}
        }
    }
    ensure!(got_state.as_deref() == Some(state), "the login that came back is not the one that was started");
    code.filter(|code| !code.is_empty()).context("the login brought no code back")
}

fn decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        match bytes[at] {
            b'%' if at + 2 < bytes.len() => {
                if let Ok(byte) = u8::from_str_radix(std::str::from_utf8(&bytes[at + 1..at + 3]).unwrap_or("zz"), 16) {
                    out.push(byte);
                    at += 3;
                    continue;
                }
                out.push(b'%');
                at += 1;
            }
            b'+' => {
                out.push(b' ');
                at += 1;
            }
            other => {
                out.push(other);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A form body, as a browser would send it.
fn form_body(fields: &[(&str, &str)]) -> String {
    fields.iter().map(|(name, value)| format!("{}={}", encode(name), encode(value))).collect::<Vec<_>>().join("&")
}

#[derive(Deserialize)]
struct TokenAnswer {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|since| since.as_millis() as i64).unwrap_or(0)
}

/// Trades the code of a login for tokens.
pub async fn exchange(http: &reqwest::Client, token_url: &str, client_id: &str, redirect: &str, code: &str, verifier: &str) -> Result<Tokens> {
    let form = [("client_id", client_id), ("code", code), ("code_verifier", verifier), ("grant_type", "authorization_code"), ("redirect_uri", redirect)];
    let answer = http
        .post(token_url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form_body(&form))
        .send()
        .await
        .context("cannot reach the login server")?;
    ensure!(answer.status().is_success(), "the login server answered {}", answer.status());
    let tokens: TokenAnswer = answer.json().await?;
    Ok(Tokens { access_token: tokens.access_token, refresh_token: tokens.refresh_token, expires_at: now() + tokens.expires_in.unwrap_or(3600) * 1000 })
}

/// Trades a refresh token for a fresh access token.
pub async fn refresh(http: &reqwest::Client, token_url: &str, client_id: &str, refresh_token: &str) -> Result<Tokens> {
    let form = [("client_id", client_id), ("refresh_token", refresh_token), ("grant_type", "refresh_token")];
    let answer = http
        .post(token_url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form_body(&form))
        .send()
        .await
        .context("cannot reach the login server")?;
    ensure!(answer.status().is_success(), "the login is no longer valid ({})", answer.status());
    let tokens: TokenAnswer = answer.json().await?;
    Ok(Tokens {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token.or(Some(refresh_token.to_owned())),
        expires_at: now() + tokens.expires_in.unwrap_or(3600) * 1000,
    })
}

/// Where the tokens are kept, and told when they change, so the app can seal them away.
#[async_trait]
pub trait TokenKeeper: Send + Sync {
    async fn tokens(&self) -> Option<Tokens>;
    async fn keep(&self, tokens: &Tokens) -> Result<()>;
}

/// Tokens in memory, for the tests.
#[derive(Default)]
pub struct MemoryTokens(pub Mutex<Option<Tokens>>);

#[async_trait]
impl TokenKeeper for MemoryTokens {
    async fn tokens(&self) -> Option<Tokens> {
        self.0.lock().await.clone()
    }

    async fn keep(&self, tokens: &Tokens) -> Result<()> {
        *self.0.lock().await = Some(tokens.clone());
        Ok(())
    }
}

// ---- The provider ----

#[derive(Deserialize)]
struct DriveFile {
    id: String,
    name: String,
    #[serde(default)]
    size: Option<String>,
}

#[derive(Deserialize)]
struct DriveList {
    #[serde(default)]
    files: Vec<DriveFile>,
    #[serde(default, rename = "nextPageToken")]
    next: Option<String>,
}

#[derive(Deserialize)]
struct About {
    #[serde(rename = "storageQuota")]
    quota: StorageQuota,
}

#[derive(Deserialize)]
struct StorageQuota {
    #[serde(default)]
    usage: Option<String>,
    #[serde(default)]
    limit: Option<String>,
}

/// Google Drive, as the vault sees it: flat names inside the app's folder.
pub struct GoogleDrive {
    http: reqwest::Client,
    api: String,
    token_url: String,
    client_id: String,
    keeper: Arc<dyn TokenKeeper>,
    folder: Mutex<Option<String>>,
    ids: Mutex<HashMap<String, String>>,
}

impl GoogleDrive {
    /// The real one. `client_id` is the OAuth client of this app on this platform.
    pub fn new(client_id: &str, keeper: Arc<dyn TokenKeeper>) -> Result<Self> {
        Self::at(API, TOKEN_URL, client_id, keeper)
    }

    /// Against another address: the tests' fake Drive.
    pub fn at(api: &str, token_url: &str, client_id: &str, keeper: Arc<dyn TokenKeeper>) -> Result<Self> {
        Ok(Self {
            http: ft_push::https_client(PATIENCE)?,
            api: api.trim_end_matches('/').to_owned(),
            token_url: token_url.to_owned(),
            client_id: client_id.to_owned(),
            keeper,
            folder: Mutex::new(None),
            ids: Mutex::new(HashMap::new()),
        })
    }

    /// A valid access token, refreshed if the one kept is about to expire.
    async fn token(&self) -> Result<String> {
        let tokens = self.keeper.tokens().await.context("not logged in")?;
        if tokens.expires_at > now() + 60_000 {
            return Ok(tokens.access_token);
        }
        let refresh_token = tokens.refresh_token.context("the login expired and cannot be renewed")?;
        let fresh = refresh(&self.http, &self.token_url, &self.client_id, &refresh_token).await?;
        self.keeper.keep(&fresh).await?;
        Ok(fresh.access_token)
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T> {
        let token = self.token().await?;
        let answer = self.http.get(url).bearer_auth(token).send().await?;
        ensure!(answer.status().is_success(), "Drive answered {} to {url}", answer.status());
        Ok(answer.json().await?)
    }

    async fn find(&self, query: &str) -> Result<Vec<DriveFile>> {
        let mut found = vec![];
        let mut page: Option<String> = None;
        loop {
            let mut url = format!("{}/drive/v3/files?q={}&fields=nextPageToken,files(id,name,size)&pageSize=1000", self.api, encode(query));
            if let Some(token) = &page {
                url.push_str(&format!("&pageToken={}", encode(token)));
            }
            let list: DriveList = self.get_json(&url).await?;
            found.extend(list.files);
            match list.next {
                Some(next) => page = Some(next),
                None => return Ok(found),
            }
        }
    }

    /// The app's folder, made if it is not there.
    async fn folder(&self) -> Result<String> {
        if let Some(id) = self.folder.lock().await.clone() {
            return Ok(id);
        }
        let found = self.find(&format!("name = '{FOLDER}' and mimeType = '{FOLDER_MIME}' and trashed = false")).await?;
        let id = match found.into_iter().next() {
            Some(folder) => folder.id,
            None => {
                let token = self.token().await?;
                let answer = self
                    .http
                    .post(format!("{}/drive/v3/files", self.api))
                    .bearer_auth(token)
                    .json(&serde_json::json!({ "name": FOLDER, "mimeType": FOLDER_MIME }))
                    .send()
                    .await?;
                ensure!(answer.status().is_success(), "Drive would not make the folder: {}", answer.status());
                let made: DriveFile = answer.json().await?;
                made.id
            }
        };
        *self.folder.lock().await = Some(id.clone());
        Ok(id)
    }

    /// The id of a file by its name in the folder, if it is there.
    async fn id_of(&self, name: &str) -> Result<Option<String>> {
        if let Some(id) = self.ids.lock().await.get(name).cloned() {
            return Ok(Some(id));
        }
        let folder = self.folder().await?;
        let found = self.find(&format!("name = '{}' and '{folder}' in parents and trashed = false", name.replace('\'', "\\'"))).await?;
        let id = found.into_iter().next().map(|file| file.id);
        if let Some(id) = &id {
            self.ids.lock().await.insert(name.to_owned(), id.clone());
        }
        Ok(id)
    }

    /// Starts a resumable upload, new or replacing, and returns where to put the bytes.
    async fn start_upload(&self, name: &str, size: u64) -> Result<String> {
        let token = self.token().await?;
        let request = match self.id_of(name).await? {
            Some(id) => self.http.patch(format!("{}/upload/drive/v3/files/{id}?uploadType=resumable", self.api)).json(&serde_json::json!({})),
            None => {
                let folder = self.folder().await?;
                self.http
                    .post(format!("{}/upload/drive/v3/files?uploadType=resumable", self.api))
                    .json(&serde_json::json!({ "name": name, "parents": [folder] }))
            }
        };
        let answer = request.bearer_auth(token).header("X-Upload-Content-Length", size.to_string()).send().await?;
        ensure!(answer.status().is_success(), "Drive would not start the upload: {}", answer.status());
        let location = answer.headers().get("location").and_then(|value| value.to_str().ok()).context("Drive gave no upload address")?;
        Ok(if location.starts_with('/') { format!("{}{location}", self.api) } else { location.to_owned() })
    }

    /// Puts the bytes of a file at an upload address, in parts, and records the file's id.
    async fn put_parts(&self, name: &str, session: &str, path: &Path, progress: Progress) -> Result<()> {
        let mut file = tokio::fs::File::open(path).await?;
        let size = file.metadata().await?.len();
        let mut at = 0u64;
        let mut buffer = vec![0u8; PART as usize];
        loop {
            let want = (size - at).min(PART) as usize;
            let mut got = 0;
            while got < want {
                let read = file.read(&mut buffer[got..want]).await?;
                if read == 0 {
                    break;
                }
                got += read;
            }
            let end = at + got as u64;
            let range = if size == 0 { "bytes */0".to_owned() } else { format!("bytes {at}-{}/{size}", end - 1) };
            let answer = self
                .http
                .put(session)
                .header("Content-Range", range)
                .header("Content-Length", got.to_string())
                .body(buffer[..got].to_vec())
                .send()
                .await?;
            let status = answer.status().as_u16();
            if status == 308 {
                // Google says how far it got; usually the end of this part.
                if let Some(range) = answer.headers().get("range").and_then(|value| value.to_str().ok()) {
                    if let Some(last) = range.rsplit('-').next().and_then(|last| last.parse::<u64>().ok()) {
                        at = last + 1;
                        file.seek(std::io::SeekFrom::Start(at)).await?;
                        progress(at, size);
                        continue;
                    }
                }
                at = end;
                progress(at, size);
                continue;
            }
            ensure!(answer.status().is_success(), "Drive refused the upload: {status}");
            let made: DriveFile = answer.json().await?;
            self.ids.lock().await.insert(name.to_owned(), made.id);
            progress(size, size);
            return Ok(());
        }
    }
}

#[async_trait]
impl Provider for GoogleDrive {
    async fn read(&self, name: &str) -> Result<Option<Vec<u8>>> {
        let Some(id) = self.id_of(name).await? else { return Ok(None) };
        let token = self.token().await?;
        let answer = self.http.get(format!("{}/drive/v3/files/{id}?alt=media", self.api)).bearer_auth(token).send().await?;
        if answer.status().as_u16() == 404 {
            self.ids.lock().await.remove(name);
            return Ok(None);
        }
        ensure!(answer.status().is_success(), "Drive answered {} reading {name}", answer.status());
        Ok(Some(answer.bytes().await?.to_vec()))
    }

    async fn write(&self, name: &str, bytes: Vec<u8>) -> Result<()> {
        let session = self.start_upload(name, bytes.len() as u64).await?;
        let temp = std::env::temp_dir().join(format!("ft-vault-{}", crate::index::new_id()));
        tokio::fs::write(&temp, &bytes).await?;
        let put = self.put_parts(name, &session, &temp, crate::provider::quiet()).await;
        let _ = tokio::fs::remove_file(&temp).await;
        put
    }

    async fn upload(&self, name: &str, path: &Path, progress: Progress) -> Result<()> {
        let size = tokio::fs::metadata(path).await?.len();
        let session = self.start_upload(name, size).await?;
        self.put_parts(name, &session, path, progress).await
    }

    async fn download(&self, name: &str, path: &Path, progress: Progress) -> Result<()> {
        let id = self.id_of(name).await?.with_context(|| format!("{name} is not in the cloud"))?;
        let token = self.token().await?;
        let mut answer = self.http.get(format!("{}/drive/v3/files/{id}?alt=media", self.api)).bearer_auth(token).send().await?;
        ensure!(answer.status().is_success(), "Drive answered {} downloading {name}", answer.status());
        let total = answer.content_length().unwrap_or(0);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut file = tokio::fs::File::create(path).await?;
        let mut done = 0u64;
        while let Some(chunk) = answer.chunk().await? {
            file.write_all(&chunk).await?;
            done += chunk.len() as u64;
            progress(done, total.max(done));
        }
        file.flush().await?;
        Ok(())
    }

    async fn remove(&self, name: &str) -> Result<()> {
        let Some(id) = self.id_of(name).await? else { return Ok(()) };
        let token = self.token().await?;
        let answer = self.http.delete(format!("{}/drive/v3/files/{id}", self.api)).bearer_auth(token).send().await?;
        self.ids.lock().await.remove(name);
        ensure!(answer.status().is_success() || answer.status().as_u16() == 404, "Drive answered {} removing {name}", answer.status());
        Ok(())
    }

    async fn list(&self) -> Result<Vec<(String, u64)>> {
        let folder = self.folder().await?;
        let found = self.find(&format!("'{folder}' in parents and trashed = false")).await?;
        let mut ids = self.ids.lock().await;
        Ok(found
            .into_iter()
            .map(|file| {
                ids.insert(file.name.clone(), file.id);
                (file.name, file.size.and_then(|size| size.parse().ok()).unwrap_or(0))
            })
            .collect())
    }

    async fn quota(&self) -> Result<Option<Quota>> {
        let about: About = self.get_json(&format!("{}/drive/v3/about?fields=storageQuota", self.api)).await?;
        let used = about.quota.usage.and_then(|value| value.parse().ok()).unwrap_or(0);
        Ok(about.quota.limit.and_then(|value| value.parse().ok()).map(|total| Quota { used, total }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_login_address_carries_the_challenge_and_nothing_secret() {
        let (verifier, challenge) = pkce();
        assert_ne!(verifier, challenge);
        assert_eq!(challenge, URL_SAFE_NO_PAD.encode(sha256(verifier.as_bytes())));
        let url = auth_url(AUTH_URL, "123.apps.googleusercontent.com", "com.example:/oauth", &challenge, "st ate");
        assert!(url.starts_with(AUTH_URL));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains(&format!("code_challenge={challenge}")));
        assert!(url.contains("redirect_uri=com.example%3A%2Foauth"));
        assert!(url.contains("state=st%20ate"));
        assert!(url.contains("scope=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fdrive.file"));
        assert!(!url.contains(&verifier), "the verifier never travels in the address");
    }

    #[test]
    fn the_redirect_gives_the_code_only_with_the_state_that_was_sent() {
        assert_eq!(code_from_redirect("com.example:/oauth?state=abc&code=4%2FxyZ&scope=x", "abc").unwrap(), "4/xyZ");
        assert!(code_from_redirect("com.example:/oauth?state=other&code=4", "abc").is_err());
        assert!(code_from_redirect("com.example:/oauth?state=abc", "abc").is_err());
        assert!(code_from_redirect("com.example:/oauth?state=abc&error=access_denied", "abc").is_err());
        assert!(code_from_redirect("garbage", "abc").is_err());
        assert_eq!(decode("a+b%20c%zz"), "a b c%zz");
    }
}
