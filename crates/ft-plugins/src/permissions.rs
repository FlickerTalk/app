use serde::{Deserialize, Serialize};

// installing grants nothing, each one is asked for and revoked on its own. A plugin never gets
// keys, the push token or the whole conversation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permissions {
    // empty = no network
    #[serde(default)]
    pub network: Vec<String>,
    #[serde(default, rename = "messages", with = "given")]
    pub reads_given_messages: bool,
    #[serde(default)]
    pub send: Sending,
    #[serde(default)]
    pub print: bool,
    // direct connection only, never the mailbox or the server
    #[serde(default)]
    pub live: bool,
    #[serde(default)]
    pub remind: bool,
    #[serde(default)]
    pub drive: bool,
    #[serde(default)]
    pub storage: Storage,
    #[serde(default)]
    pub location: bool,
    #[serde(default)]
    pub translate: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Storage {
    #[default]
    Small,
    Large,
}

impl Storage {
    pub fn quota(self) -> u64 {
        match self {
            Self::Small => 4 * 1024 * 1024,
            Self::Large => 256 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sending {
    #[default]
    Nothing,
    Propose,
    // never granted by default
    Auto,
}

mod given {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(reads: &bool, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(if *reads { "given" } else { "none" })
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
        match String::deserialize(deserializer)?.as_str() {
            "given" => Ok(true),
            "none" => Ok(false),
            other => Err(serde::de::Error::custom(format!(
                "'{other}' is not a messages permission"
            ))),
        }
    }
}

// the frame is sandboxed, so its origin is opaque and 'self' would match nothing
const PLUGIN_ORIGINS: &str =
    "http://ftplugin.localhost https://ftplugin.localhost ftplugin://localhost";

impl Permissions {
    pub fn content_security_policy(&self) -> String {
        let connect = if self.network.is_empty() {
            "'none'".to_owned()
        } else {
            self.network
                .iter()
                .map(|host| format!("https://{host}"))
                .collect::<Vec<_>>()
                .join(" ")
        };
        format!(
            "default-src 'none'; script-src {PLUGIN_ORIGINS}; style-src {PLUGIN_ORIGINS} 'unsafe-inline'; \
             img-src {PLUGIN_ORIGINS} data: blob:; font-src {PLUGIN_ORIGINS}; connect-src {connect}; base-uri 'none'; \
             form-action 'none'; child-src 'none'; frame-ancestors http://tauri.localhost tauri://localhost"
        )
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod policy_shape {
    use super::*;

    #[test]
    fn the_policy_is_one_line_without_double_spaces() {
        let policy = Permissions {
            network: vec!["api.openai.com".to_owned()],
            ..Permissions::default()
        }
        .content_security_policy();
        assert!(!policy.contains('\n') && !policy.contains("  "), "{policy}");
        assert!(
            policy.ends_with("frame-ancestors http://tauri.localhost tauri://localhost"),
            "{policy}"
        );
    }
}
