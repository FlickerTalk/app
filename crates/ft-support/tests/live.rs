//! The support contact through the live cluster (api.flickertalk.com, its STUN and TURN), with a
//! throwaway phone that writes to it and calls it. Ignored by default:
//!
//! ```sh
//! cargo test -p ft-support --test live -- --ignored --nocapture
//! # against a support contact already running (the binary, or the cluster's):
//! FT_SUPPORT_LIVE_CARD='https://flickertalk.com/add#…' cargo test -p ft-support --test live -- --ignored --nocapture
//! ```
//!
//! Without `FT_SUPPORT_LIVE_CARD`, a throwaway support contact runs here, in a temporary folder,
//! and both identities are forgotten by the router at the end.

use std::time::Duration;

use ft_core::online::Online;
use ft_media::testing::{test_voice, DeviceProbe, ToneDevice};
use ft_media::{Activation, AudioPlatform, CallRouting};
use ft_storage::Store;
use ft_support::config::Config;
use ft_webrtc::SessionConfig;

const ROUTER: &str = "https://api.flickertalk.com";

/// A throwaway phone on the live router, with a voice of the tests for calls.
async fn throwaway_phone(heard: &DeviceProbe) -> (Online, tempfile::TempDir) {
    let key = *blake3::hash(format!("{:?}", std::time::SystemTime::now()).as_bytes()).as_bytes();
    let online = ft_core::online::start(Store::open_in_memory().await.unwrap(), key, ROUTER, SessionConfig::default()).await.expect("starts");
    online.core.set_name("Live test").await.unwrap();
    let files = tempfile::tempdir().unwrap();
    online.core.set_files_dir(files.path().to_owned());
    online.core.set_call_audio(Some(AudioPlatform { backend: ToneDevice::factory(test_voice(60, false), heard.clone()), activation: Activation::Immediate }));
    (online, files)
}

/// The support contact to talk to: the one in `FT_SUPPORT_LIVE_CARD`, or one started here.
async fn support_contact() -> (String, Option<(ft_support::Running, tempfile::TempDir)>) {
    if let Ok(card) = std::env::var("FT_SUPPORT_LIVE_CARD") {
        return (card, None);
    }
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().to_string_lossy().into_owned();
    let config = Config::from_vars(|name| (name == "FT_SUPPORT_DATA").then(|| data.clone())).unwrap();
    let running = ft_support::start(&config).await.expect("the support contact starts");
    println!("throwaway support contact: {}", running.card);
    (running.card.clone(), Some((running, dir)))
}

async fn until<F, Fut>(what: &str, seconds: u64, condition: F)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..seconds * 10 {
        if condition().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timed out waiting until {what}");
}

