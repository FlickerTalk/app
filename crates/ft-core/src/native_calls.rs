//! Native voice calls (2026-09-28): on the phones, a voice call's media runs in Rust (`ft-media`)
//! instead of the WebView. On a locked iPhone a call answered from CallKit has no WebView at all,
//! and WKWebView's media is muted in the background, so the voice has to live here.
//!
//! The signalling is the same as the WebView's calls (`calls.rs`): `CallOffer`, `CallAnswer` and
//! `CallEnd` over the direct connection, with standard SDP, so a native phone and a WebView phone
//! (an older app) still talk. The desktop keeps the WebView.
//!
//! **Video** (2026-09-29, `docs/video-nativo.md`): every native call has a video line from the
//! start, and says media version 1 in its offer or answer. Switching between voice and video is
//! turning our own camera on or off (`set_call_camera`) and telling the other side with a
//! `CallMedia` (a state with a growing `seq`, retried until it gets through), never a new offer.
//! The camera is held while the app is away or the call screen does not show it.
//!
//! **Call setup time** (2026-09-29): while the call rings, its answer is prepared (connection,
//! offer taken, candidates gathered) and made again every 15 s, sending nothing and checking
//! nothing towards the caller; answering sends it at once. The caller opens the direct connection
//! while its offer gathers. `timings.rs` measures each step (temporary diagnostics).

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU16, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, ensure, Result};
use ft_media::{AudioPlatform, CallRouting, LinkState, MediaSession, Video, VideoPlatform, VideoState, Voice};
use ft_protocol::{Body, MessageId, Packet};

use crate::calls::stale;
use crate::timings::{CallClock, CallStage, CallTimings, Candidates};
use crate::{now, CallUpdate, Core, Event};

/// Where a call stands, for a WebView that comes up late (§66).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallPhase {
    /// Our call rings on the other phone.
    Calling,
    /// Their call rings here.
    Ringing,
    /// Answered, the media is not connected yet.
    Connecting,
    Active,
}

/// The call going on, if any: what the UI needs to show it again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentCall {
    pub call: String,
    pub contact: String,
    pub video: bool,
    pub outgoing: bool,
    pub phase: CallPhase,
    /// Their offer, while it rings here: a WebView call is answered with it.
    pub offer: Option<String>,
    /// Whether its media runs in Rust.
    pub native: bool,
    pub muted: bool,
    /// When the media connected (ms).
    pub connected_at: Option<i64>,
    /// The call's video, when it runs here (native video, 2026-09-29).
    pub video_state: Option<VideoState>,
    /// What is presented in the call (2026-10-08), by either side.
    pub presenting: Option<Presenting>,
    /// Whether a presentation can start: the call is on and both apps speak media version 2.
    pub can_present: bool,
}

/// Who presents in a call (2026-10-08).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentedBy {
    Me,
    Them,
}

/// What is presented in a call (2026-10-08, `docs/plan-presentar-en-llamada.md`): the plugin
/// shown on both screens and the chat file it shows, if any. The core opens nothing and reads
/// nothing of it: the plugin's own content travels over `ft.live`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Presenting {
    pub plugin: String,
    pub file: Option<MessageId>,
    pub by: PresentedBy,
}

/// The media of a native call: its connection, its voice and its video.
pub(crate) struct NativeCall {
    call: String,
    contact: String,
    session: MediaSession,
    voice: Voice,
    /// Its video, when this phone runs video natively.
    video: Option<Video>,
    /// How the call started: a video call turns our camera on as it connects.
    video_call: bool,
    /// The other side's call media version: from its offer, or from its answer to ours.
    peer_media: AtomicU16,
    /// Whether `peer_media` is known: our own call learns it with the answer.
    peer_known: AtomicBool,
    /// Our camera as the user wants it; it runs once the video is ready.
    camera: AtomicBool,
    /// The video is connected: the camera and the other side's state apply from now on.
    video_ready: AtomicBool,
    /// One change to the video at a time, always with the latest wish and the latest state heard.
    video_lock: tokio::sync::Mutex<()>,
    /// The `seq` of our latest `CallMedia`.
    sent_seq: AtomicU32,
    /// What our latest `CallMedia` said: (on, held). A call starts with the camera off, unsaid.
    said: Mutex<(bool, bool)>,
    /// What the other side last said about its camera.
    remote: Mutex<RemoteCamera>,
    /// When the media connected (ms); 0 until then.
    connected_at: AtomicI64,
    /// The call media version we say in our offer or answer (`media_version`, at most the core's
    /// `call_media_ceiling`).
    our_media: u16,
    /// What is presented in the call now, by either side (2026-10-08).
    presenting: Mutex<Option<Presenting>>,
    /// The `seq` of our latest `CallPresent`.
    present_seq: AtomicU32,
    /// The highest `seq` the other side said in a `CallPresent`.
    heard_present: Mutex<Option<u32>>,
}

impl NativeCall {
    fn connected_at(&self) -> Option<i64> {
        Some(self.connected_at.load(Ordering::SeqCst)).filter(|at| *at > 0)
    }

    fn peer_media(&self) -> u16 {
        self.peer_media.load(Ordering::SeqCst)
    }

    /// Whether our camera may turn on. Before the answer to our call the other side is not known
    /// yet: the wish is kept, and checked once it is.
    fn camera_allowed(&self) -> bool {
        !self.peer_known.load(Ordering::SeqCst) || camera_allowed(self.peer_media(), self.video_call)
    }

    /// The media version we say: `CALL_MEDIA_VERSION` when this phone runs the call's video.
    fn media(&self) -> u16 {
        self.our_media
    }

    fn presenting(&self) -> Option<Presenting> {
        self.presenting.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// The `seq` of our next `CallPresent`. Taken while `presenting` is held, so the newest `seq`
    /// always carries the newest state of ours.
    fn next_present_seq(&self) -> u32 {
        self.present_seq.fetch_add(1, Ordering::SeqCst).wrapping_add(1)
    }

    /// Whether a presentation can start (2026-10-08): the call is on and both sides said media
    /// version 2 or later.
    fn can_present(&self) -> bool {
        self.connected_at().is_some() && self.peer_known.load(Ordering::SeqCst) && presents(self.media()) && presents(self.peer_media())
    }

    /// The call's video as the UI sees it: before it is ready, our camera as the user wants it
    /// and nothing available yet (`before_ready`).
    fn view(&self) -> Option<VideoState> {
        let video = self.video.as_ref()?;
        let ready = self.video_ready.load(Ordering::SeqCst);
        let state = before_ready(video.state(), ready, self.camera.load(Ordering::SeqCst));
        Some(shown(state, self.peer_media(), self.video_call))
    }

    fn heard(&self) -> Option<(bool, bool)> {
        self.remote.lock().unwrap_or_else(PoisonError::into_inner).said()
    }

    async fn shut(&self) {
        if let Some(video) = &self.video {
            video.stop().await;
        }
        self.voice.stop().await;
        self.session.close().await;
    }
}

/// An answer prepared while the call rings (2026-09-29): its connection has taken the offer and
/// gathered our candidates, but sent nothing and checks nothing until the user answers.
pub(crate) struct Prepared {
    native: Arc<NativeCall>,
    routing: CallRouting,
    at: Instant,
}

/// The call ringing here whose answer is being prepared, and the answer once ready.
#[derive(Default)]
pub(crate) struct Preparation {
    call: Option<String>,
    ready: Option<Prepared>,
    /// Answered (or being answered): nothing is prepared for it any more.
    answered: bool,
}

/// How often (ms) a prepared answer is made again while the call rings: ICE gives up 30 s after
/// it starts checking (webrtc-rs: 5 s disconnected and 25 s failed), even with nothing to check,
/// and a NAT may forget the mapping gathered.
pub(crate) const ANSWER_REFRESH_MS: u64 = 15_000;

/// A prepared answer older than this is never used: it is made afresh.
const PREPARED_FOR: Duration = Duration::from_secs(20);

/// What the app runs before a call's video devices go (`Core::set_video_detach`).
pub type VideoDetach = Arc<dyn Fn() + Send + Sync>;

/// How long the OS's answer (or decline) waits for an offer that has not arrived yet.
pub const EARLY_ANSWER: Duration = Duration::from_secs(30);

/// What the OS said about a call before its offer arrived (2026-09-28, 2026-09-29): with the app
/// closed, the phone's own call screen rings from the push, and the user may answer or decline
/// before the core has the offer. The last word counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EarlyAnswer {
    /// The next offer, until then (ms), is answered as soon as it arrives.
    Accept { until: i64 },
    /// The next offer, until then (ms), is declined as soon as it arrives: it never rings.
    Decline { until: i64 },
    /// The call being answered, early or not: never shown ringing while its answer is built.
    Accepting { call: String },
}

