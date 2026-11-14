#![cfg(debug_assertions)]

use std::sync::Arc;

use async_trait::async_trait;
use ft_billing::Access;
use ft_core::{Core, Peer, Transport};
use ft_storage::Store;

const SWITCH: &str = "FT_DEBUG_TRIAL_OVER";

struct Offline;

#[async_trait]
impl Transport for Offline {
    async fn send_direct(&self, _to: &Peer, _bytes: Vec<u8>) -> anyhow::Result<bool> {
        Ok(false)
    }

    async fn send_mailbox(&self, _to: &Peer, _bytes: Vec<u8>) -> anyhow::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn the_debug_switch_ends_the_free_days_without_touching_the_database() {
    let core = Core::open(Store::open_in_memory().await.expect("store"), [7; 32], Arc::new(Offline))
        .await
        .expect("opens");
    let installed_at = core.store().setting("installed_at").await.expect("reads");

    std::env::remove_var(SWITCH);
    assert!(matches!(core.access().await.expect("reads"), Access::Trial { .. }), "without the switch, the days are free");

    std::env::set_var(SWITCH, "0");
    assert!(matches!(core.access().await.expect("reads"), Access::Trial { .. }), "only `1` turns it on");

    std::env::set_var(SWITCH, "1");
    assert_eq!(core.access().await.expect("reads"), Access::Limited, "without a subscription the phone is limited");
    assert_eq!(core.store().setting("installed_at").await.expect("reads"), installed_at, "nothing is written");

    let paid_until = ft_core::now() + 30 * 24 * 60 * 60 * 1000;
    core.set_entitlement(paid_until).await.expect("sets");
    assert_eq!(core.access().await.expect("reads"), Access::Subscribed { until: paid_until });

    std::env::remove_var(SWITCH);
    assert_eq!(core.access().await.expect("reads"), Access::Subscribed { until: paid_until }, "a paid date wins over the free days");

    core.set_entitlement(0).await.expect("clears");
    assert!(matches!(core.access().await.expect("reads"), Access::Trial { .. }), "back to the free days");
}
