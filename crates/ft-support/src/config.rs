//! Where the support contact keeps its things and which router it uses, from the environment.
//! Everything lives in one folder (`FT_SUPPORT_DATA`, `/data` in the container: a volume) unless a
//! variable puts a piece elsewhere (the key and the identity bundle can be Swarm secrets).

use std::path::PathBuf;

use anyhow::{ensure, Result};

/// What `--help` prints.
pub const USAGE: &str = "\
ft-support: the FlickerTalk support contact, a headless peer that answers by itself.

  ft-support        connect and answer, until SIGTERM or Ctrl-C
  ft-support init   make the identity (or open it) and print its card link, connecting to nothing

Environment (all optional):
  FT_SUPPORT_DATA           folder for everything below (default /data)
  FT_SUPPORT_KEY_FILE       the storage key, 64 hex digits; made on the first run ($DATA/support.key)
  FT_SUPPORT_DB             the SQLite database: identity, contacts, sessions ($DATA/support.db)
  FT_SUPPORT_IDENTITY_FILE  the identity bundle: restored into an empty database, written once ($DATA/identity)
  FT_SUPPORT_CARD_FILE      where the Contact Card link is written ($DATA/card.txt)
  FT_SUPPORT_FILES          received files and outgoing copies ($DATA/files)
  FT_SUPPORT_ROUTER         the router (default https://api.flickertalk.com)
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub router: String,
    pub key_file: PathBuf,
    pub db: PathBuf,
    pub identity_file: PathBuf,
    pub card_file: PathBuf,
    pub files: PathBuf,
}

impl Config {
    /// From the variables `get` finds; an empty one counts as unset.
    pub fn from_vars(get: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let get = |name: &str| get(name).map(|value| value.trim().to_owned()).filter(|value| !value.is_empty());
        let data = PathBuf::from(get("FT_SUPPORT_DATA").unwrap_or_else(|| "/data".to_owned()));
        let path = |name: &str, default: &str| get(name).map_or_else(|| data.join(default), PathBuf::from);
        let router = get("FT_SUPPORT_ROUTER").unwrap_or_else(|| crate::ROUTER.to_owned());
        ensure!(router.starts_with("https://") || router.starts_with("http://"), "FT_SUPPORT_ROUTER must be a web address");
        Ok(Self {
            router,
            key_file: path("FT_SUPPORT_KEY_FILE", "support.key"),
            db: path("FT_SUPPORT_DB", "support.db"),
            identity_file: path("FT_SUPPORT_IDENTITY_FILE", "identity"),
            card_file: path("FT_SUPPORT_CARD_FILE", "card.txt"),
            files: path("FT_SUPPORT_FILES", "files"),
        })
    }

    pub fn from_env() -> Result<Self> {
        Self::from_vars(|name| std::env::var(name).ok())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn config(vars: &[(&str, &str)]) -> Result<Config> {
        let vars: HashMap<String, String> = vars.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect();
        Config::from_vars(|name| vars.get(name).cloned())
    }

    #[test]
    fn by_default_everything_lives_in_the_data_volume() {
        let config = config(&[]).unwrap();
        assert_eq!(
            config,
            Config {
                router: "https://api.flickertalk.com".to_owned(),
                key_file: "/data/support.key".into(),
                db: "/data/support.db".into(),
                identity_file: "/data/identity".into(),
                card_file: "/data/card.txt".into(),
                files: "/data/files".into(),
            }
        );
    }

    #[test]
    fn the_folder_moves_everything_and_each_variable_one_piece() {
        let config = config(&[
            ("FT_SUPPORT_DATA", "/tmp/peer"),
            ("FT_SUPPORT_KEY_FILE", "/run/secrets/ft_support_key"),
            ("FT_SUPPORT_IDENTITY_FILE", "/run/secrets/ft_support_identity"),
            ("FT_SUPPORT_ROUTER", "http://127.0.0.1:8787"),
        ])
        .unwrap();
        assert_eq!(config.key_file, PathBuf::from("/run/secrets/ft_support_key"));
        assert_eq!(config.identity_file, PathBuf::from("/run/secrets/ft_support_identity"));
        assert_eq!(config.db, PathBuf::from("/tmp/peer/support.db"));
        assert_eq!(config.files, PathBuf::from("/tmp/peer/files"));
        assert_eq!(config.router, "http://127.0.0.1:8787");
    }

    #[test]
    fn an_empty_variable_counts_as_unset() {
        assert_eq!(config(&[("FT_SUPPORT_DB", ""), ("FT_SUPPORT_ROUTER", " ")]).unwrap(), config(&[]).unwrap());
    }

    #[test]
    fn the_router_must_be_a_web_address() {
        assert!(config(&[("FT_SUPPORT_ROUTER", "api.flickertalk.com")]).is_err());
    }

    #[test]
    fn the_help_names_every_variable() {
        for name in ["FT_SUPPORT_DATA", "FT_SUPPORT_KEY_FILE", "FT_SUPPORT_DB", "FT_SUPPORT_IDENTITY_FILE", "FT_SUPPORT_CARD_FILE", "FT_SUPPORT_FILES", "FT_SUPPORT_ROUTER"] {
            assert!(USAGE.contains(name), "{name}");
        }
    }
}