/// What a call whose offer just arrived gets from an early answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EarlyOutcome {
    Ring,
    Answer,
    Decline,
}

/// When (ms) an early answer given now for `wait` stops counting.
fn until(wait: Duration) -> i64 {
    now().saturating_add(i64::try_from(wait.as_millis()).unwrap_or(i64::MAX))
}

/// Before an audio device that would not start is tried again.
const AUDIO_RETRY: Duration = Duration::from_millis(300);

/// Between attempts to tell the other side about our camera, while the call lasts.
const CAMERA_RETRY: Duration = Duration::from_secs(2);

/// Between attempts to tell the other side what we present, while the call lasts.
const PRESENT_RETRY: Duration = Duration::from_secs(2);

/// Settings key: the call routing, as Settings writes it (`direct`, `auto`, `always`).
const CALL_ROUTING: &str = "call_routing";

impl Core {
    /// A step of the call's setup was reached (temporary diagnostics): the bridge says when the
    /// push arrived and when the user answered.
    pub fn mark_call_stage(&self, stage: CallStage) {
        let mut clock = self.call_clock.lock().unwrap_or_else(PoisonError::into_inner);
        CallClock::mark(&mut clock, stage, Instant::now());
    }

    /// How long each step of the call being set up took, so far (temporary diagnostics).
    pub fn call_timings(&self) -> Option<CallTimings> {
        let native = self.native_call.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let gathering = native.as_ref().map(|native| native.session.gathering()).unwrap_or_default();
        let later = [
            (CallStage::GatheringStarted, gathering.started),
            (CallStage::GatheringDone, gathering.finished),
            (CallStage::AudioDeviceStarted, native.as_ref().and_then(|native| native.voice.device_started())),
            (CallStage::FirstAudioPacket, native.as_ref().and_then(|native| native.voice.first_packet())),
        ];
        let candidates = gathering.finished.map(|_| Candidates {
            host: gathering.host,
            srflx: gathering.srflx,
            relay: gathering.relay,
            complete: gathering.complete,
        });
        let clock = self.call_clock.lock().unwrap_or_else(PoisonError::into_inner);
        clock.as_ref().map(|clock| clock.timings(&later, candidates))
    }

    /// The call whose answer is prepared, if any (for the tests).
    #[doc(hidden)]
    pub fn prepared_call(&self) -> Option<String> {
        let slot = self.preparation.lock().unwrap_or_else(PoisonError::into_inner);
        slot.ready.as_ref().and(slot.call.clone())
    }

    /// How often a prepared answer is made again while the call rings (for the tests).
    #[doc(hidden)]
    pub fn set_answer_refresh(&self, every: Duration) {
        self.answer_refresh.store(u64::try_from(every.as_millis()).unwrap_or(u64::MAX), Ordering::SeqCst);
    }

    /// The phone's audio device for calls; `None` keeps every call on the WebView.
    pub fn set_call_audio(&self, platform: Option<AudioPlatform>) {
        *self.call_audio.write().unwrap_or_else(PoisonError::into_inner) = platform;
    }

    /// Whether voice calls run natively on this phone.
    pub fn native_calls(&self) -> bool {
        self.call_audio.read().unwrap_or_else(PoisonError::into_inner).is_some()
    }

    fn audio_platform(&self) -> Result<AudioPlatform> {
        let platform = self.call_audio.read().unwrap_or_else(PoisonError::into_inner).clone();
        platform.ok_or_else(|| anyhow!("calls do not run natively on this device"))
    }

    /// The routing chosen in Settings (§17), kept here for calls answered with no WebView.
    pub async fn set_call_routing(&self, routing: CallRouting) -> Result<()> {
        self.store.set_setting(CALL_ROUTING, routing.as_str()).await
    }

    pub async fn call_routing(&self) -> CallRouting {
        let stored = self.store.setting(CALL_ROUTING).await.ok().flatten();
        stored.and_then(|routing| routing.parse().ok()).unwrap_or_default()
    }

    /// Calls the contact with our voice; returns the call's id at once, the offer goes on in the
    /// background (the other phone may need waking). If it cannot go out, the call fails.
    ///
    /// A `video` call turns our camera on as it connects (the camera permission is the app's
    /// business, asked before); either way the call can switch at any moment.
    pub async fn start_native_call(self: &Arc<Self>, contact: &str, routing: CallRouting, video: bool) -> Result<String> {
        let platform = self.audio_platform()?;
        self.mark_call_stage(CallStage::CallStarted);
        self.set_call_routing(routing).await?;
        let call = self.place_call(contact, video).await?;
        let (core, id) = (self.clone(), call.clone());
        tokio::spawn(async move {
            if core.offer_native(&id, routing, platform).await.is_err() {
                let _ = core.end_call(&id, true).await;
            }
        });
        Ok(call)
    }

    async fn offer_native(self: &Arc<Self>, call: &str, routing: CallRouting, platform: AudioPlatform) -> Result<()> {
        // The way to the other phone opens (waking it if it sleeps) while our offer gathers
        // (2026-09-29): the offer, once made, waits for that connection instead of starting it.
        let (core, id) = (self.clone(), call.to_owned());
        tokio::spawn(async move {
            if let Ok(contact) = core.call_contact(&id).await {
                let _ = core.open_direct_call(&contact).await;
            }
        });
        // The other side's media version comes with its answer.
        let Some(native) = self.open_native(call, routing, platform, None).await? else { return Ok(()) };
        let sdp = native.session.offer().await?;
        self.mark_call_stage(CallStage::OfferBuilt);
        self.offer_call_media(call, &sdp, native.media(), crate::calls::CALL_REACH).await
    }

    async fn call_contact(&self, call: &str) -> Result<ft_storage::Contact> {
        let Some(record) = self.store.call(call).await? else { bail!("no such call") };
        self.contact(&record.contact).await
    }

    /// Answers the ringing call with our voice. Answering a call already answered does nothing:
    /// CallKit and the WebView may both answer it.
    pub async fn answer_native_call(self: &Arc<Self>, call: &str, routing: CallRouting) -> Result<()> {
        let platform = self.audio_platform()?;
        self.mark_call_stage(CallStage::AnswerRequested);
        // From this moment it rings no more (2026-09-29), and the UI hears so at once: building the
        // answer takes seconds, and the answer prepared while it rang may still be under way (the
        // first one holds the answers back until it is ready).
        if self.offer_of(call).is_some() && self.mark_answering(call) {
            let ringing = self.store.call(call).await?;
            if let Some(record) = ringing.filter(|record| !record.outgoing && record.ended_at.is_none() && record.answered_at.is_none()) {
                self.announce_call(&record, CallUpdate::Answering);
            }
        }
        let _one_at_a_time = self.native_setup.lock().await;
        let Some(record) = self.store.call(call).await? else { bail!("no such call") };
        if record.outgoing || record.ended_at.is_some() {
            bail!("no such ringing call");
        }
        if record.answered_at.is_some() {
            return Ok(());
        }
        let Some((offer, media)) = self.offer_of(call) else { bail!("no offer for this call") };
        let answered = self.answer_native(call, &offer, media, routing, platform).await;
        if answered.is_err() {
            let _ = self.end_call(call, true).await;
        }
        answered
    }

