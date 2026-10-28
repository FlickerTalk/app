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
