//! The support contact's microphone and camera for calls: a recording on a loop. It has no sound
//! card and no camera (a container on a server), so the call's voice comes from `clips::call_voice`
//! and its video from `clips::call_video`, both already what a phone's devices would hand the
//! engine: 48 kHz mono samples every 10 ms, and H.264 access units at the clip's frame rate. What
//! the other side says and shows is dropped at once: nothing is heard, decoded or kept.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ft_media::{Activation, AudioPlatform, BackendFactory, VideoDevices, VideoPlatform};
use webrtc_engine::audio::{AudioBackend, AudioError, DeviceIo};
use webrtc_engine::video::{EncodedFrame, Facing, FrameSender, Rotation, SendOutcome, VideoConfig, VideoError, VideoSink, VideoSource};
use webrtc_engine::SAMPLE_RATE;

use crate::clips::Frame;

/// Silence between two rounds of the recording.
pub const PAUSE: Duration = Duration::from_millis(1_500);

/// What a phone's audio callback moves at a time: 10 ms.
const CHUNK: usize = SAMPLE_RATE as usize / 100;

/// A recording and the pause after it, over and over.
#[derive(Debug, Clone)]
pub struct Looper {
    clip: Arc<Vec<i16>>,
    pause: usize,
    at: usize,
}

impl Looper {
    pub fn new(clip: Arc<Vec<i16>>, pause: Duration) -> Self {
        let pause = usize::try_from(pause.as_millis()).unwrap_or(usize::MAX) * SAMPLE_RATE as usize / 1_000;
        Self { clip, pause, at: 0 }
    }

    /// Fills `out` with what comes next.
    pub fn fill(&mut self, out: &mut [i16]) {
        let round = self.clip.len() + self.pause;
        if self.clip.is_empty() || round == 0 {
            out.fill(0);
            return;
        }
        for sample in out.iter_mut() {
            *sample = self.clip.get(self.at).copied().unwrap_or(0);
            self.at = (self.at + 1) % round;
        }
    }
}

/// The call's voice device: says the recording every 10 ms, as a phone's microphone would, and
/// throws away what it should play.
pub struct LoopDevice {
    looper: Looper,
    worker: Option<(Arc<AtomicBool>, JoinHandle<()>)>,
}

impl LoopDevice {
    /// A factory that makes one for every call, starting from the top of the recording.
    pub fn factory(clip: Arc<Vec<i16>>) -> BackendFactory {
        Arc::new(move || Ok(Box::new(LoopDevice { looper: Looper::new(clip.clone(), PAUSE), worker: None }) as Box<dyn AudioBackend>))
    }
}

impl Drop for LoopDevice {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

impl AudioBackend for LoopDevice {
    fn start(&mut self, mut io: DeviceIo) -> Result<(), AudioError> {
        self.stop()?;
        let stop = Arc::new(AtomicBool::new(false));
        let (stopping, mut looper) = (stop.clone(), self.looper.clone());
        let worker = std::thread::Builder::new()
            .name("ft-support-voice".to_owned())
            .spawn(move || {
                let (mut said, mut heard) = ([0i16; CHUNK], [0i16; CHUNK]);
                let mut next = Instant::now();
                while !stopping.load(Ordering::SeqCst) {
                    looper.fill(&mut said);
                    io.capture.push(&said);
                    // What the other side says is taken out of the ring and dropped.
                    io.playout.pop(&mut heard);
                    next += Duration::from_millis(10);
                    std::thread::sleep(next.saturating_duration_since(Instant::now()));
                }
            })
            .map_err(|error| AudioError::Backend(error.to_string()))?;
        self.worker = Some((stop, worker));
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        stop_worker(self.worker.take());
        Ok(())
    }
}

fn stop_worker(worker: Option<(Arc<AtomicBool>, JoinHandle<()>)>) {
    if let Some((stop, worker)) = worker {
        stop.store(true, Ordering::SeqCst);
        let _ = worker.join();
    }
}

/// Calls run here with the recording as the voice, as soon as they connect.
pub fn audio_platform(clip: Arc<Vec<i16>>) -> AudioPlatform {
    AudioPlatform { backend: LoopDevice::factory(clip), activation: Activation::Immediate }
}

/// Encoded frames over and over, jumping ahead to the next keyframe when one is wanted.
#[derive(Debug, Clone)]
pub struct Reel {
    frames: Arc<Vec<Frame>>,
    at: usize,
}

impl Reel {
    /// `frames` must start with a keyframe.
    pub fn new(frames: Arc<Vec<Frame>>) -> Self {
        Self { frames, at: 0 }
    }

