//! Starting the core online (Plan §106 M3): the router client signs with the core's identity, the
//! core sends through the network and the network talks through that router client. This module
//! ties the knot, registers the device, listens to the router, retries the outbox and resumes
//! stalled file transfers.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use ft_push::{RouterClient, Signer};
use ft_storage::Store;
use ft_webrtc::SessionConfig;

use crate::net::Network;
use crate::Core;

/// How often the outbox is checked for messages whose next attempt is due (§26).
pub const RETRY_EVERY: Duration = Duration::from_secs(5);

/// The running client: the core, its router client and its network.
pub struct Online {
    pub core: Arc<Core>,
    pub router: Arc<RouterClient>,
    pub network: Arc<Network>,
    /// Set once, by `shutdown`: the background work of this client ends.
    stop: tokio::sync::watch::Sender<bool>,
}

/// The router client is created before the core, so it reaches the identity through this slot.
struct CoreSigner(Arc<OnceLock<Arc<Core>>>);

impl CoreSigner {
    fn core(&self) -> &Core {
        self.0.get().expect("the core is set before the router is used")
    }
}

#[async_trait]
impl Signer for CoreSigner {
    fn device_id(&self) -> String {
        self.core().device_id().to_string()
    }

    async fn signing_key(&self) -> String {
        self.core().signing_key().await
    }

    async fn sign(&self, message: &[u8]) -> String {
        self.core().sign(message).await
    }
}

/// Opens the core over `store` and connects it to the router at `router` (for example
/// `https://api.flickertalk.com`). WebRTC listens as `base` says; STUN and TURN come from the
/// router. Registering is retried in the background if the router cannot be reached now.
pub async fn start(store: Store, key: [u8; 32], router: &str, base: SessionConfig) -> Result<Online> {
    let slot = Arc::new(OnceLock::new());
    let router = Arc::new(RouterClient::new(router, Arc::new(CoreSigner(slot.clone())))?);
    let network = Network::new(router.clone(), base);
    let core = Core::open(store, key, network.clone()).await.context("cannot open the core")?;
    let _ = slot.set(core.clone());
    network.attach(&core);

    // Always eight capabilities: our own and seven for hidden sessions, used or not (app#9).
    let capabilities = core.route_capability_hashes().await?;
    if router.register(&capabilities, 0).await.is_err() {
        let later = router.clone();
        tokio::spawn(async move {
            let mut wait = Duration::from_secs(2);
            // A closed client (the phone was erased) stops trying.
            while !later.is_closed() && later.register(&capabilities, 0).await.is_err() {
                tokio::time::sleep(wait).await;
                wait = (wait * 2).min(Duration::from_secs(60));
            }
        });
    }
    network.listen(router.listen());

    // An upgrade that made the envelope key (A1): every contact gets the card with it, so that
    // what they send through the router from now on names nobody.
    if core.card_stale().await.unwrap_or(false) {
        let stale = core.clone();
        tokio::spawn(async move {
            let _ = stale.reintroduce().await;
        });
    }

    let stop = tokio::sync::watch::Sender::new(false);
    let mut stopping = stop.subscribe();
    let retrying = Arc::downgrade(&core);
    tokio::spawn(async move {
        let mut every = tokio::time::interval(RETRY_EVERY);
        loop {
            tokio::select! {
                _ = every.tick() => {}
                _ = stopping.wait_for(|stopped| *stopped) => break,
            }
            let Some(core) = retrying.upgrade() else { break };
            let _ = core.retry_due().await;
            let _ = core.resume_files().await;
            // Issue app#1: histories that expire and read messages that burn.
            let _ = core.sweep_history().await;
        }
    });

    Ok(Online { core, router, network, stop })
}

impl Online {
    /// Stops this client for good (erasing the phone, 2026-09-30). iOS cannot start the app
    /// again, so the running client stops in place and a new one starts from what is left on
    /// disk: a call going on ends, the router socket and the direct connections close, the
    /// background work ends and the database is closed, so that it can be deleted.
    pub async fn shutdown(&self) {
        if let Ok(Some(call)) = self.core.current_call().await {
            let _ = self.core.end_call(&call.call, false).await;
        }
        self.stop.send_replace(true);
        self.router.close();
        self.network.close().await;
        self.core.store().close().await;
    }

    /// Resolves once the client is stopped: whoever follows its events lets them go.
    pub fn stopped(&self) -> impl std::future::Future<Output = ()> + Send + 'static {
        let mut stop = self.stop.subscribe();
        async move {
            let _ = stop.wait_for(|stopped| *stopped).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn due_messages_are_checked_every_few_seconds() {
        assert!(RETRY_EVERY <= Duration::from_secs(5));
    }

    // Erasing the phone (2026-09-30): iOS cannot start the app again, so the running core stops
    // in place. Nothing of the old identity may reach the router or the database afterwards.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_stopped_client_lets_go_of_the_router_and_the_database() {
        let store = Store::open_in_memory().await.unwrap();
        // Nothing listens there: registering is left retrying in the background.
        let online = start(store, [7; 32], "http://127.0.0.1:9", SessionConfig::default()).await.unwrap();
        assert!(online.core.store().contacts().await.is_ok());

        online.shutdown().await;
        assert!(online.router.is_closed());
        assert!(online.core.store().contacts().await.is_err(), "the database is closed");
        let stopped = tokio::time::timeout(Duration::from_secs(1), online.stopped()).await;
        assert!(stopped.is_ok(), "whoever waits for the stop hears it");
    }
}
