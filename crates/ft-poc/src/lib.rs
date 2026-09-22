//! PoC 0 only (Plan §87): connects a `ft-webrtc` session to the other peer through the signalling
//! relay of `server/ft-router`. In the real design signals travel encrypted through push (§13),
//! through `ft-push`; this crate goes away then.

use anyhow::{Context, Result};
use ft_webrtc::{Inbox, Role, Session, SessionConfig, Signal};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

/// What travels through the relay. The relay itself only sends the presence events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Envelope {
    PeerPresent,
    PeerJoined,
    PeerLeft,
    Signal { signal: Signal },
}

/// Joins `room` on the relay and starts a session. The caller sends its offer as soon as the other
/// peer is in the room, whichever of the two arrived first.
pub async fn connect(relay: &str, room: &str, role: Role, config: SessionConfig) -> Result<(Session, Inbox)> {
    let url = format!("{}/poc/rooms/{room}", relay.trim_end_matches('/'));
    let (socket, _) = connect_async(url.as_str()).await.with_context(|| format!("cannot reach the relay at {url}"))?;
    let (mut sink, mut source) = socket.split();

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
        while let Some(Ok(message)) = source.next().await {
            let Message::Text(text) = message else { continue };
            let Ok(envelope) = serde_json::from_str::<Envelope>(text.as_str()) else { continue };
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
    fn wraps_signals_so_they_cross_the_relay_intact() {
        let envelope = Envelope::Signal { signal: Signal::Sdp("{\"type\":\"offer\"}".to_owned()) };
        let text = serde_json::to_string(&envelope).expect("serialises");
        assert_eq!(serde_json::from_str::<Envelope>(&text).expect("parses back"), envelope);
    }
}