    /// Answers with the answer prepared while it rang, if it is still good for this routing, or
    /// with one made now.
    async fn answer_native(self: &Arc<Self>, call: &str, offer: &str, media: u16, routing: CallRouting, platform: AudioPlatform) -> Result<()> {
        let prepared = self.take_prepared(call);
        let usable = prepared.as_ref().is_some_and(|prepared| {
            prepared.routing == routing
                && prepared.at.elapsed() < PREPARED_FOR
                && *prepared.native.session.state().borrow() == LinkState::Connecting
        });
        let (native, sdp) = match prepared {
            Some(prepared) if usable => {
                let Some(native) = self.install_native(prepared.native).await? else { bail!("the call is over") };
                let sdp = native.session.release_answer().await?;
                (native, sdp)
            }
            unusable => {
                if let Some(unusable) = unusable {
                    unusable.native.shut().await;
                }
                let Some(native) = self.open_native(call, routing, platform, Some(media)).await? else { bail!("the call is over") };
                let sdp = native.session.answer(offer).await?;
                (native, sdp)
            }
        };
        self.mark_call_stage(CallStage::AnswerBuilt);
        self.answer_call_media(call, &sdp, native.media()).await
    }

    /// Their call rings here (2026-09-29): its answer is prepared in the background, and made
    /// again every so often while it rings. Nothing reaches the caller until the user answers
    /// (`MediaSession::prepare_answer`); the microphone and the camera stay untouched.
    pub(crate) fn prepare_while_ringing(&self, call: &str) {
        if !self.native_calls() {
            return;
        }
        let stale = {
            let mut slot = self.preparation.lock().unwrap_or_else(PoisonError::into_inner);
            let stale = slot.ready.take();
            *slot = Preparation { call: Some(call.to_owned()), ready: None, answered: false };
            stale
        };
        let Some(core) = self.this.upgrade() else { return };
        let call = call.to_owned();
        tokio::spawn(async move {
            if let Some(stale) = stale {
                stale.native.shut().await;
            }
            core.keep_prepared(&call).await;
        });
    }

    async fn keep_prepared(self: &Arc<Self>, call: &str) {
        let mut first = true;
        loop {
            let prepared = {
                // The first preparation holds answers back: one that comes meanwhile takes it
                // when ready, sooner than it could make its own.
                let _answers_wait = if first { Some(self.native_setup.lock().await) } else { None };
                if !self.still_preparing(call) {
                    return;
                }
                self.prepare_answer(call).await
            };
            first = false;
            // One that could not be made is made by the answer.
            if let Ok(prepared) = prepared {
                self.keep_prepared_answer(call, prepared).await;
            }
            let every = Duration::from_millis(self.answer_refresh.load(Ordering::SeqCst));
            tokio::time::sleep(every).await;
            if !self.still_preparing(call) {
                return;
            }
        }
    }

    async fn prepare_answer(self: &Arc<Self>, call: &str) -> Result<Prepared> {
        let platform = self.audio_platform()?;
        let Some((offer, media)) = self.offer_of(call) else { bail!("no offer for this call") };
        let routing = self.call_routing().await;
        let native = self.build_native(call, routing, platform, Some(media)).await?;
        if let Err(error) = native.session.prepare_answer(&offer).await {
            native.shut().await;
            return Err(error);
        }
        Ok(Prepared { native, routing, at: Instant::now() })
    }

    fn still_preparing(&self, call: &str) -> bool {
        let slot = self.preparation.lock().unwrap_or_else(PoisonError::into_inner);
        slot.call.as_deref() == Some(call) && !slot.answered
    }

    /// Keeps `prepared` for the call, if it still rings unanswered; the one it replaces, or itself
    /// otherwise, is closed.
    async fn keep_prepared_answer(&self, call: &str, prepared: Prepared) {
        let unused = {
            let mut slot = self.preparation.lock().unwrap_or_else(PoisonError::into_inner);
            if slot.call.as_deref() == Some(call) && !slot.answered {
                slot.ready.replace(prepared)
            } else {
                Some(prepared)
            }
        };
        if let Some(unused) = unused {
            unused.native.shut().await;
        }
    }

    /// The call is being answered: its prepared answer, if ready, and no more preparing.
    fn take_prepared(&self, call: &str) -> Option<Prepared> {
        let mut slot = self.preparation.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.call.as_deref() != Some(call) {
            return None;
        }
        slot.answered = true;
        slot.ready.take()
    }

    /// The call was answered or is over: whatever was prepared for it goes.
    pub(crate) async fn discard_prepared(&self, call: &str) {
        let unused = {
            let mut slot = self.preparation.lock().unwrap_or_else(PoisonError::into_inner);
            if slot.call.as_deref() != Some(call) {
                return;
            }
            std::mem::take(&mut *slot).ready
        };
        if let Some(unused) = unused {
            // Never shown: it has no views to take away.
            unused.native.shut().await;
        }
    }

    /// The OS answered (CallKit on a locked iPhone, with no WebView): the ringing call is answered
    /// here with the routing the core keeps, a video call too (native video, 2026-09-29): its
    /// camera is wanted and held until the app is on the screen. `false` when nothing rings.
    ///
    /// When nothing rings yet, the answer waits `EARLY_ANSWER` for the offer (2026-09-28): a
    /// suspended iPhone rings through PushKit and may be answered before its socket to the router
    /// is back and the offer arrives.
    pub async fn answer_ringing_call(self: &Arc<Self>) -> Result<bool> {
        self.answer_ringing_call_within(EARLY_ANSWER).await
    }

    /// Like `answer_ringing_call`, with the answer waiting `wait` for an offer yet to come.
    pub async fn answer_ringing_call_within(self: &Arc<Self>, wait: Duration) -> Result<bool> {
        let current = self.current_call().await?;
        if current.is_none() {
            self.answer_early(EarlyAnswer::Accept { until: until(wait) });
        }
        let Some(current) = current else { return Ok(false) };
        if current.phase != CallPhase::Ringing {
            return Ok(false);
        }
        let routing = self.call_routing().await;
        self.answer_native_call(&current.call, routing).await?;
        Ok(true)
    }

    /// The OS's audio session for the CallKit call of `generation`, which grows with each call
    /// (2026-09-28): a late event of an older call (its `didDeactivate` after the new call's
    /// `didActivate`) changes nothing.
    pub async fn set_call_audio_session(&self, active: bool, generation: u64) -> Result<()> {
        let newest = self.audio_generation.fetch_max(generation, Ordering::SeqCst);
        if generation < newest {
            return Ok(());
        }
        self.set_call_audio_active(active).await
    }

    /// Mutes or unmutes our voice in the call.
    pub async fn mute_call(&self, call: &str, muted: bool) -> Result<()> {
        let Some(native) = self.native_of(call) else { bail!("no native call") };
        native.voice.set_muted(muted).await;
        self.announce(&native, CallUpdate::Muted { muted });
        Ok(())
    }

    /// Mutes or unmutes the call going on (CallKit's button names no call).
    pub async fn mute_current_call(&self, muted: bool) -> Result<()> {
        let native = self.native_call.lock().unwrap_or_else(PoisonError::into_inner).clone();
        match native {
            Some(native) => self.mute_call(&native.call, muted).await,
            None => Ok(()),
        }
    }

