//! Native voice calls (2026-09-28): two complete devices, a fake router in memory and real WebRTC
//! on loopback, with the voice in Rust (`ft-media`) instead of the WebView. The phones' audio
//! devices are fakes that speak a test voice in real time and record what they play.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use ft_core::net::{Network, Relay};
use ft_core::timings::CallStage::{self, *};
use ft_core::timings::CallTimings;
use ft_core::{CallPhase, CallUpdate, Core, Event};
use ft_media::testing::{broken_device, mean_heard, rms, test_voice, webview_video_offer, webview_voice_offer, DeviceProbe, ToneDevice};
use ft_media::{Activation, AudioPlatform, BackendFactory, CallRouting};
use ft_push::{RouterEvent, Signalled, TurnGrant};
use ft_storage::{CallOutcome, Store};
use ft_webrtc::SessionConfig;
use tokio::sync::{broadcast, mpsc};

/// The fake router: who is connected and their capabilities. Calls never use the mailbox.
#[derive(Default)]
struct Bus {
    online: Mutex<HashMap<String, mpsc::UnboundedSender<RouterEvent>>>,
    capabilities: Mutex<HashMap<String, [u8; 32]>>,
    /// Router 0.4.0 (2026-09-29): a signal for a device that is not connected waits for its next
    /// connection. Off: a router before 0.4, which loses it.
    retaining: std::sync::atomic::AtomicBool,
    held: Mutex<HashMap<String, Vec<Vec<u8>>>>,
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

    async fn signal_as(&self, to: &str, capability: &[u8; 32], bytes: Vec<u8>, _call: bool) -> anyhow::Result<Signalled> {
        if self.signal(to, capability, bytes.clone()).await? {
            return Ok(Signalled::Delivered);
        }
        if !self.bus.retaining.load(std::sync::atomic::Ordering::SeqCst) {
            return Ok(Signalled::NotConnected);
        }
        self.bus.held.lock().unwrap().entry(to.to_owned()).or_default().push(bytes);
        Ok(Signalled::Retained)
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
    probe: DeviceProbe,
}

impl Phone {
    fn id(&self) -> String {
        self.core.device_id().as_str().to_owned()
    }

    fn go_online(&self, bus: &Arc<Bus>) {
        self.go_online_with(bus, vec![], None);
    }

