use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use anyhow::{anyhow, Result};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use webrtc_engine::audio::{audio_io, AudioBackend, AudioError, DeviceIo};
use webrtc_engine::call::{Call, CallConfig, PacketSink, PacketSource, RemoteAudio};
use webrtc_engine::rtp::{AudioPacket, AudioSender};

use crate::MediaSession;

// called on the thread that will own the device
pub type BackendFactory = Arc<dyn Fn() -> Result<Box<dyn AudioBackend>, AudioError> + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activation {
    Immediate,
    // on iOS wait for CallKit's didActivate before starting the audio unit
    WhenSessionActive,
}

#[derive(Clone)]
pub struct AudioPlatform {
    pub backend: BackendFactory,
    pub activation: Activation,
}

pub trait Outgoing: Send + Sync + 'static {
    fn send(&self, payload: &[u8]) -> impl Future<Output = Result<(), ()>> + Send;
}

impl Outgoing for AudioSender {
    async fn send(&self, payload: &[u8]) -> Result<(), ()> {
        AudioSender::send(self, payload).await.map_err(drop)
    }
}

struct Outbound<O>(Arc<O>);

impl<O: Outgoing> PacketSink for Outbound<O> {
    type Error = ();
    fn send(&mut self, payload: &[u8]) -> impl Future<Output = Result<(), ()>> + Send {
        let outgoing = self.0.clone();
        let payload = payload.to_vec();
        async move { outgoing.send(&payload).await }
    }
}

struct Inbound(Arc<Mutex<mpsc::Receiver<AudioPacket>>>);

impl PacketSource for Inbound {
    type Error = ();
    async fn recv(&mut self) -> Result<Option<AudioPacket>, ()> {
        Ok(self.0.lock().await.recv().await)
    }
}

const UPKEEP_EVERY: std::time::Duration = std::time::Duration::from_millis(100);

// its own thread, so a backend never has to cross threads
struct Device {
    stop: std::sync::mpsc::Sender<()>,
    thread: std::thread::JoinHandle<()>,
}

impl Device {
    fn start(factory: BackendFactory, io: DeviceIo) -> Result<Self, AudioError> {
        let (started, starting) = std::sync::mpsc::channel();
        let (stop, stopping) = std::sync::mpsc::channel::<()>();
        let thread = std::thread::Builder::new()
            .name("ft-call-audio".to_owned())
            .spawn(move || {
                let mut backend = match factory() {
                    Ok(backend) => backend,
                    Err(error) => {
                        let _ = started.send(Err(error));
                        return;
                    }
                };
                if let Err(error) = backend.start(io) {
                    let _ = started.send(Err(error));
                    return;
                }
                let _ = started.send(Ok(()));
                while let Err(std::sync::mpsc::RecvTimeoutError::Timeout) = stopping.recv_timeout(UPKEEP_EVERY) {
                    // Android reopens its streams here after a headset comes or goes, a failed
                    // upkeep is retried next time
                    let _ = backend.maintain();
                }
                let _ = backend.stop();
            })
            .map_err(|error| AudioError::Backend(error.to_string()))?;
        match starting.recv() {
            Ok(Ok(())) => Ok(Self { stop, thread }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => Err(AudioError::Backend("the audio thread ended".to_owned())),
        }
    }

    fn stop(self) {
        let _ = self.stop.send(());
        let _ = self.thread.join();
    }
}

struct Running {
    device: Device,
    call: Call,
}

#[derive(Default)]
struct State {
    connected: bool,
    session_active: bool,
    stopped: bool,
    running: Option<Running>,
}

const RING_FRAMES: usize = 16;

pub struct Voice<O: Outgoing = AudioSender> {
    outgoing: Arc<O>,
    incoming: Arc<Mutex<mpsc::Receiver<AudioPacket>>>,
    platform: AudioPlatform,
    muted: AtomicBool,
    state: Mutex<State>,
    forwarder: Option<JoinHandle<()>>,
    device_started: OnceLock<Instant>,
    first_packet: Arc<OnceLock<Instant>>,
}

impl<O: Outgoing> Voice<O> {
    pub fn new(outgoing: Arc<O>, incoming: mpsc::Receiver<AudioPacket>, platform: AudioPlatform) -> Self {
        Self {
            outgoing,
            incoming: Arc::new(Mutex::new(incoming)),
            platform,
            muted: AtomicBool::new(false),
            state: Mutex::new(State::default()),
            forwarder: None,
            device_started: OnceLock::new(),
            first_packet: Arc::new(OnceLock::new()),
        }
    }

    pub async fn connected(&self) -> Result<()> {
        let mut state = self.state.lock().await;
        state.connected = true;
        self.settle(&mut state).await
    }

    pub async fn set_session_active(&self, active: bool) -> Result<()> {
        let mut state = self.state.lock().await;
        state.session_active = active;
        self.settle(&mut state).await
    }