    /// The OS activated (or took back) the call's audio session: CallKit's `didActivate` on iOS.
    /// It may come before the call's connection exists: the voice learns it when it is made.
    pub async fn set_call_audio_active(&self, active: bool) -> Result<()> {
        self.call_audio_active.store(active, Ordering::SeqCst);
        let native = self.native_call.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let Some(native) = native else { return Ok(()) };
        let Err(error) = native.voice.set_session_active(active).await else { return Ok(()) };
        if !active {
            return Err(error);
        }
        // The audio unit would not start (2026-09-28): once more, then the call fails rather than
        // going on in silence.
        tokio::time::sleep(AUDIO_RETRY).await;
        if let Err(error) = native.voice.set_session_active(true).await {
            let _ = self.end_call(&native.call, true).await;
            return Err(error);
        }
        Ok(())
    }

    /// Whether the voice of the call going on has its device running (temporary diagnostics).
    pub async fn call_device_running(&self) -> bool {
        let native = self.native_call.lock().unwrap_or_else(PoisonError::into_inner).clone();
        match native {
            Some(native) => native.voice.is_running().await,
            None => false,
        }
    }

    /// Hangs up whatever call is going on (CallKit's end, or the notification's button). An answer
    /// still waiting for its offer is taken back.
    pub async fn end_current_call(&self) -> Result<()> {
        self.early_answer.lock().unwrap_or_else(PoisonError::into_inner).take();
        let active = self.active_call.lock().unwrap_or_else(PoisonError::into_inner).clone();
        match active {
            Some(call) => self.end_call(&call, false).await,
            None => Ok(()),
        }
    }

    /// The OS declined (2026-09-29): the incoming call notification's decline, or CallKit's end
    /// of an incoming call whose voice never connected. The call going on ends (declined if it
    /// rings). When nothing rings yet, the decline waits `EARLY_ANSWER` for the offer, which is
    /// then declined as it arrives and never rings: the push rang the phone before the offer
    /// came, as with an early answer.
    pub async fn decline_ringing_call(&self) -> Result<()> {
        self.decline_ringing_call_within(EARLY_ANSWER).await
    }

    /// Like `decline_ringing_call`, with the decline waiting `wait` for an offer yet to come.
    pub async fn decline_ringing_call_within(&self, wait: Duration) -> Result<()> {
        match self.current_call().await? {
            Some(current) => {
                self.early_answer.lock().unwrap_or_else(PoisonError::into_inner).take();
                self.end_call(&current.call, false).await
            }
            None => {
                self.answer_early(EarlyAnswer::Decline { until: until(wait) });
                Ok(())
            }
        }
    }

    /// What the OS said before the offer came: it replaces whatever it said before.
    fn answer_early(&self, answer: EarlyAnswer) {
        *self.early_answer.lock().unwrap_or_else(PoisonError::into_inner) = Some(answer);
    }

    /// `call` is being answered: the UI shows it connecting. `false` if it already was.
    fn mark_answering(&self, call: &str) -> bool {
        let mut early = self.early_answer.lock().unwrap_or_else(PoisonError::into_inner);
        if matches!(&*early, Some(EarlyAnswer::Accepting { call: accepting }) if accepting == call) {
            return false;
        }
        *early = Some(EarlyAnswer::Accepting { call: call.to_owned() });
        true
    }

    /// Whether `call`, unanswered, is being answered by an early answer (or will be, the moment
    /// it is taken): the UI shows it connecting, never ringing.
    fn answered_early(&self, call: &str) -> bool {
        match &*self.early_answer.lock().unwrap_or_else(PoisonError::into_inner) {
            Some(EarlyAnswer::Accepting { call: accepting }) => accepting == call,
            Some(EarlyAnswer::Accept { until }) => now() <= *until,
            _ => false,
        }
    }

    /// The ringing or active call, if any.
    pub async fn current_call(&self) -> Result<Option<CurrentCall>> {
        let active = self.active_call.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let Some(call) = active else { return Ok(None) };
        let Some(record) = self.store.call(&call).await? else { return Ok(None) };
        if record.ended_at.is_some() || stale(&record, now()) {
            return Ok(None);
        }
        let native = self.native_of(&call);
        let connected_at = native.as_ref().and_then(|native| native.connected_at());
        let phase = match (record.answered_at.is_some(), record.outgoing, connected_at.is_some()) {
            (false, true, _) => CallPhase::Calling,
            // Being answered (2026-09-29), maybe before its offer came: never ringing again.
            (false, false, _) if self.answered_early(&call) => CallPhase::Connecting,
            (false, false, _) => CallPhase::Ringing,
            (true, _, true) => CallPhase::Active,
            (true, _, false) => CallPhase::Connecting,
        };
        Ok(Some(CurrentCall {
            offer: (phase == CallPhase::Ringing).then(|| self.offer_of(&call).map(|(sdp, _)| sdp)).flatten(),
            native: native.is_some(),
            muted: native.as_ref().is_some_and(|native| native.voice.is_muted()),
            connected_at,
            video_state: native.as_ref().and_then(|native| native.view()),
            presenting: native.as_ref().and_then(|native| native.presenting()),
            can_present: native.as_ref().is_some_and(|native| native.can_present()),
            call,
            contact: record.contact,
            video: record.video,
            outgoing: record.outgoing,
            phase,
        }))
    }

    /// The offer of `call` arrived: what the OS said before it came, if it is still in time.
    /// Answered, the call is marked as being answered until it is (`answered_early`).
    pub(crate) fn take_early_answer(&self, call: &str) -> EarlyOutcome {
        let mut early = self.early_answer.lock().unwrap_or_else(PoisonError::into_inner);
        let outcome = match early.take() {
            Some(EarlyAnswer::Accept { until }) if now() <= until => EarlyOutcome::Answer,
            Some(EarlyAnswer::Decline { until }) if now() <= until => EarlyOutcome::Decline,
            _ => EarlyOutcome::Ring,
        };
        if outcome == EarlyOutcome::Answer {
            *early = Some(EarlyAnswer::Accepting { call: call.to_owned() });
        }
        outcome
    }

    /// A call the OS answered before its offer came is answered now, in the background, with the
    /// routing the core keeps.
    pub(crate) fn answer_offered_early(&self, call: &str) {
        let Some(core) = self.this.upgrade() else { return };
        let call = call.to_owned();
        tokio::spawn(async move {
            let routing = core.call_routing().await;
            let _ = core.answer_native_call(&call, routing).await;
        });
    }

    /// Their offer and call media version, kept while the call rings here.
    pub(crate) fn remember_offer(&self, call: &str, sdp: &str, media: u16) {
        *self.ringing_offer.lock().unwrap_or_else(PoisonError::into_inner) = Some((call.to_owned(), sdp.to_owned(), media));
    }

    fn offer_of(&self, call: &str) -> Option<(String, u16)> {
        let ringing = self.ringing_offer.lock().unwrap_or_else(PoisonError::into_inner);
        ringing.as_ref().filter(|(id, _, _)| id == call).map(|(_, sdp, media)| (sdp.clone(), *media))
    }

    fn native_of(&self, call: &str) -> Option<Arc<NativeCall>> {
        let native = self.native_call.lock().unwrap_or_else(PoisonError::into_inner);
        native.as_ref().filter(|native| native.call == call).cloned()
    }

    /// The media for the call: its connection, with our audio and video tracks, its voice and
    /// its video. `peer_media` is the other side's call media version, if known. `None` if the
    /// call ended meanwhile.
    async fn open_native(
        self: &Arc<Self>,
        call: &str,
        routing: CallRouting,
        platform: AudioPlatform,
        peer_media: Option<u16>,
    ) -> Result<Option<Arc<NativeCall>>> {
        let native = self.build_native(call, routing, platform, peer_media).await?;
        self.install_native(native).await
    }

