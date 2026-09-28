//! The user's own cloud, in the core (plan-drive, 2026-09-27): the login, the drive and the
//! backup. The plugin and the app only ever see what is here; the tokens and the vault key never
//! cross to the WebView (§54). Sealing and the cloud itself are `ft-vault`.
//!
//! What this phone keeps, in its settings: which cloud (`vault.provider`), the login's tokens
//! sealed with the storage key (`vault.tokens`), the vault key sealed the same way (`vault.key`)
//! and how many wrong recovery phrases were tried (`vault.tries`). Forgetting the drive removes
//! the first three; the cloud is never touched. The phrase itself is kept nowhere (2026-09-28).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, bail, ensure, Context, Result};
use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use ft_storage::Store;
use ft_vault::google::{self, GoogleDrive, TokenKeeper, Tokens};
use ft_vault::{cipher, Backup, Listing, Progress, Provider, Vault};

use crate::{Core, Event};

const PROVIDER: &str = "vault.provider";
const TOKENS: &str = "vault.tokens";
const KEY: &str = "vault.key";
/// Wrong phrases tried here, and until when the recovery is locked (decision 2026-09-28).
const TRIES: &str = "vault.tries";
/// How many wrong phrases in a row lock the recovery, and for how long.
pub const MAX_TRIES: u32 = 5;
const LOCKED_FOR: i64 = 24 * 3600 * 1000;
/// The OAuth client of this app, set at build time or in the settings; without it Google Drive
/// cannot be connected, and the app says so.
const GOOGLE_CLIENT_ID: &str = "vault.google_client_id";
pub const GOOGLE: &str = "google";

/// Who opens the system browser for a login and brings the redirect back: the platform bridge.
#[async_trait]
pub trait Authorizer: Send + Sync {
    async fn authorize(&self, url: &str, scheme: &str) -> Result<String>;
}

/// How a cloud is reached. The app has Google Drive; the tests, a memory.
#[async_trait]
pub trait Cloud: Send + Sync {
    /// Logs the user in, through the browser the authorizer opens, and returns the tokens.
    async fn login(&self, provider: &str, client_id: &str, authorizer: &dyn Authorizer) -> Result<Tokens>;
    /// The cloud, given where its tokens are kept.
    fn provider(&self, provider: &str, client_id: &str, keeper: Arc<dyn TokenKeeper>) -> Result<Arc<dyn Provider>>;
}

/// The redirect scheme Google gives an Android or iOS OAuth client: its id, reversed.
pub fn google_redirect_scheme(client_id: &str) -> Result<String> {
    let numeric = client_id.strip_suffix(".apps.googleusercontent.com").context("that is not a Google OAuth client id")?;
    ensure!(!numeric.is_empty() && !numeric.contains('/'), "that is not a Google OAuth client id");
    Ok(format!("com.googleusercontent.apps.{numeric}"))
}

/// Google Drive, the real one.
pub struct GoogleCloud;

#[async_trait]
impl Cloud for GoogleCloud {
    async fn login(&self, provider: &str, client_id: &str, authorizer: &dyn Authorizer) -> Result<Tokens> {
        ensure!(provider == GOOGLE, "only Google Drive is supported in this version");
        ensure!(!client_id.is_empty(), "no Google client id is set up for this app");
        let scheme = google_redirect_scheme(client_id)?;
        let redirect = format!("{scheme}:/oauth2redirect");
        let (verifier, challenge) = google::pkce();
        let state = ft_vault::index::new_id();
        let url = google::auth_url(google::AUTH_URL, client_id, &redirect, &challenge, &state);
        let back = authorizer.authorize(&url, &scheme).await?;
        let code = google::code_from_redirect(&back, &state)?;
        let http = ft_push::https_client(std::time::Duration::from_secs(30))?;
        google::exchange(&http, google::TOKEN_URL, client_id, &redirect, &code, &verifier).await
    }

    fn provider(&self, provider: &str, client_id: &str, keeper: Arc<dyn TokenKeeper>) -> Result<Arc<dyn Provider>> {
        ensure!(provider == GOOGLE, "only Google Drive is supported in this version");
        Ok(Arc::new(GoogleDrive::new(client_id, keeper)?))
    }
}