    /// Online, with the STUN and TURN servers the router's welcome names.
    fn go_online_with(&self, bus: &Arc<Bus>, stun: Vec<String>, turn: Option<TurnGrant>) {
        let (events, mut incoming) = mpsc::unbounded_channel();
        bus.online.lock().unwrap().insert(self.id(), events.clone());
        let network = self.network.clone();
        tokio::spawn(async move {
            while let Some(event) = incoming.recv().await {
                network.handle(event).await;
            }
        });
        let _ = events.send(RouterEvent::Connected { stun, turn });
        // Router 0.4.0: what waited for this device comes right after the welcome.
        for bytes in bus.held.lock().unwrap().remove(&self.id()).unwrap_or_default() {
            let _ = events.send(RouterEvent::Signal(bytes));
        }
    }
}

const VOICE_SECONDS: usize = 8;

/// A phone whose microphone speaks the test voice (`inverted` for the other side).
async fn phone(bus: &Arc<Bus>, name: &str, inverted: bool, activation: Activation) -> Phone {
    let relay = Arc::new(FakeRelay { bus: bus.clone(), me: OnceLock::new() });
    let network = Network::new(relay.clone(), SessionConfig::offline());
    let core = Core::open(Store::open_in_memory().await.unwrap(), [4; 32], network.clone()).await.expect("opens");
    core.set_name(name).await.unwrap();
    relay.me.set(core.device_id().as_str().to_owned()).unwrap();
    network.attach(&core);
    bus.capabilities.lock().unwrap().insert(core.device_id().as_str().to_owned(), *core.route_capability().as_bytes());
    let probe = DeviceProbe::default();
    core.set_call_audio(Some(AudioPlatform {
        backend: ToneDevice::factory(test_voice(VOICE_SECONDS, inverted), probe.clone()),
        activation,
    }));
    Phone { core, network, probe }
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

async fn two_phones(bob_activation: Activation) -> (Phone, Phone) {
    let bus = Arc::new(Bus::default());
    let alice = phone(&bus, "Alice", false, Activation::Immediate).await;
    let bob = phone(&bus, "Bob", true, bob_activation).await;
    alice.go_online(&bus);
    bob.go_online(&bus);
    pair(&alice, &bob).await;
    (alice, bob)
}

/// The next update of `call` that `wanted` accepts.
async fn next_update(events: &mut broadcast::Receiver<Event>, call: &str, wanted: fn(&CallUpdate) -> bool) -> CallUpdate {
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

#[tokio::test(flavor = "multi_thread")]
async fn a_native_voice_call_carries_each_voice_to_the_other_side() {
    let (alice, bob) = two_phones(Activation::Immediate).await;
    let (mut alice_events, mut bob_events) = (alice.core.events(), bob.core.events());

    let call = alice.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.expect("alice calls");
    assert_eq!(ringing_call(&bob).await, call);
    // A WebView that comes up late finds the ringing call and its offer in the core.
    let ringing = bob.core.current_call().await.unwrap().expect("ringing");
    assert!(!ringing.outgoing && !ringing.video);
    assert!(ringing.offer.as_deref().is_some_and(|sdp| sdp.contains("m=audio ")), "{ringing:?}");
    assert_eq!(alice.core.current_call().await.unwrap().expect("calling").phase, CallPhase::Calling);
    assert_eq!(alice.probe.starts(), 0, "nothing plays before an answer");

    bob.core.answer_native_call(&call, CallRouting::Auto).await.expect("bob answers");
    next_update(&mut alice_events, &call, |update| *update == CallUpdate::Connected).await;
    next_update(&mut bob_events, &call, |update| *update == CallUpdate::Connected).await;
    until("both devices run", || async { alice.probe.running() && bob.probe.running() }).await;
    let active = bob.core.current_call().await.unwrap().expect("active");
    assert_eq!(active.phase, CallPhase::Active);
    assert!(active.native && active.connected_at.is_some());

    tokio::time::sleep(Duration::from_millis(3_500)).await;

    // Muted, Bob hears silence from Alice; the UI hears it too (CallKit may have done it).
    alice.core.mute_call(&call, true).await.expect("mutes");
    next_update(&mut alice_events, &call, |update| *update == CallUpdate::Muted { muted: true }).await;
    assert!(alice.core.current_call().await.unwrap().expect("active").muted);
    tokio::time::sleep(Duration::from_millis(600)).await;
    let muted_from = bob.probe.played().len();
    tokio::time::sleep(Duration::from_millis(1_200)).await;

    alice.core.end_call(&call, false).await.expect("alice hangs up");
    until("both devices stop", || async { !alice.probe.running() && !bob.probe.running() }).await;
    until("bob's call ends", || async { bob.core.current_call().await.unwrap().is_none() }).await;
    assert!(alice.core.current_call().await.unwrap().is_none());
    let record = bob.core.store().call(&call).await.unwrap().expect("in the history");
    assert_eq!(record.outcome, Some(CallOutcome::Answered));

    let (alice_played, bob_played) = (alice.probe.played(), bob.probe.played());
    let bob_heard = mean_heard(&test_voice(VOICE_SECONDS, false), &bob_played, 48_000, 12_000, 8);
    let alice_heard = mean_heard(&test_voice(VOICE_SECONDS, true), &alice_played, 48_000, 12_000, 8);
    assert!(bob_heard > 0.7, "bob heard alice at {bob_heard}");
    assert!(alice_heard > 0.7, "alice heard bob at {alice_heard}");
    let loud = rms(&bob_played[48_000..96_000]);
    let quiet = rms(&bob_played[muted_from..]);
    eprintln!("bob heard alice at {bob_heard:.3}, alice heard bob at {alice_heard:.3}; rms {loud:.0} before muting, {quiet:.1} muted");
    assert!(loud > 3_000.0 && quiet < 100.0, "before muting {loud}, muted {quiet}");
}

// iOS: the audio unit may start only once CallKit has activated the audio session.
#[tokio::test(flavor = "multi_thread")]
async fn on_ios_the_device_starts_only_after_callkit_activates_the_audio_session() {
    let (alice, bob) = two_phones(Activation::WhenSessionActive).await;
    let mut bob_events = bob.core.events();
    let call = alice.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.expect("alice calls");
    ringing_call(&bob).await;
    bob.core.answer_native_call(&call, CallRouting::Auto).await.expect("bob answers");
    next_update(&mut bob_events, &call, |update| *update == CallUpdate::Connected).await;

    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(bob.probe.starts(), 0, "connected, but CallKit has not activated the session");
    assert!(!bob.core.call_device_running().await, "the core says so too (diagnostics)");
    until("alice's device runs", || async { alice.probe.running() }).await;

    bob.core.set_call_audio_active(true).await.expect("activated");
    until("bob's device runs", || async { bob.probe.running() }).await;
    assert!(bob.core.call_device_running().await);
    bob.core.set_call_audio_active(false).await.expect("deactivated");
    assert!(!bob.probe.running());
    bob.core.set_call_audio_active(true).await.expect("activated again");
    assert!(bob.probe.running());

    // Hanging up from the callee stops both sides.
    bob.core.end_current_call().await.expect("bob hangs up");
    until("both devices stop", || async { !alice.probe.running() && !bob.probe.running() }).await;
    until("alice's call ends", || async { alice.core.current_call().await.unwrap().is_none() }).await;
}

// An older app's WebView offers video: the native side answers audio and video (native video,
// 2026-09-29: every native call has a video line).
#[tokio::test(flavor = "multi_thread")]
async fn a_webview_video_offer_is_answered_with_audio_and_video() {
    let (alice, bob) = two_phones(Activation::Immediate).await;
    let mut alice_events = alice.core.events();
    let call = alice.core.place_call(&bob.id(), true).await.expect("alice calls like a WebView");
    alice.core.offer_call_within(&call, &webview_video_offer(), Duration::from_secs(5)).await.expect("offered");
    ringing_call(&bob).await;

    bob.core.answer_native_call(&call, CallRouting::Auto).await.expect("bob answers");
    let CallUpdate::Answered { sdp } = next_update(&mut alice_events, &call, |update| matches!(update, CallUpdate::Answered { .. })).await
    else {
        unreachable!()
    };
    assert!(sdp.contains("a=rtpmap:111 opus/48000/2"), "{sdp}");
    let video: Vec<&str> = sdp.lines().filter(|line| line.starts_with("m=video")).collect();
    assert_eq!(video.len(), 1, "{sdp}");
    assert!(!video[0].starts_with("m=video 0 "), "video is answered: {sdp}");
    bob.core.end_call(&call, false).await.unwrap();
}

// An older app's WebView voice call has no video line: the native side answers the voice only
// (no video line appears that the older app never offered).
#[tokio::test(flavor = "multi_thread")]
async fn a_webview_voice_offer_is_answered_with_audio_only() {
    let (alice, bob) = two_phones(Activation::Immediate).await;
    let mut alice_events = alice.core.events();
    let call = alice.core.place_call(&bob.id(), false).await.expect("alice calls like a WebView");
    alice.core.offer_call_within(&call, &webview_voice_offer(), Duration::from_secs(5)).await.expect("offered");
    ringing_call(&bob).await;

    bob.core.answer_native_call(&call, CallRouting::Auto).await.expect("bob answers");
    let CallUpdate::Answered { sdp } = next_update(&mut alice_events, &call, |update| matches!(update, CallUpdate::Answered { .. })).await
    else {
        unreachable!()
    };
    assert!(sdp.contains("a=rtpmap:111 opus/48000/2"), "{sdp}");
    assert!(!sdp.lines().any(|line| line.starts_with("m=video")), "no video line: {sdp}");
    bob.core.end_call(&call, false).await.unwrap();
}

// CallKit's answer on a locked iPhone, with no WebView: the core answers the ringing call with the
// routing it keeps, a video call too since native video (2026-09-29).
#[tokio::test(flavor = "multi_thread")]
async fn the_os_answers_the_ringing_voice_call_without_a_webview() {
    let (alice, bob) = two_phones(Activation::Immediate).await;
    let mut alice_events = alice.core.events();
    assert!(!bob.core.answer_ringing_call().await.unwrap(), "nothing rings");
    // That answer would wait for the next offer (a video call's too): CallKit's end takes it back.
    bob.core.end_current_call().await.unwrap();

    let video = alice.core.start_native_call(&bob.id(), CallRouting::Auto, true).await.unwrap();
    ringing_call(&bob).await;
    assert!(bob.core.answer_ringing_call().await.unwrap(), "a video call is answered natively too");
    next_update(&mut alice_events, &video, |update| *update == CallUpdate::Connected).await;
    alice.core.end_call(&video, false).await.unwrap();
    until("the video call is over", || async { bob.core.current_call().await.unwrap().is_none() }).await;

    bob.core.set_call_routing(CallRouting::Direct).await.unwrap();
    let call = alice.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.unwrap();
    ringing_call(&bob).await;
    let mut bob_events = bob.core.events();
    assert!(bob.core.answer_ringing_call().await.unwrap());
    next_update(&mut alice_events, &call, |update| *update == CallUpdate::Connected).await;
    // CallKit's mute button names no call: it is the one going on.
    bob.core.mute_current_call(true).await.expect("mutes");
    next_update(&mut bob_events, &call, |update| *update == CallUpdate::Muted { muted: true }).await;
    // The WebView may answer the same call once it is up: nothing changes.
    bob.core.answer_native_call(&call, CallRouting::Auto).await.expect("answering twice is fine");
    alice.core.end_call(&call, false).await.unwrap();
}

// The routing chosen in Settings lives in the core too: CallKit answers with no WebView at all.
#[tokio::test(flavor = "multi_thread")]
async fn the_core_keeps_the_call_routing() {
    let bus = Arc::new(Bus::default());
    let alice = phone(&bus, "Alice", false, Activation::Immediate).await;
    assert_eq!(alice.core.call_routing().await, CallRouting::Auto);
    alice.core.set_call_routing(CallRouting::Always).await.unwrap();
    assert_eq!(alice.core.call_routing().await, CallRouting::Always);
    assert!(alice.core.native_calls());
    alice.core.set_call_audio(None);
    assert!(!alice.core.native_calls(), "without a device, calls stay on the WebView");
}

// Bug of 2026-09-28: a suspended iPhone rang through PushKit and was answered on the lock screen
// while its socket to the router was still being opened again, so the offer came after the
// answer. The answer waits for it: when the voice call's offer arrives, it is answered at once.
#[tokio::test(flavor = "multi_thread")]
async fn an_answer_that_comes_before_the_offer_answers_it_when_it_arrives() {
    let (alice, bob) = two_phones(Activation::Immediate).await;
    let mut alice_events = alice.core.events();
    assert!(!bob.core.answer_ringing_call().await.unwrap(), "nothing rings yet: the answer waits");

    let call = alice.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.expect("alice calls");
    next_update(&mut alice_events, &call, |update| *update == CallUpdate::Connected).await;
    until("both devices run", || async { alice.probe.running() && bob.probe.running() }).await;
    alice.core.end_call(&call, false).await.unwrap();
}

// The waiting answer is short-lived, and the system's hang-up takes it back: a later call just
// rings.
#[tokio::test(flavor = "multi_thread")]
async fn an_early_answer_is_forgotten_after_its_window_or_a_hang_up() {
    let (alice, bob) = two_phones(Activation::Immediate).await;
    assert!(!bob.core.answer_ringing_call_within(Duration::from_millis(300)).await.unwrap());
    tokio::time::sleep(Duration::from_millis(600)).await;
    let late = alice.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.unwrap();
    ringing_call(&bob).await;
    tokio::time::sleep(Duration::from_millis(1_500)).await;
    assert_eq!(bob.core.current_call().await.unwrap().expect("rings").phase, CallPhase::Ringing, "it just rings");
    alice.core.end_call(&late, false).await.unwrap();
    until("the call is over", || async { bob.core.current_call().await.unwrap().is_none() }).await;

    assert!(!bob.core.answer_ringing_call().await.unwrap());
    bob.core.end_current_call().await.expect("CallKit's end, with nothing going on");
    let declined = alice.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.unwrap();
    ringing_call(&bob).await;
    tokio::time::sleep(Duration::from_millis(1_500)).await;
    assert_eq!(bob.core.current_call().await.unwrap().expect("rings").phase, CallPhase::Ringing, "the hang-up took the answer back");
    alice.core.end_call(&declined, false).await.unwrap();
}

// The phone's own call screen rang from a push and the user answered (or declined) it before the
// offer came; the call turned out to be one this phone refuses without a trace (here: Calls off).
// What the user said was about that call: it must not answer or decline the next one.
#[tokio::test(flavor = "multi_thread")]
async fn an_early_answer_goes_with_a_call_refused_without_a_trace() {
    let bus = Arc::new(Bus::default());
    let alice = phone(&bus, "Alice", false, Activation::Immediate).await;
    let bob = phone(&bus, "Bob", true, Activation::Immediate).await;
    let carol = phone(&bus, "Carol", false, Activation::Immediate).await;
    for one in [&alice, &bob, &carol] {
        one.go_online(&bus);
    }
    pair(&alice, &bob).await;
    pair(&carol, &bob).await;
    let rules = ft_storage::ContactRules { accepts_calls: false, ..ft_storage::ContactRules::default() };
    bob.core.set_rules(&carol.id(), rules).await.expect("calls off for carol");

    for early in ["answer", "decline"] {
        let mut bob_events = bob.core.events();
        if early == "answer" {
            assert!(!bob.core.answer_ringing_call().await.unwrap(), "nothing rings yet: the answer waits");
        } else {
            bob.core.decline_ringing_call().await.expect("nothing rings yet: the decline waits");
        }
        let refused = carol.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.expect("carol calls");
        let heard = async {
            while !matches!(bob_events.recv().await, Ok(Event::CallRefused)) {}
        };
        tokio::time::timeout(Duration::from_secs(10), heard).await.expect("refused");
        let _ = carol.core.end_call(&refused, false).await;

        let call = alice.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.expect("alice calls");
        ringing_call(&bob).await;
        tokio::time::sleep(Duration::from_millis(1_500)).await;
        let shown = bob.core.current_call().await.unwrap().expect("alice's call is there");
        assert_eq!(shown.phase, CallPhase::Ringing, "the early {early} went with the refused call");
        alice.core.end_call(&call, false).await.unwrap();
        until("the call is over", || async { bob.core.current_call().await.unwrap().is_none() }).await;
    }
}

// Bug of 2026-09-29 (QA on Android emulators, the owner on the iPhone): with the app closed, the
// user answered on the phone's own screen before the offer arrived, and the call then rang again
// in the app, asking for a second answer. A call answered early never shows as ringing: from the
// moment the UI can hear of it, it is connecting.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_answered_before_its_offer_never_shows_as_ringing() {
    let (alice, bob) = two_phones(Activation::Immediate).await;
    let (mut alice_events, mut bob_events) = (alice.core.events(), bob.core.events());
    assert!(!bob.core.answer_ringing_call().await.unwrap(), "nothing rings yet: the answer waits");

    let call = alice.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.expect("alice calls");
    next_update(&mut bob_events, &call, |update| matches!(update, CallUpdate::Incoming { .. })).await;
    let shown = bob.core.current_call().await.unwrap().expect("the call");
    assert_ne!(shown.phase, CallPhase::Ringing, "answered already: {shown:?}");
    next_update(&mut alice_events, &call, |update| *update == CallUpdate::Connected).await;
    alice.core.end_call(&call, false).await.unwrap();
}

// A decline that comes before the offer (the notification's decline, or CallKit's, with the app
// closed) declines the call when its offer arrives: the phone never rings again, and the caller
// hears that it was declined.
#[tokio::test(flavor = "multi_thread")]
async fn a_decline_before_the_offer_declines_the_call_when_it_arrives() {
    let (alice, bob) = two_phones(Activation::Immediate).await;
    let (mut alice_events, mut bob_events) = (alice.core.events(), bob.core.events());
    bob.core.decline_ringing_call().await.expect("nothing rings yet: the decline waits");

    let call = alice.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.expect("alice calls");
    let declined = CallUpdate::Ended { outcome: CallOutcome::Declined };
    assert_eq!(next_update(&mut alice_events, &call, |update| matches!(update, CallUpdate::Ended { .. })).await, declined);
    assert_eq!(next_update(&mut bob_events, &call, |_| true).await, declined, "it never rang");
    assert!(bob.core.current_call().await.unwrap().is_none());
    assert_eq!(bob.core.store().call(&call).await.unwrap().expect("in the history").outcome, Some(CallOutcome::Declined));
}

// The user's last word before the offer is the one that counts (a decline, then an answer on a
// new call's screen: answered), and a waiting decline is short-lived too.
#[tokio::test(flavor = "multi_thread")]
async fn the_last_early_choice_wins_and_a_decline_is_forgotten_after_its_window() {
    let (alice, bob) = two_phones(Activation::Immediate).await;
    let mut alice_events = alice.core.events();
    bob.core.decline_ringing_call().await.unwrap();
    assert!(!bob.core.answer_ringing_call().await.unwrap());
    let answered = alice.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.unwrap();
    next_update(&mut alice_events, &answered, |update| *update == CallUpdate::Connected).await;
    alice.core.end_call(&answered, false).await.unwrap();
    until("the call is over", || async { bob.core.current_call().await.unwrap().is_none() }).await;

    assert!(!bob.core.answer_ringing_call().await.unwrap());
    bob.core.decline_ringing_call().await.unwrap();
    let declined = alice.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.unwrap();
    let ended = next_update(&mut alice_events, &declined, |update| matches!(update, CallUpdate::Ended { .. })).await;
    assert_eq!(ended, CallUpdate::Ended { outcome: CallOutcome::Declined }, "answered, then hung up before the offer");

    bob.core.decline_ringing_call_within(Duration::from_millis(300)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(600)).await;
    let late = alice.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.unwrap();
    assert_eq!(ringing_call(&bob).await, late, "the decline's window is over: it just rings");
    alice.core.end_call(&late, false).await.unwrap();
}

// Bug of 2026-09-29 (the iPhone, with the app on the screen): CallKit answered the ringing call,
// but for the seconds the answer took to build (ICE), the core still said "ringing", so the app
// kept ringing and asked again. From the moment a ringing call is answered, by any path, it is
// connecting, and the UI hears so (`Answering`) before the media connects. Answering again
// meanwhile does nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_ringing_call_is_connecting_from_the_moment_it_is_answered() {
    let (alice, bob) = two_phones(Activation::Immediate).await;
    let mut bob_events = bob.core.events();
    let call = alice.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.unwrap();
    ringing_call(&bob).await;

    let core = bob.core.clone();
    let answering = tokio::spawn(async move { core.answer_ringing_call().await });
    next_update(&mut bob_events, &call, |update| *update == CallUpdate::Answering).await;
    assert_ne!(bob.core.current_call().await.unwrap().expect("the call").phase, CallPhase::Ringing);
    assert!(!bob.core.answer_ringing_call().await.unwrap(), "answered already: a second answer does nothing");
    assert!(answering.await.expect("runs").expect("answers"));
    next_update(&mut bob_events, &call, |update| *update == CallUpdate::Connected).await;
    alice.core.end_call(&call, false).await.unwrap();
    until("the call is over", || async { bob.core.current_call().await.unwrap().is_none() }).await;

    // The app's own answer button says the same.
    let call = alice.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.unwrap();
    ringing_call(&bob).await;
    let core = bob.core.clone();
    let answered = call.clone();
    let answering = tokio::spawn(async move { core.answer_native_call(&answered, CallRouting::Auto).await });
    next_update(&mut bob_events, &call, |update| *update == CallUpdate::Answering).await;
    assert_ne!(bob.core.current_call().await.unwrap().expect("the call").phase, CallPhase::Ringing);
    answering.await.expect("runs").expect("answers");
    next_update(&mut bob_events, &call, |update| *update == CallUpdate::Connected).await;
    alice.core.end_call(&call, false).await.unwrap();
}

// The system's decline with the call ringing declines it at once.
#[tokio::test(flavor = "multi_thread")]
async fn the_os_declines_the_ringing_call() {
    let (alice, bob) = two_phones(Activation::Immediate).await;
    let mut alice_events = alice.core.events();
    let call = alice.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.unwrap();
    ringing_call(&bob).await;
    bob.core.decline_ringing_call().await.expect("declines");
    let ended = next_update(&mut alice_events, &call, |update| matches!(update, CallUpdate::Ended { .. })).await;
    assert_eq!(ended, CallUpdate::Ended { outcome: CallOutcome::Declined });
    assert!(bob.core.current_call().await.unwrap().is_none());
}

// Bug of 2026-09-28: the caller gave up while the other phone was asleep and out of reach, and
// its hang-up was sent once and lost: the other phone kept ringing. It keeps trying for a while,
// and arrives as soon as the other phone is back.
#[tokio::test(flavor = "multi_thread")]
async fn a_hang_up_reaches_the_other_phone_once_it_is_back() {
    let bus = Arc::new(Bus::default());
    let alice = phone(&bus, "Alice", false, Activation::Immediate).await;
    let bob = phone(&bus, "Bob", true, Activation::Immediate).await;
    alice.go_online(&bus);
    bob.go_online(&bus);
    pair(&alice, &bob).await;

    let call = alice.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.expect("alice calls");
    ringing_call(&bob).await;
    // Bob's phone is suspended: off the router, and its direct connection is gone.
    bus.online.lock().unwrap().remove(&bob.id());
    alice.network.disconnect(&bob.id()).await;
    let alice_id = alice.id();
    until("bob's connection is gone", || async { !bob.network.is_connected(&alice_id).await }).await;

    alice.core.end_call(&call, false).await.expect("alice gives up");
    tokio::time::sleep(Duration::from_millis(1_000)).await;
    assert_eq!(bob.core.current_call().await.unwrap().expect("still rings").phase, CallPhase::Ringing);

    bob.go_online(&bus);
    until("bob's phone stops ringing", || async { bob.core.current_call().await.unwrap().is_none() }).await;
    let record = bob.core.store().call(&call).await.unwrap().expect("in the history");
    assert_eq!(record.outcome, Some(CallOutcome::Missed));
}

// Each CallKit call has its generation (2026-09-28): a late deactivation of the previous call
// must not stop this call's voice.
#[tokio::test(flavor = "multi_thread")]
async fn a_late_audio_event_of_an_older_call_changes_nothing() {
    let (alice, bob) = two_phones(Activation::WhenSessionActive).await;
    let mut bob_events = bob.core.events();
    let call = alice.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.expect("alice calls");
    ringing_call(&bob).await;
    bob.core.answer_native_call(&call, CallRouting::Auto).await.expect("bob answers");
    next_update(&mut bob_events, &call, |update| *update == CallUpdate::Connected).await;

    bob.core.set_call_audio_session(true, 2).await.expect("activated");
    until("bob's device runs", || async { bob.probe.running() }).await;
    bob.core.set_call_audio_session(false, 1).await.expect("an older call's event");
    assert!(bob.probe.running(), "the older call's deactivation changes nothing");
    bob.core.set_call_audio_session(false, 2).await.expect("deactivated");
    assert!(!bob.probe.running());
    alice.core.end_call(&call, false).await.unwrap();
}

/// A device that refuses its first `failures` starts, then speaks like the others.
fn flaky(failures: usize, probe: DeviceProbe) -> BackendFactory {
    let (broken, working) = (broken_device(), ToneDevice::factory(test_voice(VOICE_SECONDS, true), probe));
    let tries = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    Arc::new(move || if tries.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < failures { broken() } else { working() })
}

async fn activated_call_with(device: BackendFactory) -> (Phone, Phone, String) {
    let (alice, bob) = two_phones(Activation::WhenSessionActive).await;
    bob.core.set_call_audio(Some(AudioPlatform { backend: device, activation: Activation::WhenSessionActive }));
    let mut bob_events = bob.core.events();
    let call = alice.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.expect("alice calls");
    ringing_call(&bob).await;
    bob.core.answer_native_call(&call, CallRouting::Auto).await.expect("bob answers");
    next_update(&mut bob_events, &call, |update| *update == CallUpdate::Connected).await;
    (alice, bob, call)
}

// Bug of 2026-09-28: the audio unit that would not start on activation was ignored, and the call
// went on in silence. It is tried once more; if it still will not start, the call fails.
#[tokio::test(flavor = "multi_thread")]
async fn an_audio_device_that_fails_to_start_is_tried_again_once() {
    let probe = DeviceProbe::default();
    let (alice, bob, call) = activated_call_with(flaky(1, probe.clone())).await;
    bob.core.set_call_audio_session(true, 1).await.expect("started on the second try");
    assert!(probe.running());
    alice.core.end_call(&call, false).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_call_whose_audio_device_never_starts_fails() {
    let (alice, bob, call) = activated_call_with(broken_device()).await;
    assert!(bob.core.set_call_audio_session(true, 1).await.is_err());
    until("alice's call ends", || async { alice.core.current_call().await.unwrap().is_none() }).await;
    assert_eq!(bob.core.store().call(&call).await.unwrap().expect("in the history").outcome, Some(CallOutcome::Failed));
}

/// Asserts the stages were all reached, in this order (the same millisecond is in order too).
fn in_order(timings: &CallTimings, stages: &[CallStage]) {
    let mut last = 0;
    for stage in stages {
        let at = timings.at(*stage).unwrap_or_else(|| panic!("{} never reached: {}", stage.name(), timings.line()));
        assert!(at >= last, "{} before the stage ahead of it: {}", stage.name(), timings.line());
        last = at;
    }
}

// Call setup timings (2026-09-29, temporary diagnostics): each side knows how long each step of
// the setup took, from its call or its offer to the first audio, and says nothing about who.
#[tokio::test(flavor = "multi_thread")]
async fn each_side_times_the_steps_of_the_call_setup() {
    let (alice, bob) = two_phones(Activation::Immediate).await;
    let (mut alice_events, mut bob_events) = (alice.core.events(), bob.core.events());
    assert!(alice.core.call_timings().is_none(), "no call yet");

    let call = alice.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.expect("alice calls");
    ringing_call(&bob).await;
    bob.core.answer_native_call(&call, CallRouting::Auto).await.expect("bob answers");
    next_update(&mut alice_events, &call, |update| *update == CallUpdate::Connected).await;
    next_update(&mut bob_events, &call, |update| *update == CallUpdate::Connected).await;
    let heard = |phone: &Phone| phone.core.call_timings().is_some_and(|timings| timings.at(CallStage::FirstAudioPacket).is_some());
    until("audio arrives on both sides", || async { heard(&alice) && heard(&bob) }).await;

    let caller = alice.core.call_timings().expect("the caller's timings");
    let callee = bob.core.call_timings().expect("the callee's timings");
    eprintln!("caller: {}", caller.line());
    eprintln!("callee: {}", callee.line());
    in_order(&caller, &[CallStarted, ConnectionBuilt, GatheringStarted, GatheringDone, OfferBuilt, OfferSent, AnswerReceived, Connected]);
    in_order(&caller, &[Connected, AudioDeviceStarted]);
    in_order(&caller, &[AnswerReceived, FirstAudioPacket]);
    in_order(&callee, &[OfferReceived, Ringing, AnswerRequested, AnswerSent, Connected, AudioDeviceStarted]);
    in_order(&callee, &[ConnectionBuilt, GatheringStarted, GatheringDone, AnswerBuilt, AnswerSent, FirstAudioPacket]);
    assert!(callee.candidates.is_some_and(|found| found.host >= 1), "{}", callee.line());
    assert!(caller.candidates.is_some_and(|found| found.host >= 1 && found.complete), "loopback completes: {}", caller.line());
    for line in [caller.line(), callee.line()] {
        assert!(!line.contains("ft_") && !line.contains(&call) && !line.contains("127.0.0.1"), "{line}");
    }
    alice.core.end_call(&call, false).await.unwrap();
}

/// STUN and TURN servers that never answer: local UDP sockets nobody reads (a server behind a
/// network that drops the packets, or an interface with no way out). Nothing leaves the machine.
struct Blackhole {
    _stun: std::net::UdpSocket,
    _turn: std::net::UdpSocket,
    stun: Vec<String>,
    turn: TurnGrant,
}

fn blackhole() -> Blackhole {
    let (stun, turn) = (std::net::UdpSocket::bind("127.0.0.1:0").unwrap(), std::net::UdpSocket::bind("127.0.0.1:0").unwrap());
    let (stun_port, turn_port) = (stun.local_addr().unwrap().port(), turn.local_addr().unwrap().port());
    Blackhole {
        stun: vec![format!("stun:127.0.0.1:{stun_port}")],
        turn: TurnGrant { urls: vec![format!("turn:127.0.0.1:{turn_port}?transport=udp")], username: "u".to_owned(), credential: "c".to_owned() },
        _stun: stun,
        _turn: turn,
    }
}

/// Two paired phones whose router names servers that never answer.
async fn two_phones_behind(hole: &Blackhole) -> (Phone, Phone) {
    two_phones_behind_on(&Arc::new(Bus::default()), hole).await
}

async fn two_phones_behind_on(bus: &Arc<Bus>, hole: &Blackhole) -> (Phone, Phone) {
    let bus = bus.clone();
    let alice = phone(&bus, "Alice", false, Activation::Immediate).await;
    let bob = phone(&bus, "Bob", true, Activation::Immediate).await;
    alice.go_online_with(&bus, hole.stun.clone(), Some(hole.turn.clone()));
    bob.go_online_with(&bus, hole.stun.clone(), Some(hole.turn.clone()));
    pair(&alice, &bob).await;
    (alice, bob)
}

/// One call from Alice to Bob, answered as soon as it rings: (call started → ringing, answered →
/// connected on Bob's side), with both sides' timings printed.
async fn timed_call(alice: &Phone, bob: &Phone, what: &str) -> (Duration, Duration) {
    let mut bob_events = bob.core.events();
    let started = std::time::Instant::now();
    let call = alice.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.expect("alice calls");
    next_update(&mut bob_events, &call, |update| matches!(update, CallUpdate::Incoming { .. })).await;
    let ringing = started.elapsed();
    let answered = std::time::Instant::now();
    bob.core.answer_native_call(&call, CallRouting::Auto).await.expect("bob answers");
    next_update(&mut bob_events, &call, |update| *update == CallUpdate::Connected).await;
    let connected = answered.elapsed();
    eprintln!("{what}: ringing after {} ms, connected {} ms after the answer", ringing.as_millis(), connected.as_millis());
    eprintln!("  caller: {}", alice.core.call_timings().map(|timings| timings.line()).unwrap_or_default());
    eprintln!("  callee: {}", bob.core.call_timings().map(|timings| timings.line()).unwrap_or_default());
    alice.core.end_call(&call, false).await.unwrap();
    until("the call is over", || async { bob.core.current_call().await.unwrap().is_none() }).await;
    (ringing, connected)
}

// Call setup time (2026-09-29): a STUN or TURN server that never answers (on one interface, or
// all of them) held every description for the whole gathering deadline, 3 s. An answer connects
// in well under a second and a half however the servers behave.
#[tokio::test(flavor = "multi_thread")]
async fn with_servers_that_never_answer_an_answer_still_connects_at_once() {
    let hole = blackhole();
    let (alice, bob) = two_phones_behind(&hole).await;
    let (_, connected) = timed_call(&alice, &bob, "unreachable servers, link open").await;
    assert!(connected < Duration::from_millis(1_500), "connected {} ms after the answer", connected.as_millis());
}

// The phone that was asleep (2026-09-29): the call's direct connection is opened on demand, and
// it opens while our media offer gathers, not after it.
#[tokio::test(flavor = "multi_thread")]
async fn with_servers_that_never_answer_the_direct_link_opens_while_the_offer_gathers() {
    let hole = blackhole();
    let (alice, bob) = two_phones_behind(&hole).await;
    alice.network.disconnect(&bob.id()).await;
    let alice_id = alice.id();
    until("bob's connection is gone", || async { !bob.network.is_connected(&alice_id).await }).await;
    let (ringing, _) = timed_call(&alice, &bob, "unreachable servers, link closed").await;
    let caller = alice.core.call_timings().expect("the caller's timings");
    let offered = caller.at(CallStage::LinkOffered).expect("the link was offered");
    // DataChannel setup time (2026-09-29): the link's gathering is capped as the media's is (a
    // second here, where it waited the whole 3 s), on both sides.
    assert!(offered < 1_500, "the link was offered {offered} ms after the call started: {}", caller.line());
    assert!(ringing < Duration::from_millis(3_500), "it rang after {} ms", ringing.as_millis());
}

// DataChannel setup timings (2026-09-29, temporary diagnostics): each side times its direct
// connection's steps, with the candidates its description carried, and says nothing about who.
#[tokio::test(flavor = "multi_thread")]
async fn each_side_times_the_steps_of_the_direct_connection() {
    let hole = blackhole();
    let (alice, bob) = two_phones_behind(&hole).await;
    alice.network.disconnect(&bob.id()).await;
    let alice_id = alice.id();
    until("bob's connection is gone", || async { !bob.network.is_connected(&alice_id).await }).await;
    timed_call(&alice, &bob, "link stages").await;

    let caller = alice.core.call_timings().expect("the caller's timings");
    let callee = bob.core.call_timings().expect("the callee's timings");
    in_order(&caller, &[CallStarted, LinkGatheringStarted, LinkGatheringDone, LinkOffered, LinkOfferSent, LinkAnswerReceived, LinkOpened, OfferSent]);
    in_order(&callee, &[LinkOfferReceived, LinkGatheringStarted, LinkGatheringDone, LinkAnswered, LinkOpened, OfferReceived, Ringing]);
    for (side, timings) in [("caller", &caller), ("callee", &callee)] {
        let link = timings.link.unwrap_or_else(|| panic!("the {side}'s link candidates: {}", timings.line()));
        assert!(link.host >= 1 && !link.complete, "the {side}'s link gathering was cut short: {}", timings.line());
        let line = timings.line();
        assert!(!line.contains("ft_") && !line.contains("127.0.0.1"), "{line}");
    }
}

/// Alice calls Bob behind servers that never answer (gathering takes its whole second); returns
/// the call once it has rung for `ringing`.
async fn ringing_for(alice: &Phone, bob: &Phone, ringing: Duration) -> String {
    let call = alice.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.expect("alice calls");
    assert_eq!(ringing_call(bob).await, call);
    tokio::time::sleep(ringing).await;
    call
}

// Call setup time (2026-09-29): while the phone rings, the callee prepares its answer (the
// connection, the offer taken, our candidates gathered) without sending anything; answering sends
// it at once.
#[tokio::test(flavor = "multi_thread")]
async fn an_answer_prepared_while_ringing_goes_at_once() {
    let hole = blackhole();
    let (alice, bob) = two_phones_behind(&hole).await;
    let (mut alice_events, mut bob_events) = (alice.core.events(), bob.core.events());
    let call = ringing_for(&alice, &bob, Duration::from_millis(1_500)).await;
    assert_eq!(bob.core.prepared_call().as_deref(), Some(call.as_str()), "prepared while it rings");
    // Nothing reached the caller: no answer, and it still calls.
    assert_eq!(alice.core.current_call().await.unwrap().expect("calling").phase, CallPhase::Calling);
    assert!(alice.core.call_timings().is_some_and(|timings| timings.at(CallStage::AnswerReceived).is_none()));

    let answered = std::time::Instant::now();
    bob.core.answer_native_call(&call, CallRouting::Auto).await.expect("bob answers");
    let sent = answered.elapsed();
    next_update(&mut alice_events, &call, |update| *update == CallUpdate::Connected).await;
    let connected = answered.elapsed();
    next_update(&mut bob_events, &call, |update| *update == CallUpdate::Connected).await;
    let callee = bob.core.call_timings().expect("the callee's timings");
    eprintln!("prepared: answer sent {} ms and connected {} ms after the answer; callee: {}", sent.as_millis(), connected.as_millis(), callee.line());
    in_order(&callee, &[OfferReceived, GatheringStarted, GatheringDone, AnswerRequested, AnswerBuilt, AnswerSent, Connected]);
    assert!(sent < Duration::from_millis(300), "the answer went {} ms after answering", sent.as_millis());
    assert!(connected < Duration::from_millis(800), "connected {} ms after answering", connected.as_millis());
    assert_eq!(bob.core.prepared_call(), None, "taken by the answer");
    alice.core.end_call(&call, false).await.unwrap();
}

// A call declined, given up by the caller or left to ring out closes its prepared answer; one
// answered with another routing than it was prepared with is made again with that one.
#[tokio::test(flavor = "multi_thread")]
async fn a_prepared_answer_goes_with_its_call() {
    let (alice, bob) = two_phones(Activation::Immediate).await;
    let call = ringing_for(&alice, &bob, Duration::from_millis(300)).await;
    until("prepared", || async { bob.core.prepared_call().is_some() }).await;
    bob.core.end_call(&call, false).await.expect("bob declines");
    assert_eq!(bob.core.prepared_call(), None, "declined");
    until("alice's call is over", || async { alice.core.current_call().await.unwrap().is_none() }).await;

    let call = ringing_for(&alice, &bob, Duration::from_millis(300)).await;
    until("prepared", || async { bob.core.prepared_call().is_some() }).await;
    alice.core.end_call(&call, false).await.expect("alice gives up");
    until("bob's call is over", || async { bob.core.current_call().await.unwrap().is_none() }).await;
    assert_eq!(bob.core.prepared_call(), None, "given up");

    bob.core.set_call_routing(CallRouting::Direct).await.unwrap();
    let mut alice_events = alice.core.events();
    let call = ringing_for(&alice, &bob, Duration::from_millis(300)).await;
    until("prepared", || async { bob.core.prepared_call().is_some() }).await;
    bob.core.answer_native_call(&call, CallRouting::Auto).await.expect("answered with another routing");
    next_update(&mut alice_events, &call, |update| *update == CallUpdate::Connected).await;
    let callee = bob.core.call_timings().expect("timings");
    in_order(&callee, &[AnswerRequested, GatheringStarted, AnswerSent]);
    alice.core.end_call(&call, false).await.unwrap();
}

// A prepared answer cannot wait for ever: ICE gives up 30 s after it starts checking, and a NAT
// may forget the mapping it gathered. While the phone rings it is made again every so often.
#[tokio::test(flavor = "multi_thread")]
async fn a_long_ring_keeps_its_prepared_answer_fresh() {
    let (alice, bob) = two_phones(Activation::Immediate).await;
    bob.core.set_answer_refresh(Duration::from_millis(700));
    let mut alice_events = alice.core.events();
    let call = ringing_for(&alice, &bob, Duration::from_millis(2_500)).await;
    bob.core.answer_native_call(&call, CallRouting::Auto).await.expect("bob answers");
    next_update(&mut alice_events, &call, |update| *update == CallUpdate::Connected).await;
    let callee = bob.core.call_timings().expect("timings");
    let (gathered, answered) = (callee.at(GatheringStarted).expect("gathered"), callee.at(AnswerRequested).expect("answered"));
    assert!(gathered < answered && answered - gathered < 1_200, "the answer used an old preparation: {}", callee.line());
    alice.core.end_call(&call, false).await.unwrap();
}

/// Alice calls Bob behind servers that never answer, with Bob's app closed (no router socket, no
/// direct connection); it opens 4 s later. Returns (connected to the router → ringing,
/// connected to the router → call connected), with both sides' timings printed.
async fn call_to_a_closed_app(retaining: bool) -> (Duration, Duration) {
    let hole = blackhole();
    let bus = Arc::new(Bus::default());
    bus.retaining.store(retaining, std::sync::atomic::Ordering::SeqCst);
    let (alice, bob) = two_phones_behind_on(&bus, &hole).await;
    bus.online.lock().unwrap().remove(&bob.id());
    bob.network.disconnect(&alice.id()).await;
    alice.network.disconnect(&bob.id()).await;

    let mut bob_events = bob.core.events();
    let call = alice.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.expect("alice calls");
    // Well after the first offer went (its gathering takes a second here), and halfway between
    // two of the retries a router before 0.4 needs (every 2 s): the average case.
    tokio::time::sleep(Duration::from_millis(4_000)).await;
    let back = std::time::Instant::now();
    bob.go_online_with(&bus, hole.stun.clone(), Some(hole.turn.clone()));
    next_update(&mut bob_events, &call, |update| matches!(update, CallUpdate::Incoming { .. })).await;
    let ringing = back.elapsed();
    bob.core.answer_native_call(&call, CallRouting::Auto).await.expect("bob answers");
    next_update(&mut bob_events, &call, |update| *update == CallUpdate::Connected).await;
    let connected = back.elapsed();
    let router = if retaining { "retaining router" } else { "router before 0.4" };
    eprintln!("closed app, {router}: ringing {} ms and connected {} ms after its app connected", ringing.as_millis(), connected.as_millis());
    eprintln!("  caller: {}", alice.core.call_timings().map(|timings| timings.line()).unwrap_or_default());
    eprintln!("  callee: {}", bob.core.call_timings().map(|timings| timings.line()).unwrap_or_default());
    alice.core.end_call(&call, false).await.unwrap();
    (ringing, connected)
}

// A call to a phone whose app is closed (2026-09-29): the router (0.4.0) keeps the direct
// connection's offer and wakes the phone; the offer is answered as soon as its app connects, with
// no retry round and no new gathering on the caller's side.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_to_a_closed_app_rings_as_soon_as_it_connects() {
    let (ringing, _) = call_to_a_closed_app(true).await;
    // Bob's side gathers its link answer (a second here) and the channel opens.
    assert!(ringing < Duration::from_millis(2_000), "it rang {} ms after the app connected", ringing.as_millis());
}

// Interop: a router before 0.4 loses the offer; the caller keeps trying every 2 s, as before.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_to_a_closed_app_still_rings_through_a_router_before_0_4() {
    let (ringing, _) = call_to_a_closed_app(false).await;
    assert!(ringing < Duration::from_millis(5_000), "it rang {} ms after the app connected", ringing.as_millis());
}