    /// The call's media, made but not yet the call's: nothing runs, nothing is followed.
    async fn build_native(&self, call: &str, routing: CallRouting, platform: AudioPlatform, peer_media: Option<u16>) -> Result<Arc<NativeCall>> {
        let Some(record) = self.store.call(call).await? else { bail!("no such call") };
        let mut config = self.transport.media_config();
        config.routing = routing;
        let session = MediaSession::open(&config).await?;
        self.mark_call_stage(CallStage::ConnectionBuilt);
        let voice = Voice::for_session(&session, platform);
        let video = self.video_platform().map(|platform| Video::for_session(&session, platform));
        let our_media = media_version(video.is_some()).min(self.call_media_ceiling.load(Ordering::SeqCst));
        let native = Arc::new(NativeCall {
            call: call.to_owned(),
            contact: record.contact,
            session,
            voice,
            video,
            video_call: record.video,
            peer_media: AtomicU16::new(peer_media.unwrap_or(0)),
            peer_known: AtomicBool::new(peer_media.is_some()),
            camera: AtomicBool::new(record.video),
            video_ready: AtomicBool::new(false),
            video_lock: tokio::sync::Mutex::new(()),
            sent_seq: AtomicU32::new(0),
            said: Mutex::new((false, false)),
            remote: Mutex::new(RemoteCamera::default()),
            connected_at: AtomicI64::new(0),
            our_media,
            presenting: Mutex::new(None),
            present_seq: AtomicU32::new(0),
            heard_present: Mutex::new(None),
        });
        Ok(native)
    }

    /// Makes `native` the call's media: the voice learns the audio session, and the connection is
    /// followed. `None` if the call ended meanwhile.
    async fn install_native(self: &Arc<Self>, native: Arc<NativeCall>) -> Result<Option<Arc<NativeCall>>> {
        let call = native.call.as_str();
        // A new call is shown on the call screen until the WebView says otherwise.
        self.call_shown.store(true, Ordering::SeqCst);
        let replaced = self.native_call.lock().unwrap_or_else(PoisonError::into_inner).replace(native.clone());
        if let Some(replaced) = replaced {
            self.shut_native(&replaced).await;
        }
        // Read after the call is in its place: an activation from now on reaches it directly.
        native.voice.set_session_active(self.call_audio_active.load(Ordering::SeqCst)).await?;
        // Hung up while the connection was being made: close_call found nothing to stop then.
        if self.store.call(call).await?.is_none_or(|record| record.ended_at.is_some()) {
            self.drop_native(call).await;
            return Ok(None);
        }
        self.follow(&native);
        Ok(Some(native))
    }

    /// Follows the connection: once connected the voice starts and the UI hears it; if it
    /// fails the call ends as failed.
    fn follow(self: &Arc<Self>, native: &Arc<NativeCall>) {
        let (core, native) = (Arc::downgrade(self), native.clone());
        tokio::spawn(async move {
            let mut state = native.session.state();
            loop {
                let now = *state.borrow_and_update();
                match now {
                    LinkState::Connecting => {}
                    LinkState::Connected => {
                        let Some(core) = core.upgrade() else { break };
                        core.native_connected(&native).await;
                    }
                    LinkState::Failed => {
                        if let Some(core) = core.upgrade() {
                            let _ = core.end_call(&native.call, true).await;
                        }
                        break;
                    }
                    LinkState::Closed => break,
                }
                if state.changed().await.is_err() {
                    break;
                }
            }
        });
    }

    async fn native_connected(&self, native: &Arc<NativeCall>) {
        if native.connected_at.compare_exchange(0, now(), Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return;
        }
        self.mark_call_stage(CallStage::Connected);
        if native.voice.connected().await.is_err() {
            // No microphone or speaker: a call nobody can hear is a failed call.
            let _ = self.end_call(&native.call, true).await;
            return;
        }
        self.announce(native, CallUpdate::Connected);
        self.video_connected(native).await;
    }

    /// The connection is up: the video follows the cameras from now on. A video call turns our
    /// camera on; an older app's video call shows its camera at once (it never says so).
    async fn video_connected(&self, native: &Arc<NativeCall>) {
        let Some(video) = &native.video else { return };
        if video.connected().await.is_err() {
            // The voice goes on without video; a camera wanted cannot run.
            if native.camera.swap(false, Ordering::SeqCst) {
                self.announce(native, CallUpdate::CameraFailed);
            }
            return;
        }
        self.follow_video(native, video);
        native.video_ready.store(true, Ordering::SeqCst);
        if remote_on_at_connect(native.peer_media(), native.video_call) {
            let _ = video.set_remote(true, false).await;
        } else {
            let _ = self.apply_remote(native).await;
        }
        if native.camera.load(Ordering::SeqCst) {
            if native.camera_allowed() {
                if self.apply_camera(native).await.is_err() {
                    // Nobody asked for it right now, so nobody gets an error: the UI hears it,
                    // and the call goes on as voice with the camera off.
                    native.camera.store(false, Ordering::SeqCst);
                    self.announce(native, CallUpdate::CameraFailed);
                }
            } else {
                // Kept before the answer, for an older app's voice call: it has no video line.
                native.camera.store(false, Ordering::SeqCst);
            }
        }
    }

    /// Follows the call's video: the UI hears every state, and the other side every change of
    /// our camera. The state it has already (`available`, set as the video connected) is heard
    /// first: a new receiver counts it as seen, and waiting for a change would never tell it.
    fn follow_video(&self, native: &Arc<NativeCall>, video: &Video) {
        let (core, weak) = (self.this.clone(), Arc::downgrade(native));
        let mut changes = video.changes();
        tokio::spawn(async move {
            loop {
                let state = *changes.borrow_and_update();
                let (Some(core), Some(native)) = (core.upgrade(), weak.upgrade()) else { break };
                if core.native_of(&native.call).is_none() {
                    break;
                }
                core.video_changed(&native, state).await;
                drop((core, native));
                if changes.changed().await.is_err() {
                    break;
                }
            }
        });
    }

    async fn video_changed(&self, native: &Arc<NativeCall>, state: VideoState) {
        self.announce(native, CallUpdate::Video(shown(state, native.peer_media(), native.video_call)));
        // A picture from the other side with nothing said about its camera: it is on.
        if state.shape.is_some() && !state.remote && native.heard().is_none() {
            if let Some(video) = &native.video {
                let _ = video.set_remote(true, false).await;
            }
        }
        if !speaks_camera_state(native.peer_media()) {
            return;
        }
        let said = camera_said(&state);
        let seq = {
            let mut last = native.said.lock().unwrap_or_else(PoisonError::into_inner);
            if *last == said {
                return;
            }
            *last = said;
            native.sent_seq.fetch_add(1, Ordering::SeqCst).wrapping_add(1)
        };
        let Some(core) = self.this.upgrade() else { return };
        let native = native.clone();
        tokio::spawn(async move { core.deliver_camera_state(&native, seq, said).await });
    }

    /// Tells the other side about our camera until it gets through, a newer state replaces it or
    /// the call ends.
    async fn deliver_camera_state(&self, native: &NativeCall, seq: u32, (video, paused): (bool, bool)) {
        let Ok(call) = MessageId::parse(&native.call) else { return };
        let Ok(contact) = self.contact(&native.contact).await else { return };
        let packet = Packet::new(Body::CallMedia { call, seq, video, paused });
        loop {
            if native.sent_seq.load(Ordering::SeqCst) != seq || self.native_of(&native.call).is_none() {
                return;
            }
            if self.transmit_direct(&contact, &packet).await.unwrap_or(false) {
                return;
            }
            tokio::time::sleep(CAMERA_RETRY).await;
        }
    }

    /// The other side said what its camera does (`CallMedia`): the newest state wins.
    pub(crate) async fn call_media_received(&self, contact: &ft_storage::Contact, call: MessageId, seq: u32, video: bool, paused: bool) -> Result<()> {
        let Some(native) = self.native_of(&call.to_string()) else { return Ok(()) };
        if native.contact != contact.device_id || native.video.is_none() {
            return Ok(());
        }
        let newer = native.remote.lock().unwrap_or_else(PoisonError::into_inner).hear(seq, video, paused);
        if !newer || !native.video_ready.load(Ordering::SeqCst) {
            // Old news, or kept for when the video is ready.
            return Ok(());
        }
        self.apply_remote(&native).await
    }

