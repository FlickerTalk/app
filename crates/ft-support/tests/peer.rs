//! The support contact against a user's phone: two complete cores, a fake router in memory and
//! real WebRTC on loopback (as `ft-core`'s `tests/native_video.rs`). The user's phone adds the
//! support contact's card, as the app does with the one it carries, and writes, sends files and
//! calls; the support contact answers by itself.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use async_trait::async_trait;
use ft_core::net::{Network, Relay};
use ft_core::Core;
use ft_media::testing::{test_voice, DeviceProbe, ToneDevice};
use ft_media::{Activation, AudioPlatform, CallRouting, VideoDevices, VideoPlatform};
use ft_push::RouterEvent;
use ft_storage::{MessageState, Store};
use ft_support::clips::{SAMPLE_FILE, VOICE_MESSAGE};
use ft_support::limit::Limits;
use ft_support::peer::{Settings, Support};
use ft_webrtc::SessionConfig;
use tokio::sync::mpsc;
use webrtc_engine::video::fake::FakeSource;
use webrtc_engine::video::{EncodedFrame, VideoError, VideoSink};

#[derive(Default)]
struct Bus {
    online: Mutex<HashMap<String, mpsc::UnboundedSender<RouterEvent>>>,
    capabilities: Mutex<HashMap<String, [u8; 32]>>,
}

struct FakeRelay {
    bus: Arc<Bus>,
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
        Ok(Vec::new())
    }

    async fn acknowledge(&self, _id: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

/// What the user's display got of the support contact's video.
#[derive(Clone, Default)]
struct Screen(Arc<Mutex<Vec<EncodedFrame>>>);

impl Screen {
    fn frames(&self) -> Vec<EncodedFrame> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

impl VideoSink for Screen {
    fn start(&mut self) -> Result<(), VideoError> {
        Ok(())
    }

    fn push(&mut self, frame: EncodedFrame) -> Result<(), VideoError> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).push(frame);
        Ok(())
    }

    fn stop(&mut self) -> Result<(), VideoError> {
        Ok(())
    }
}

struct Phone {
    core: Arc<Core>,
    _files: tempfile::TempDir,
    heard: DeviceProbe,
    screen: Screen,
}

impl Phone {
    fn id(&self) -> String {
        self.core.device_id().as_str().to_owned()
    }

    /// What the other side said to this phone, oldest first.
    async fn received(&self, from: &str) -> Vec<ft_storage::Message> {
        let mut received: Vec<_> = self.core.store().messages(from, 100).await.unwrap().into_iter().filter(|message| !message.outgoing).collect();
        received.sort_by_key(|message| message.received_at);
        received
    }

    async fn texts_from(&self, from: &str) -> Vec<String> {
        let mut texts = Vec::new();
        for message in self.received(from).await {
            if self.core.store().file(&message.message_id).await.unwrap().is_none() {
                texts.push(message.body);
            }
        }
        texts
    }
}

async fn phone(bus: &Arc<Bus>, name: &str) -> Phone {
    let relay = Arc::new(FakeRelay { bus: bus.clone() });
    let network = Network::new(relay, SessionConfig::offline());
    let core = Core::open(Store::open_in_memory().await.unwrap(), [5; 32], network.clone()).await.expect("opens");
    core.set_name(name).await.unwrap();
    network.attach(&core);
    let id = core.device_id().as_str().to_owned();
    bus.capabilities.lock().unwrap().insert(id.clone(), *core.route_capability().as_bytes());
    let (events, mut incoming) = mpsc::unbounded_channel();
    bus.online.lock().unwrap().insert(id, events.clone());
    tokio::spawn(async move {
        while let Some(event) = incoming.recv().await {
            network.handle(event).await;
        }
    });
    let _ = events.send(RouterEvent::Connected { stun: vec![], turn: None });
    let files = tempfile::tempdir().unwrap();
    Phone { core, _files: files, heard: DeviceProbe::default(), screen: Screen::default() }
}

