//! Two complete devices against the live router (api.flickertalk.com), with WebRTC over the real
//! network and the cluster's STUN and TURN. Ignored by default: `cargo test -p ft-core -- --ignored`.

use std::time::Duration;

use ft_core::online::Online;
use ft_storage::{MessageState, Store};
use ft_webrtc::SessionConfig;

const ROUTER: &str = "https://api.flickertalk.com";

async fn phone(name: &str) -> Online {
    let online = ft_core::online::start(Store::open_in_memory().await.unwrap(), rand_key(), ROUTER, SessionConfig::default())
        .await
        .expect("starts");
    online.core.set_name(name).await.unwrap();
    online
}

fn rand_key() -> [u8; 32] {
    *blake3::hash(format!("{:?}", std::time::SystemTime::now()).as_bytes()).as_bytes()
}

async fn until<F, Fut>(what: &str, condition: F)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..600 {
        if condition().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timed out waiting until {what}");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs api.flickertalk.com and the internet"]
async fn two_phones_chat_through_the_live_cluster() {
    let (alice, bob) = (phone("Alice").await, phone("Bob").await);
    tokio::time::sleep(Duration::from_secs(1)).await;
    let (a, b) = (alice.core.device_id().to_string(), bob.core.device_id().to_string());

    let link = bob.core.my_card().await.unwrap().to_link();
    alice.core.add_contact(&link, None).await.expect("pairs");
    until("bob knows alice", || async { bob.core.store().contact(&a).await.unwrap().is_some() }).await;

    let sent = alice.core.send_text(&b, "hello through the cluster").await.expect("sends");
    until("bob has it", || async { bob.core.store().messages(&a, 10).await.unwrap().len() == 1 }).await;
    until("alice sees it delivered", || async {
        alice.core.store().message(&sent).await.unwrap().unwrap().state == MessageState::Delivered
    })
    .await;

    alice.router.forget().await.ok();
    bob.router.forget().await.ok();
}
