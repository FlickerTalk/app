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

/// M4 by hand: writes to a real phone whose Contact Card link is in `FT_LIVE_CARD`, and waits for
/// its receipt. With the app closed there, the router wakes it (FCM notification); opening the
/// app delivers. `FT_LIVE_CARD=… cargo test -p ft-core --test live real_phone -- --ignored --nocapture`
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a real phone, api.flickertalk.com and the internet"]
async fn writes_to_a_real_phone() {
    let link = std::env::var("FT_LIVE_CARD").expect("FT_LIVE_CARD with the phone's card link");
    let mac = phone("Mac").await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let contact = mac.core.add_contact(&link, None).await.expect("adds the phone");
    let sent = mac.core.send_text(&contact.device_id, "hello from the Mac: wake up!").await.expect("sends");
    println!("sent; waiting for the phone's receipt (open the app there when the notification shows)");
    for _ in 0..1800 {
        let state = mac.core.store().message(&sent).await.unwrap().unwrap().state;
        if state >= MessageState::Delivered {
            println!("delivered");
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("no receipt within three minutes");
}