/// A user's phone: a voice and a camera of the tests, a display that keeps what it is shown.
async fn user(bus: &Arc<Bus>) -> Phone {
    let phone = phone(bus, "Reviewer").await;
    phone.core.set_files_dir(phone._files.path().to_owned());
    phone.core.set_call_audio(Some(AudioPlatform {
        backend: ToneDevice::factory(test_voice(30, false), phone.heard.clone()),
        activation: Activation::Immediate,
    }));
    let screen = phone.screen.clone();
    phone.core.set_call_video(Some(VideoPlatform {
        devices: Arc::new(move || Ok(VideoDevices { source: Box::new(FakeSource::new()), sink: Box::new(screen.clone()) })),
    }));
    phone
}

fn quick() -> Settings {
    Settings {
        answer_after: Duration::ZERO,
        scan_every: Duration::from_millis(200),
        tidy_every: Duration::from_secs(3_600),
        limits: Limits { gap: Duration::from_millis(200), per_day: 200 },
        ..Settings::default()
    }
}

/// The support contact running on a router, and a user's phone that has its card.
struct World {
    bus: Arc<Bus>,
    support: Phone,
    running: Arc<Support>,
    user: Phone,
}

impl World {
    async fn new(settings: Settings) -> Self {
        let bus = Arc::new(Bus::default());
        let support = phone(&bus, "anything").await;
        Support::prepare(&support.core, support._files.path()).await.expect("prepares");
        let running = Support::new(support.core.clone(), settings, support._files.path());
        running.spawn();
        let user = Self::user_of(&bus, &support).await;
        Self { bus, support, running, user }
    }

    /// Another user with the support contact's card (the card the app carries is added without a
    /// word: nothing is said until the user writes).
    async fn another_user(&self) -> Phone {
        Self::user_of(&self.bus, &self.support).await
    }

    async fn user_of(bus: &Arc<Bus>, support: &Phone) -> Phone {
        let user = user(bus).await;
        let card = support.core.my_card().await.unwrap();
        assert_eq!(card.name(), Some("FlickerTalk"));
        user.core.add_contact(&card.to_link(), None).await.expect("adds the support contact");
        user
    }
}

async fn until<F, Fut>(what: &str, condition: F)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..400 {
        if condition().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting until {what}");
}

async fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.join(name);
    tokio::fs::write(&path, bytes).await.unwrap();
    path
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stranger_who_writes_is_accepted_welcomed_and_gets_a_thumbs_up() {
    let World { support, user, .. } = World::new(quick()).await;
    let (s, u) = (support.id(), user.id());
    let hello = user.core.send_text(&s, "hello").await.expect("sends");

    until("the welcome arrives", || async { !user.texts_from(&s).await.is_empty() }).await;
    let welcome = &user.texts_from(&s).await[0];
    assert!(welcome.contains("automatic") && welcome.contains("https://flickertalk.com/support"), "{welcome}");
    assert!(support.core.store().contact(&u).await.unwrap().expect("known").accepted, "accepted without anyone");
    until("the thumbs up arrives", || async {
        user.core.store().reactions(&s).await.unwrap().get(&hello).and_then(|reactions| reactions.theirs.clone()).as_deref() == Some("👍")
    })
    .await;
    until("hello is read", || async { user.core.store().message(&hello).await.unwrap().unwrap().state == MessageState::Read }).await;
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(user.texts_from(&s).await.len(), 1, "one welcome, and nothing for its own replies");
}

