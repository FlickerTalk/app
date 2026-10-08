//! Presenting in a call (2026-10-08, `docs/plan-presentar-en-llamada.md`): two complete devices,
//! a fake router in memory and real WebRTC on loopback, voice and video in Rust (`ft-media`).
//! What is presented travels as `CallPresent`, a state like the camera's, only between two apps
//! at call media version 2; the core never opens the plugin nor reads what it shows.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use ft_core::net::{Network, Relay};
use ft_core::{CallPhase, CallUpdate, Core, Event, MessageId, PresentedBy, Presenting, Transport, PEER_CANNOT_PRESENT};
use ft_media::testing::{fake_video, test_voice, DeviceProbe, ToneDevice};
use ft_media::{Activation, AudioPlatform, CallRouting, MediaSession};
use ft_push::RouterEvent;
use ft_storage::Store;
use ft_webrtc::SessionConfig;
use tokio::sync::{broadcast, mpsc};

#[derive(Default)]
struct Bus {
    online: Mutex<HashMap<String, mpsc::UnboundedSender<RouterEvent>>>,
    capabilities: Mutex<HashMap<String, [u8; 32]>>,
}

struct FakeRelay {
    bus: Arc<Bus>,
    me: OnceLock<String>,
}

#[async_trait]
impl Relay for FakeRelay {
    async fn signal(&self, to: &str, capability: &[u8; 32], bytes: Vec<u8>) -> anyhow::Result<bool> {
        if self.bus.capabilities.lock().unwrap().get(to) != Some(capability) {
            anyhow::bail!("wrong capability");
        }
        let online = self.bus.online.lock().unwrap().get(to).cloned();
        Ok(online.is_some_and(|device| device.send(RouterEvent::Signal(bytes)).is_ok()))
    }

    async fn deposit(&self, _to: &str, _capability: &[u8; 32], _blob: Vec<u8>) -> anyhow::Result<()> {
        Ok(())
    }

    async fn collect(&self) -> anyhow::Result<Vec<(String, Vec<u8>)>> {
        let _ = self.me.get();
        Ok(Vec::new())
    }

