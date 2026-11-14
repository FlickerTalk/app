use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use ft_core::net::{Network, Relay};
use ft_core::{CallPhase, CallUpdate, Core, Event, MessageId, PresentedBy, Presenting, Transport, NEEDS_SUBSCRIPTION, PEER_CANNOT_PRESENT};
use ft_plugins::{sign_package, Kind, Permissions};
use ft_media::testing::{fake_video, test_voice, DeviceProbe, ToneDevice};
use ft_media::{Activation, AudioPlatform, CallRouting, MediaSession};
use ft_push::RouterEvent;
use ft_storage::Store;
use ft_webrtc::SessionConfig;
use tokio::sync::{broadcast, mpsc};
use vodozemac::Ed25519SecretKey;

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

async fn connected_call(alice: &Phone, bob: &Phone) -> String {
    let (mut alice_events, mut bob_events) = (alice.core.events(), bob.core.events());
    let call = alice.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.expect("alice calls");
    ringing_call(bob).await;
    bob.core.answer_native_call(&call, CallRouting::Auto).await.expect("bob answers");
    next_update(&mut alice_events, &call, |update| *update == CallUpdate::Connected).await;
    next_update(&mut bob_events, &call, |update| *update == CallUpdate::Connected).await;
    call
}

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

async fn next_presenting(events: &mut broadcast::Receiver<Event>, call: &str) -> (Option<String>, Option<MessageId>, Option<PresentedBy>) {
    match next_update(events, call, |update| matches!(update, CallUpdate::Presenting { .. })).await {
        CallUpdate::Presenting { plugin, file, by } => (plugin, file, by),
        _ => unreachable!(),
    }
}

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

fn a_file(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("ft-present-{}-{name}", MessageId::new()));
    std::fs::write(&path, vec![7u8; 64]).expect("writes");
    path
}

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