#[tokio::test(flavor = "multi_thread")]
async fn later_texts_are_acknowledged_with_their_length_and_never_echoed() {
    let World { support, user, .. } = World::new(quick()).await;
    let s = support.id();
    user.core.send_text(&s, "hello").await.unwrap();
    until("the welcome", || async { user.texts_from(&s).await.len() == 1 }).await;

    user.core.send_text(&s, "my secret is 4711").await.unwrap();
    until("the acknowledgement", || async { user.texts_from(&s).await.len() == 2 }).await;
    let answer = &user.texts_from(&s).await[1];
    assert!(answer.contains("17 characters"), "{answer}");
    assert!(!answer.contains("4711"), "{answer}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_gets_the_sample_back_and_a_voice_message_gets_a_voice_message() {
    let World { support, user, .. } = World::new(quick()).await;
    let s = support.id();
    user.core.send_text(&s, "hello").await.unwrap();
    until("the welcome", || async { user.texts_from(&s).await.len() == 1 }).await;

    let photo = write_file(user._files.path(), "photo.jpg", &[7; 70_000]).await;
    user.core.send_file(&s, &photo, "photo.jpg", "image/jpeg").await.expect("offers");
    let voice = write_file(user._files.path(), "voice-20261028-120000.m4a", &[8; 20_000]).await;
    user.core.send_file(&s, &voice, "voice-20261028-120000.m4a", "audio/mp4").await.expect("offers");

    let complete_files = || async {
        let mut files = Vec::new();
        for message in user.received(&s).await {
            if let Some(file) = user.core.store().file(&message.message_id).await.unwrap().filter(|file| file.complete) {
                files.push((file.name.clone(), file.mime.clone(), std::fs::read(user.core.file_path(&file)).unwrap()));
            }
        }
        files
    };
    until("both files come back whole", || async { complete_files().await.len() == 2 }).await;
    let files = complete_files().await;
    let sample = files.iter().find(|(name, ..)| name == "flickertalk-sample.png").expect("the sample");
    assert_eq!((sample.1.as_str(), sample.2.as_slice()), ("image/png", SAMPLE_FILE));
    let voice = files.iter().find(|(_, mime, _)| mime == "audio/mp4").expect("a voice message");
    assert!(voice.0.starts_with("voice-") && voice.0.ends_with(".m4a"), "{}", voice.0);
    assert_eq!(voice.2.as_slice(), VOICE_MESSAGE);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_day_s_replies_run_out_with_one_notice() {
    let settings = Settings { limits: Limits { gap: Duration::from_millis(100), per_day: 2 }, ..quick() };
    let World { support, user, .. } = World::new(settings).await;
    let s = support.id();
    for (n, text) in ["one", "two", "three", "four"].into_iter().enumerate() {
        user.core.send_text(&s, text).await.unwrap();
        let expected = n.min(2) + 1;
        until("the answer, or the notice", || async { user.texts_from(&s).await.len() >= expected.min(3) }).await;
    }
    tokio::time::sleep(Duration::from_millis(800)).await;
    let texts = user.texts_from(&s).await;
    assert_eq!(texts.len(), 3, "{texts:?}");
    assert!(texts[2].contains("all the automated replies for today"), "{}", texts[2]);
}

#[tokio::test(flavor = "multi_thread")]
async fn nothing_said_is_kept_for_long() {
    let settings = Settings { keep_for: Duration::from_secs(1), ..quick() };
    let World { support, running, user, .. } = World::new(settings).await;
    let (s, u) = (support.id(), user.id());
    user.core.send_text(&s, "hello").await.unwrap();
    until("the welcome", || async { user.texts_from(&s).await.len() == 1 }).await;
    until("the welcome is delivered", || async { support.core.store().outbox().await.unwrap().is_empty() }).await;

    tokio::time::sleep(Duration::from_millis(2_100)).await;
    running.tidy().await;
    assert!(support.core.store().messages(&u, 10).await.unwrap().is_empty(), "the conversation is gone from the support contact");
    assert!(support.core.store().contact(&u).await.unwrap().is_some(), "the contact stays, so the user can write again");
    assert_eq!(user.texts_from(&s).await.len(), 1, "the user keeps theirs");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_voice_call_is_answered_with_the_recording_and_ends_by_itself() {
    let settings = Settings { max_call: Duration::from_secs(4), ..quick() };
    let World { support, running, user, .. } = World::new(settings).await;
    let s = support.id();
    user.core.send_text(&s, "hello").await.unwrap();
    until("the welcome", || async { user.texts_from(&s).await.len() == 1 }).await;

    let call = user.core.start_native_call(&s, CallRouting::Auto, false).await.expect("calls");
    until("the call connects", || async {
        user.core.current_call().await.unwrap().is_some_and(|current| current.call == call && current.connected_at.is_some())
    })
    .await;
    tokio::time::sleep(Duration::from_millis(1_500)).await;
    let played = user.heard.played();
    let tail = &played[played.len().saturating_sub(48_000)..];
    let rms = (tail.iter().map(|sample| f64::from(*sample).powi(2)).sum::<f64>() / tail.len().max(1) as f64).sqrt();
    assert!(rms > 300.0, "the recording is heard: rms {rms}");

    until("the support contact hangs up", || async { user.core.current_call().await.unwrap().is_none() }).await;
    running.tidy().await;
    assert!(support.core.store().calls(10).await.unwrap().iter().all(|call| call.ended_at.is_some()));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_video_call_shows_the_clip() {
    let settings = Settings { max_call: Duration::from_secs(5), ..quick() };
    let World { support, user, .. } = World::new(settings).await;
    let s = support.id();
    user.core.send_text(&s, "hello").await.unwrap();
    until("the welcome", || async { user.texts_from(&s).await.len() == 1 }).await;

    user.core.start_native_call(&s, CallRouting::Auto, true).await.expect("calls");
    until("frames of the clip arrive", || async { user.screen.frames().len() >= 10 }).await;
    let frames = user.screen.frames();
    let first_key = frames.iter().find(|frame| frame.keyframe).expect("a keyframe");
    assert!(first_key.data.windows(5).any(|bytes| bytes == [0, 0, 0, 1, 0x67]), "with its SPS");
    until("the support contact hangs up", || async { user.core.current_call().await.unwrap().is_none() }).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_caller_while_another_call_goes_on_is_told_to_try_later() {
    let settings = Settings { max_call: Duration::from_secs(8), ..quick() };
    let world = World::new(settings).await;
    let (support, first) = (&world.support, &world.user);
    let s = support.id();
    first.core.send_text(&s, "hello").await.unwrap();
    until("the welcome", || async { first.texts_from(&s).await.len() == 1 }).await;
    first.core.start_native_call(&s, CallRouting::Auto, false).await.expect("calls");
    until("the first call connects", || async { first.core.current_call().await.unwrap().is_some_and(|current| current.connected_at.is_some()) }).await;

    let second = world.another_user().await;
    second.core.send_text(&s, "hello").await.unwrap();
    until("the second welcome", || async { second.texts_from(&s).await.len() == 1 }).await;
    let _ = second.core.start_native_call(&s, CallRouting::Auto, false).await.expect("calls");
    until("the second hears the line is busy", || async {
        second.texts_from(&s).await.iter().any(|text| text.contains("another call"))
    })
    .await;
}

// A reviewer may call before writing anything: the call's own connection brings the card, the
// stranger is accepted at once, and the call is answered, not refused as a stranger's.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_before_any_message_is_answered_too() {
    let settings = Settings { max_call: Duration::from_secs(3), ..quick() };
    let World { support, user, .. } = World::new(settings).await;
    let s = support.id();
    let call = user.core.start_native_call(&s, CallRouting::Auto, false).await.expect("calls");
    until("the call connects", || async {
        user.core.current_call().await.unwrap().is_some_and(|current| current.call == call && current.connected_at.is_some())
    })
    .await;
    assert!(support.core.store().contact(&user.id()).await.unwrap().expect("known").accepted);
    until("the support contact hangs up", || async { user.core.current_call().await.unwrap().is_none() }).await;
}
