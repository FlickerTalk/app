//! Stand-ins for the tests of this crate and of the core (feature `testing`): a device that
//! speaks a test voice in real time and records what it plays, and a measure of how well a voice
//! came through.

use std::f64::consts::TAU;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use webrtc_engine::audio::{AudioBackend, AudioError, DeviceIo};
use webrtc_engine::SAMPLE_RATE;

use crate::voice::BackendFactory;

/// What the tests see of a fake device: how often it started and stopped, and what it played.
#[derive(Clone, Default)]
pub struct DeviceProbe {
    starts: Arc<AtomicUsize>,
    stops: Arc<AtomicUsize>,
    played: Arc<Mutex<Vec<i16>>>,
}

impl DeviceProbe {
    pub fn starts(&self) -> usize {
        self.starts.load(Ordering::SeqCst)
    }

    pub fn stops(&self) -> usize {
        self.stops.load(Ordering::SeqCst)
    }

    /// Whether it is running now.
    pub fn running(&self) -> bool {
        self.starts() > self.stops()
    }

    /// Everything the speaker played, in order.
    pub fn played(&self) -> Vec<i16> {
        self.played.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

/// Every 10 ms, as a phone's audio callback would, it captures the next 10 ms of its voice (then
/// silence) and plays 10 ms.
pub struct ToneDevice {
    voice: Arc<Vec<i16>>,
    probe: DeviceProbe,
    worker: Option<(Arc<AtomicBool>, JoinHandle<()>)>,
}

const CHUNK: usize = SAMPLE_RATE as usize / 100;

impl ToneDevice {
    pub fn new(voice: Vec<i16>, probe: DeviceProbe) -> Self {
        Self { voice: Arc::new(voice), probe, worker: None }
    }

    /// A factory that makes one of these, speaking `voice`, for every start.
    pub fn factory(voice: Vec<i16>, probe: DeviceProbe) -> BackendFactory {
        let voice = Arc::new(voice);
        Arc::new(move || {
            Ok(Box::new(ToneDevice { voice: voice.clone(), probe: probe.clone(), worker: None }) as Box<dyn AudioBackend>)
        })
    }
}

impl AudioBackend for ToneDevice {
    fn start(&mut self, mut io: DeviceIo) -> Result<(), AudioError> {
        let stop = Arc::new(AtomicBool::new(false));
        let (voice, played, stopping) = (self.voice.clone(), self.probe.played.clone(), stop.clone());
        let worker = std::thread::spawn(move || {
            let mut next = Instant::now();
            let mut position = 0;
            while !stopping.load(Ordering::SeqCst) {
                let mut chunk = [0i16; CHUNK];
                if let Some(speech) = voice.get(position..) {
                    let take = speech.len().min(CHUNK);
                    chunk[..take].copy_from_slice(&speech[..take]);
                }
                position += CHUNK;
                io.capture.push(&chunk);
                let mut out = [0i16; CHUNK];
                io.playout.pop(&mut out);
                played.lock().unwrap_or_else(PoisonError::into_inner).extend_from_slice(&out);
                next += Duration::from_millis(10);
                std::thread::sleep(next.saturating_duration_since(Instant::now()));
            }
        });
        self.probe.starts.fetch_add(1, Ordering::SeqCst);
        self.worker = Some((stop, worker));
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        if let Some((stop, worker)) = self.worker.take() {
            stop.store(true, Ordering::SeqCst);
            let _ = worker.join();
            self.probe.stops.fetch_add(1, Ordering::SeqCst);
        }
        Ok(())
    }
}

/// A device that cannot open: no microphone, say.
pub fn broken_device() -> BackendFactory {
    Arc::new(|| Err(AudioError::NoDevice))
}

/// A test voice: a tone gliding between 230 and 470 Hz plus a steady 1100 Hz one. The glide makes
/// every stretch different, so aligning what went in with what came out has one answer. The
/// other side speaks it `inverted`: hearing oneself instead of the other correlates negatively.
pub fn test_voice(seconds: usize, inverted: bool) -> Vec<i16> {
    let rate = f64::from(SAMPLE_RATE);
    let mut phase = 0.0f64;
    (0..seconds * SAMPLE_RATE as usize)
        .map(|n| {
            let t = n as f64 / rate;
            phase += TAU * (350.0 + 120.0 * (TAU * 0.9 * t).sin()) / rate;
            let level = 0.25 * phase.sin() + 0.1 * (TAU * 1_100.0 * t).sin();
            let sample = (level * f64::from(i16::MAX)) as i16;
            if inverted {
                sample.saturating_neg()
            } else {
                sample
            }
        })
        .collect()
}

fn correlation_at(reference: &[i16], output: &[i16], lag: usize, step: usize) -> f64 {
    let Some(shifted) = output.get(lag..lag + reference.len()) else { return 0.0 };
    let (mut dot, mut ours, mut theirs) = (0.0, 0.0, 0.0);
    for (a, b) in reference.iter().zip(shifted).step_by(step) {
        let (a, b) = (f64::from(*a), f64::from(*b));
        dot += a * b;
        ours += a * a;
        theirs += b * b;
    }
    if ours == 0.0 || theirs == 0.0 {
        return 0.0;
    }
    dot / (ours * theirs).sqrt()
}

/// How well `input[start..start + window]` shows up in `output`, at the best delay up to
/// `max_lag` samples: from -1 to 1.
pub fn heard(input: &[i16], output: &[i16], start: usize, window: usize, max_lag: usize) -> f64 {
    const COARSE: usize = 8;
    let Some(reference) = input.get(start..start + window) else { return 0.0 };
    let tail = output.get(start..).unwrap_or_default();
    let best = |lags: &mut dyn Iterator<Item = usize>, step| {
        lags.map(|lag| (correlation_at(reference, tail, lag, step), lag))
            .fold((f64::MIN, 0), |best, next| if next.0 > best.0 { next } else { best })
    };
    let (_, coarse) = best(&mut (0..=max_lag).step_by(COARSE), COARSE);
    best(&mut (coarse.saturating_sub(COARSE)..=coarse + COARSE), 1).0
}

/// The mean of `heard` over `count` consecutive windows: the jitter buffer changes the delay
/// during a call, so each window finds its own.
pub fn mean_heard(input: &[i16], output: &[i16], start: usize, window: usize, count: usize) -> f64 {
    let max_lag = SAMPLE_RATE as usize / 2;
    (0..count).map(|index| heard(input, output, start + index * window, window, max_lag)).sum::<f64>() / count as f64
}

/// The loudness of `samples`.
pub fn rms(samples: &[i16]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|&s| f64::from(s).powi(2)).sum::<f64>() / samples.len() as f64).sqrt()
}