/// The tokens, sealed with the storage key, in the settings. Never in clear on disk.
struct SealedTokens {
    store: Store,
    key: [u8; 32],
}

#[async_trait]
impl TokenKeeper for SealedTokens {
    async fn tokens(&self) -> Option<Tokens> {
        let sealed = self.store.setting(TOKENS).await.ok().flatten()?;
        let bytes = STANDARD.decode(sealed).ok()?;
        let json = cipher::open(&self.key, "vault tokens", &bytes).ok()?;
        serde_json::from_slice(&json).ok()
    }

    async fn keep(&self, tokens: &Tokens) -> Result<()> {
        let sealed = cipher::seal(&self.key, "vault tokens", &serde_json::to_vec(tokens)?)?;
        self.store.set_setting(TOKENS, &STANDARD.encode(sealed)).await
    }
}

/// Where the drive stands, for the app and the plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VaultState {
    /// No cloud connected.
    None,
    /// Logged in to a cloud that has no drive yet: `vault_setup` makes one.
    Empty,
    /// Logged in to a cloud with a drive this phone has no key for: `vault_unlock` with the phrase.
    Locked,
    /// Logged in to a cloud with a drive of the first version (a generated code): `vault_setup`
    /// makes it again, and nothing of the old one opens.
    Outdated,
    /// Open.
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultStatus {
    pub state: VaultState,
    pub provider: Option<String>,
    pub drive: Option<ft_vault::Status>,
    /// Why the drive could not be opened at start, if it could not.
    pub problem: Option<String>,
    /// Wrong phrases this phone may still try before the recovery locks.
    pub tries_left: u32,
    /// Until when (ms) the recovery is locked here after too many wrong phrases.
    pub retry_at: Option<i64>,
}

/// Wrong phrases in a row, and the end of the lock they caused.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct Tries {
    failed: u32,
    until: i64,
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|since| since.as_millis() as i64).unwrap_or(0)
}

impl Core {
    /// How clouds are reached; the app sets Google Drive once, the tests a memory.
    pub fn set_cloud(&self, cloud: Arc<dyn Cloud>) {
        let _ = self.cloud.set(cloud);
    }

    /// Where the vault keeps what waits to go up; the app sets it once, at start.
    pub fn set_vault_dir(&self, dir: PathBuf) {
        let _ = self.vault_dir.set(dir);
    }

    fn vault_dir(&self) -> Result<PathBuf> {
        self.vault_dir.get().cloned().ok_or_else(|| anyhow!("no directory for the vault"))
    }

    fn cloud(&self) -> Result<Arc<dyn Cloud>> {
        self.cloud.get().cloned().ok_or_else(|| anyhow!("no cloud is set up in this app"))
    }

    async fn google_client_id(&self) -> String {
        match self.store.setting(GOOGLE_CLIENT_ID).await.ok().flatten() {
            Some(id) if !id.is_empty() => id,
            _ => option_env!("FT_GOOGLE_CLIENT_ID").unwrap_or_default().to_owned(),
        }
    }

    /// The OAuth client id, for a build that did not carry one (development).
    pub async fn set_google_client_id(&self, id: &str) -> Result<()> {
        self.store.set_setting(GOOGLE_CLIENT_ID, id.trim()).await
    }

    fn keeper(&self) -> Arc<dyn TokenKeeper> {
        Arc::new(SealedTokens { store: self.store.clone(), key: self.key })
    }

    async fn provider(&self) -> Result<Arc<dyn Provider>> {
        let provider = self.store.setting(PROVIDER).await?.context("no cloud is connected")?;
        self.cloud()?.provider(&provider, &self.google_client_id().await, self.keeper())
    }

    async fn vault(&self) -> Result<Arc<Vault>> {
        self.vault.lock().await.clone().context("the drive is not open")
    }

    async fn progress(&self) -> Progress {
        let events = self.events.clone();
        Arc::new(move |done, total| {
            let _ = events.send(Event::VaultProgress { done, total });
        })
    }

