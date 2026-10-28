//! The contact the app carries (2026-10-28): the support peer's Contact Card
//! goes inside the app, and a phone whose identity was just made has it in its list from the first
//! start, like any contact. It is kept on the phone only: nothing goes out, not even our card,
//! until the user writes to it (§100). Writing to it is the usual first contact.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use ft_core::{Core, Peer, Transport};
use ft_identity::Identity;
use ft_storage::Store;
use tokio::sync::mpsc;

const KEY: [u8; 32] = [9; 32];

/// The network between the test phones: it writes down every way out a phone takes (a packet, a
/// connection opened, a mailbox deposit) and carries direct packets in order.
#[derive(Default)]
struct Net {
    out: Mutex<Vec<String>>,
    queues: Mutex<HashMap<String, mpsc::UnboundedSender<Vec<u8>>>>,
}

impl Net {
    /// How many times anything went, or tried to go, to the device.
    fn towards(&self, device: &str) -> usize {
        self.out.lock().unwrap().iter().filter(|to| *to == device).count()
    }

    fn anything(&self) -> usize {
        self.out.lock().unwrap().len()
    }
}

struct Link {
    net: Arc<Net>,
}

#[async_trait]
impl Transport for Link {
    async fn send_direct(&self, to: &Peer, bytes: Vec<u8>) -> anyhow::Result<bool> {
        self.net.out.lock().unwrap().push(to.device_id.clone());
        let queue = self.net.queues.lock().unwrap().get(&to.device_id).cloned();
        Ok(queue.is_some_and(|queue| queue.send(bytes).is_ok()))
    }

    async fn reach(&self, to: &Peer, _fallback: bool) -> anyhow::Result<bool> {
        self.net.out.lock().unwrap().push(to.device_id.clone());
        Ok(true)
    }

    async fn open_direct_call(&self, to: &Peer) -> anyhow::Result<bool> {
        self.net.out.lock().unwrap().push(to.device_id.clone());
        Ok(false)
    }

    async fn send_mailbox(&self, to: &Peer, _bytes: Vec<u8>) -> anyhow::Result<()> {
        self.net.out.lock().unwrap().push(to.device_id.clone());
        Ok(())
    }
}

async fn phone(net: &Arc<Net>, name: &str) -> Arc<Core> {
    phone_on(net, name, Store::open_in_memory().await.expect("store")).await
}

async fn phone_on(net: &Arc<Net>, name: &str, store: Store) -> Arc<Core> {
    let core = Core::open(store, KEY, Arc::new(Link { net: net.clone() })).await.expect("opens");
    core.set_name(name).await.expect("names");
    let (queue, mut incoming) = mpsc::unbounded_channel::<Vec<u8>>();
    net.queues.lock().unwrap().insert(id(&core), queue);
    let receiver = Arc::downgrade(&core);
    tokio::spawn(async move {
        while let Some(bytes) = incoming.recv().await {
            let Some(core) = receiver.upgrade() else { break };
            let _ = core.receive(&bytes).await;
        }
    });
    core
}

fn id(core: &Core) -> String {
    core.device_id().as_str().to_owned()
}

async fn link_of(core: &Core) -> String {
    core.my_card().await.expect("card").to_link()
}

async fn texts(core: &Core, contact: &str) -> Vec<String> {
    core.store().messages(contact, 100).await.expect("lists").into_iter().map(|m| m.body).collect()
}