async fn agree(alice: &Phone, bob: &Phone) -> bool {
    match (presenting(alice).await, presenting(bob).await) {
        (Some(a), Some(b)) => a.plugin == b.plugin && a.by != b.by,
        _ => false,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn both_presenting_at_once_end_up_showing_the_same() {
    let (alice, bob) = two_phones().await;
    let call = connected_call(&alice, &bob).await;
    let (by_alice, by_bob) = tokio::join!(alice.core.present_in_call(&call, BOARD, None), bob.core.present_in_call(&call, VIEWER, None));
    assert!(by_alice.is_ok() || by_bob.is_ok(), "someone presents");
    until("both show the same presentation", || agree(&alice, &bob)).await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(agree(&alice, &bob).await, "and it stays so");
    let winner = match (by_alice.is_ok(), by_bob.is_ok()) {
        (true, true) if alice.id() < bob.id() => &alice,
        (true, true) => &bob,
        (true, false) => &alice,
        _ => &bob,
    };
    assert_eq!(presenting(winner).await.map(|shown| shown.by), Some(PresentedBy::Me), "the winner keeps its own");
    alice.core.end_call(&call, false).await.unwrap();
}

async fn last_heard(events: &mut broadcast::Receiver<Event>, call: &str) -> Option<Option<Presenting>> {
    let mut last = None;
    loop {
        let next = tokio::time::timeout(Duration::from_millis(1500), events.recv()).await;
        match next {
            Ok(Ok(Event::Call { call: id, update: CallUpdate::Presenting { plugin, file, by }, .. })) if id == call => {
                last = Some(plugin.zip(by).map(|(plugin, by)| Presenting { plugin, file, by }));
            }
            Ok(Ok(_)) => {}
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => panic!("events lost"),
            Ok(Err(_)) | Err(_) => return last,
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn after_a_clash_the_last_word_each_screen_hears_is_what_its_core_keeps() {
    let (alice, bob) = two_phones().await;
    let call = connected_call(&alice, &bob).await;
    for round in 0..6 {
        let (mut alice_events, mut bob_events) = (alice.core.events(), bob.core.events());
        let (by_alice, by_bob) = tokio::join!(alice.core.present_in_call(&call, BOARD, None), bob.core.present_in_call(&call, BOARD, None));
        assert!(by_alice.is_ok() || by_bob.is_ok(), "round {round}: someone presents");
        until("both show the same presentation", || agree(&alice, &bob)).await;
        let (alice_heard, bob_heard) = tokio::join!(last_heard(&mut alice_events, &call), last_heard(&mut bob_events, &call));
        assert_eq!(alice_heard, Some(presenting(&alice).await), "round {round}: alice's screen");
        assert_eq!(bob_heard, Some(presenting(&bob).await), "round {round}: bob's screen");
        for phone in [&alice, &bob] {
            phone.core.stop_presenting(&call).await.expect("stops");
        }
        until("nothing presented", || async { presenting(&alice).await.is_none() && presenting(&bob).await.is_none() }).await;
    }
    alice.core.end_call(&call, false).await.unwrap();
}

const TOOL: &str = "com.example.board";
const DAY: i64 = 24 * 60 * 60 * 1000;

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("after 1970").as_millis() as i64
}

async fn install_tools(phone: &Phone, ids: &[&str]) {
    let tools: Vec<(&str, bool)> = ids.iter().map(|id| (*id, true)).collect();
    install_tools_asking(phone, &tools).await;
}

async fn install_tools_asking(phone: &Phone, tools: &[(&str, bool)]) {
    let dir = std::env::temp_dir().join(format!("ft-present-plugins-{}", MessageId::new()));
    std::fs::create_dir_all(&dir).expect("a folder for plugins");
    phone.core.set_plugins_dir(dir);
    let catalogue = Ed25519SecretKey::new();
    for (id, live) in tools {
        let manifest = format!(
            r#"{{"id":"{id}","name":"X","version":"1.0.0","minCoreVersion":"0.1.0","components":["ft-x"],"kind":"tool","permissions":{{"live":{live}}}}}"#
        );
        let package = sign_package(&[("module.json".to_owned(), manifest.into_bytes()), ("dist/index.js".to_owned(), b"".to_vec())], &catalogue);
        phone.core.install_plugin(&package, &catalogue.public_key(), Permissions::default()).await.expect("installs while free");
    }
}

async fn lock_tools(phone: &Phone) {
    let installed = now_ms() - 366 * DAY;
    phone.core.store().set_setting("installed_at", &installed.to_string()).await.expect("sets");
    assert!(!phone.core.may_use(Kind::Tool).await.unwrap(), "the tools are closed");
}

async fn plan_changed(events: &mut broadcast::Receiver<Event>) -> bool {
    let heard = async {
        loop {
            if let Ok(Event::PlanChanged) = events.recv().await {
                return;
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(5), heard).await.is_ok()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_student_with_locked_tools_may_follow_a_presentation() {
    let (alice, bob) = two_phones().await;
    install_tools(&alice, &[BOARD]).await;
    install_tools(&bob, &[BOARD, VIEWER]).await;
    lock_tools(&bob).await;
    let call = connected_call(&alice, &bob).await;
    assert_eq!(bob.core.open_plugin(BOARD).await.expect_err("closed").to_string(), NEEDS_SUBSCRIPTION);

    let mut bob_events = bob.core.events();
    alice.core.present_in_call(&call, BOARD, None).await.expect("alice, in her free days, presents");
    assert!(plan_changed(&mut bob_events).await, "the WebView serves the tool again");
    next_presenting(&mut bob_events, &call).await;
    bob.core.open_plugin(BOARD).await.expect("bob follows it");
    assert!(bob.core.may_use_plugin(BOARD, Kind::Tool).await.unwrap());
    let refused = bob.core.open_plugin(VIEWER).await.expect_err("only the one presented");
    assert_eq!(refused.to_string(), NEEDS_SUBSCRIPTION);

    alice.core.stop_presenting(&call).await.expect("she stops");
    next_presenting(&mut bob_events, &call).await;
    assert!(plan_changed(&mut bob_events).await, "and stops serving it");
    assert_eq!(bob.core.open_plugin(BOARD).await.expect_err("closed again").to_string(), NEEDS_SUBSCRIPTION);
    alice.core.end_call(&call, false).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_teacher_with_locked_tools_may_not_present() {
    let (alice, bob) = two_phones().await;
    install_tools(&alice, &[TOOL]).await;
    install_tools(&bob, &[TOOL]).await;
    lock_tools(&alice).await;
    let call = connected_call(&alice, &bob).await;
    let mut bob_events = bob.core.events();
    let refused = alice.core.present_in_call(&call, TOOL, None).await.expect_err("her tools are closed");
    assert_eq!(refused.to_string(), NEEDS_SUBSCRIPTION);
    assert!(quiet(&mut bob_events, &call, Duration::from_secs(3)).await, "bob hears nothing");
    assert_eq!(presenting(&alice).await, None);
    alice.core.end_call(&call, false).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn following_does_not_unlock_the_tool_outside_the_call() {
    let (alice, bob) = two_phones().await;
    install_tools(&alice, &[BOARD]).await;
    install_tools(&bob, &[BOARD]).await;
    lock_tools(&bob).await;
    let call = connected_call(&alice, &bob).await;
    let mut bob_events = bob.core.events();
    alice.core.present_in_call(&call, BOARD, None).await.expect("alice presents");
    assert!(plan_changed(&mut bob_events).await, "the WebView serves the tool");
    next_presenting(&mut bob_events, &call).await;
    bob.core.open_plugin(BOARD).await.expect("bob follows it");

    bob.core.end_call(&call, false).await.expect("bob hangs up");
    assert!(plan_changed(&mut bob_events).await, "the WebView stops serving it");
    assert_eq!(bob.core.open_plugin(BOARD).await.expect_err("closed again").to_string(), NEEDS_SUBSCRIPTION);
    assert!(!bob.core.may_use_plugin(BOARD, Kind::Tool).await.unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_presentation_of_a_tool_that_cannot_follow_unlocks_nothing() {
    let (alice, bob) = two_phones().await;
    install_tools_asking(&bob, &[(BOARD, false), (TOOL, true)]).await;
    lock_tools(&bob).await;
    let call = connected_call(&alice, &bob).await;
    let mut bob_events = bob.core.events();
    alice.core.present_in_call(&call, BOARD, None).await.expect("alice presents a board without the live channel");
    assert_eq!(next_presenting(&mut bob_events, &call).await.0, Some(BOARD.to_owned()));
    assert_eq!(bob.core.open_plugin(BOARD).await.expect_err("still closed").to_string(), NEEDS_SUBSCRIPTION);
    assert!(!bob.core.may_use_plugin(BOARD, Kind::Tool).await.unwrap());

    alice.core.present_in_call(&call, TOOL, None).await.expect("alice presents a live tool that does not present");
    assert_eq!(next_presenting(&mut bob_events, &call).await.0, Some(TOOL.to_owned()));
    assert_eq!(bob.core.open_plugin(TOOL).await.expect_err("not a presenting tool").to_string(), NEEDS_SUBSCRIPTION);
    assert!(!bob.core.may_use_plugin(TOOL, Kind::Tool).await.unwrap());
    alice.core.end_call(&call, false).await.unwrap();
}

#[derive(Debug, PartialEq, Eq)]
enum First {
    Plan,
    Presenting,
}

async fn first_of(events: &mut broadcast::Receiver<Event>, call: &str) -> First {
    let waiting = async {
        loop {
            match events.recv().await {
                Ok(Event::PlanChanged) => return First::Plan,
                Ok(Event::Call { call: id, update: CallUpdate::Presenting { .. }, .. }) if id == call => return First::Presenting,
                _ => {}
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(15), waiting).await.expect("one of them comes")
}

#[tokio::test(flavor = "multi_thread")]
async fn the_followed_tool_is_served_before_it_shows_and_after_it_goes() {
    let (alice, bob) = two_phones().await;
    let call = connected_call(&alice, &bob).await;
    let mut bob_events = bob.core.events();
    alice.core.present_in_call(&call, BOARD, None).await.expect("alice presents");
    assert_eq!(first_of(&mut bob_events, &call).await, First::Plan, "served first");
    assert_eq!(next_presenting(&mut bob_events, &call).await.0, Some(BOARD.to_owned()), "then shown");

    alice.core.present_in_call(&call, VIEWER, None).await.expect("she switches");
    assert_eq!(first_of(&mut bob_events, &call).await, First::Plan, "the new one served first");
    assert_eq!(next_presenting(&mut bob_events, &call).await.0, Some(VIEWER.to_owned()));

    alice.core.stop_presenting(&call).await.expect("she stops");
    assert_eq!(first_of(&mut bob_events, &call).await, First::Presenting, "the sheet goes first");
    assert!(plan_changed(&mut bob_events).await, "then the frame stops being served");
    alice.core.end_call(&call, false).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_presentation_of_a_file_of_another_chat_is_ignored() {
    let (alice, bob, carol) = three_phones().await;
    pair(&carol, &bob).await;
    let elsewhere = bob.core.send_file(&carol.id(), &a_file("carol.pdf"), "carol.pdf", "application/pdf").await.expect("carol's file");
    let elsewhere = MessageId::parse(&elsewhere).unwrap();
    let text = alice.core.send_text(&bob.id(), "the slides follow").await.expect("a text");
    let text = MessageId::parse(&text).unwrap();
    until("bob has alice's text", || async { bob.core.store().message(&text.to_string()).await.unwrap().is_some() }).await;
    assert!(!bob.core.file_belongs_to(&elsewhere, &alice.id()).await.unwrap(), "not a file of alice's chat");
    assert!(bob.core.file_belongs_to(&elsewhere, &carol.id()).await.unwrap(), "a file of carol's");
    assert!(!bob.core.file_belongs_to(&text, &alice.id()).await.unwrap(), "a text is not a file");

    let call = connected_call(&alice, &bob).await;
    let mut bob_events = bob.core.events();
    for (seq, wrong) in [(1, elsewhere), (2, text)] {
        assert!(alice.core.say_call_present(&bob.id(), &call, seq, Some(VIEWER.to_owned()), Some(wrong)).await.unwrap(), "said");
    }
    let not_yet = MessageId::new();
    assert!(alice.core.say_call_present(&bob.id(), &call, 3, Some(VIEWER.to_owned()), Some(not_yet)).await.unwrap(), "said");
    assert_eq!(next_presenting(&mut bob_events, &call).await, (Some(VIEWER.to_owned()), Some(not_yet), Some(PresentedBy::Them)), "only the last is taken");
    alice.core.end_call(&call, false).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_presentation_before_the_answer_changes_nothing() {
    let (alice, bob) = two_phones().await;
    let mut alice_events = alice.core.events();
    let call = alice.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.expect("alice calls");
    ringing_call(&bob).await;
    assert!(bob.core.say_call_present(&alice.id(), &call, 1, Some(BOARD.to_owned()), None).await.unwrap(), "said while ringing");
    bob.core.answer_native_call(&call, CallRouting::Auto).await.expect("bob answers");
    next_update(&mut alice_events, &call, |update| *update == CallUpdate::Connected).await;
    assert_eq!(presenting(&alice).await, None, "nothing from before the answer");
    assert!(bob.core.say_call_present(&alice.id(), &call, 2, Some(VIEWER.to_owned()), None).await.unwrap(), "said in the call");
    assert_eq!(next_presenting(&mut alice_events, &call).await, (Some(VIEWER.to_owned()), None, Some(PresentedBy::Them)), "the first she shows");
    alice.core.end_call(&call, false).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stop_lost_with_the_link_arrives_once_it_is_back() {
    let (alice, bob) = two_phones().await;
    let call = connected_call(&alice, &bob).await;
    let mut bob_events = bob.core.events();
    alice.core.present_in_call(&call, BOARD, None).await.expect("alice presents");
    next_presenting(&mut bob_events, &call).await;

    bob.network.vanish(&alice.id()).await;
    assert!(alice.network.is_connected(&bob.id()).await, "alice's side still looks open");
    alice.core.stop_presenting(&call).await.expect("she stops");
    assert!(quiet(&mut bob_events, &call, Duration::from_secs(2)).await, "the stop is lost with the link");
    assert_eq!(presenting(&bob).await.map(|shown| shown.by), Some(PresentedBy::Them), "bob still shows it");

    bob.core.send_text(&alice.id(), "still there?").await.expect("the link comes back");
    assert_eq!(next_presenting(&mut bob_events, &call).await, (None, None, None), "the stop arrives");
    assert_eq!(presenting(&bob).await, None);
    alice.core.end_call(&call, false).await.unwrap();
}