    /// Logs in to a cloud. Then, if this phone has the drive's key, opens it.
    pub async fn vault_connect(&self, provider: &str, authorizer: &dyn Authorizer) -> Result<VaultStatus> {
        let tokens = self.cloud()?.login(provider, &self.google_client_id().await, authorizer).await?;
        self.keeper().keep(&tokens).await?;
        self.store.set_setting(PROVIDER, provider).await?;
        let _ = self.events.send(Event::VaultChanged);
        self.vault_reopen().await
    }

    /// Opens the drive from what the phone keeps, at start or after a login. Never asks anyone.
    pub async fn vault_reopen(&self) -> Result<VaultStatus> {
        let Some(provider_name) = self.store.setting(PROVIDER).await? else {
            return Ok(self.status_of(VaultState::None, None, None, None).await);
        };
        let provider = match self.provider().await {
            Ok(provider) => provider,
            Err(error) => return Ok(self.status_of(VaultState::Locked, Some(provider_name), None, Some(error.to_string())).await),
        };
        let key = match self.store.setting(KEY).await? {
            Some(sealed) => {
                let bytes = STANDARD.decode(sealed)?;
                let key: [u8; 32] = cipher::open(&self.key, "vault key", &bytes)?.try_into().map_err(|_| anyhow!("the drive's key is corrupt"))?;
                Some(key)
            }
            None => None,
        };
        let status = match key {
            Some(key) => match Vault::open(provider, self.vault_dir()?, self.device_id.as_str(), key).await {
                Ok(vault) => {
                    let status = vault.status().await.ok();
                    *self.vault.lock().await = Some(Arc::new(vault));
                    self.status_of(VaultState::Ready, Some(provider_name), status, None).await
                }
                Err(error) => self.status_of(VaultState::Locked, Some(provider_name), None, Some(error.to_string())).await,
            },
            None => match Vault::exists(provider.as_ref()).await {
                Ok(true) if Vault::outdated(provider.as_ref()).await.unwrap_or(false) => self.status_of(VaultState::Outdated, Some(provider_name), None, None).await,
                Ok(true) => self.status_of(VaultState::Locked, Some(provider_name), None, None).await,
                Ok(false) => self.status_of(VaultState::Empty, Some(provider_name), None, None).await,
                Err(error) => self.status_of(VaultState::Locked, Some(provider_name), None, Some(error.to_string())).await,
            },
        };
        Ok(status)
    }

    async fn status_of(&self, state: VaultState, provider: Option<String>, drive: Option<ft_vault::Status>, problem: Option<String>) -> VaultStatus {
        let tries = self.tries().await;
        let locked = tries.until > now();
        VaultStatus {
            state,
            provider,
            drive,
            problem,
            tries_left: if locked { 0 } else { MAX_TRIES.saturating_sub(tries.failed) },
            retry_at: locked.then_some(tries.until),
        }
    }

    async fn tries(&self) -> Tries {
        let kept = self.store.setting(TRIES).await.ok().flatten();
        kept.and_then(|json| serde_json::from_str(&json).ok()).unwrap_or_default()
    }

    async fn keep_tries(&self, tries: &Tries) -> Result<()> {
        self.store.set_setting(TRIES, &serde_json::to_string(tries)?).await
    }

    pub async fn vault_status(&self) -> Result<VaultStatus> {
        let provider = self.store.setting(PROVIDER).await?;
        let Some(provider) = provider else {
            return Ok(self.status_of(VaultState::None, None, None, None).await);
        };
        if let Some(vault) = self.vault.lock().await.clone() {
            return Ok(self.status_of(VaultState::Ready, Some(provider), vault.status().await.ok(), None).await);
        }
        self.vault_reopen().await
    }

    async fn keep_key(&self, key: &[u8; 32]) -> Result<()> {
        let sealed = cipher::seal(&self.key, "vault key", key)?;
        self.store.set_setting(KEY, &STANDARD.encode(sealed)).await
    }

    /// A strong phrase for whoever wants the app to suggest one. Never kept.
    pub fn vault_suggest_phrase(&self) -> String {
        ft_vault::recovery::suggest()
    }

