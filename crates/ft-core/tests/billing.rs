//! What the user may do once the free year is over (Plan §40–§47). Everything is decided on the
//! phone: the server never learns who pays, and no date of birth is ever kept.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use ft_billing::{Access, AgeClass};
use ft_core::{Core, Peer, Transport};
use ft_storage::{Message, MessageState, Store};

/// The phone's own clock, in ms, as the core counts it.
fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("after 1970").as_millis() as i64
}

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

async fn core() -> Arc<Core> {
    let core = Core::open(Store::open_in_memory().await.expect("store"), [7; 32], Arc::new(Offline))
        .await
        .expect("opens");
    core.set_files_dir(std::env::temp_dir().join("ft-billing-files"));
    core
}

/// Moves the phone's own clock: the free year started a year and a day ago.
async fn the_year_is_over(core: &Core) {
    let long_ago = now() - (366 * 24 * 60 * 60 * 1000);
    core.store().set_setting("installed_at", &long_ago.to_string()).await.expect("sets");
}

/// Someone to write to, without a network.
async fn a_contact(core: &Core) -> String {
    let other = Core::open(Store::open_in_memory().await.expect("store"), [8; 32], Arc::new(Offline))
        .await
        .expect("opens");
    let link = other.my_card().await.expect("card").to_link();
    core.add_contact(&link, None).await.expect("adds");
    other.device_id().as_str().to_owned()
}

#[tokio::test]
async fn the_first_year_is_free_and_nothing_is_asked() {
    let core = core().await;
    let contact = a_contact(&core).await;
    assert!(matches!(core.access().await.expect("reads"), Access::Trial { .. }));
    core.send_text(&contact, "hello").await.expect("writes to someone new");
    assert_eq!(core.age_class().await.expect("reads"), AgeClass::Unknown, "nobody was asked anything");
}

// Ioan, 2026-09-23: an adult who does not pay keeps receiving and keeps answering. Nobody loses a
// message over the euro; what stops is starting something new.
#[tokio::test]
async fn after_the_year_an_adult_answers_but_starts_nothing() {
    let core = core().await;
    let contact = a_contact(&core).await;
    the_year_is_over(&core).await;
    core.set_age_class(AgeClass::Adult).await.expect("sets");
    assert_eq!(core.access().await.expect("reads"), Access::Limited);

    // Writing to someone we have never written to is starting.
    let refused = core.send_text(&contact, "hello?").await;
    assert!(refused.is_err(), "starting a conversation needs the subscription");

    // A message that arrived is delivered whatever the plan says, and it can be answered.
    arrived(&core, &contact, "are you there?").await;
    core.send_text(&contact, "here").await.expect("answers");

    let (path, _) = a_file();
    assert!(core.send_file(&contact, &path, "a.bin", "application/octet-stream").await.is_err());
}

#[tokio::test]
async fn under_twenty_one_never_pays() {
    let core = core().await;
    let contact = a_contact(&core).await;
    the_year_is_over(&core).await;
    core.set_age_class(AgeClass::Minor).await.expect("sets");

    assert_eq!(core.access().await.expect("reads"), Access::Young);
    core.send_text(&contact, "hello").await.expect("writes to someone new");
}

#[tokio::test]
async fn paying_opens_it_again() {
    let core = core().await;
    let contact = a_contact(&core).await;
    the_year_is_over(&core).await;
    core.set_age_class(AgeClass::Adult).await.expect("sets");
    assert!(core.send_text(&contact, "hello").await.is_err());

    let a_year_from_now = now() + (365 * 24 * 60 * 60 * 1000);
    core.set_entitlement(a_year_from_now).await.expect("the Store said so");
    assert_eq!(core.access().await.expect("reads"), Access::Subscribed { until: a_year_from_now });
    core.send_text(&contact, "hello").await.expect("writes to someone new");
}

/// A message that came in from that contact, as the phone keeps it.
async fn arrived(core: &Core, from: &str, text: &str) {
    let message = Message {
        message_id: ft_protocol::MessageId::new().to_string(),
        contact: from.to_owned(),
        outgoing: false,
        body: text.to_owned(),
        sent_at: now(),
        state: MessageState::Delivered,
    };
    core.store().insert_message(&message).await.expect("keeps it");
}

fn a_file() -> (PathBuf, Vec<u8>) {
    let path = std::env::temp_dir().join(format!("ft-billing-{}.bin", ft_protocol::MessageId::new()));
    let bytes = vec![7u8; 64];
    std::fs::write(&path, &bytes).expect("writes");
    (path, bytes)
}
