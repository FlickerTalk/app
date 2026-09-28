//! A call's voice: the device (microphone and speaker) and `webrtc-engine`'s pipeline between it
//! and the connection.
//!
//! The device runs only while the call is connected and, on iOS, while CallKit has the audio
//! session active: the audio unit may start only after CallKit says so. The connection comes
//! first; the device starts when both are true, stops when either is not, and starts again (with
//! fresh rings) when they are both true again.

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use webrtc_engine::audio::{audio_io, AudioBackend, AudioError, DeviceIo};
use webrtc_engine::call::{Call, CallConfig, PacketSink, PacketSource, RemoteAudio};
use webrtc_engine::rtp::{AudioPacket, AudioSender};

use crate::MediaSession;

/// Opens the platform's audio device. Called on the thread that will own the device, so a
/// backend that must stay on one thread is fine.
pub type BackendFactory = Arc<dyn Fn() -> Result<Box<dyn AudioBackend>, AudioError> + Send + Sync>;

/// When the device may start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activation {
    /// As soon as the call connects (Android, and the tests).
    Immediate,
    /// Only once the OS has activated the call's audio session (iOS, CallKit's `didActivate`).
    WhenSessionActive,
}

/// A platform's audio device and when it may start.
#[derive(Clone)]
pub struct AudioPlatform {
    pub backend: BackendFactory,
    pub activation: Activation,
}

/// Where our Opus packets go: one per 20 ms.
pub trait Outgoing: Send + Sync + 'static {
    fn send(&self, payload: &[u8]) -> impl Future<Output = Result<(), ()>> + Send;
}

impl Outgoing for AudioSender {
    async fn send(&self, payload: &[u8]) -> Result<(), ()> {
        AudioSender::send(self, payload).await.map_err(drop)
    }
}

/// The engine's side of our voice: a handle to where packets go.
struct Outbound<O>(Arc<O>);

impl<O: Outgoing> PacketSink for Outbound<O> {
    type Error = ();
    fn send(&mut self, payload: &[u8]) -> impl Future<Output = Result<(), ()>> + Send {
        let outgoing = self.0.clone();
        let payload = payload.to_vec();
        async move { outgoing.send(&payload).await }
    }
}

/// The engine's side of their voice. Shared, so a pipeline started again reads on where the last
/// one stopped.
struct Inbound(Arc<Mutex<mpsc::Receiver<AudioPacket>>>);

impl PacketSource for Inbound {
    type Error = ();
    async fn recv(&mut self) -> Result<Option<AudioPacket>, ()> {
        Ok(self.0.lock().await.recv().await)
    }
}

/// The device, on a thread of its own: a backend never has to cross threads.
struct Device {
    stop: std::sync::mpsc::Sender<()>,
    thread: std::thread::JoinHandle<()>,
}

impl Device {
    /// Opens the device and starts it with `io`; returns once it runs, or why it could not.
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
                // Until told, or until the voice is gone.
                let _ = stopping.recv();
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

/// Rings between the device and the pipeline: 320 ms each way, far more than either side leaves.
const RING_FRAMES: usize = 16;

/// The voice of one call.
pub struct Voice<O: Outgoing = AudioSender> {
    outgoing: Arc<O>,
    incoming: Arc<Mutex<mpsc::Receiver<AudioPacket>>>,
    platform: AudioPlatform,
    muted: AtomicBool,
    state: Mutex<State>,
    forwarder: Option<JoinHandle<()>>,
}

impl<O: Outgoing> Voice<O> {
    /// A voice that sends through `outgoing` and plays what arrives on `incoming`.
    pub fn new(outgoing: Arc<O>, incoming: mpsc::Receiver<AudioPacket>, platform: AudioPlatform) -> Self {
        Self {
            outgoing,
            incoming: Arc::new(Mutex::new(incoming)),
            platform,
            muted: AtomicBool::new(false),
            state: Mutex::new(State::default()),
            forwarder: None,
        }
    }

    /// The connection is up.
    pub async fn connected(&self) -> Result<()> {
        let mut state = self.state.lock().await;
        state.connected = true;
        self.settle(&mut state).await
    }

    /// The OS activated (or took back) the call's audio session.
    pub async fn set_session_active(&self, active: bool) -> Result<()> {
        let mut state = self.state.lock().await;
        state.session_active = active;
        self.settle(&mut state).await
    }

    /// Muted, silence goes out in place of the microphone.
    pub async fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::SeqCst);
        if let Some(running) = &self.state.lock().await.running {
            running.call.set_muted(muted);
        }
    }

    pub fn is_muted(&self) -> bool {
        self.muted.load(Ordering::SeqCst)
    }

    /// Whether the device and the pipeline are running.
    pub async fn is_running(&self) -> bool {
        self.state.lock().await.running.is_some()
    }

    /// Stops the device and the pipeline for good.
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

    /// Starts or stops the device and the pipeline, as the state says.
    async fn settle(&self, state: &mut State) -> Result<()> {
        match (self.may_run(state), state.running.is_some()) {
            (true, false) => state.running = Some(self.start().await?),
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
        // What arrived while nothing played is old by now: the jitter buffer starts afresh.
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

    /// The device first, then the pipeline.
    async fn halt(running: Running) {
        let Running { device, call } = running;
        let _ = tokio::task::spawn_blocking(move || device.stop()).await;
        call.stop().await;
    }
}

impl Voice<AudioSender> {
    /// The voice of `session`'s call: ours goes out on its track, theirs comes from its remote
    /// track once the first packet arrives.
    pub fn for_session(session: &MediaSession, platform: AudioPlatform) -> Self {
        let (arrived, incoming) = mpsc::channel(INCOMING_PACKETS);
        let mut voice = Self::new(session.sender(), incoming, platform);
        if let Some(track) = session.take_remote() {
            voice.forwarder = Some(tokio::spawn(async move {
                let mut remote = RemoteAudio::new(track);
                while let Ok(Some(packet)) = remote.recv().await {
                    // Full means nothing plays right now (the device waits): dropping is what a
                    // network does.
                    let _ = arrived.try_send(packet);
                }
            }));
        }
        voice
    }
}

/// Packets kept while nothing plays them: 1 s.
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

    /// Numbers packets like RTP and hands them straight to the other side.
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

    /// Two voices whose wires cross, each with its own tone device.
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

    // iOS: the audio unit may start only after CallKit activates the audio session.
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

        // CallKit may activate the session before the connection is up (an answer, say).
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

    #[tokio::test(flavor = "multi_thread")]
    async fn a_device_that_cannot_open_fails_the_start() {
        let (outgoing, incoming) = wire();
        let voice = Voice::new(outgoing, incoming, platform(broken_device(), Activation::Immediate));
        assert!(voice.connected().await.is_err());
        assert!(!voice.is_running().await);
    }
}
