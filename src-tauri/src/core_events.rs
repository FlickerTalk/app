//! The core's events on their way to the UI (2026-10-01): one step of the loop that forwards them.

use std::future::Future;

use tokio::sync::broadcast;

/// What the loop that forwards the core's events gets next.
#[derive(Debug, PartialEq, Eq)]
pub enum Next<T> {
    /// An event to forward.
    Event(T),
    /// The loop fell behind and the channel dropped some events: what the UI shows is fetched
    /// again, and the loop goes on.
    Lost,
    /// The core stopped (erasing the phone) or its channel closed: the loop ends.
    End,
}

/// What `ft://changed` carries after a loss: no contact, and `all`, so the UI reloads its lists
/// and every conversation and circle it has open.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EverythingChanged {
    contact: Option<String>,
    all: bool,
}

impl Default for EverythingChanged {
    fn default() -> Self {
        Self { contact: None, all: true }
    }
}

/// Waits for the next event, or for `stopped`.
pub async fn next<T: Clone>(events: &mut broadcast::Receiver<T>, stopped: impl Future) -> Next<T> {
    match tokio::select! {
        event = events.recv() => Some(event),
        _ = stopped => None,
    } {
        Some(Ok(event)) => Next::Event(event),
        // A burst (a full mailbox, a big sync) outran the loop: it must not go deaf for good.
        Some(Err(broadcast::error::RecvError::Lagged(_))) => Next::Lost,
        Some(Err(broadcast::error::RecvError::Closed)) | None => Next::End,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::{pending, ready};

    #[test]
    fn events_after_a_burst_still_arrive_after_the_loss_is_told() {
        tauri::async_runtime::block_on(async {
            let (sender, mut events) = broadcast::channel(1);
            sender.send(1).unwrap();
            sender.send(2).unwrap();
            assert_eq!(next(&mut events, pending::<()>()).await, Next::Lost);
            assert_eq!(next(&mut events, pending::<()>()).await, Next::Event(2));
        });
    }

    #[test]
    fn a_loss_tells_the_ui_to_reload_everything_it_shows() {
        let payload = serde_json::to_value(EverythingChanged::default()).unwrap();
        assert_eq!(payload, serde_json::json!({ "contact": null, "all": true }));
    }

    #[test]
    fn a_closed_channel_ends_the_loop() {
        tauri::async_runtime::block_on(async {
            let (sender, mut events) = broadcast::channel::<u32>(1);
            drop(sender);
            assert_eq!(next(&mut events, pending::<()>()).await, Next::End);
        });
    }

    #[test]
    fn a_stopped_core_ends_the_loop() {
        tauri::async_runtime::block_on(async {
            let (_sender, mut events) = broadcast::channel::<u32>(1);
            assert_eq!(next(&mut events, ready(())).await, Next::End);
        });
    }
}
