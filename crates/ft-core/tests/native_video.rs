//! Native video (2026-09-29, `docs/video-nativo.md`): two complete devices, a fake router in
//! memory and real WebRTC on loopback, with the voice and the video in Rust (`ft-media`). The
//! phones' audio devices speak a test voice; their cameras and displays are the engine's fakes.
//!
//! Every native call negotiates audio and video from the start; switching is turning our own
//! camera on or off and telling the other side with `CallMedia`, never a new offer.
//!

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use ft_core::net::{Network, Relay};
use ft_core::{CallPhase, CallUpdate, Core, Event, Transport};
use ft_media::testing::{fake_video, mean_heard, test_voice, DeviceProbe, ToneDevice, VideoProbe};
use ft_media::{Activation, AudioPlatform, CallRouting, Facing, MediaSession, VideoState};
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
    voice: DeviceProbe,
    video: VideoProbe,
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

    fn video(&self, call: &str) -> VideoState {
        self.core.call_video(call).unwrap_or_default()
    }
}

const VOICE_SECONDS: usize = 12;

async fn phone(bus: &Arc<Bus>, name: &str, inverted: bool) -> Phone {
    let relay = Arc::new(FakeRelay { bus: bus.clone(), me: OnceLock::new() });
    let network = Network::new(relay.clone(), SessionConfig::offline());
    let core = Core::open(Store::open_in_memory().await.unwrap(), [4; 32], network.clone()).await.expect("opens");
    core.set_name(name).await.unwrap();
    relay.me.set(core.device_id().as_str().to_owned()).unwrap();
    network.attach(&core);
    bus.capabilities.lock().unwrap().insert(core.device_id().as_str().to_owned(), *core.route_capability().as_bytes());
    let voice = DeviceProbe::default();
    core.set_call_audio(Some(AudioPlatform {
        backend: ToneDevice::factory(test_voice(VOICE_SECONDS, inverted), voice.clone()),
        activation: Activation::Immediate,
    }));
    let (platform, video) = fake_video();
    core.set_call_video(Some(platform));
    Phone { core, network, voice, video }
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

/// The next video state of `call` that `wanted` accepts.
async fn next_video(events: &mut broadcast::Receiver<Event>, call: &str, wanted: impl Fn(&VideoState) -> bool) -> VideoState {
    match next_update(events, call, |update| matches!(update, CallUpdate::Video(state) if wanted(state))).await {
        CallUpdate::Video(state) => state,
        _ => unreachable!(),
    }
}

async fn ringing_call(bob: &Phone) -> String {
    until("bob's phone rings", || async {
        bob.core.current_call().await.unwrap().is_some_and(|call| call.phase == CallPhase::Ringing)
    })
    .await;
    bob.core.current_call().await.unwrap().expect("a call").call
}

/// Alice calls Bob natively (`video` as she starts it), Bob answers, both connect.
async fn connected_call(alice: &Phone, bob: &Phone, video: bool) -> String {
    let (mut alice_events, mut bob_events) = (alice.core.events(), bob.core.events());
    let call = alice.core.start_native_call(&bob.id(), CallRouting::Auto, video).await.expect("alice calls");
    ringing_call(bob).await;
    bob.core.answer_native_call(&call, CallRouting::Auto).await.expect("bob answers");
    next_update(&mut alice_events, &call, |update| *update == CallUpdate::Connected).await;
    next_update(&mut bob_events, &call, |update| *update == CallUpdate::Connected).await;
    call
}

fn frames_shown(phone: &Phone) -> usize {
    phone.video.display().shown.len()
}

// The heart of the design: voice → video → voice from either side,
// with no new offer, and the voice sounding all along.
#[tokio::test(flavor = "multi_thread")]
async fn a_voice_call_turns_to_video_and_back_with_the_voice_going_on() {
    let (alice, bob) = two_phones().await;
    let call = connected_call(&alice, &bob, false).await;
    let (mut alice_events, mut bob_events) = (alice.core.events(), bob.core.events());
    until("video is available on both sides", || async { alice.video(&call).available && bob.video(&call).available }).await;
    assert!(!alice.video(&call).camera && !bob.video(&call).remote, "a voice call starts with no camera");
    assert!(!alice.video.camera().running);
    // The switches are spread over the stretch of voice measured below (from 1 s to 5 s).
    let pause = || tokio::time::sleep(Duration::from_millis(600));
    pause().await;

    let state = alice.core.set_call_camera(&call, true).await.expect("alice turns her camera on");
    assert!(state.camera && state.sending());
    let seen = next_video(&mut bob_events, &call, |state| state.remote).await;
    assert!(!seen.remote_paused && !seen.camera, "bob sees alice; his own camera stays off");
    until("bob shows alice's frames", || async { frames_shown(&bob) > 5 }).await;
    assert!(!bob.video.camera().running, "alice's picture does not open bob's camera");
    pause().await;

    bob.core.set_call_camera(&call, true).await.expect("bob turns his camera on too");
    next_video(&mut alice_events, &call, |state| state.remote && state.camera).await;
    until("alice shows bob's frames", || async { frames_shown(&alice) > 5 }).await;
    pause().await;

    alice.core.set_call_camera(&call, false).await.expect("alice turns hers off");
    next_video(&mut bob_events, &call, |state| !state.remote && state.camera).await;
    until("alice's camera stops", || async { !alice.video.camera().running }).await;
    pause().await;
    bob.core.set_call_camera(&call, false).await.expect("back to voice");
    next_video(&mut alice_events, &call, |state| !state.any()).await;
    assert!(alice.video.made() == 1 && bob.video.made() == 1, "the devices are made once per call");

    until("bob played 6 s of the call", || async { bob.voice.played().len() > 6 * 48_000 }).await;
    alice.core.end_call(&call, false).await.expect("alice hangs up");
    until("bob's call ends", || async { bob.core.current_call().await.unwrap().is_none() }).await;
    let bob_heard = mean_heard(&test_voice(VOICE_SECONDS, false), &bob.voice.played(), 48_000, 12_000, 16);
    assert!(bob_heard > 0.7, "bob heard alice at {bob_heard} while the cameras went on and off");
}

// The camera turned on before the call connects (from the calling screen) is kept, and turns on
// as soon as the call has video both ways: ft-media refuses it before.
#[tokio::test(flavor = "multi_thread")]
async fn a_camera_turned_on_before_the_call_connects_turns_on_once_it_does() {
    let (alice, bob) = two_phones().await;
    let (mut alice_events, mut bob_events) = (alice.core.events(), bob.core.events());
    let call = alice.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.expect("alice calls");
    ringing_call(&bob).await;
    let kept = alice.core.set_call_camera(&call, true).await.expect("kept for when it connects");
    assert!(kept.camera && !kept.available, "{kept:?}");
    assert!(alice.video(&call).camera, "the core says it is wanted");
    assert!(!alice.video.camera().running, "nothing opens before the call connects");

    bob.core.answer_native_call(&call, CallRouting::Auto).await.expect("bob answers");
    next_update(&mut alice_events, &call, |update| *update == CallUpdate::Connected).await;
    next_video(&mut bob_events, &call, |state| state.remote).await;
    until("alice's camera runs", || async { alice.video.camera().running }).await;

    // And turned off again before connecting, it stays off.
    alice.core.end_call(&call, false).await.unwrap();
    until("the call is over", || async { bob.core.current_call().await.unwrap().is_none() }).await;
    let call = alice.core.start_native_call(&bob.id(), CallRouting::Auto, true).await.expect("alice calls with video");
    ringing_call(&bob).await;
    assert!(!alice.core.set_call_camera(&call, false).await.expect("changed her mind").camera);
    bob.core.answer_native_call(&call, CallRouting::Auto).await.expect("bob answers");
    next_video(&mut alice_events, &call, |state| state.remote).await;
    assert!(!alice.video(&call).camera, "her camera stays off");
    alice.core.end_call(&call, false).await.unwrap();
}

// Both press at once: two states, one per camera, nothing to clash.
#[tokio::test(flavor = "multi_thread")]
async fn both_turning_their_cameras_on_at_once_ends_with_both_on() {
    let (alice, bob) = two_phones().await;
    let call = connected_call(&alice, &bob, false).await;
    until("video is available", || async { alice.video(&call).available && bob.video(&call).available }).await;
    let (alice_on, bob_on) = tokio::join!(alice.core.set_call_camera(&call, true), bob.core.set_call_camera(&call, true));
    alice_on.expect("alice's camera");
    bob_on.expect("bob's camera");
    until("each sees the other", || async {
        let (a, b) = (alice.video(&call), bob.video(&call));
        a.camera && a.remote && b.camera && b.remote
    })
    .await;
    until("frames both ways", || async { frames_shown(&alice) > 5 && frames_shown(&bob) > 5 }).await;
    alice.core.end_call(&call, false).await.unwrap();
}

// A video call from the start: the caller's camera turns on as it
// connects, and so does the callee's when it answers from the app.
#[tokio::test(flavor = "multi_thread")]
async fn a_video_call_starts_with_both_cameras_on() {
    let (alice, bob) = two_phones().await;
    let call = connected_call(&alice, &bob, true).await;
    until("both cameras on and seen", || async {
        let (a, b) = (alice.video(&call), bob.video(&call));
        a.sending() && a.remote && b.sending() && b.remote
    })
    .await;
    let current = bob.core.current_call().await.unwrap().expect("active");
    assert!(current.video && current.video_state.is_some_and(|state| state.camera), "{current:?}");
    alice.core.end_call(&call, false).await.unwrap();
}

// CallKit answers a video call on a locked iPhone: the camera is
// wanted but held (iOS stops it in the background) and the other side sees it paused; once the app
// is on the screen it runs.
#[tokio::test(flavor = "multi_thread")]
async fn a_video_call_answered_by_the_os_with_the_app_away_holds_the_camera_until_the_app_shows() {
    let (alice, bob) = two_phones().await;
    bob.core.set_app_visible(false).await.expect("the phone is locked");
    let mut alice_events = alice.core.events();
    let call = alice.core.start_native_call(&bob.id(), CallRouting::Auto, true).await.expect("alice calls");
    ringing_call(&bob).await;
    assert!(bob.core.answer_ringing_call().await.expect("answers"), "the OS answers a video call too");
    next_update(&mut alice_events, &call, |update| *update == CallUpdate::Connected).await;

    let seen = next_video(&mut alice_events, &call, |state| state.remote && state.remote_paused).await;
    assert!(seen.camera, "alice's own camera runs");
    let held = bob.video(&call);
    assert!(held.camera && held.paused && !held.sending(), "{held:?}");
    assert!(!bob.video.camera().running, "nothing is captured with the app away");

    bob.core.set_app_visible(true).await.expect("unlocked, the app on the screen");
    next_video(&mut alice_events, &call, |state| state.remote && !state.remote_paused).await;
    until("bob's camera runs", || async { bob.video.camera().running }).await;
    until("alice shows bob", || async { frames_shown(&alice) > 5 }).await;
    alice.core.end_call(&call, false).await.unwrap();
}

// Leaving the call screen (the WebView's `null` layout) holds the
// camera; coming back gives it back.
#[tokio::test(flavor = "multi_thread")]
async fn leaving_the_call_screen_holds_the_camera() {
    let (alice, bob) = two_phones().await;
    let call = connected_call(&alice, &bob, false).await;
    let mut bob_events = bob.core.events();
    until("video is available", || async { alice.video(&call).available }).await;
    alice.core.set_call_camera(&call, true).await.expect("camera on");
    next_video(&mut bob_events, &call, |state| state.remote && !state.remote_paused).await;

    alice.core.set_call_shown(false).await.expect("alice leaves the call screen");
    next_video(&mut bob_events, &call, |state| state.remote && state.remote_paused).await;
    let state = alice.video(&call);
    assert!(state.camera && state.paused, "still wanted, held: {state:?}");
    until("alice's camera stops", || async { !alice.video.camera().running }).await;

    alice.core.set_call_shown(true).await.expect("back on the call screen");
    next_video(&mut bob_events, &call, |state| state.remote && !state.remote_paused).await;
    until("alice's camera runs again", || async { alice.video.camera().running }).await;
    alice.core.end_call(&call, false).await.unwrap();
}

// The other camera: front to back and back again.
#[tokio::test(flavor = "multi_thread")]
async fn the_camera_switches_between_front_and_back() {
    let (alice, bob) = two_phones().await;
    let call = connected_call(&alice, &bob, true).await;
    until("alice's camera runs", || async { alice.video.camera().running }).await;
    assert_eq!(alice.video(&call).facing, Facing::Front);
    let mut alice_events = alice.core.events();
    assert_eq!(alice.core.switch_call_camera(&call).await.expect("switches").facing, Facing::Back);
    // The UI hears which camera is on (it mirrors only the front one), and so does a late WebView.
    let heard = next_video(&mut alice_events, &call, |state| state.facing == Facing::Back).await;
    assert!(heard.camera, "{heard:?}");
    let current = alice.core.current_call().await.unwrap().expect("going on");
    assert_eq!(current.video_state.map(|state| state.facing), Some(Facing::Back));
    until("the camera faces back", || async { alice.video.camera().facing == Some(Facing::Back) }).await;
    assert!(alice.video.camera().running, "the camera kept running");
    assert_eq!(alice.core.switch_call_camera(&call).await.expect("switches").facing, Facing::Front);
    next_video(&mut alice_events, &call, |state| state.facing == Facing::Front).await;
    until("the camera faces front", || async { alice.video.camera().facing == Some(Facing::Front) }).await;
    alice.core.end_call(&call, false).await.unwrap();
}

// Switched with the camera off, or before the call connects: the camera opens facing that way.
#[tokio::test(flavor = "multi_thread")]
async fn a_camera_switched_while_off_opens_facing_that_way() {
    let (alice, bob) = two_phones().await;
    let mut alice_events = alice.core.events();
    let call = alice.core.start_native_call(&bob.id(), CallRouting::Auto, true).await.expect("alice calls with video");
    ringing_call(&bob).await;
    assert_eq!(alice.core.switch_call_camera(&call).await.expect("switched before connecting").facing, Facing::Back);
    assert_eq!(alice.video(&call).facing, Facing::Back);
    bob.core.answer_native_call(&call, CallRouting::Auto).await.expect("bob answers");
    next_update(&mut alice_events, &call, |update| *update == CallUpdate::Connected).await;
    until("alice's camera runs facing back", || async {
        let camera = alice.video.camera();
        camera.running && camera.facing == Some(Facing::Back)
    })
    .await;

    assert!(!alice.core.set_call_camera(&call, false).await.expect("off").camera);
    until("her camera stops", || async { !alice.video.camera().running }).await;
    assert_eq!(alice.core.switch_call_camera(&call).await.expect("switched while off").facing, Facing::Front);
    next_video(&mut alice_events, &call, |state| !state.camera && state.facing == Facing::Front).await;
    assert!(!alice.video.camera().running, "switching does not turn it on");
    assert!(alice.core.set_call_camera(&call, true).await.expect("on").camera);
    until("alice's camera runs facing front", || async {
        let camera = alice.video.camera();
        camera.running && camera.facing == Some(Facing::Front)
    })
    .await;
    alice.core.end_call(&call, false).await.unwrap();
}

// The system's video button (CallKit) or the notification's camera
// action: our camera turns on in the call going on.
#[tokio::test(flavor = "multi_thread")]
async fn the_system_s_video_button_turns_our_camera_on() {
    let (alice, bob) = two_phones().await;
    let call = connected_call(&alice, &bob, false).await;
    let mut alice_events = alice.core.events();
    until("video is available", || async { bob.video(&call).available }).await;
    bob.core.request_call_video().await.expect("bob asks for video from the system screen");
    next_video(&mut alice_events, &call, |state| state.remote).await;
    assert!(bob.video(&call).camera);
    alice.core.end_call(&call, false).await.unwrap();
}

// Hanging up stops the cameras and the displays on both sides, and
// the native views are taken away before the devices go (iOS: the layers are the devices').
#[tokio::test(flavor = "multi_thread")]
async fn hanging_up_stops_the_video_after_the_views_are_taken_away() {
    let (alice, bob) = two_phones().await;
    let running_at_detach = Arc::new(Mutex::new(None));
    let (seen, probe) = (running_at_detach.clone(), bob.video.clone());
    bob.core.set_video_detach(Some(Arc::new(move || {
        *seen.lock().unwrap() = Some(probe.display().running);
    })));
    let call = connected_call(&alice, &bob, true).await;
    until("video both ways", || async { frames_shown(&alice) > 5 && frames_shown(&bob) > 5 }).await;

    bob.core.end_call(&call, false).await.expect("bob hangs up");
    until("everything stops", || async {
        !alice.video.camera().running && !alice.video.display().running && !bob.video.camera().running && !bob.video.display().running
    })
    .await;
    assert_eq!(*running_at_detach.lock().unwrap(), Some(true), "the views go while the devices are still there");
}

// The native views go when a call with video ends, once, before its devices (the core cannot
// know what the bridge attached: the app's hook decides). Passes against the stubs too.
#[tokio::test(flavor = "multi_thread")]
async fn the_app_takes_its_video_views_away_when_a_native_call_ends() {
    let (alice, bob) = two_phones().await;
    let detached = Arc::new(AtomicUsize::new(0));
    let count = detached.clone();
    alice.core.set_video_detach(Some(Arc::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
    })));
    let call = alice.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.expect("alice calls");
    ringing_call(&bob).await;
    assert_eq!(detached.load(Ordering::SeqCst), 0);
    alice.core.end_call(&call, false).await.expect("alice gives up");
    until("the views are taken away", || async { detached.load(Ordering::SeqCst) == 1 }).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(detached.load(Ordering::SeqCst), 1, "once");
}

