//! PoC 0 (Plan §87): lets the UI connect to another peer through the signalling relay and exchange
//! messages over WebRTC. Temporary: the real app connects through push (ft-push) and ft-core.

use ft_webrtc::{Role, Session, SessionConfig};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex;

pub const STATE_EVENT: &str = "poc://state";
pub const MESSAGE_EVENT: &str = "poc://message";

/// Allowed during the PoC (Plan §16).
const GOOGLE_STUN: &str = "stun:stun.l.google.com:19302";

/// The session of the PoC screen, if connected.
#[derive(Default)]
pub struct Poc(Mutex<Option<Session>>);

/// The cluster's relay behind the load balancer (Plan §75). The PoC screen lets a local one be
/// typed in (the Android emulator reaches the developer's Mac at 10.0.2.2).
pub fn default_relay() -> &'static str {
    "wss://api.flickertalk.com"
}

/// Google's STUN as the fallback; the relay's welcome adds ours and a temporary TURN user (ft-poc).
/// `relay_only` forces every packet through TURN.
fn session_config(relay_only: bool) -> SessionConfig {
    SessionConfig { relay_only, ..SessionConfig::with_stun([GOOGLE_STUN.to_owned()]) }
}

#[tauri::command]
pub fn poc_default_relay() -> String {
    default_relay().to_owned()
}

/// Joins the room with the given role. Progress and incoming messages arrive as events.
#[tauri::command]
pub async fn poc_connect(
    relay: String,
    room: String,
    role: Role,
    relay_only: bool,
    app: AppHandle,
    poc: State<'_, Poc>,
) -> Result<(), String> {
    let (session, mut inbox) = ft_poc::connect(&relay, &room, role, session_config(relay_only))
        .await
        .map_err(|error| error.to_string())?;
    *poc.0.lock().await = Some(session.clone());
    let _ = app.emit(STATE_EVENT, "waiting");

    tauri::async_runtime::spawn(async move {
        let state = if session.wait_open().await.is_ok() { "open" } else { "closed" };
        let _ = app.emit(STATE_EVENT, state);
        while let Some(message) = inbox.next_text().await {
            let _ = app.emit(MESSAGE_EVENT, message);
        }
    });
    Ok(())
}

#[tauri::command]
pub async fn poc_send(text: String, poc: State<'_, Poc>) -> Result<(), String> {
    let session = poc.0.lock().await.clone().ok_or_else(|| "not connected".to_owned())?;
    session.send(&text).await.map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // The cluster's relay behind the load balancer (Plan §75); a local one can still be typed in.
    #[test]
    fn every_build_uses_the_cluster_relay_by_default() {
        assert_eq!(default_relay(), "wss://api.flickertalk.com");
    }

    // The relay hands out our STUN and a temporary TURN user (ft-poc); Google's STUN is only the
    // fallback for a relay that offers none (Plan §16).
    #[test]
    fn the_relay_provides_turn_and_google_stun_is_only_the_fallback() {
        let config = session_config(false);
        assert_eq!(config.stun_servers, [GOOGLE_STUN]);
        assert!(config.turn_servers.is_empty());
        assert!(!config.relay_only);
    }

    #[test]
    fn always_relay_reaches_the_session() {
        assert!(session_config(true).relay_only);
    }

    #[test]
    fn events_are_namespaced_for_the_poc() {
        assert!(STATE_EVENT.starts_with("poc://"));
        assert!(MESSAGE_EVENT.starts_with("poc://"));
    }
}
