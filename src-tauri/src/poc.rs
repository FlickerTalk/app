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

/// The relay runs on the developer's Mac; the Android emulator reaches the host at 10.0.2.2.
pub fn default_relay() -> &'static str {
    if cfg!(target_os = "android") {
        "ws://10.0.2.2:8787"
    } else {
        "ws://127.0.0.1:8787"
    }
}

#[tauri::command]
pub fn poc_default_relay() -> String {
    default_relay().to_owned()
}

/// Joins the room as the caller. Progress and incoming messages arrive as events.
#[tauri::command]
pub async fn poc_connect(relay: String, room: String, app: AppHandle, poc: State<'_, Poc>) -> Result<(), String> {
    let config = SessionConfig::with_stun([GOOGLE_STUN.to_owned()]);
    let (session, mut inbox) =
        ft_poc::connect(&relay, &room, Role::Caller, config).await.map_err(|error| error.to_string())?;
    *poc.0.lock().await = Some(session.clone());
    let _ = app.emit(STATE_EVENT, "waiting");

    tauri::async_runtime::spawn(async move {
        let state = if session.wait_open().await.is_ok() { "open" } else { "closed" };
        let _ = app.emit(STATE_EVENT, state);
        while let Some(message) = inbox.next().await {
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

    #[test]
    fn the_desktop_build_looks_for_the_relay_on_this_machine() {
        assert_eq!(default_relay(), "ws://127.0.0.1:8787");
    }

    #[test]
    fn events_are_namespaced_for_the_poc() {
        assert!(STATE_EVENT.starts_with("poc://"));
        assert!(MESSAGE_EVENT.starts_with("poc://"));
    }
}
