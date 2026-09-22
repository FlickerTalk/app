//! Two complete devices against the live router (api.flickertalk.com), with WebRTC over the real
//! network and the cluster's STUN and TURN. Ignored by default: `cargo test -p ft-core -- --ignored`.

use std::sync::Arc;
use std::time::Duration;

use ft_core::net::Network;
use ft_core::Core;
use ft_push::RouterClient;
use ft_storage::{MessageState, Store};
use ft_webrtc::SessionConfig;

const ROUTER: &str = "https://api.flickertalk.com";

struct Phone {
    core: Arc<Core>,
    router: Arc<RouterClient>,
}

async fn phone(name: &str) -> Phone {
    let slot: Arc<std::sync::OnceLock<Arc<Core>>> = Arc::default();
    let signer = Arc::new(LateSigner(slot.clone()));
    let router = Arc::new(RouterClient::new(ROUTER, signer).expect("client"));
    let network = Network::new(router.clone(), SessionConfig::default());
    let core = Core::open(Store::open_in_memory().await.unwrap(), rand_key(), network.clone()).await.expect("opens");
    core.set_name(name).await.unwrap();
    slot.set(core.clone()).ok();
    network.attach(&core);
    router.register(&core.route_capability().hash()).await.expect("registers");
    network.listen(router.listen());
    Phone { core, router }
}

/// The router client needs the core to sign, and the core needs the network: resolved late.
struct LateSigner(Arc<std::sync::OnceLock<Arc<Core>>>);

#[async_trait::async_trait]
impl ft_push::Signer for LateSigner {
    fn device_id(&self) -> String {
        self.0.get().expect("core").device_id().to_string()
    }
    async fn signing_key(&self) -> String {
        self.0.get().expect("core").signing_key().await
    }
    async fn sign(&self, message: &[u8]) -> String {
        self.0.get().expect("core").sign(message).await
    }
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
