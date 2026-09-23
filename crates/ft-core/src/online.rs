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
    if router.register(&capabilities).await.is_err() {
        let later = router.clone();
        tokio::spawn(async move {
            let mut wait = Duration::from_secs(2);
            while later.register(&capabilities).await.is_err() {
                tokio::time::sleep(wait).await;
                wait = (wait * 2).min(Duration::from_secs(60));
            }
        });
    }
    network.listen(router.listen());

    let retrying = Arc::downgrade(&core);
    tokio::spawn(async move {
        let mut every = tokio::time::interval(RETRY_EVERY);
        loop {
            every.tick().await;
            let Some(core) = retrying.upgrade() else { break };
            let _ = core.retry_due().await;
            let _ = core.resume_files().await;
            // Issue app#1: histories that expire and read messages that burn.
            let _ = core.sweep_history().await;
        }
    });

    Ok(Online { core, router, network })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn due_messages_are_checked_every_few_seconds() {
        assert!(RETRY_EVERY <= Duration::from_secs(5));
    }
}