#[cfg(test)]
mod tests {
    use webrtc_engine::audio::audio_io;
    use webrtc_engine::FRAME_SAMPLES;

    use super::*;

    #[test]
    fn a_voice_heard_after_a_delay_correlates_and_its_mirror_does_not() {
        let voice = test_voice(1, false);
        let delayed: Vec<i16> = std::iter::repeat_n(0, 2_000).chain(voice.iter().copied()).collect();
        assert!(mean_heard(&voice, &delayed, 12_000, 6_000, 4) > 0.95);
        let mirror = test_voice(1, true);
        assert!(heard(&voice, &mirror, 12_000, 6_000, 0) < -0.5, "a mirror is not the voice");
    }

    #[test]
    fn the_tone_device_speaks_in_real_time_and_plays_what_it_is_given() {
        let probe = DeviceProbe::default();
        let mut device = ToneDevice::new(test_voice(1, false), probe.clone());
        let (io, mut engine) = audio_io(16);
        assert!(engine.playout.write_frame(&[1_000; FRAME_SAMPLES]));
        device.start(io).expect("starts");
        assert!(probe.running());
        std::thread::sleep(Duration::from_millis(100));
        device.stop().expect("stops");
        device.stop().expect("stopping twice does nothing");
        assert_eq!((probe.starts(), probe.stops()), (1, 1));

        let mut frame = [0; FRAME_SAMPLES];
        assert!(engine.capture.read_frame(&mut frame), "it captured at least 20 ms");
        assert_eq!(&frame[..], &test_voice(1, false)[..FRAME_SAMPLES]);
        assert!(probe.played().starts_with(&[1_000; FRAME_SAMPLES]));
    }

    #[test]
    fn a_broken_device_does_not_open() {
        assert!(broken_device()().is_err());
    }
}