    /// The next frame; with `keyframe`, the next keyframe.
    pub fn next(&mut self, keyframe: bool) -> &Frame {
        let count = self.frames.len();
        let mut index = self.at % count;
        if keyframe {
            if let Some(offset) = (0..count).find(|offset| self.frames[(index + offset) % count].keyframe) {
                index = (index + offset) % count;
            }
        }
        self.at = index + 1;
        &self.frames[index]
    }
}

/// The call's camera: the clip at its frame rate, with timestamps that keep going across rounds.
pub struct LoopSource {
    reel: Reel,
    fps: u32,
    wanted: Arc<AtomicBool>,
    worker: Option<(Arc<AtomicBool>, JoinHandle<()>)>,
}

impl LoopSource {
    pub fn new(frames: Arc<Vec<Frame>>, fps: u32) -> Self {
        Self { reel: Reel::new(frames), fps: fps.max(1), wanted: Arc::default(), worker: None }
    }
}

impl VideoSource for LoopSource {
    /// The size and rate asked for do not matter: the clip has its own, and the SPS says it.
    fn start(&mut self, _config: VideoConfig, _facing: Facing, out: FrameSender) -> Result<(), VideoError> {
        stop_worker(self.worker.take());
        let stop = Arc::new(AtomicBool::new(false));
        let (stopping, wanted, mut reel) = (stop.clone(), self.wanted.clone(), self.reel.clone());
        let interval = Duration::from_secs(1) / self.fps;
        let worker = std::thread::Builder::new()
            .name("ft-support-video".to_owned())
            .spawn(move || {
                let started = Instant::now();
                let mut sent: u32 = 0;
                while !stopping.load(Ordering::SeqCst) {
                    let keyframe = wanted.swap(false, Ordering::SeqCst) || out.keyframe_needed();
                    let frame = reel.next(keyframe);
                    let encoded =
                        EncodedFrame { data: frame.data.clone(), keyframe: frame.keyframe, timestamp: interval * sent, rotation: Rotation::Deg0 };
                    if out.try_send(encoded) == SendOutcome::Closed {
                        return;
                    }
                    sent = sent.wrapping_add(1);
                    std::thread::sleep((started + interval * sent).saturating_duration_since(Instant::now()));
                }
            })
            .map_err(|error| VideoError::Backend(error.to_string()))?;
        self.worker = Some((stop, worker));
        Ok(())
    }

    fn stop(&mut self) -> Result<(), VideoError> {
        stop_worker(self.worker.take());
        Ok(())
    }

    fn request_keyframe(&mut self) {
        self.wanted.store(true, Ordering::SeqCst);
    }

    /// The clip is what it is.
    fn set_bitrate(&mut self, _bps: u32) {}

    /// There is one camera.
    fn switch_camera(&mut self, _facing: Facing) -> Result<(), VideoError> {
        Ok(())
    }
}

impl Drop for LoopSource {
    fn drop(&mut self) {
        stop_worker(self.worker.take());
    }
}

/// The call's display: drops every frame.
pub struct NullSink;

impl VideoSink for NullSink {
    fn start(&mut self) -> Result<(), VideoError> {
        Ok(())
    }

    fn push(&mut self, _frame: EncodedFrame) -> Result<(), VideoError> {
        Ok(())
    }

    fn stop(&mut self) -> Result<(), VideoError> {
        Ok(())
    }
}

/// Video calls run here with the clip as the camera.
pub fn video_platform(frames: Arc<Vec<Frame>>, fps: u32) -> VideoPlatform {
    VideoPlatform {
        devices: Arc::new(move || Ok(VideoDevices { source: Box::new(LoopSource::new(frames.clone(), fps)), sink: Box::new(NullSink) })),
    }
}

#[cfg(test)]
mod tests {
    use webrtc_engine::audio::audio_io;
    use webrtc_engine::video::frame_channel;
    use webrtc_engine::FRAME_SAMPLES;

    use super::*;

    fn clip(samples: &[i16]) -> Arc<Vec<i16>> {
        Arc::new(samples.to_vec())
    }

    #[test]
    fn the_looper_says_the_clip_then_the_pause_then_the_clip_again() {
        // A pause of 1 ms is 48 samples.
        let mut looper = Looper::new(clip(&[1, 2, 3]), Duration::from_millis(1));
        let mut out = vec![9; 3 + 48 + 4];
        looper.fill(&mut out);
        assert_eq!(&out[..3], &[1, 2, 3]);
        assert!(out[3..51].iter().all(|sample| *sample == 0));
        assert_eq!(&out[51..], &[1, 2, 3, 0], "and round again, into the next pause");
    }

    #[test]
    fn the_looper_goes_on_where_it_stopped() {
        let mut looper = Looper::new(clip(&[1, 2, 3, 4, 5]), Duration::ZERO);
        let (mut a, mut b) = ([0; 3], [0; 4]);
        looper.fill(&mut a);
        looper.fill(&mut b);
        assert_eq!((a, b), ([1, 2, 3], [4, 5, 1, 2]));
    }