    pub async fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::SeqCst);
        if let Some(running) = &self.state.lock().await.running {
            running.call.set_muted(muted);
        }
    }

    pub fn is_muted(&self) -> bool {
        self.muted.load(Ordering::SeqCst)
    }

    pub fn device_started(&self) -> Option<Instant> {
        self.device_started.get().copied()
    }

    pub fn first_packet(&self) -> Option<Instant> {
        self.first_packet.get().copied()
    }

    pub async fn is_running(&self) -> bool {
        self.state.lock().await.running.is_some()
    }

    pub async fn stop(&self) {
        let mut state = self.state.lock().await;
        state.stopped = true;
        let _ = self.settle(&mut state).await;
        if let Some(forwarder) = &self.forwarder {
            forwarder.abort();
        }
    }

    fn may_run(&self, state: &State) -> bool {
        let activated = match self.platform.activation {
            Activation::Immediate => true,
            Activation::WhenSessionActive => state.session_active,
        };
        state.connected && activated && !state.stopped
    }

    async fn settle(&self, state: &mut State) -> Result<()> {
        match (self.may_run(state), state.running.is_some()) {
            (true, false) => {
                state.running = Some(self.start().await?);
                let _ = self.device_started.set(Instant::now());
            }
            (false, true) => {
                if let Some(running) = state.running.take() {
                    Self::halt(running).await;
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn start(&self) -> Result<Running> {
        // what came in while nothing played is stale, start the jitter buffer afresh
        {
            let mut incoming = self.incoming.lock().await;
            while incoming.try_recv().is_ok() {}
        }
        let (device_io, engine_io) = audio_io(RING_FRAMES);
        let call = Call::start(
            engine_io,
            Outbound(self.outgoing.clone()),
            Inbound(self.incoming.clone()),
            CallConfig::default(),
        )
        .map_err(|error| anyhow!("{error}"))?;
        call.set_muted(self.is_muted());
        let factory = self.platform.backend.clone();
        let device = tokio::task::spawn_blocking(move || Device::start(factory, device_io)).await?;
        match device {
            Ok(device) => Ok(Running { device, call }),
            Err(error) => {
                call.stop().await;
                Err(anyhow!("the audio device cannot start: {error}"))
            }
        }
    }

    // device first, then the pipeline
    async fn halt(running: Running) {
        let Running { device, call } = running;
        let _ = tokio::task::spawn_blocking(move || device.stop()).await;
        call.stop().await;
    }
}

impl Voice<AudioSender> {
    pub fn for_session(session: &MediaSession, platform: AudioPlatform) -> Self {
        let (arrived, incoming) = mpsc::channel(INCOMING_PACKETS);
        let mut voice = Self::new(session.sender(), incoming, platform);
        let first_packet = voice.first_packet.clone();
        if let Some(track) = session.take_remote() {
            voice.forwarder = Some(tokio::spawn(async move {
                let mut remote = RemoteAudio::new(track);
                while let Ok(Some(packet)) = remote.recv().await {
                    let _ = first_packet.set(Instant::now());
                    // full means nothing is playing yet, dropping is what a network would do
                    let _ = arrived.try_send(packet);
                }
            }));
        }
        voice
    }
}

// 1 s of audio
const INCOMING_PACKETS: usize = 50;

impl<O: Outgoing> Drop for Voice<O> {
    fn drop(&mut self) {
        if let Some(forwarder) = &self.forwarder {
            forwarder.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU16;
    use std::time::Duration;

    use webrtc_engine::FRAME_SAMPLES;

    use super::*;
    use crate::testing::{broken_device, mean_heard, rms, test_voice, DeviceProbe, ToneDevice};

    struct Wire {
        packets: mpsc::Sender<AudioPacket>,
        sequence: AtomicU16,
    }

    impl Outgoing for Wire {
        async fn send(&self, payload: &[u8]) -> Result<(), ()> {
            let sequence = self.sequence.fetch_add(1, Ordering::SeqCst);
            let packet = AudioPacket { sequence, timestamp: u32::from(sequence) * FRAME_SAMPLES as u32, payload: payload.to_vec() };
            self.packets.try_send(packet).map_err(drop)
        }
    }

    fn wire() -> (Arc<Wire>, mpsc::Receiver<AudioPacket>) {
        let (packets, arrived) = mpsc::channel(256);
        (Arc::new(Wire { packets, sequence: AtomicU16::new(0) }), arrived)
    }

    fn platform(backend: BackendFactory, activation: Activation) -> AudioPlatform {
        AudioPlatform { backend, activation }
    }

    fn pair(activation: Activation, seconds: usize) -> (Voice<Wire>, DeviceProbe, Voice<Wire>, DeviceProbe) {
        let (to_bob, bob_hears) = wire();
        let (to_alice, alice_hears) = wire();
        let (alice_probe, bob_probe) = (DeviceProbe::default(), DeviceProbe::default());
        let alice = Voice::new(to_bob, alice_hears, platform(ToneDevice::factory(test_voice(seconds, false), alice_probe.clone()), activation));
        let bob = Voice::new(to_alice, bob_hears, platform(ToneDevice::factory(test_voice(seconds, true), bob_probe.clone()), activation));
        (alice, alice_probe, bob, bob_probe)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_device_starts_once_the_call_connects_and_stops_with_it() {
        let (alice, probe, _bob, _) = pair(Activation::Immediate, 1);
        assert_eq!(probe.starts(), 0, "nothing plays before the call connects");
        alice.connected().await.expect("starts");
        assert_eq!(probe.starts(), 1);
        assert!(alice.is_running().await);

        alice.stop().await;
        assert_eq!(probe.stops(), 1);
        assert!(!alice.is_running().await);
        alice.connected().await.expect("a stopped voice stays stopped");
        assert_eq!(probe.starts(), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn with_callkit_the_device_waits_for_the_audio_session() {
        let (alice, probe, bob, bob_probe) = pair(Activation::WhenSessionActive, 1);
        alice.connected().await.expect("connects");
        assert_eq!(probe.starts(), 0, "connected, but the session is not active yet");

        alice.set_session_active(true).await.expect("activates");
        assert_eq!(probe.starts(), 1);
        alice.set_session_active(false).await.expect("deactivates");
        assert_eq!(probe.stops(), 1, "an interruption takes the device away");
        alice.set_session_active(true).await.expect("activates again");
        assert_eq!(probe.starts(), 2, "and gives it back");
        alice.stop().await;
        assert_eq!(probe.stops(), 2);

        bob.set_session_active(true).await.expect("activates");
        assert_eq!(bob_probe.starts(), 0);
        bob.connected().await.expect("connects");
        assert_eq!(bob_probe.starts(), 1);
        bob.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn each_side_hears_the_other() {
        let (alice, alice_probe, bob, bob_probe) = pair(Activation::Immediate, 3);
        alice.connected().await.expect("alice");
        bob.connected().await.expect("bob");
        tokio::time::sleep(Duration::from_millis(3_200)).await;
        alice.stop().await;
        bob.stop().await;

        let bob_heard = mean_heard(&test_voice(3, false), &bob_probe.played(), 48_000, 12_000, 6);
        let alice_heard = mean_heard(&test_voice(3, true), &alice_probe.played(), 48_000, 12_000, 6);
        assert!(bob_heard > 0.7, "bob heard alice at {bob_heard}");
        assert!(alice_heard > 0.7, "alice heard bob at {alice_heard}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn muted_the_other_side_hears_silence() {
        let (alice, _, bob, bob_probe) = pair(Activation::Immediate, 2);
        alice.set_muted(true).await;
        alice.connected().await.expect("alice");
        bob.connected().await.expect("bob");
        assert!(alice.is_muted());
        tokio::time::sleep(Duration::from_millis(1_500)).await;
        alice.stop().await;
        bob.stop().await;
        let played = bob_probe.played();
        assert!(played.len() > 48_000);
        assert!(rms(&played[24_000..]) < 30.0, "rms {}", rms(&played[24_000..]));
    }

    struct Upkept(Arc<std::sync::atomic::AtomicUsize>);

    impl AudioBackend for Upkept {
        fn start(&mut self, _io: webrtc_engine::audio::DeviceIo) -> Result<(), AudioError> {
            Ok(())
        }
        fn stop(&mut self) -> Result<(), AudioError> {
            Ok(())
        }
        fn maintain(&mut self) -> Result<(), AudioError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_device_is_looked_after_while_it_runs() {
        let upkeeps = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = upkeeps.clone();
        let backend: BackendFactory = Arc::new(move || Ok(Box::new(Upkept(counted.clone())) as Box<dyn AudioBackend>));
        let (outgoing, incoming) = wire();
        let voice = Voice::new(outgoing, incoming, platform(backend, Activation::Immediate));
        voice.connected().await.expect("starts");
        tokio::time::sleep(Duration::from_millis(450)).await;
        voice.stop().await;
        let looked_after = upkeeps.load(Ordering::SeqCst);
        assert!((2..=5).contains(&looked_after), "{looked_after} upkeeps in 450 ms");
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert_eq!(upkeeps.load(Ordering::SeqCst), looked_after, "none once stopped");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_voice_says_when_its_device_first_ran() {
        let (alice, _, _bob, _) = pair(Activation::WhenSessionActive, 1);
        alice.connected().await.expect("connects");
        assert_eq!(alice.device_started(), None, "the session is not active yet");
        let before = std::time::Instant::now();
        alice.set_session_active(true).await.expect("activates");
        let first = alice.device_started().expect("it ran");
        assert!(first >= before);
        alice.set_session_active(false).await.expect("deactivates");
        alice.set_session_active(true).await.expect("activates again");
        assert_eq!(alice.device_started(), Some(first), "the first time is kept");
        assert_eq!(alice.first_packet(), None, "no connection, nothing arrived from it");
        alice.stop().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_device_that_cannot_open_fails_the_start() {
        let (outgoing, incoming) = wire();
        let voice = Voice::new(outgoing, incoming, platform(broken_device(), Activation::Immediate));
        assert!(voice.connected().await.is_err());
        assert!(!voice.is_running().await);
    }
}