    async fn acknowledge(&self, _id: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

struct Phone {
    core: Arc<Core>,
    network: Arc<Network>,
}

impl Phone {
    fn id(&self) -> String {
        self.core.device_id().as_str().to_owned()
    }

    fn go_online(&self, bus: &Arc<Bus>) {
        let (events, mut incoming) = mpsc::unbounded_channel();
        bus.online.lock().unwrap().insert(self.id(), events.clone());
        let network = self.network.clone();
        tokio::spawn(async move {
            while let Some(event) = incoming.recv().await {
                network.handle(event).await;
            }
        });
        let _ = events.send(RouterEvent::Connected { stun: vec![], turn: None });
    }
}

async fn phone(bus: &Arc<Bus>, name: &str, inverted: bool) -> Phone {
    let relay = Arc::new(FakeRelay { bus: bus.clone(), me: OnceLock::new() });
    let network = Network::new(relay.clone(), SessionConfig::offline());
    let core = Core::open(Store::open_in_memory().await.unwrap(), [4; 32], network.clone()).await.expect("opens");
    core.set_name(name).await.unwrap();
    relay.me.set(core.device_id().as_str().to_owned()).unwrap();
    network.attach(&core);
    bus.capabilities.lock().unwrap().insert(core.device_id().as_str().to_owned(), *core.route_capability().as_bytes());
    let files = std::env::temp_dir().join(format!("ft-present-{name}-{}", ft_core::MessageId::new()));
    std::fs::create_dir_all(&files).expect("a folder for files");
    core.set_files_dir(files);
    core.set_call_audio(Some(AudioPlatform {
        backend: ToneDevice::factory(test_voice(12, inverted), DeviceProbe::default()),
        activation: Activation::Immediate,
    }));
    let (platform, _video) = fake_video();
    core.set_call_video(Some(platform));
    Phone { core, network }
}

async fn until<F, Fut>(what: &str, condition: F)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..300 {
        if condition().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting until {what}");
}

async fn pair(alice: &Phone, bob: &Phone) {
    let link = bob.core.my_card().await.unwrap().to_link();
    alice.core.add_contact(&link, None).await.expect("alice adds bob");
    let id = alice.id();
    until("bob knows alice", || async { bob.core.store().contact(&id).await.unwrap().is_some() }).await;
    bob.core.accept_contact(&id).await.expect("bob accepts alice");
}

async fn two_phones() -> (Phone, Phone) {
    let bus = Arc::new(Bus::default());
    let alice = phone(&bus, "Alice", false).await;
    let bob = phone(&bus, "Bob", true).await;
    alice.go_online(&bus);
    bob.go_online(&bus);
    pair(&alice, &bob).await;
    (alice, bob)
}

/// The next update of `call` that `wanted` accepts.
async fn next_update(events: &mut broadcast::Receiver<Event>, call: &str, wanted: impl Fn(&CallUpdate) -> bool) -> CallUpdate {
    let waiting = async {
        loop {
            if let Ok(Event::Call { call: id, update, .. }) = events.recv().await {
                if id == call && wanted(&update) {
                    return update;
                }
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(15), waiting).await.expect("the call update comes")
}

async fn ringing_call(bob: &Phone) -> String {
    until("bob's phone rings", || async {
        bob.core.current_call().await.unwrap().is_some_and(|call| call.phase == CallPhase::Ringing)
    })
    .await;
    bob.core.current_call().await.unwrap().expect("a call").call
}

/// Alice calls Bob natively, Bob answers, both connect.
async fn connected_call(alice: &Phone, bob: &Phone) -> String {
    let (mut alice_events, mut bob_events) = (alice.core.events(), bob.core.events());
    let call = alice.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.expect("alice calls");
    ringing_call(bob).await;
    bob.core.answer_native_call(&call, CallRouting::Auto).await.expect("bob answers");
    next_update(&mut alice_events, &call, |update| *update == CallUpdate::Connected).await;
    next_update(&mut bob_events, &call, |update| *update == CallUpdate::Connected).await;
    call
}

/// Alice as an older app (media version 0): her own connection, her offer through the core as
/// the WebView sends it, and Bob's answer applied by hand, as her WebView would.
async fn older_app_calls(alice: &Phone, bob: &Phone) -> (String, MediaSession) {
    let mut alice_events = alice.core.events();
    let session = MediaSession::open(&alice.network.media_config()).await.expect("alice's connection");
    let call = alice.core.place_call(&bob.id(), false).await.expect("alice calls like an older app");
    let offer = session.offer().await.expect("her offer");
    alice.core.offer_call_within(&call, &offer, Duration::from_secs(5)).await.expect("offered");
    ringing_call(bob).await;
    bob.core.answer_native_call(&call, CallRouting::Auto).await.expect("bob answers natively");
    let CallUpdate::Answered { sdp } = next_update(&mut alice_events, &call, |update| matches!(update, CallUpdate::Answered { .. })).await
    else {
        unreachable!()
    };
    session.accept(&sdp).await.expect("her WebView takes the answer");
    (call, session)
}

const BOARD: &str = "com.flickertalk.board";
const VIEWER: &str = "com.flickertalk.pdfviewer";

/// The next presentation `call` hears: (plugin, file, by).
async fn next_presenting(events: &mut broadcast::Receiver<Event>, call: &str) -> (Option<String>, Option<MessageId>, Option<PresentedBy>) {
    match next_update(events, call, |update| matches!(update, CallUpdate::Presenting { .. })).await {
        CallUpdate::Presenting { plugin, file, by } => (plugin, file, by),
        _ => unreachable!(),
    }
}

/// Whether `call` hears no presentation for `wait`.
async fn quiet(events: &mut broadcast::Receiver<Event>, call: &str, wait: Duration) -> bool {
    let heard = async {
        loop {
            if let Ok(Event::Call { call: id, update: CallUpdate::Presenting { .. }, .. }) = events.recv().await {
                if id == call {
                    return;
                }
            }
        }
    };
    tokio::time::timeout(wait, heard).await.is_err()
}

async fn presenting(phone: &Phone) -> Option<Presenting> {
    phone.core.current_call().await.unwrap().and_then(|current| current.presenting)
}

// Once a call between two apps at media version 2 is on, either side can present; not before.
#[tokio::test(flavor = "multi_thread")]
async fn both_sides_can_present_once_a_call_between_new_apps_is_on() {
    let (alice, bob) = two_phones().await;
    let (mut alice_events, mut bob_events) = (alice.core.events(), bob.core.events());
    let call = alice.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.expect("alice calls");
    ringing_call(&bob).await;
    let calling = alice.core.current_call().await.unwrap().expect("calling");
    assert!(!calling.can_present, "not before the call is on");
    assert_eq!(calling.presenting, None);
    bob.core.answer_native_call(&call, CallRouting::Auto).await.expect("bob answers");
    next_update(&mut alice_events, &call, |update| *update == CallUpdate::Connected).await;
    next_update(&mut bob_events, &call, |update| *update == CallUpdate::Connected).await;
    for phone in [&alice, &bob] {
        let current = phone.core.current_call().await.unwrap().expect("on");
        assert!(current.can_present, "{current:?}");
        assert_eq!(current.presenting, None, "nothing presented yet");
    }
    alice.core.end_call(&call, false).await.unwrap();
}

// An app at media version 1 (1.2 to 1.5) cannot show a presentation: neither side may present.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_with_an_app_at_media_one_cannot_present() {
    let (alice, bob) = two_phones().await;
    bob.core.set_call_media_version(1);
    let call = connected_call(&alice, &bob).await;
    for phone in [&alice, &bob] {
        assert!(!phone.core.current_call().await.unwrap().expect("on").can_present);
    }
    alice.core.end_call(&call, false).await.unwrap();
}

// An older app's WebView call (media version 0) cannot present either.
#[tokio::test(flavor = "multi_thread")]
async fn an_older_app_s_call_cannot_present() {
    let (alice, bob) = two_phones().await;
    let mut bob_events = bob.core.events();
    let (call, session) = older_app_calls(&alice, &bob).await;
    next_update(&mut bob_events, &call, |update| *update == CallUpdate::Connected).await;
    assert!(!bob.core.current_call().await.unwrap().expect("on").can_present);
    bob.core.end_call(&call, false).await.unwrap();
    session.close().await;
}

// The heart of it: Alice presents, both cores say so; a newer presentation replaces it; she
// stops, and both say nothing is presented.
#[tokio::test(flavor = "multi_thread")]
async fn a_presentation_and_its_end_reach_the_other_side() {
    let (alice, bob) = two_phones().await;
    let call = connected_call(&alice, &bob).await;
    let (mut alice_events, mut bob_events) = (alice.core.events(), bob.core.events());

    alice.core.present_in_call(&call, BOARD, None).await.expect("alice presents the board");
    assert_eq!(next_presenting(&mut alice_events, &call).await, (Some(BOARD.to_owned()), None, Some(PresentedBy::Me)));
    assert_eq!(next_presenting(&mut bob_events, &call).await, (Some(BOARD.to_owned()), None, Some(PresentedBy::Them)));
    assert_eq!(presenting(&alice).await, Some(Presenting { plugin: BOARD.to_owned(), file: None, by: PresentedBy::Me }));
    assert_eq!(presenting(&bob).await, Some(Presenting { plugin: BOARD.to_owned(), file: None, by: PresentedBy::Them }));

    alice.core.present_in_call(&call, VIEWER, None).await.expect("she switches to the viewer");
    assert_eq!(next_presenting(&mut bob_events, &call).await.0, Some(VIEWER.to_owned()));

    alice.core.stop_presenting(&call).await.expect("she stops");
    assert_eq!(next_presenting(&mut bob_events, &call).await, (None, None, None));
    assert_eq!(presenting(&alice).await, None);
    assert_eq!(presenting(&bob).await, None);
    alice.core.end_call(&call, false).await.unwrap();
}

// An app at media version 1 never hears of a presentation: the command fails with the key the
// call screen translates, and nothing is sent or shown.
#[tokio::test(flavor = "multi_thread")]
async fn presenting_to_an_app_at_media_one_fails_and_sends_nothing() {
    let (alice, bob) = two_phones().await;
    bob.core.set_call_media_version(1);
    let call = connected_call(&alice, &bob).await;
    let mut bob_events = bob.core.events();
    let refused = alice.core.present_in_call(&call, BOARD, None).await.expect_err("bob's app cannot show it");
    assert_eq!(refused.to_string(), PEER_CANNOT_PRESENT);
    let refused = bob.core.present_in_call(&call, BOARD, None).await.expect_err("nor can bob's present");
    assert_eq!(refused.to_string(), PEER_CANNOT_PRESENT);
    assert!(quiet(&mut bob_events, &call, Duration::from_secs(3)).await, "bob hears nothing");
    assert_eq!(presenting(&alice).await, None);
    assert_eq!(presenting(&bob).await, None);
    alice.core.end_call(&call, false).await.unwrap();
}

// An older app's WebView call: the same refusal.
#[tokio::test(flavor = "multi_thread")]
async fn presenting_in_an_older_app_s_call_fails() {
    let (alice, bob) = two_phones().await;
    let mut bob_events = bob.core.events();
    let (call, session) = older_app_calls(&alice, &bob).await;
    next_update(&mut bob_events, &call, |update| *update == CallUpdate::Connected).await;
    let refused = bob.core.present_in_call(&call, BOARD, None).await.expect_err("her app cannot show it");
    assert_eq!(refused.to_string(), PEER_CANNOT_PRESENT);
    bob.core.end_call(&call, false).await.unwrap();
    session.close().await;
}

// Hanging up ends the presentation with the call, and the end says it all; the next call
// starts with nothing presented.
#[tokio::test(flavor = "multi_thread")]
async fn hanging_up_ends_the_presentation() {
    let (alice, bob) = two_phones().await;
    let call = connected_call(&alice, &bob).await;
    let mut bob_events = bob.core.events();
    alice.core.present_in_call(&call, BOARD, None).await.expect("alice presents");
    next_presenting(&mut bob_events, &call).await;
    bob.core.end_call(&call, false).await.expect("bob hangs up");
    next_update(&mut bob_events, &call, |update| matches!(update, CallUpdate::Ended { .. })).await;
    assert!(quiet(&mut bob_events, &call, Duration::from_secs(1)).await, "no presentation after the end");
    until("alice's call is over", || async { alice.core.current_call().await.unwrap().is_none() }).await;

    let again = connected_call(&alice, &bob).await;
    assert_eq!(presenting(&alice).await, None);
    assert_eq!(presenting(&bob).await, None);
    alice.core.end_call(&again, false).await.unwrap();
}

/// Alice and Bob, paired, and Carol, paired with Alice only.
async fn three_phones() -> (Phone, Phone, Phone) {
    let bus = Arc::new(Bus::default());
    let alice = phone(&bus, "Alice", false).await;
    let bob = phone(&bus, "Bob", true).await;
    let carol = phone(&bus, "Carol", false).await;
    for one in [&alice, &bob, &carol] {
        one.go_online(&bus);
    }
    pair(&alice, &bob).await;
    pair(&alice, &carol).await;
    (alice, bob, carol)
}

/// A small file on disk to send.
fn a_file(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("ft-present-{}-{name}", MessageId::new()));
    std::fs::write(&path, vec![7u8; 64]).expect("writes");
    path
}

// The file a presentation shows is a file of this conversation: not a text, not a file of
// another chat, not an id from nowhere.
#[tokio::test(flavor = "multi_thread")]
async fn the_file_presented_is_a_file_of_this_conversation() {
    let (alice, bob, carol) = three_phones().await;
    let text = alice.core.send_text(&bob.id(), "the slides follow").await.expect("a text");
    let elsewhere = alice.core.send_file(&carol.id(), &a_file("other.pdf"), "other.pdf", "application/pdf").await.expect("carol's file");
    let slides = alice.core.send_file(&bob.id(), &a_file("slides.pdf"), "slides.pdf", "application/pdf").await.expect("bob's file");
    let call = connected_call(&alice, &bob).await;
    let mut bob_events = bob.core.events();
    for wrong in [text, elsewhere, MessageId::new().to_string()] {
        let wrong = MessageId::parse(&wrong).unwrap();
        assert!(alice.core.present_in_call(&call, VIEWER, Some(wrong)).await.is_err(), "{wrong} is not a file of this chat");
    }
    assert_eq!(presenting(&alice).await, None, "nothing presented by a refusal");
    let slides = MessageId::parse(&slides).unwrap();
    alice.core.present_in_call(&call, VIEWER, Some(slides)).await.expect("the slides of this chat");
    assert_eq!(next_presenting(&mut bob_events, &call).await, (Some(VIEWER.to_owned()), Some(slides), Some(PresentedBy::Them)));
    alice.core.end_call(&call, false).await.unwrap();
}

// One presents at a time: while Bob presents, Alice's Present is refused and her stop leaves his.
#[tokio::test(flavor = "multi_thread")]
async fn presenting_is_refused_while_the_other_side_presents() {
    let (alice, bob) = two_phones().await;
    let call = connected_call(&alice, &bob).await;
    let mut alice_events = alice.core.events();
    bob.core.present_in_call(&call, BOARD, None).await.expect("bob presents");
    next_presenting(&mut alice_events, &call).await;
    assert!(alice.core.present_in_call(&call, VIEWER, None).await.is_err(), "bob is presenting");
    alice.core.stop_presenting(&call).await.expect("nothing of hers to stop");
    assert_eq!(presenting(&alice).await.map(|shown| shown.by), Some(PresentedBy::Them));
    bob.core.stop_presenting(&call).await.expect("bob stops");
    assert_eq!(next_presenting(&mut alice_events, &call).await, (None, None, None));
    alice.core.present_in_call(&call, VIEWER, None).await.expect("her turn now");
    alice.core.end_call(&call, false).await.unwrap();
}

/// Whether both show the same presentation, one as its presenter and the other as its watcher.
async fn agree(alice: &Phone, bob: &Phone) -> bool {
    match (presenting(alice).await, presenting(bob).await) {
        (Some(a), Some(b)) => a.plugin == b.plugin && a.by != b.by,
        _ => false,
    }
}

// Both press Present at once: whatever the order things cross in, they end up showing the same.
#[tokio::test(flavor = "multi_thread")]
async fn both_presenting_at_once_end_up_showing_the_same() {
    let (alice, bob) = two_phones().await;
    let call = connected_call(&alice, &bob).await;
    let (by_alice, by_bob) = tokio::join!(alice.core.present_in_call(&call, BOARD, None), bob.core.present_in_call(&call, VIEWER, None));
    assert!(by_alice.is_ok() || by_bob.is_ok(), "someone presents");
    until("both show the same presentation", || agree(&alice, &bob)).await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(agree(&alice, &bob).await, "and it stays so");
    alice.core.end_call(&call, false).await.unwrap();
}