// The two lines of work meet in the answer (2026-09-29): `fix-bridge` (a call is connecting from
// the moment it is answered; an answer or a decline given before the offer counts when it comes)
// and `call-setup-time` (the answer is prepared while the phone rings, sending nothing). What
// follows are the orders of events neither had alone.

/// Whether anything arrives on `spy` within `wait`.
async fn hears_within(spy: &std::net::UdpSocket, wait: Duration) -> bool {
    let spy = spy.try_clone().expect("the spy");
    tokio::task::spawn_blocking(move || {
        spy.set_read_timeout(Some(wait)).expect("a timeout");
        spy.recv(&mut [0; 1500]).is_ok()
    })
    .await
    .expect("listened")
}

/// Alice calls Bob as an older app's WebView would, with every candidate of her offer pointing at
/// `spy`, which stands for the caller: whatever Bob's side sends towards the caller, an ICE check
/// included, reaches the spy. Returns the call once its offer went.
async fn call_watched_by(alice: &Phone, bob: &Phone, spy: &std::net::UdpSocket) -> String {
    let at = spy.local_addr().expect("the spy's address");
    let offer = webview_voice_offer().replace("127.0.0.1 50000", &format!("{} {}", at.ip(), at.port()));
    assert_ne!(offer, webview_voice_offer(), "the offer points at the spy");
    let call = alice.core.place_call(&bob.id(), false).await.expect("alice calls");
    alice.core.offer_call_within(&call, &offer, Duration::from_secs(5)).await.expect("offered");
    call
}