/// Polls until the condition holds, for at most five seconds.
async fn until<F, Fut>(what: &str, condition: F)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..250 {
        if condition().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out waiting until {what}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_phone_has_the_bundled_contact_and_tells_nobody() {
    let net = Arc::new(Net::default());
    let (support, ann) = (phone(&net, "Support").await, phone(&net, "Ann").await);

    let added = ann.add_bundled_contact(&link_of(&support).await, Some("FlickerTalk".into())).await.expect("adds");

    let contact = added.expect("the contact the app carries is added");
    assert_eq!(contact.device_id, id(&support));
    assert_eq!(contact.name, "FlickerTalk");
    assert!(contact.accepted, "it is in the list, not in the requests");
    assert!(!contact.introduced);
    let conversations = ann.store().conversations().await.expect("lists");
    assert_eq!(conversations.len(), 1, "it shows in the chats like any contact");
    assert_eq!(conversations[0].contact.device_id, id(&support));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(net.anything(), 0, "nothing went out");
    assert!(support.store().contact(&id(&ann)).await.unwrap().is_none());
}

// The card the app carries may be missing (a build without one) or broken: the phone simply has
// no such contact, and the first start goes on as always.
#[tokio::test(flavor = "multi_thread")]
async fn an_empty_or_broken_card_adds_nothing() {
    let net = Arc::new(Net::default());
    let (support, ann) = (phone(&net, "Support").await, phone(&net, "Ann").await);
    let good = link_of(&support).await;
    let mut tampered = good.clone();
    tampered.truncate(good.len() - 4);
    tampered.push_str("AAAA");

    for link in ["", "  \n", "not a link", "https://flickertalk.com/add#AAAA", &tampered, &link_of(&ann).await] {
        assert!(ann.add_bundled_contact(link, Some("FlickerTalk".into())).await.expect("no error").is_none(), "{link:?}");
    }
    assert!(ann.store().contacts().await.unwrap().is_empty());
    assert_eq!(net.anything(), 0);
    // The phone is still new: a card that reads is added.
    assert!(ann.add_bundled_contact(&good, Some("FlickerTalk".into())).await.unwrap().is_some());
}

// An install from before this version already has its contacts: it does not get one more.
#[tokio::test(flavor = "multi_thread")]
async fn an_install_from_before_gets_no_bundled_contact() {
    let net = Arc::new(Net::default());
    let support = phone(&net, "Support").await;
    let store = Store::open_in_memory().await.expect("store");
    store.save_identity(&Identity::generate().seal(&KEY), &[7; 32]).await.expect("an identity made by an older app");
    let ann = phone_on(&net, "Ann", store).await;

    assert!(ann.add_bundled_contact(&link_of(&support).await, Some("FlickerTalk".into())).await.unwrap().is_none());
    assert!(ann.store().contacts().await.unwrap().is_empty());
}

// It is added once, on the first start. Removed, it stays removed, also after the app starts
// again (or the database moves to a new phone, which is the same database).
#[tokio::test(flavor = "multi_thread")]
async fn the_bundled_contact_is_added_once_and_a_removal_holds() {
    let net = Arc::new(Net::default());
    let support = phone(&net, "Support").await;
    let store = Store::open_in_memory().await.expect("store");
    let ann = phone_on(&net, "Ann", store.clone()).await;
    let link = link_of(&support).await;

    assert!(ann.add_bundled_contact(&link, Some("FlickerTalk".into())).await.unwrap().is_some());
    ann.rename_contact(&id(&support), "Help").await.expect("renames");
    assert!(ann.add_bundled_contact(&link, Some("FlickerTalk".into())).await.unwrap().is_none());
    assert_eq!(ann.store().contact(&id(&support)).await.unwrap().expect("still there").name, "Help");

    ann.remove_contact(&id(&support)).await.expect("removes");
    assert!(ann.add_bundled_contact(&link, Some("FlickerTalk".into())).await.unwrap().is_none());
    drop(ann);
    let again = phone_on(&net, "Ann", store).await;
    assert!(again.add_bundled_contact(&link, Some("FlickerTalk".into())).await.unwrap().is_none());
    assert!(again.store().contacts().await.unwrap().is_empty());
    assert_eq!(net.anything(), 0);
}

// What this phone tells every contact (a new mailbox preference, a new link, the card after a
// restore) does not reach the contact the app carries before the user writes to it. Writing is
// the usual first contact: our card goes first, and the support peer can answer.
#[tokio::test(flavor = "multi_thread")]
async fn nothing_reaches_the_bundled_contact_until_the_user_writes() {
    let net = Arc::new(Net::default());
    let (support, ann, bob) = (phone(&net, "Support").await, phone(&net, "Ann").await, phone(&net, "Bob").await);
    ann.add_bundled_contact(&link_of(&support).await, Some("FlickerTalk".into())).await.unwrap().expect("added");
    ann.add_contact(&link_of(&bob).await, None).await.expect("ann scans bob");
    let to_bob = net.towards(&id(&bob));

    ann.set_mailbox(false).await.expect("mailbox off");
    ann.renew_link(None).await.expect("a new link");
    ann.reintroduce().await.expect("the card again");
    assert!(net.towards(&id(&bob)) > to_bob, "a contact the user added hears of it");
    assert_eq!(net.towards(&id(&support)), 0, "the contact the app carries hears nothing");

    ann.send_text(&id(&support), "Hello?").await.expect("writes");
    until("the support peer has the text", || async { texts(&support, &id(&ann)).await == ["Hello?"] }).await;
    support.accept_contact(&id(&ann)).await.expect("the support peer takes it");
    support.send_text(&id(&ann), "How can I help?").await.expect("answers");
    until("ann has the answer", || async { texts(&ann, &id(&support)).await == ["Hello?", "How can I help?"] }).await;
    assert!(ann.store().contact(&id(&support)).await.unwrap().expect("there").introduced);
}
