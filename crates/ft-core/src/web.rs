//! The little bit of web the core does on someone else's behalf (Plan §55–§56): the catalogue of
//! plugins, and the calls a plugin is allowed to make. It is a trait so the tests never touch the
//! network, and so the rule —what may be reached— lives in the core, not in the frame.

use anyhow::{ensure, Context, Result};
use async_trait::async_trait;

/// What a plugin asks the core to send for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebRequest {
    pub url: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

/// What came back. The core never reads it: it hands it to the plugin as it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebAnswer {
    pub status: u16,
    pub body: Vec<u8>,
}

#[async_trait]
pub trait Fetch: Send + Sync {
    /// Downloads a file, refusing anything longer than `limit`.
    async fn get(&self, url: &str, limit: u64) -> Result<Vec<u8>>;
    /// Sends one request and brings the answer back, refusing anything longer than `limit`.
    async fn call(&self, request: &WebRequest, limit: u64) -> Result<WebAnswer>;
}

/// The host of an `https://` address, or an error. Anything with a user in it, or any other
/// scheme, is refused: a plugin may only be granted a host, so only a host may be checked.
pub fn host_of(url: &str) -> Result<String> {
    let rest = url.strip_prefix("https://").context("a plugin may only reach https addresses")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    ensure!(!authority.is_empty(), "that address has no host");
    ensure!(!authority.contains('@'), "that address hides its host behind a user");
    let host = authority.split(':').next().unwrap_or_default().to_ascii_lowercase();
    ensure!(!host.is_empty(), "that address has no host");
    Ok(host)
}

/// The real one, used by the app. It follows nothing, keeps nothing and sends no cookies.
pub struct Web {
    http: reqwest::Client,
}

impl Default for Web {
    fn default() -> Self {
        Self::new()
    }
}

/// How long the core waits for the catalogue, or for a call it makes for a plugin.
const PATIENCE: std::time::Duration = std::time::Duration::from_secs(30);

impl Web {
    /// It follows nothing and carries nothing of its own: no redirects, no cookies, and its own
    /// cryptography, which a phone does not install by itself.
    pub fn new() -> Self {
        Self { http: ft_push::https_client(PATIENCE).expect("the HTTPS client is built the same way as the router's") }
    }

    async fn body_within(answer: reqwest::Response, limit: u64) -> Result<Vec<u8>> {
        if let Some(length) = answer.content_length() {
            ensure!(length <= limit, "the answer is too big");
        }
        let bytes = answer.bytes().await?;
        ensure!(bytes.len() as u64 <= limit, "the answer is too big");
        Ok(bytes.to_vec())
    }
}

#[async_trait]
impl Fetch for Web {
    async fn get(&self, url: &str, limit: u64) -> Result<Vec<u8>> {
        let answer = self.http.get(url).send().await?;
        ensure!(answer.status().is_success(), "{url} answered {}", answer.status());
        Self::body_within(answer, limit).await
    }

    async fn call(&self, request: &WebRequest, limit: u64) -> Result<WebAnswer> {
        let method = reqwest::Method::from_bytes(request.method.as_bytes()).context("that is not a method")?;
        let mut sending = self.http.request(method, &request.url);
        for (name, value) in &request.headers {
            sending = sending.header(name, value);
        }
        if let Some(body) = &request.body {
            sending = sending.body(body.clone());
        }
        let answer = sending.send().await?;
        let status = answer.status().as_u16();
        Ok(WebAnswer { status, body: Self::body_within(answer, limit).await? })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // On a phone there is no crypto provider installed by default: a client built without one
    // aborts the whole app the first time it is made (seen on a real Android, 2026-09-23).
    #[test]
    fn the_client_brings_its_own_crypto() {
        let _ = Web::new();
    }

    #[test]
    fn only_an_https_host_can_be_checked_against_what_was_granted() {
        assert_eq!(host_of("https://api.openai.com/v1/chat").unwrap(), "api.openai.com");
        assert_eq!(host_of("https://API.OpenAI.com:443/x").unwrap(), "api.openai.com");
        assert_eq!(host_of("https://api.openai.com").unwrap(), "api.openai.com");
        for refused in [
            "http://api.openai.com/x",
            "https://user@evil.example/x",
            "file:///etc/passwd",
            "https:///x",
            "ftplugin://localhost/x",
            "",
        ] {
            assert!(host_of(refused).is_err(), "{refused} should be refused");
        }
    }
}