// An answer while the first preparation still gathers (it holds other answers back, to hand them
// what it makes): the UI hears at once that the call is being answered, not when the preparation
// is done. Behind servers that never answer, that preparation takes a whole second.
#[tokio::test(flavor = "multi_thread")]
async fn an_answer_while_the_first_preparation_gathers_is_heard_at_once() {
    let hole = blackhole();
    let (alice, bob) = two_phones_behind(&hole).await;
    let (mut alice_events, mut bob_events) = (alice.core.events(), bob.core.events());
    let call = alice.core.start_native_call(&bob.id(), CallRouting::Auto, false).await.expect("alice calls");
    next_update(&mut bob_events, &call, |update| matches!(update, CallUpdate::Incoming { .. })).await;

    let answered = std::time::Instant::now();
    let (core, id) = (bob.core.clone(), call.clone());
    let answering = tokio::spawn(async move { core.answer_native_call(&id, CallRouting::Auto).await });
    next_update(&mut bob_events, &call, |update| *update == CallUpdate::Answering).await;
    let heard = answered.elapsed();
    assert!(heard < Duration::from_millis(300), "the UI heard of the answer {} ms after it", heard.as_millis());
    assert_ne!(bob.core.current_call().await.unwrap().expect("the call").phase, CallPhase::Ringing);
    answering.await.expect("runs").expect("answers");
    next_update(&mut alice_events, &call, |update| *update == CallUpdate::Connected).await;
    alice.core.end_call(&call, false).await.unwrap();
}

