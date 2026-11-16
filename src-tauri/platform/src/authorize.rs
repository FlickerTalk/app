use serde::{Deserialize, Serialize};
use tauri::Runtime;

use crate::{Platform, Result};

#[derive(Serialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct Authorize<'a> {
    url: &'a str,
    scheme: &'a str,
}

#[derive(Deserialize)]
#[cfg_attr(not(mobile), allow(dead_code))]
struct Authorized {
    url: String,
}

impl<R: Runtime> Platform<R> {
    /// Custom Tabs / ASWebAuthenticationSession, so the WebView never sees the login page or the
    /// tokens.
    pub fn authorize(&self, url: &str, scheme: &str) -> Result<String> {
        #[cfg(mobile)]
        {
            Ok(self
                .handle
                .run_mobile_plugin::<Authorized>("authorize", Authorize { url, scheme })?
                .url)
        }
        #[cfg(not(mobile))]
        {
            let _ = (url, scheme);
            Err(crate::Error::Unsupported)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kotlin_side_gets_the_login_names_it_expects() {
        let login = serde_json::to_value(Authorize {
            url: "https://accounts.example/auth",
            scheme: "com.flickertalk.app",
        })
        .unwrap();
        assert_eq!(login["scheme"], "com.flickertalk.app");
        let back: Authorized = serde_json::from_value(
            serde_json::json!({ "url": "com.flickertalk.app:/oauth?code=1" }),
        )
        .unwrap();
        assert!(back.url.starts_with("com.flickertalk.app:"));
    }
}