async fn texts_from(phone: &Online, contact: &str) -> Vec<String> {
    let mut texts = Vec::new();
    for message in phone.core.store().messages(contact, 100).await.unwrap().into_iter().filter(|message| !message.outgoing) {
        if phone.core.store().file(&message.message_id).await.unwrap().is_none() {
            texts.push(message.body);
        }
    }
    texts
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs api.flickertalk.com and the internet"]
async fn a_text_through_the_live_cluster_gets_an_automatic_reply() {
    let heard = DeviceProbe::default();
    let (support_card, local) = support_contact().await;
    let (phone, files) = throwaway_phone(&heard).await;
    tokio::time::sleep(Duration::from_secs(1)).await;

    let support = phone.core.add_contact(&support_card, None).await.expect("adds the support contact");
    assert_eq!(support.name, "FlickerTalk");
    let started = std::time::Instant::now();
    let hello = phone.core.send_text(&support.device_id, "hello from the live test").await.expect("sends");
    until("the welcome arrives", 60, || async { !texts_from(&phone, &support.device_id).await.is_empty() }).await;
    println!("welcome after {:?}: {}", started.elapsed(), texts_from(&phone, &support.device_id).await[0]);
    until("the thumbs up arrives", 30, || async {
        phone.core.store().reactions(&support.device_id).await.unwrap().get(&hello).and_then(|reactions| reactions.theirs.clone()).as_deref() == Some("👍")
    })
    .await;

    phone.core.send_text(&support.device_id, "twelve chars").await.unwrap();
    until("the acknowledgement arrives", 60, || async { texts_from(&phone, &support.device_id).await.len() >= 2 }).await;
    let answer = texts_from(&phone, &support.device_id).await.into_iter().find(|text| text.contains("12 characters"));
    println!("acknowledgement: {answer:?}");
    assert!(answer.is_some());

    let picture = files.path().join("picture.png");
    tokio::fs::write(&picture, [9u8; 30_000]).await.unwrap();
    phone.core.send_file(&support.device_id, &picture, "picture.png", "image/png").await.unwrap();
    until("the sample file comes back whole", 90, || async {
        for message in phone.core.store().messages(&support.device_id, 100).await.unwrap().into_iter().filter(|message| !message.outgoing) {
            if phone.core.store().file(&message.message_id).await.unwrap().is_some_and(|file| file.complete && file.name == "flickertalk-sample.png") {
                return true;
            }
        }
        false
    })
    .await;
    println!("sample file back after {:?}", started.elapsed());

    phone.router.forget().await.ok();
    if let Some((running, _dir)) = local {
        running.online.router.forget().await.ok();
        running.stop().await;
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs api.flickertalk.com and the internet"]
async fn a_call_through_the_live_cluster_is_answered_with_the_recording() {
    let heard = DeviceProbe::default();
    let (support_card, local) = support_contact().await;
    let (phone, _files) = throwaway_phone(&heard).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let support = phone.core.add_contact(&support_card, None).await.expect("adds the support contact");
    phone.core.send_text(&support.device_id, "hello, I am going to call").await.unwrap();
    until("the welcome arrives", 60, || async { !texts_from(&phone, &support.device_id).await.is_empty() }).await;

    let call = phone.core.start_native_call(&support.device_id, CallRouting::Auto, false).await.expect("calls");
    until("the call connects", 60, || async {
        phone.core.current_call().await.unwrap().is_some_and(|current| current.call == call && current.connected_at.is_some())
    })
    .await;
    tokio::time::sleep(Duration::from_secs(4)).await;
    let played = heard.played();
    let tail = &played[played.len().saturating_sub(96_000)..];
    let rms = (tail.iter().map(|sample| f64::from(*sample).powi(2)).sum::<f64>() / tail.len().max(1) as f64).sqrt();
    println!("heard the recording at rms {rms:.0}");
    assert!(rms > 300.0);
    phone.core.end_current_call().await.unwrap();

    phone.router.forget().await.ok();
    if let Some((running, _dir)) = local {
        running.online.router.forget().await.ok();
        running.stop().await;
    }
}

/// What the throwaway phone's display got.
#[derive(Clone, Default)]
struct Screen(std::sync::Arc<std::sync::Mutex<Vec<webrtc_engine::video::EncodedFrame>>>);

impl webrtc_engine::video::VideoSink for Screen {
    fn start(&mut self) -> Result<(), webrtc_engine::video::VideoError> {
        Ok(())
    }

    fn push(&mut self, frame: webrtc_engine::video::EncodedFrame) -> Result<(), webrtc_engine::video::VideoError> {
        self.0.lock().unwrap().push(frame);
        Ok(())
    }

    fn stop(&mut self) -> Result<(), webrtc_engine::video::VideoError> {
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs api.flickertalk.com and the internet"]
async fn a_video_call_through_the_live_cluster_shows_the_clip() {
    let heard = DeviceProbe::default();
    let (support_card, local) = support_contact().await;
    let (phone, _files) = throwaway_phone(&heard).await;
    let screen = Screen::default();
    let display = screen.clone();
    phone.core.set_call_video(Some(ft_media::VideoPlatform {
        devices: std::sync::Arc::new(move || {
            Ok(ft_media::VideoDevices { source: Box::new(webrtc_engine::video::fake::FakeSource::new()), sink: Box::new(display.clone()) })
        }),
    }));
    tokio::time::sleep(Duration::from_secs(1)).await;
    let support = phone.core.add_contact(&support_card, None).await.expect("adds the support contact");
    phone.core.send_text(&support.device_id, "hello, a video call is coming").await.unwrap();
    until("the welcome arrives", 60, || async { !texts_from(&phone, &support.device_id).await.is_empty() }).await;

    phone.core.start_native_call(&support.device_id, CallRouting::Auto, true).await.expect("calls");
    until("a second of the clip arrives", 60, || async { screen.0.lock().unwrap().len() >= 15 }).await;
    let frames = screen.0.lock().unwrap().clone();
    println!("video: {} frames, {} keyframes", frames.len(), frames.iter().filter(|frame| frame.keyframe).count());
    assert!(frames.iter().any(|frame| frame.keyframe));
    phone.core.end_current_call().await.unwrap();

    phone.router.forget().await.ok();
    if let Some((running, _dir)) = local {
        running.online.router.forget().await.ok();
        running.stop().await;
    }
}

/// Candidates in a description, by kind: "host 2, srflx 1, relay 1", and the addresses of the
/// non-host ones.
fn candidate_summary(sdp: &str) -> String {
    let mut counts = std::collections::BTreeMap::new();
    let mut addresses = Vec::new();
    for line in sdp.lines().filter(|line| line.starts_with("a=candidate:")) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let kind = fields.iter().position(|field| *field == "typ").and_then(|at| fields.get(at + 1)).copied().unwrap_or("?");
        *counts.entry(kind.to_owned()).or_insert(0) += 1;
        if kind != "host" {
            addresses.push(format!("{kind} {}", fields.get(4).copied().unwrap_or("?")));
        }
    }
    let counts: Vec<String> = counts.iter().map(|(kind, count)| format!("{kind} {count}")).collect();
    format!("{} [{}]", counts.join(", "), addresses.join("; "))
}

/// Several calls in a row from one phone, as a reviewer would make them, each given the app's
/// 30 seconds to connect: how long each took to be answered and to connect, and what the support
/// contact's answer offered. `FT_SUPPORT_LIVE_CALLS` calls (3 by default).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs api.flickertalk.com and the internet"]
async fn calls_in_a_row_through_the_live_cluster_connect_within_the_app_s_limit() {
    let calls: usize = std::env::var("FT_SUPPORT_LIVE_CALLS").ok().and_then(|calls| calls.parse().ok()).unwrap_or(3);
    let heard = DeviceProbe::default();
    let (support_card, local) = support_contact().await;
    let (phone, _files) = throwaway_phone(&heard).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let support = phone.core.add_contact(&support_card, None).await.expect("adds the support contact");
    phone.core.send_text(&support.device_id, "hello, calls are coming").await.unwrap();
    until("the welcome arrives", 60, || async { !texts_from(&phone, &support.device_id).await.is_empty() }).await;

    let mut connected = 0;
    for n in 1..=calls {
        let mut events = phone.core.events();
        let started = std::time::Instant::now();
        let call = phone.core.start_native_call(&support.device_id, CallRouting::Auto, false).await.expect("calls");
        let (mut answered, mut answer, mut up, mut ended) = (None, String::new(), None, None);
        let limit = tokio::time::Instant::now() + Duration::from_secs(30);
        while up.is_none() && ended.is_none() {
            match tokio::time::timeout_at(limit, events.recv()).await {
                Ok(Ok(ft_core::Event::Call { call: id, update, .. })) if id == call => match update {
                    ft_core::CallUpdate::Answered { sdp } => {
                        answered = Some(started.elapsed());
                        answer = candidate_summary(&sdp);
                    }
                    ft_core::CallUpdate::Connected => up = Some(started.elapsed()),
                    ft_core::CallUpdate::Ended { outcome } => ended = Some(format!("{outcome:?}")),
                    _ => {}
                },
                Ok(_) => {}
                Err(_) => break,
            }
        }
        let timings = phone.core.call_timings().map(|timings| timings.line()).unwrap_or_default();
        println!(
            "call {n}: answered {answered:?}, connected {up:?}, ended {ended:?}; answer: {answer}\n  timings: {timings}"
        );
        connected += usize::from(up.is_some());
        if phone.core.current_call().await.unwrap().is_some() {
            phone.core.end_current_call().await.unwrap();
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    println!("{connected} of {calls} calls connected within 30 s");

    phone.router.forget().await.ok();
    if let Some((running, _dir)) = local {
        running.online.router.forget().await.ok();
        running.stop().await;
    }
    assert_eq!(connected, calls);
}

/// The support contact renews its TURN user every minute (`ft_support::turn`): the router hands a
/// fresh one to a registered device, a different one each time.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs api.flickertalk.com and the internet"]
async fn the_live_router_hands_out_fresh_turn_users() {
    let heard = DeviceProbe::default();
    let (phone, _files) = throwaway_phone(&heard).await;
    let mut first = None;
    for _ in 0..50 {
        if let Ok(grant) = phone.router.turn_credentials().await {
            first = Some(grant);
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let first = first.expect("a TURN user once registered");
    let second = phone.router.turn_credentials().await.expect("another");
    println!("TURN users for {:?}", first.urls);
    assert!(!first.urls.is_empty());
    assert_ne!(first.username, second.username, "a new user each time");
    phone.router.forget().await.ok();
}

/// Two phones call at once: one call is taken, and the other phone hears the line is busy, in the
/// call (it ends as busy) and in a text.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs api.flickertalk.com and the internet"]
async fn two_callers_at_once_through_the_live_cluster() {
    let (support_card, local) = support_contact().await;
    let (heard_a, heard_b) = (DeviceProbe::default(), DeviceProbe::default());
    let ((a, _files_a), (b, _files_b)) = (throwaway_phone(&heard_a).await, throwaway_phone(&heard_b).await);
    tokio::time::sleep(Duration::from_secs(1)).await;
    let support = a.core.add_contact(&support_card, None).await.unwrap();
    b.core.add_contact(&support_card, None).await.unwrap();
    for phone in [&a, &b] {
        phone.core.send_text(&support.device_id, "hello").await.unwrap();
    }
    for phone in [&a, &b] {
        until("the welcome arrives", 60, || async { !texts_from(phone, &support.device_id).await.is_empty() }).await;
    }
    let (mut events_a, mut events_b) = (a.core.events(), b.core.events());
    let (call_a, call_b) = tokio::join!(
        a.core.start_native_call(&support.device_id, CallRouting::Auto, false),
        b.core.start_native_call(&support.device_id, CallRouting::Auto, false)
    );
    let (call_a, call_b) = (call_a.unwrap(), call_b.unwrap());
    async fn outcome(events: &mut tokio::sync::broadcast::Receiver<ft_core::Event>, call: &str) -> String {
        let limit = tokio::time::Instant::now() + Duration::from_secs(30);
        while let Ok(Ok(event)) = tokio::time::timeout_at(limit, events.recv()).await {
            if let ft_core::Event::Call { call: id, update, .. } = event {
                if id == call {
                    match update {
                        ft_core::CallUpdate::Connected => return "connected".to_owned(),
                        ft_core::CallUpdate::Ended { outcome } => return format!("ended {outcome:?}"),
                        _ => {}
                    }
                }
            }
        }
        "nothing within 30 s".to_owned()
    }
    let (got_a, got_b) = tokio::join!(outcome(&mut events_a, &call_a), outcome(&mut events_b, &call_b));
    println!("caller A: {got_a}; caller B: {got_b}");
    tokio::time::sleep(Duration::from_secs(3)).await;
    for (name, phone) in [("A", &a), ("B", &b)] {
        let busy = texts_from(phone, &support.device_id).await.iter().any(|text| text.contains("another call"));
        println!("caller {name} got the busy text: {busy}");
    }
    for phone in [&a, &b] {
        if phone.core.current_call().await.unwrap().is_some() {
            phone.core.end_current_call().await.unwrap();
        }
        phone.router.forget().await.ok();
    }
    if let Some((running, _dir)) = local {
        running.online.router.forget().await.ok();
        running.stop().await;
    }
}