// An answer given before the offer (CallKit on a locked iPhone): when the offer comes it is
// answered at once, with no preparation in between (there is nothing to prepare for: the answer
// exists) and never shown ringing. What reaches the caller comes after that answer.
#[tokio::test(flavor = "multi_thread")]
async fn an_early_answer_goes_as_soon_as_the_offer_comes_with_nothing_prepared() {
    let hole = blackhole();
    let (alice, bob) = two_phones_behind(&hole).await;
    let (mut alice_events, mut bob_events) = (alice.core.events(), bob.core.events());
    assert!(!bob.core.answer_ringing_call().await.unwrap(), "nothing rings yet: the answer waits");

    let spy = std::net::UdpSocket::bind("127.0.0.1:0").expect("a spy");
    let call = call_watched_by(&alice, &bob, &spy).await;
    next_update(&mut bob_events, &call, |update| matches!(update, CallUpdate::Incoming { .. })).await;
    let offered = std::time::Instant::now();
    let watching = async {
        loop {
            assert_eq!(bob.core.prepared_call(), None, "an answered call is not prepared");
            let phase = bob.core.current_call().await.unwrap().map(|call| call.phase);
            assert_ne!(phase, Some(CallPhase::Ringing), "answered already");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    };
    let answered = next_update(&mut alice_events, &call, |update| matches!(update, CallUpdate::Answered { .. }));
    tokio::select! {
        _ = answered => {}
        () = watching => unreachable!(),
    }
    let sent = offered.elapsed();
    // One gathering (a second here, the servers never answer), not a preparation and then that.
    assert!(sent < Duration::from_millis(1_600), "answered {} ms after the offer came", sent.as_millis());
    assert!(hears_within(&spy, Duration::from_millis(1_500)).await, "once answered, the checks go");
    assert_eq!(bob.core.prepared_call(), None);
    bob.core.end_call(&call, false).await.unwrap();
}

// A decline while the answer is being prepared (the first preparation, which takes a second here)
// declines at once, the preparation is thrown away when it is done, and nothing ever reached the
// caller.
#[tokio::test(flavor = "multi_thread")]
async fn a_decline_while_the_answer_is_prepared_reaches_nobody() {
    let hole = blackhole();
    let (alice, bob) = two_phones_behind(&hole).await;
    let (mut alice_events, mut bob_events) = (alice.core.events(), bob.core.events());
    let spy = std::net::UdpSocket::bind("127.0.0.1:0").expect("a spy");
    let listening = hears_within(&spy, Duration::from_millis(3_000));
    let calling = async {
        let call = call_watched_by(&alice, &bob, &spy).await;
        next_update(&mut bob_events, &call, |update| matches!(update, CallUpdate::Incoming { .. })).await;
        let declined = std::time::Instant::now();
        bob.core.decline_ringing_call().await.expect("bob declines");
        let ended = next_update(&mut alice_events, &call, |update| matches!(update, CallUpdate::Ended { .. })).await;
        assert_eq!(ended, CallUpdate::Ended { outcome: CallOutcome::Declined });
        assert!(declined.elapsed() < Duration::from_millis(500), "declined {} ms after the decline", declined.elapsed().as_millis());
        // Well after the preparation that was under way is done.
        tokio::time::sleep(Duration::from_millis(1_500)).await;
        assert_eq!(bob.core.prepared_call(), None, "thrown away");
        assert!(bob.core.current_call().await.unwrap().is_none());
    };
    let (heard, ()) = tokio::join!(listening, calling);
    assert!(!heard, "nothing reached the caller");
}

// An answer while the prepared answer is being made again (it is made every so often while the
// phone rings): the one ready goes at once, the one being made is thrown away, and nothing
// reached the caller before the answer, through every preparation.
#[tokio::test(flavor = "multi_thread")]
async fn an_answer_while_the_prepared_answer_is_made_again_takes_the_one_ready() {
    let hole = blackhole();
    let (alice, bob) = two_phones_behind(&hole).await;
    // Made again 50 ms after each one is ready: behind these servers, one is nearly always being
    // made.
    bob.core.set_answer_refresh(Duration::from_millis(50));
    let mut alice_events = alice.core.events();
    let spy = std::net::UdpSocket::bind("127.0.0.1:0").expect("a spy");
    let call = call_watched_by(&alice, &bob, &spy).await;
    assert_eq!(ringing_call(&bob).await, call);
    until("the first answer is prepared", || async { bob.core.prepared_call().is_some() }).await;
    // The next one is being made (it takes a second): the phone rings on meanwhile.
    assert!(!hears_within(&spy, Duration::from_millis(400)).await, "nothing reaches the caller while it rings");

    let answered = std::time::Instant::now();
    bob.core.answer_native_call(&call, CallRouting::Auto).await.expect("bob answers");
    next_update(&mut alice_events, &call, |update| matches!(update, CallUpdate::Answered { .. })).await;
    let sent = answered.elapsed();
    assert!(sent < Duration::from_millis(300), "the answer went {} ms after answering", sent.as_millis());
    assert!(hears_within(&spy, Duration::from_millis(1_500)).await, "once answered, the checks go");
    // The one being made is done by now, and thrown away.
    tokio::time::sleep(Duration::from_millis(1_200)).await;
    assert_eq!(bob.core.prepared_call(), None);
    bob.core.end_call(&call, false).await.unwrap();
}
