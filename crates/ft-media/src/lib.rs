//! Native call media (2026-09-28): the voice of a call runs in Rust, not in the WebView.
//!
//! On iOS a call answered from CallKit on a locked iPhone has no WebView to carry its audio, so a
//! voice call's peer connection and its audio pipeline (Opus, RTP, jitter buffer, the device)
//! live here, on top of `webrtc-engine`. The core decides when; this crate knows how.
//!
//! The descriptions stay standard SDP, so a native phone talks to a WebView phone (an older app).

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use webrtc::peer_connection::{RTCIceServer, RTCIceTransportPolicy};

/// How a call may reach the other phone (Plan §17), as chosen in Settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CallRouting {
    /// Never through the relay: a call without a direct path fails.
    Direct,
    /// The relay only when no direct path works.
    #[default]
    Auto,
    /// Always through the relay: the contact never learns this phone's address.
    Always,
}

impl CallRouting {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Auto => "auto",
            Self::Always => "always",
        }
    }
}

impl FromStr for CallRouting {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "direct" => Ok(Self::Direct),
            "auto" => Ok(Self::Auto),
            "always" => Ok(Self::Always),
            _ => Err(anyhow::anyhow!("unknown call routing")),
        }
    }
}

impl fmt::Display for CallRouting {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A TURN relay with its short-lived user (Plan §17).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnRelay {
    pub urls: Vec<String>,
    pub username: String,
    pub credential: String,
}

/// Where a call's connection listens and which servers help it: the router's STUN and TURN.
#[derive(Debug, Clone)]
pub struct MediaConfig {
    pub stun: Vec<String>,
    pub turn: Option<TurnRelay>,
    /// Local UDP addresses to listen on, one per network interface.
    pub bind: Vec<String>,
    pub routing: CallRouting,
    /// How long to wait for ICE gathering before sending what was gathered.
    pub gather_timeout: Duration,
}

impl Default for MediaConfig {
    fn default() -> Self {
        Self {
            stun: Vec::new(),
            turn: None,
            bind: vec!["127.0.0.1:0".to_owned()],
            routing: CallRouting::Auto,
            gather_timeout: Duration::from_secs(3),
        }
    }
}

/// The ICE servers and policy of a call, as its routing allows.
pub fn ice_setup(config: &MediaConfig) -> (Vec<RTCIceServer>, RTCIceTransportPolicy) {
    let stun = (!config.stun.is_empty() && config.routing != CallRouting::Always)
        .then(|| RTCIceServer { urls: config.stun.clone(), ..Default::default() });
    let turn = config.turn.as_ref().filter(|_| config.routing != CallRouting::Direct).map(|relay| RTCIceServer {
        urls: relay.urls.clone(),
        username: relay.username.clone(),
        credential: relay.credential.clone(),
    });
    let policy = match config.routing {
        CallRouting::Always => RTCIceTransportPolicy::Relay,
        CallRouting::Direct | CallRouting::Auto => RTCIceTransportPolicy::All,
    };
    (stun.into_iter().chain(turn).collect(), policy)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(routing: CallRouting) -> MediaConfig {
        MediaConfig {
            stun: vec!["stun:s:3478".to_owned()],
            turn: Some(TurnRelay { urls: vec!["turn:t:3478".to_owned()], username: "u".to_owned(), credential: "c".to_owned() }),
            routing,
            ..MediaConfig::default()
        }
    }

    fn urls(servers: &[RTCIceServer]) -> Vec<String> {
        servers.iter().flat_map(|server| server.urls.clone()).collect()
    }

    // §17, the same three choices as the WebView's calls.
    #[test]
    fn the_routing_decides_whether_the_relay_is_used() {
        let (servers, policy) = ice_setup(&config(CallRouting::Auto));
        assert_eq!(urls(&servers), ["stun:s:3478", "turn:t:3478"]);
        assert_eq!(servers[1].username, "u");
        assert_eq!(servers[1].credential, "c");
        assert_eq!(policy, RTCIceTransportPolicy::All);

        let (servers, policy) = ice_setup(&config(CallRouting::Direct));
        assert_eq!(urls(&servers), ["stun:s:3478"]);
        assert_eq!(policy, RTCIceTransportPolicy::All);

        let (servers, policy) = ice_setup(&config(CallRouting::Always));
        assert_eq!(urls(&servers), ["turn:t:3478"]);
        assert_eq!(policy, RTCIceTransportPolicy::Relay);
    }

    #[test]
    fn routings_read_as_the_settings_write_them() {
        for routing in [CallRouting::Direct, CallRouting::Auto, CallRouting::Always] {
            assert_eq!(routing.as_str().parse::<CallRouting>().unwrap(), routing);
        }
        assert!("sometimes".parse::<CallRouting>().is_err());
        assert_eq!(CallRouting::default(), CallRouting::Auto);
    }
}