    #[test]
    fn an_empty_clip_is_silence() {
        let mut looper = Looper::new(clip(&[]), Duration::ZERO);
        let mut out = [7; 10];
        looper.fill(&mut out);
        assert_eq!(out, [0; 10]);
    }

    #[test]
    fn the_device_speaks_in_real_time_and_stops_when_told() {
        let voice: Vec<i16> = (1..=4_800).map(|n| n as i16).collect();
        let factory = LoopDevice::factory(Arc::new(voice));
        let mut device = factory().expect("opens");
        let (io, mut engine) = audio_io(50);
        device.start(io).expect("starts");
        std::thread::sleep(Duration::from_millis(200));
        let ready = engine.capture.frames_ready();
        assert!((4..=20).contains(&ready), "about 10 frames of 20 ms in 200 ms, got {ready}");
        let mut frame = [0; FRAME_SAMPLES];
        assert!(engine.capture.read_frame(&mut frame));
        assert_eq!(frame[0], 1, "from the top");
        assert_eq!(frame[FRAME_SAMPLES - 1], FRAME_SAMPLES as i16);
        device.stop().expect("stops");
        let after = engine.capture.frames_ready();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(engine.capture.frames_ready(), after, "nothing more once stopped");
        device.stop().expect("stopping twice does nothing");
    }

    fn frames(keys: &[bool]) -> Arc<Vec<Frame>> {
        Arc::new(keys.iter().enumerate().map(|(n, key)| Frame { data: vec![n as u8], keyframe: *key }).collect())
    }

    #[test]
    fn the_reel_plays_in_order_and_round_again() {
        let mut reel = Reel::new(frames(&[true, false, false]));
        let seen: Vec<u8> = (0..7).map(|_| reel.next(false).data[0]).collect();
        assert_eq!(seen, vec![0, 1, 2, 0, 1, 2, 0]);
    }

    #[test]
    fn a_wanted_keyframe_skips_ahead_to_the_next_one() {
        let mut reel = Reel::new(frames(&[true, false, false, true, false]));
        assert_eq!(reel.next(false).data[0], 0);
        assert_eq!(reel.next(true).data[0], 3, "skips the deltas");
        assert_eq!(reel.next(true).data[0], 0, "round again to the first keyframe");
        assert_eq!(reel.next(true).data[0], 3);
    }

    #[test]
    fn the_source_sends_the_clip_at_its_rate_and_a_keyframe_when_asked() {
        let mut source = LoopSource::new(frames(&[true, false, false, false, true, false]), 50);
        let (sender, receiver) = frame_channel(8);
        source.start(VideoConfig::default(), Facing::Front, sender).expect("starts");
        let started = Instant::now();
        let first = receiver.recv_timeout(Duration::from_secs(1)).expect("a frame");
        assert!(first.keyframe && first.data == vec![0]);
        let second = receiver.recv_timeout(Duration::from_secs(1)).expect("a frame");
        assert_eq!(second.data, vec![1]);
        assert_eq!(second.timestamp - first.timestamp, Duration::from_millis(20), "50 fps");
        source.request_keyframe();
        // A frame may already be on its way as the request lands: the keyframe is the next or the one after.
        let next = receiver.recv_timeout(Duration::from_secs(1)).expect("a frame");
        let next = if next.keyframe { next } else { receiver.recv_timeout(Duration::from_secs(1)).expect("a frame") };
        assert!(next.keyframe, "{:?}", next.data);
        assert!(started.elapsed() < Duration::from_millis(500));
        source.stop().expect("stops");
        while receiver.try_recv().is_ok() {}
        std::thread::sleep(Duration::from_millis(100));
        assert!(receiver.try_recv().is_err(), "nothing more once stopped");
    }

    #[test]
    fn the_source_ends_when_the_call_s_video_is_gone() {
        let mut source = LoopSource::new(frames(&[true]), 100);
        let (sender, receiver) = frame_channel(1);
        source.start(VideoConfig::default(), Facing::Front, sender).expect("starts");
        drop(receiver);
        std::thread::sleep(Duration::from_millis(100));
        source.stop().expect("its thread is already over");
    }

    #[test]
    fn the_platforms_make_devices_for_every_call() {
        let video = video_platform(frames(&[true]), 15);
        assert!((video.devices)().is_ok());
        assert!((video.devices)().is_ok());
        assert_eq!(audio_platform(clip(&[1])).activation, Activation::Immediate);
        let mut sink = NullSink;
        assert!(sink.start().is_ok() && sink.push(EncodedFrame { data: vec![1], keyframe: true, timestamp: Duration::ZERO, rotation: Rotation::Deg0 }).is_ok());
    }
}
