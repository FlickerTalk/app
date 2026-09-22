//! PoC 0 only (Plan §87): connects a `ft-webrtc` session to the other peer through the signalling
//! relay of `server/ft-router`. In the real design signals travel encrypted through push (§13),
//! through `ft-push`; this crate goes away then.

use std::time::Duration;

use anyhow::{Context, Result};
use ft_webrtc::{Inbox, Role, Session, SessionConfig, Signal, TurnServer};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

/// The relay sends its welcome right away; an older relay sends none.
const WELCOME_WAIT: Duration = Duration::from_secs(2);

/// A temporary TURN user issued by the relay for this session (Plan §17).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnGrant {
    pub urls: Vec<String>,
    pub username: String,
    pub credential: String,
}

/// What travels through the relay. The relay itself only sends the presence events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Envelope {
    /// First message from the relay: the ICE servers to use.
    Welcome {
        #[serde(default)]
        stun: Vec<String>,
        #[serde(default)]
        turn: Option<TurnGrant>,
    },
    PeerPresent,
    PeerJoined,
    PeerLeft,
    Signal { signal: Signal },
}

/// The relay's STUN replaces the configured one, which stays only as a fallback (Plan §16), and its
/// TURN user is added.
pub fn with_relay_servers(config: SessionConfig, stun: Vec<String>, turn: Option<TurnGrant>) -> SessionConfig {
    let mut config = config;
    if !stun.is_empty() {
        config.stun_servers = stun;
    }
    if let Some(grant) = turn {
        config.turn_servers.extend(grant.urls.into_iter().map(|url| TurnServer {
            url,
            username: grant.username.clone(),
            credential: grant.credential.clone(),
        }));
    }
    config
}

fn parse(message: Message) -> Option<Envelope> {
    let Message::Text(text) = message else { return None };
    serde_json::from_str(text.as_str()).ok()
}

/// Joins `room` on the relay and starts a session with the ICE servers the relay offers. The
/// caller sends its offer as soon as the other peer is in the room, whichever arrived first.
pub async fn connect(relay: &str, room: &str, role: Role, config: SessionConfig) -> Result<(Session, Inbox)> {
    let url = format!("{}/poc/rooms/{room}", relay.trim_end_matches('/'));
    let (socket, _) = connect_async(url.as_str()).await.with_context(|| format!("cannot reach the relay at {url}"))?;
    let (mut sink, mut source) = socket.split();

    let first = match timeout(WELCOME_WAIT, source.next()).await {
        Ok(Some(Ok(message))) => parse(message),
        _ => None,
    };
    let (config, early) = match first {
        Some(Envelope::Welcome { stun, turn }) => (with_relay_servers(config, stun, turn), None),
        other => (config, other),
    };

    let (signals, mut outgoing) = mpsc::channel(32);
    let (session, inbox) = Session::start(config, role, signals).await?;

    tokio::spawn(async move {
        while let Some(signal) = outgoing.recv().await {
            let Ok(text) = serde_json::to_string(&Envelope::Signal { signal }) else { continue };
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });

    let peer = session.clone();
    tokio::spawn(async move {
        let later = source.filter_map(|message| async move { message.ok().and_then(parse) });
        let mut envelopes = Box::pin(futures_util::stream::iter(early).chain(later));
        while let Some(envelope) = envelopes.next().await {
            // Errors are dropped: in the PoC a failed step simply shows up as no connection.
            let _ = match envelope {
                Envelope::PeerPresent | Envelope::PeerJoined if role == Role::Caller => peer.invite().await,
                Envelope::Signal { signal } => peer.handle_signal(signal).await,
                _ => Ok(()),
            };
        }
    });

    Ok((session, inbox))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_presence_events_the_relay_sends() {
        let parsed: Envelope = serde_json::from_str(r#"{"kind":"peer_present"}"#).expect("parses");
        assert_eq!(parsed, Envelope::PeerPresent);
        let parsed: Envelope = serde_json::from_str(r#"{"kind":"peer_joined"}"#).expect("parses");
        assert_eq!(parsed, Envelope::PeerJoined);
    }

    #[test]
    fn reads_the_welcome_with_the_ice_servers() {
        let text = r#"{"kind":"welcome","stun":["stun:ours:3478"],
            "turn":{"urls":["turn:ours:3478"],"username":"1700000600:ab","credential":"c2VjcmV0"}}"#;
        let parsed: Envelope = serde_json::from_str(text).expect("parses");
        assert_eq!(
            parsed,
            Envelope::Welcome {
                stun: vec!["stun:ours:3478".to_owned()],
                turn: Some(TurnGrant {
                    urls: vec!["turn:ours:3478".to_owned()],
                    username: "1700000600:ab".to_owned(),
                    credential: "c2VjcmV0".to_owned(),
                }),
            }
        );
    }

    #[test]
    fn reads_a_welcome_without_turn() {
        let parsed: Envelope = serde_json::from_str(r#"{"kind":"welcome","stun":[],"turn":null}"#).expect("parses");
        assert_eq!(parsed, Envelope::Welcome { stun: vec![], turn: None });
    }

    // Plan §16: our STUN first; Google's only when ours is not offered.
    #[test]
    fn the_relays_stun_replaces_the_fallback_and_its_turn_is_added() {
        let fallback = SessionConfig { relay_only: true, ..SessionConfig::with_stun(["stun:google:19302".to_owned()]) };
        let turn = TurnGrant {
            urls: vec!["turn:ours:3478".to_owned()],
            username: "1700000600:ab".to_owned(),
            credential: "c2VjcmV0".to_owned(),
        };

        let config = with_relay_servers(fallback, vec!["stun:ours:3478".to_owned()], Some(turn));

        assert_eq!(config.stun_servers, ["stun:ours:3478"]);
        assert_eq!(
            config.turn_servers,
            [TurnServer {
                url: "turn:ours:3478".to_owned(),
                username: "1700000600:ab".to_owned(),
                credential: "c2VjcmV0".to_owned(),
            }]
        );
        assert!(config.relay_only);
    }

    #[test]
    fn without_our_stun_the_fallback_stays() {
        let fallback = SessionConfig::with_stun(["stun:google:19302".to_owned()]);
        let config = with_relay_servers(fallback, vec![], None);
        assert_eq!(config.stun_servers, ["stun:google:19302"]);
        assert!(config.turn_servers.is_empty());
    }

    // The cluster's relay is wss://api.flickertalk.com (Plan §75).
    #[tokio::test]
    async fn secure_relays_are_supported() {
        // Accepts the TCP connection and hangs up, so the failure comes from TLS itself.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("binds");
        let address = listener.local_addr().expect("has an address");
        tokio::spawn(async move { while listener.accept().await.is_ok() {} });

        let relay = format!("wss://{address}");
        let Err(error) = connect(&relay, "r", Role::Callee, SessionConfig::offline()).await else {
            panic!("there is no relay there");
        };
        assert!(!format!("{error:#}").contains("not compiled"), "{error:#}");
    }

    #[test]
    fn wraps_signals_so_they_cross_the_relay_intact() {
        let envelope = Envelope::Signal { signal: Signal::Sdp("{\"type\":\"offer\"}".to_owned()) };
        let text = serde_json::to_string(&envelope).expect("serialises");
        assert_eq!(serde_json::from_str::<Envelope>(&text).expect("parses back"), envelope);
    }
}