    /// Makes the drive in a cloud that has none (or one of the first version), its key sealed
    /// with the phrase the user chose. The phrase is kept by the user, never by the app.
    pub async fn vault_setup(&self, phrase: &str) -> Result<()> {
        ensure!(self.vault.lock().await.is_none(), "the drive is already open");
        let provider = self.provider().await?;
        let vault = Vault::create(provider, self.vault_dir()?, self.device_id.as_str(), phrase).await?;
        self.keep_key(&vault.key()).await?;
        *self.vault.lock().await = Some(Arc::new(vault));
        let _ = self.events.send(Event::VaultChanged);
        Ok(())
    }

    /// Opens, on this phone, a drive made on another: the cloud and the recovery phrase. Five
    /// wrong phrases in a row lock this for a day (decision 2026-09-28); not even the right one
    /// opens it until then.
    pub async fn vault_unlock(&self, phrase: &str) -> Result<()> {
        ensure!(self.vault.lock().await.is_none(), "the drive is already open");
        let mut tries = self.tries().await;
        if tries.until > now() {
            bail!("too many wrong phrases; try again after {}", tries.until);
        }
        let provider = self.provider().await?;
        let vault = match Vault::recover(provider, self.vault_dir()?, self.device_id.as_str(), phrase).await {
            Ok(vault) => vault,
            Err(error) => {
                if error.downcast_ref::<ft_vault::recovery::WrongPhrase>().is_some() {
                    tries.failed += 1;
                    if tries.failed >= MAX_TRIES {
                        tries = Tries { failed: 0, until: now() + LOCKED_FOR };
                    }
                    self.keep_tries(&tries).await?;
                    let _ = self.events.send(Event::VaultChanged);
                }
                return Err(error);
            }
        };
        self.store.forget_setting(TRIES).await?;
        self.keep_key(&vault.key()).await?;
        *self.vault.lock().await = Some(Arc::new(vault));
        let _ = self.events.send(Event::VaultChanged);
        Ok(())
    }

    /// Seals the drive's key with a new phrase, from the phone that has it open: the old phrase
    /// no longer opens it anywhere.
    pub async fn vault_change_phrase(&self, phrase: &str) -> Result<()> {
        self.vault().await?.change_phrase(phrase).await?;
        let _ = self.events.send(Event::VaultChanged);
        Ok(())
    }

    /// Forgets the cloud on this phone: tokens and key. The cloud keeps the drive, sealed.
    pub async fn vault_disconnect(&self) -> Result<()> {
        *self.vault.lock().await = None;
        for key in [PROVIDER, TOKENS, KEY] {
            self.store.forget_setting(key).await?;
        }
        let _ = self.events.send(Event::VaultChanged);
        Ok(())
    }

    // ---- The drive ----

    pub async fn vault_list(&self, parent: Option<&str>) -> Result<Listing> {
        self.vault().await?.list(parent).await
    }

    pub async fn vault_mkdir(&self, name: &str, parent: Option<&str>) -> Result<String> {
        let id = self.vault().await?.mkdir(name, parent).await?;
        let _ = self.events.send(Event::VaultChanged);
        Ok(id)
    }

    pub async fn vault_rename(&self, id: &str, name: &str) -> Result<()> {
        self.vault().await?.rename(id, name).await?;
        let _ = self.events.send(Event::VaultChanged);
        Ok(())
    }

    pub async fn vault_move(&self, id: &str, parent: Option<&str>) -> Result<()> {
        self.vault().await?.move_to(id, parent).await?;
        let _ = self.events.send(Event::VaultChanged);
        Ok(())
    }

    pub async fn vault_remove(&self, id: &str) -> Result<()> {
        self.vault().await?.remove(id).await?;
        let _ = self.events.send(Event::VaultChanged);
        Ok(())
    }

    /// Seals a file of the phone and puts it in the drive. None when it had to wait for the network.
    pub async fn vault_upload(&self, path: &Path, name: &str, mime: &str, parent: Option<&str>) -> Result<Option<String>> {
        let vault = self.vault().await?;
        let id = vault.upload(path, name, mime, parent, self.progress().await).await?;
        let _ = self.events.send(Event::VaultChanged);
        Ok(id)
    }