/// Alice as an older app (media version 0): her own connection, her offer through the core as
/// the WebView sends it, and Bob's answer applied by hand, as her WebView would.
async fn older_app_calls(alice: &Phone, bob: &Phone, video: bool) -> (String, MediaSession) {
    let mut alice_events = alice.core.events();
    let session = MediaSession::open(&alice.network.media_config()).await.expect("alice's connection");
    let call = alice.core.place_call(&bob.id(), video).await.expect("alice calls like an older app");
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

// An older app's voice call has no video line: the camera button is disabled and our camera
// cannot turn on. Passes against the stubs too.
#[tokio::test(flavor = "multi_thread")]
async fn an_older_app_s_voice_call_has_no_camera() {
    let (alice, bob) = two_phones().await;
    let (call, session) = older_app_calls(&alice, &bob, false).await;
    assert!(bob.core.set_call_camera(&call, true).await.is_err(), "no video with an older app's voice call");
    assert!(!bob.video(&call).available);
    assert!(bob.core.request_call_video().await.is_err(), "nor from the system's screen");
    assert!(bob.core.current_call().await.unwrap().expect("going on").video_state.is_some_and(|state| !state.available && !state.camera));
    bob.core.end_call(&call, false).await.unwrap();
    session.close().await;
}

// Our camera, kept before the answer, stays off when the answer is an older app's voice call: it
// has no video line to send it on.
#[tokio::test(flavor = "multi_thread")]
async fn a_camera_kept_before_an_older_app_answers_a_voice_call_stays_off() {
    let (alice, bob) = two_phones().await;
    let mut alice_events = alice.core.events();
    let call = alice.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.expect("alice calls");
    ringing_call(&bob).await;
    assert!(alice.core.set_call_camera(&call, true).await.expect("kept").camera);

    // Bob answers as an older app's WebView would.
    let offer = bob.core.current_call().await.unwrap().and_then(|call| call.offer).expect("alice's offer");
    let session = MediaSession::open(&bob.network.media_config()).await.expect("bob's connection");
    let answer = session.answer(&offer).await.expect("his answer");
    bob.core.answer_call(&call, &answer).await.expect("his WebView answers");
    next_update(&mut alice_events, &call, |update| *update == CallUpdate::Connected).await;
    until("alice's video is connected", || async { alice.core.current_call().await.unwrap().is_some_and(|c| c.phase == CallPhase::Active) }).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let state = alice.video(&call);
    assert!(!state.camera && !state.available, "{state:?}");
    assert!(!alice.video.camera().running);
    alice.core.end_call(&call, false).await.unwrap();
    session.close().await;
}

// An older app's video call sends its camera from the start and
// never says so: its picture is shown at once, our camera turns on as for any video call.
#[tokio::test(flavor = "multi_thread")]
async fn an_older_app_s_video_call_has_video_from_the_start() {
    let (alice, bob) = two_phones().await;
    let mut bob_events = bob.core.events();
    let (call, session) = older_app_calls(&alice, &bob, true).await;
    next_update(&mut bob_events, &call, |update| *update == CallUpdate::Connected).await;
    let state = next_video(&mut bob_events, &call, |state| state.remote && state.camera).await;
    assert!(state.available && !state.remote_paused, "{state:?}");
    until("bob's camera runs", || async { bob.video.camera().running }).await;
    // Turning it off works on this side (the older app keeps the last picture).
    assert!(!bob.core.set_call_camera(&call, false).await.expect("off").camera);
    bob.core.end_call(&call, false).await.unwrap();
    session.close().await;
}