    /// Presents `plugin` in the call going on (2026-10-08), with the chat `file` it shows, if
    /// any: both screens show it, ours leading. Only once the call is on and both apps show
    /// presentations (`PEER_CANNOT_PRESENT` otherwise). The core opens nothing and reads nothing:
    /// what the plugin shows travels over `ft.live`. Never while the other side presents, and the
    /// file must be a file message of this conversation, either way.
    pub async fn present_in_call(&self, call: &str, plugin: &str, file: Option<MessageId>) -> Result<()> {
        let Some(native) = self.native_of(call) else { bail!("no native call") };
        ensure!(native.connected_at().is_some(), "the call is not on yet");
        ensure!(native.can_present(), crate::PEER_CANNOT_PRESENT);
        ensure!(!plugin.is_empty(), "no plugin to present");
        if let Some(file) = file {
            ensure!(self.file_in(&native.contact, &file.to_string()).await?, "not a file of this conversation");
        }
        let seq = {
            let mut presenting = native.presenting.lock().unwrap_or_else(PoisonError::into_inner);
            ensure!(presenting.as_ref().is_none_or(|now| now.by == PresentedBy::Me), "the other side is presenting");
            *presenting = Some(Presenting { plugin: plugin.to_owned(), file, by: PresentedBy::Me });
            native.next_present_seq()
        };
        self.announce_presenting(&native);
        self.send_presenting(&native, seq, Some(plugin.to_owned()), file);
        Ok(())
    }

    /// Whether `message_id` is a file message of the conversation with `contact`, either way.
    async fn file_in(&self, contact: &str, message_id: &str) -> Result<bool> {
        let Some(message) = self.store.message(message_id).await? else { return Ok(false) };
        Ok(message.contact == contact && self.store.file(message_id).await?.is_some())
    }

    /// Stops our presentation in the call going on; nothing to do when we present nothing.
    pub async fn stop_presenting(&self, call: &str) -> Result<()> {
        let Some(native) = self.native_of(call) else { bail!("no native call") };
        let seq = {
            let mut presenting = native.presenting.lock().unwrap_or_else(PoisonError::into_inner);
            let ours = presenting.as_ref().is_some_and(|now| now.by == PresentedBy::Me);
            if ours {
                *presenting = None;
            }
            ours.then(|| native.next_present_seq())
        };
        if let Some(seq) = seq {
            self.announce_presenting(&native);
            self.send_presenting(&native, seq, None, None);
        }
        Ok(())
    }

    /// Tells the other side what we present, with its `seq`, in the background.
    fn send_presenting(&self, native: &Arc<NativeCall>, seq: u32, plugin: Option<String>, file: Option<MessageId>) {
        let Some(core) = self.this.upgrade() else { return };
        let native = native.clone();
        tokio::spawn(async move { core.deliver_presenting(&native, seq, plugin, file).await });
    }

    /// Like `deliver_camera_state`: until it gets through, a newer word replaces it or the call ends.
    async fn deliver_presenting(&self, native: &NativeCall, seq: u32, plugin: Option<String>, file: Option<MessageId>) {
        let Ok(call) = MessageId::parse(&native.call) else { return };
        let Ok(contact) = self.contact(&native.contact).await else { return };
        let packet = Packet::new(Body::CallPresent { call, seq, plugin, file });
        loop {
            if native.present_seq.load(Ordering::SeqCst) != seq || self.native_of(&native.call).is_none() {
                return;
            }
            if self.transmit_direct(&contact, &packet).await.unwrap_or(false) {
                return;
            }
            tokio::time::sleep(PRESENT_RETRY).await;
        }
    }

    /// The other side says what it presents (`CallPresent`): the newest word wins. Ignored when
    /// it is not this call's contact, or when this phone said it does not show presentations.
    pub(crate) async fn call_present_received(&self, contact: &ft_storage::Contact, call: MessageId, seq: u32, plugin: Option<String>, file: Option<MessageId>) -> Result<()> {
        let Some(native) = self.native_of(&call.to_string()) else { return Ok(()) };
        if native.contact != contact.device_id || !presents(native.media()) {
            return Ok(());
        }
        if !newer_present(&mut native.heard_present.lock().unwrap_or_else(PoisonError::into_inner), seq) {
            return Ok(());
        }
        let mine_wins = mine_wins_clash(self.device_id().as_str(), &native.contact);
        let changed = {
            let mut presenting = native.presenting.lock().unwrap_or_else(PoisonError::into_inner);
            match after_their_present(presenting.as_ref(), plugin, file, mine_wins) {
                Some(next) => {
                    *presenting = next;
                    true
                }
                None => false,
            }
        };
        if changed {
            self.announce_presenting(&native);
        }
        Ok(())
    }

    /// Tells the UI what is presented now, as a whole.
    fn announce_presenting(&self, native: &NativeCall) {
        let now = native.presenting();
        let update = CallUpdate::Presenting {
            plugin: now.as_ref().map(|now| now.plugin.clone()),
            file: now.as_ref().and_then(|now| now.file),
            by: now.as_ref().map(|now| now.by),
        };
        self.announce(native, update);
    }

    /// Shows the other side's camera as it last said.
    async fn apply_remote(&self, native: &NativeCall) -> Result<()> {
        let Some(video) = &native.video else { return Ok(()) };
        let _one_at_a_time = native.video_lock.lock().await;
        let Some((on, held)) = native.heard() else { return Ok(()) };
        video.set_remote(on, held).await?;
        Ok(())
    }

    /// Runs our camera as the user wants it, held if the phone holds it.
    async fn apply_camera(&self, native: &NativeCall) -> Result<VideoState> {
        let Some(video) = &native.video else { bail!("no video on this device") };
        let _one_at_a_time = native.video_lock.lock().await;
        video.set_paused(self.camera_held()).await?;
        video.set_camera(native.camera.load(Ordering::SeqCst)).await
    }

    /// Holds or gives back the camera of the call going on, as the app and its call screen are.
    async fn apply_hold(&self) -> Result<()> {
        let native = self.native_call.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let Some(native) = native else { return Ok(()) };
        let Some(video) = &native.video else { return Ok(()) };
        if !native.video_ready.load(Ordering::SeqCst) {
            // Applied as the video gets ready.
            return Ok(());
        }
        let _one_at_a_time = native.video_lock.lock().await;
        video.set_paused(self.camera_held()).await?;
        Ok(())
    }

    fn camera_held(&self) -> bool {
        held(self.app_visible.load(Ordering::SeqCst), self.call_shown.load(Ordering::SeqCst))
    }