    /// Tries again what waits. How many still wait.
    pub async fn vault_run_queue(&self) -> Result<usize> {
        let Some(vault) = self.vault.lock().await.clone() else { return Ok(0) };
        let left = vault.run_queue(self.progress().await).await?;
        let _ = self.events.send(Event::VaultChanged);
        Ok(left)
    }

    pub async fn vault_cancel_pending(&self, blob: &str) -> Result<()> {
        self.vault().await?.cancel_pending(blob).await?;
        let _ = self.events.send(Event::VaultChanged);
        Ok(())
    }

    /// Brings a file of the drive down, opened, into the app's files folder. Its path and record.
    pub async fn vault_download(&self, id: &str) -> Result<(PathBuf, ft_vault::File)> {
        let vault = self.vault().await?;
        let file = vault.file(id).await.context("no such file")?;
        let dir = self.files_dir()?.join("drive").join(id);
        let to = dir.join(crate::files::safe_file_name(&file.name));
        if to.exists() && tokio::fs::metadata(&to).await.map(|meta| meta.len()).unwrap_or(0) == file.size {
            return Ok((to, file));
        }
        let file = vault.download(id, &to, self.progress().await).await?;
        Ok((to, file))
    }

    /// Sends a file of the drive to a contact: down from the cloud, then as any file (§62).
    pub async fn vault_send(&self, id: &str, contact: &str) -> Result<String> {
        let (path, file) = self.vault_download(id).await?;
        self.send_file(contact, &path, &file.name, &file.mime).await
    }

    // ---- The backup (§61) ----

    /// Puts a consistent copy of the database, the storage key and the files in the drive.
    pub async fn vault_backup(&self) -> Result<Backup> {
        let vault = self.vault().await?;
        let snapshot = self.vault_dir()?.join("snapshot.db");
        let _ = tokio::fs::remove_file(&snapshot).await;
        self.store.snapshot(&snapshot).await?;
        let files = self.files_dir()?.to_owned();
        let backup = vault.backup(&snapshot, &self.key, &files, self.progress().await).await;
        let _ = tokio::fs::remove_file(&snapshot).await;
        let backup = backup?;
        let _ = self.events.send(Event::VaultChanged);
        Ok(backup)
    }

    pub async fn vault_backup_info(&self) -> Result<Option<Backup>> {
        Ok(self.vault().await?.backup_info().await)
    }

    /// Brings the backup down into the move folder and the files folder. The app swaps the
    /// database in at the next start, as after a move (§60): nothing changes until then.
    pub async fn vault_restore(&self) -> Result<Backup> {
        let vault = self.vault().await?;
        let move_dir = self.move_dir.get().cloned().context("no directory for moving")?;
        let files = self.files_dir()?.to_owned();
        let backup = vault.restore(&move_dir, &files, self.progress().await).await?;
        let _ = self.events.send(Event::VaultChanged);
        Ok(backup)
    }

    /// Whether a plugin was granted the drive (`drive` in its manifest).
    pub async fn plugin_may_use_drive(&self, id: &str) -> Result<bool> {
        Ok(self.granted_to(id).await?.drive)
    }

    /// Where the files a plugin hands over for the drive may come from: the pickers' folders.
    pub fn vault_upload_source(&self, path: &Path) -> Result<PathBuf> {
        let files = self.files_dir()?;
        let real = path.canonicalize().context("that file is no longer there")?;
        let allowed = [files.join("uploads"), files.join("outgoing"), files.join("drive")];
        let inside = allowed.iter().filter_map(|root| root.canonicalize().ok()).any(|root| real.starts_with(&root));
        if inside && real.is_file() {
            Ok(real)
        } else {
            bail!("that file is not one the user picked")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_redirect_scheme_is_the_client_id_reversed() {
        assert_eq!(google_redirect_scheme("123-abc.apps.googleusercontent.com").unwrap(), "com.googleusercontent.apps.123-abc");
        assert!(google_redirect_scheme("not a client id").is_err());
        assert!(google_redirect_scheme(".apps.googleusercontent.com").is_err());
    }
}
