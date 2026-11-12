//! A call answered from CallKit on a locked iPhone has no WebView to carry its audio.

pub mod platform;
pub mod session;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
pub mod video;
pub mod views;
pub mod voice;

pub use platform::{device_stats, platform_audio, platform_video, DeviceStats};
pub use session::{Gathering, LinkState, MediaSession};
pub use video::{Facing, Video, VideoDevices, VideoError, VideoFactory, VideoPlatform, VideoState, VideoStats};
pub use views::{Layers, RemoteShape, ViewSlot};
pub use voice::{Activation, AudioPlatform, BackendFactory, Voice};

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use webrtc::peer_connection::{RTCIceServer, RTCIceTransportPolicy};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CallRouting {
    Direct,
    #[default]
    Auto,
    // the contact never learns our address
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnRelay {
    pub urls: Vec<String>,
    pub username: String,
    pub credential: String,
}

#[derive(Debug, Clone)]
pub struct MediaConfig {
    pub stun: Vec<String>,
    pub turn: Option<TurnRelay>,
    pub bind: Vec<String>,
    pub routing: CallRouting,
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