    fn video_platform(&self) -> Option<VideoPlatform> {
        self.call_video.read().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// The other side's answer to our native call, with its call media version. Nothing to do
    /// for a WebView call.
    pub(crate) async fn accept_native_answer(&self, call: &str, sdp: &str, media: u16) -> Result<()> {
        match self.native_of(call) {
            Some(native) => {
                native.peer_media.store(media, Ordering::SeqCst);
                native.peer_known.store(true, Ordering::SeqCst);
                native.session.accept(sdp).await
            }
            None => Ok(()),
        }
    }

    /// Stops the call's voice and closes its connection, if it has them.
    pub(crate) async fn drop_native(&self, call: &str) {
        self.discard_prepared(call).await;
        {
            let mut ringing = self.ringing_offer.lock().unwrap_or_else(PoisonError::into_inner);
            if ringing.as_ref().is_some_and(|(id, _, _)| id == call) {
                *ringing = None;
            }
        }
        let taken = {
            let mut native = self.native_call.lock().unwrap_or_else(PoisonError::into_inner);
            if native.as_ref().is_some_and(|native| native.call == call) {
                native.take()
            } else {
                None
            }
        };
        if let Some(native) = taken {
            self.shut_native(&native).await;
            // The follower stops with the call and would not tell it: the UI hears the video is
            // gone (nothing available, no camera) before it hears the call ended.
            if let Some(video) = &native.video {
                self.announce(&native, CallUpdate::Video(video.state()));
            }
            // CallKit gives the session back at the end; the next call waits for its own.
            self.call_audio_active.store(false, Ordering::SeqCst);
        }
    }

    /// The phone's camera and display for calls (native video, 2026-09-29); `None` leaves native
    /// calls with voice only (by default, `ft_media::platform_video()`).
    pub fn set_call_video(&self, platform: Option<VideoPlatform>) {
        *self.call_video.write().unwrap_or_else(PoisonError::into_inner) = platform;
    }

    /// Says at most this call media version in our offers and answers from the next call on: a
    /// phone that speaks as an older app does, for the tests (`tests/native_present.rs`).
    #[doc(hidden)]
    pub fn set_call_media_version(&self, version: u16) {
        self.call_media_ceiling.store(version, Ordering::SeqCst);
    }

    /// What the app runs right before a call's video devices go: the bridge takes the native
    /// views away (on iOS the layers belong to the devices). It may block.
    pub fn set_video_detach(&self, detach: Option<VideoDetach>) {
        *self.video_detach.write().unwrap_or_else(PoisonError::into_inner) = detach;
    }

    /// Our camera on or off in the call, as the user chose (the camera permission is the app's
    /// business, asked before). Before the call connects it is kept for then, even before the
    /// answer to our call says what the other side runs. An older app's voice call has no video
    /// line: the camera cannot turn on.
    pub async fn set_call_camera(&self, call: &str, on: bool) -> Result<VideoState> {
        let Some(native) = self.native_of(call) else { bail!("no native call") };
        if native.video.is_none() {
            bail!("no video on this device");
        }
        if on && !native.camera_allowed() {
            bail!("the other side has no video in this call");
        }
        native.camera.store(on, Ordering::SeqCst);
        if native.video_ready.load(Ordering::SeqCst) {
            let state = self.apply_camera(&native).await?;
            return Ok(shown(state, native.peer_media(), native.video_call));
        }
        native.view().ok_or_else(|| anyhow!("no video on this device"))
    }

    /// The other camera: front to back and back again, on or off.
    pub async fn switch_call_camera(&self, call: &str) -> Result<VideoState> {
        let Some(native) = self.native_of(call) else { bail!("no native call") };
        let Some(video) = &native.video else { bail!("no video on this device") };
        let state = video.switch_camera().await?;
        Ok(shown(state, native.peer_media(), native.video_call))
    }

    /// Whether the call screen shows the video: the WebView lays it out, or says `null` when it
    /// leaves the screen. Off it, our camera is held.
    pub async fn set_call_shown(&self, shown: bool) -> Result<()> {
        self.call_shown.store(shown, Ordering::SeqCst);
        self.apply_hold().await
    }

    /// The app came to the screen or left it (`NativeCallEvent::Visible`): away, our camera is
    /// held (iOS stops it in the background anyway).
    pub async fn set_app_visible(&self, visible: bool) -> Result<()> {
        self.app_visible.store(visible, Ordering::SeqCst);
        self.apply_hold().await
    }

    /// The system's call screen asked for video (`NativeCallEvent::VideoRequested`: CallKit's
    /// video button, the notification's camera action): our camera turns on in the call going on.
    pub async fn request_call_video(&self) -> Result<()> {
        let native = self.native_call.lock().unwrap_or_else(PoisonError::into_inner).clone();
        match native {
            Some(native) => self.set_call_camera(&native.call, true).await.map(|_| ()),
            None => Ok(()),
        }
    }

    /// The call's video as the UI shows it, while it runs here.
    pub fn call_video(&self, call: &str) -> Option<VideoState> {
        self.native_of(call)?.view()
    }

    /// Stops the call's media. The app takes the video views away first: only then may the
    /// devices go.
    async fn shut_native(&self, native: &NativeCall) {
        if native.video.is_some() {
            let detach = self.video_detach.read().unwrap_or_else(PoisonError::into_inner).clone();
            if let Some(detach) = detach {
                let _ = tokio::task::spawn_blocking(move || detach()).await;
            }
        }
        native.shut().await;
    }

    fn announce(&self, native: &NativeCall, update: CallUpdate) {
        let _ = self.events.send(Event::Call { contact: native.contact.clone(), call: native.call.clone(), update });
    }
}

/// What the other side last said about its camera (`CallMedia`, native video 2026-09-29). A state,
/// not a change: the highest `seq` wins, so a repeat or a late one changes nothing.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RemoteCamera {
    seq: Option<u32>,
    video: bool,
    paused: bool,
}

impl RemoteCamera {
    /// Takes a `CallMedia`: `false` when it is a repeat or older than the one kept.
    fn hear(&mut self, seq: u32, video: bool, paused: bool) -> bool {
        if self.seq.is_some_and(|kept| seq <= kept) {
            return false;
        }
        *self = Self { seq: Some(seq), video, paused };
        true
    }

    /// The camera the other side last said, if it said anything: (on, held).
    fn said(&self) -> Option<(bool, bool)> {
        self.seq.map(|_| (self.video, self.paused))
    }
}

/// The call media version we say in our offer or answer: `CALL_MEDIA_VERSION` when this phone
/// runs the call's video, 0 (an older app's word) when it does not.
fn media_version(has_video: bool) -> u16 {
    if has_video {
        ft_protocol::CALL_MEDIA_VERSION
    } else {
        0
    }
}

/// Whether the other side reads our `CallMedia`: from media version 1 on.
fn speaks_camera_state(peer_media: u16) -> bool {
    peer_media >= ft_protocol::CALL_MEDIA_CAMERA
}

/// Whether a side at this call media version shows presentations (`CallPresent`): from 2 on.
fn presents(media: u16) -> bool {
    media >= ft_protocol::CALL_MEDIA_PRESENT
}

/// Takes a `CallPresent`'s `seq`: `false` when it is a repeat or older than the one kept.
fn newer_present(kept: &mut Option<u32>, seq: u32) -> bool {
    if kept.is_some_and(|kept| seq <= kept) {
        return false;
    }
    *kept = Some(seq);
    true
}

/// What a `CallPresent` from the other side makes of what is presented here; `None` when
/// nothing changes. Theirs replaces anything, except ours when ours wins a clash (`mine_wins`);
/// their stop ends only theirs.
fn after_their_present(now: Option<&Presenting>, plugin: Option<String>, file: Option<MessageId>, mine_wins: bool) -> Option<Option<Presenting>> {
    let ours = now.is_some_and(|now| now.by == PresentedBy::Me);
    let theirs = now.is_some_and(|now| now.by == PresentedBy::Them);
    match plugin {
        Some(_) if ours && mine_wins => None,
        Some(plugin) => Some(Some(Presenting { plugin, file, by: PresentedBy::Them })),
        None if theirs => Some(None),
        None => None,
    }
}

/// Both pressed Present at once and each heard the other's while showing its own: the side whose
/// device id sorts first keeps its presentation and the other takes it, so both show the same.
fn mine_wins_clash(me: &str, them: &str) -> bool {
    me < them
}

/// Whether our camera may turn on: an older app's voice call has no video line at all.
fn camera_allowed(peer_media: u16, video_call: bool) -> bool {
    video_call || speaks_camera_state(peer_media)
}

/// An older app's video call sends its camera from the start and never says so (no `CallMedia`).
fn remote_on_at_connect(peer_media: u16, video_call: bool) -> bool {
    video_call && !speaks_camera_state(peer_media)
}

/// What a `CallMedia` says of our camera: (on, held). Held means nothing when it is off.
fn camera_said(state: &VideoState) -> (bool, bool) {
    (state.camera, state.camera && state.paused)
}

/// Whether the phone holds our camera: the app is away or the call screen does not show it.
fn held(app_visible: bool, call_shown: bool) -> bool {
    !(app_visible && call_shown)
}

/// The video's state until it is ready (`video_ready`): our camera as the user wants it, and not
/// available yet. The engine says `available` as the connection comes up, a moment before the
/// core can apply the camera; a camera turned on in between would be kept as a wish.
fn before_ready(state: VideoState, ready: bool, wish: bool) -> VideoState {
    if ready {
        state
    } else {
        VideoState { camera: wish, available: false, ..state }
    }
}

/// The call's video as the UI sees it: with an older app's voice call, never available.
fn shown(state: VideoState, peer_media: u16, video_call: bool) -> VideoState {
    VideoState { available: state.available && camera_allowed(peer_media, video_call), ..state }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases_compare() {
        assert_ne!(CallPhase::Ringing, CallPhase::Active);
    }

    // Native video: the other side's camera is a state; the highest `seq` wins (docs/video-nativo.md §1).
    #[test]
    fn the_newest_camera_state_wins_and_a_repeat_or_a_late_one_changes_nothing() {
        let mut remote = RemoteCamera::default();
        assert_eq!(remote.said(), None, "nothing said yet");
        assert!(remote.hear(2, true, false));
        assert_eq!(remote.said(), Some((true, false)));
        assert!(!remote.hear(2, false, false), "a repeat");
        assert!(!remote.hear(1, false, false), "a late one");
        assert_eq!(remote.said(), Some((true, false)));
        assert!(remote.hear(5, true, true), "seq may skip");
        assert_eq!(remote.said(), Some((true, true)));
        assert!(remote.hear(6, false, false));
        assert_eq!(remote.said(), Some((false, false)));
    }

    // The first state of a call may be seq 0 from a sender that counts from there.
    #[test]
    fn the_first_camera_state_is_taken_whatever_its_seq() {
        let mut remote = RemoteCamera::default();
        assert!(remote.hear(0, true, false));
        assert_eq!(remote.said(), Some((true, false)));
        assert!(!remote.hear(0, false, false));
    }

    // §23: only two sides at media version 1 speak `CallMedia`; an older app says nothing (0).
    #[test]
    fn camera_states_go_only_to_a_side_at_media_version_one() {
        assert_eq!(media_version(true), ft_protocol::CALL_MEDIA_VERSION);
        assert_eq!(media_version(false), 0, "a phone without native video says what an older app says");
        assert!(!speaks_camera_state(0));
        assert!(speaks_camera_state(1));
        assert!(speaks_camera_state(2), "a newer app still reads it");
    }

    // 2026-10-08: presentations go only between two sides at media version 2 or later.
    #[test]
    fn presentations_go_only_to_a_side_at_media_version_two() {
        assert!(!presents(0), "an older app's WebView");
        assert!(!presents(1), "apps 1.2 to 1.5");
        assert!(presents(2));
        assert!(presents(3), "a newer app still shows them");
    }

    // The older app's fallback (docs/video-nativo.md, the table of §1).
    #[test]
    fn an_older_app_s_voice_call_has_no_camera_and_its_video_call_has_video_from_the_start() {
        assert!(!camera_allowed(0, false), "an older app's voice call has no video line");
        assert!(camera_allowed(0, true), "an older app's video call");
        assert!(camera_allowed(1, false) && camera_allowed(1, true));
        assert!(remote_on_at_connect(0, true), "an older app's video call sends its camera from the start");
        assert!(!remote_on_at_connect(0, false));
        assert!(!remote_on_at_connect(1, true), "a new app says so itself with CallMedia");

        let available = VideoState { available: true, ..VideoState::default() };
        assert!(!shown(available, 0, false).available, "the camera button is disabled");
        assert!(shown(available, 0, true).available);
        assert!(shown(available, 1, false).available);
        assert!(!shown(VideoState::default(), 1, false).available, "never more than ft-media says");
    }

    // Until the video is ready, our camera is the user's wish, and the camera cannot be turned
    // on right now: a wish is kept for the connection. Saying `available` in that moment (the
    // engine says so a moment before the core is ready) let a camera turned on then answer `Ok`
    // and fail later with an event, as if nobody had asked (app#94).
    #[test]
    fn before_the_video_is_ready_the_camera_is_the_wish_and_not_available() {
        let engine = VideoState { available: true, ..VideoState::default() };
        let waiting = before_ready(engine, false, true);
        assert!(waiting.camera, "the user's wish");
        assert!(!waiting.available, "not available until the video is ready");
        assert_eq!(before_ready(engine, true, true), engine, "once ready, the engine's state as it is");
    }

    // What `CallMedia` says of our camera: held only means something while it is on.
    #[test]
    fn our_camera_is_said_on_and_held_only_while_on() {
        let off_held = VideoState { paused: true, ..VideoState::default() };
        assert_eq!(camera_said(&off_held), (false, false));
        let on = VideoState { camera: true, ..VideoState::default() };
        assert_eq!(camera_said(&on), (true, false));
        assert_eq!(camera_said(&VideoState { paused: true, ..on }), (true, true));
        let with_theirs = VideoState { remote: true, shape: None, ..on };
        assert_eq!(camera_said(&with_theirs), (true, false), "their camera is not ours");
    }

    // Held = the app is not on the screen, or the call screen does not show the video (§ 4).
    #[test]
    fn the_camera_is_held_unless_the_app_and_the_call_screen_show_it() {
        assert!(!held(true, true));
        assert!(held(false, true), "the app is in the background or the phone locked");
        assert!(held(true, false), "the user left the call screen");
        assert!(held(false, false));
    }

    // 2026-10-08: what the other side presents is a state; the highest `seq` wins.
    #[test]
    fn the_newest_presentation_wins_and_a_repeat_or_a_late_one_changes_nothing() {
        let mut kept = None;
        assert!(newer_present(&mut kept, 0), "the first is taken whatever its seq");
        assert!(!newer_present(&mut kept, 0), "a repeat");
        assert!(newer_present(&mut kept, 3), "seq may skip");
        assert!(!newer_present(&mut kept, 2), "a late one");
        assert_eq!(kept, Some(3));
    }

    // Theirs replaces what is shown (ours too, unless ours wins a clash); their stop ends only theirs.
    #[test]
    fn theirs_replaces_what_is_shown_and_their_stop_ends_only_theirs() {
        let board = || Some("com.flickertalk.board".to_owned());
        let theirs = Presenting { plugin: "com.flickertalk.board".to_owned(), file: None, by: PresentedBy::Them };
        let mine = Presenting { plugin: "com.flickertalk.pdfviewer".to_owned(), file: None, by: PresentedBy::Me };
        assert_eq!(after_their_present(None, board(), None, false), Some(Some(theirs.clone())));
        assert_eq!(after_their_present(None, board(), None, true), Some(Some(theirs.clone())), "nothing of ours to keep");
        assert_eq!(after_their_present(Some(&mine), board(), None, false), Some(Some(theirs.clone())), "ours loses the clash");
        assert_eq!(after_their_present(Some(&mine), board(), None, true), None, "ours wins the clash");
        assert_eq!(after_their_present(Some(&theirs), None, None, false), Some(None), "their stop");
        assert_eq!(after_their_present(Some(&mine), None, None, false), None, "their stop leaves ours");
        assert_eq!(after_their_present(None, None, None, true), None, "nothing to stop");
    }

    // Both pressed Present at once: the side whose device id sorts first keeps its presentation.
    #[test]
    fn in_a_clash_the_device_id_that_sorts_first_keeps_its_presentation() {
        assert!(mine_wins_clash("ft_a", "ft_b"));
        assert!(!mine_wins_clash("ft_b", "ft_a"));
    }
}
