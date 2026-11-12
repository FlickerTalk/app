//! Call media runs in Rust on phones because WKWebView mutes media in the background and a CallKit answer on a locked iPhone has no WebView.

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU16, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, ensure, Result};
use ft_media::{AudioPlatform, CallRouting, LinkState, MediaSession, Video, VideoPlatform, VideoState, Voice};
use ft_protocol::{Body, MessageId, Packet};

use crate::calls::stale;
use crate::timings::{CallClock, CallStage, CallTimings, Candidates};
use crate::{now, CallUpdate, Core, Event};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallPhase {
    Calling,
    Ringing,
    Connecting,
    Active,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentCall {
    pub call: String,
    pub contact: String,
    pub video: bool,
    pub outgoing: bool,
    pub phase: CallPhase,
    pub offer: Option<String>,
    pub native: bool,
    pub muted: bool,
    pub connected_at: Option<i64>,
    pub video_state: Option<VideoState>,
    pub presenting: Option<Presenting>,
    pub can_present: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentedBy {
    Me,
    Them,
}

// The core never opens or reads what is presented, the plugin's own content goes over `ft.live`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Presenting {
    pub plugin: String,
    pub file: Option<MessageId>,
    pub by: PresentedBy,
}

pub(crate) struct NativeCall {
    call: String,
    contact: String,
    session: MediaSession,
    voice: Voice,
    video: Option<Video>,
    video_call: bool,
    peer_media: AtomicU16,
    peer_known: AtomicBool,
    camera: AtomicBool,
    video_ready: AtomicBool,
    video_lock: tokio::sync::Mutex<()>,
    sent_seq: AtomicU32,
    said: Mutex<(bool, bool)>,
    remote: Mutex<RemoteCamera>,
    // ms, 0 until connected
    connected_at: AtomicI64,
    our_media: u16,
    presenting: Mutex<PresentState>,
    present_seq: AtomicU32,
}

// One lock so a seq and the state it produces are decided together and reach the UI in order
#[derive(Default)]
struct PresentState {
    now: Option<Presenting>,
    heard: Option<u32>,
}

impl NativeCall {
    fn connected_at(&self) -> Option<i64> {
        Some(self.connected_at.load(Ordering::SeqCst)).filter(|at| *at > 0)
    }

    fn peer_media(&self) -> u16 {
        self.peer_media.load(Ordering::SeqCst)
    }

    fn camera_allowed(&self) -> bool {
        !self.peer_known.load(Ordering::SeqCst) || camera_allowed(self.peer_media(), self.video_call)
    }

    fn media(&self) -> u16 {
        self.our_media
    }

    fn presenting(&self) -> Option<Presenting> {
        self.presenting.lock().unwrap_or_else(PoisonError::into_inner).now.clone()
    }

    fn next_present_seq(&self) -> u32 {
        next_seq(&self.present_seq)
    }

    fn can_present(&self) -> bool {
        self.connected_at().is_some() && self.peer_known.load(Ordering::SeqCst) && presents(self.media()) && presents(self.peer_media())
    }

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

pub(crate) struct Prepared {
    native: Arc<NativeCall>,
    routing: CallRouting,
    at: Instant,
}

#[derive(Default)]
pub(crate) struct Preparation {
    call: Option<String>,
    ready: Option<Prepared>,
    answered: bool,
}

// ms. ICE gives up 30 s after it starts checking (webrtc-rs: 5 s disconnected, 25 s failed)
// even with nothing to check, and a NAT may forget the gathered mapping.
pub(crate) const ANSWER_REFRESH_MS: u64 = 15_000;

const PREPARED_FOR: Duration = Duration::from_secs(20);

pub type VideoDetach = Arc<dyn Fn() + Send + Sync>;

pub const EARLY_ANSWER: Duration = Duration::from_secs(30);

// The system call screen rings from the push, so the user can answer or decline before we
// have the offer. The last word wins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EarlyAnswer {
    Accept { until: i64 },
    Decline { until: i64 },
    Accepting { call: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EarlyOutcome {
    Ring,
    Answer,
    Decline,
}

fn until(wait: Duration) -> i64 {
    now().saturating_add(i64::try_from(wait.as_millis()).unwrap_or(i64::MAX))
}

const AUDIO_RETRY: Duration = Duration::from_millis(300);

const CAMERA_RETRY: Duration = Duration::from_secs(2);

const PRESENT_RETRY: Duration = Duration::from_secs(2);

// Must match what src/present.ts opens on the other phone. These are the only tools a
// follower gets with the tools closed.
pub(crate) const BOARD: &str = "com.flickertalk.board";
pub(crate) const VIEWER: &str = "com.flickertalk.pdfviewer";

// written by Settings as direct, auto or always
const CALL_ROUTING: &str = "call_routing";

impl Core {
    pub fn mark_call_stage(&self, stage: CallStage) {
        let mut clock = self.call_clock.lock().unwrap_or_else(PoisonError::into_inner);
        CallClock::mark(&mut clock, stage, Instant::now());
    }

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

    #[doc(hidden)]
    pub fn prepared_call(&self) -> Option<String> {
        let slot = self.preparation.lock().unwrap_or_else(PoisonError::into_inner);
        slot.ready.as_ref().and(slot.call.clone())
    }

    #[doc(hidden)]
    pub fn set_answer_refresh(&self, every: Duration) {
        self.answer_refresh.store(u64::try_from(every.as_millis()).unwrap_or(u64::MAX), Ordering::SeqCst);
    }

    pub fn set_call_audio(&self, platform: Option<AudioPlatform>) {
        *self.call_audio.write().unwrap_or_else(PoisonError::into_inner) = platform;
    }

    pub fn native_calls(&self) -> bool {
        self.call_audio.read().unwrap_or_else(PoisonError::into_inner).is_some()
    }

    fn audio_platform(&self) -> Result<AudioPlatform> {
        let platform = self.call_audio.read().unwrap_or_else(PoisonError::into_inner).clone();
        platform.ok_or_else(|| anyhow!("calls do not run natively on this device"))
    }

    pub async fn set_call_routing(&self, routing: CallRouting) -> Result<()> {
        self.store.set_setting(CALL_ROUTING, routing.as_str()).await
    }

    pub async fn call_routing(&self) -> CallRouting {
        let stored = self.store.setting(CALL_ROUTING).await.ok().flatten();
        stored.and_then(|routing| routing.parse().ok()).unwrap_or_default()
    }

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
        let (core, id) = (self.clone(), call.to_owned());
        tokio::spawn(async move {
            if let Ok(contact) = core.call_contact(&id).await {
                let _ = core.open_direct_call(&contact).await;
            }
        });
        let Some(native) = self.open_native(call, routing, platform, None).await? else { return Ok(()) };
        let sdp = native.session.offer().await?;
        self.mark_call_stage(CallStage::OfferBuilt);
        self.offer_call_media(call, &sdp, native.media(), crate::calls::CALL_REACH).await
    }

    async fn call_contact(&self, call: &str) -> Result<ft_storage::Contact> {
        let Some(record) = self.store.call(call).await? else { bail!("no such call") };
        self.contact(&record.contact).await
    }

    // CallKit and the WebView can both answer, the second one does nothing
    pub async fn answer_native_call(self: &Arc<Self>, call: &str, routing: CallRouting) -> Result<()> {
        let platform = self.audio_platform()?;
        self.mark_call_stage(CallStage::AnswerRequested);
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

    // Mic and camera stay untouched and nothing reaches the caller before the user answers
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
                // Answers wait for the first preparation, which beats making their own
                let _answers_wait = if first { Some(self.native_setup.lock().await) } else { None };
                if !self.still_preparing(call) {
                    return;
                }
                self.prepare_answer(call).await
            };
            first = false;
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

    fn take_prepared(&self, call: &str) -> Option<Prepared> {
        let mut slot = self.preparation.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.call.as_deref() != Some(call) {
            return None;
        }
        slot.answered = true;
        slot.ready.take()
    }

    pub(crate) async fn discard_prepared(&self, call: &str) {
        let unused = {
            let mut slot = self.preparation.lock().unwrap_or_else(PoisonError::into_inner);
            if slot.call.as_deref() != Some(call) {
                return;
            }
            std::mem::take(&mut *slot).ready
        };
        if let Some(unused) = unused {
            unused.native.shut().await;
        }
    }

    // A suspended iPhone rings via PushKit and can be answered before the offer arrives
    pub async fn answer_ringing_call(self: &Arc<Self>) -> Result<bool> {
        self.answer_ringing_call_within(EARLY_ANSWER).await
    }

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

    // A late didDeactivate of an older call can arrive after the new call's didActivate
    pub async fn set_call_audio_session(&self, active: bool, generation: u64) -> Result<()> {
        let newest = self.audio_generation.fetch_max(generation, Ordering::SeqCst);
        if generation < newest {
            return Ok(());
        }
        self.set_call_audio_active(active).await
    }

    pub async fn mute_call(&self, call: &str, muted: bool) -> Result<()> {
        let Some(native) = self.native_of(call) else { bail!("no native call") };
        native.voice.set_muted(muted).await;
        self.announce(&native, CallUpdate::Muted { muted });
        Ok(())
    }

    // CallKit's mute button doesn't say which call
    pub async fn mute_current_call(&self, muted: bool) -> Result<()> {
        let native = self.native_call.lock().unwrap_or_else(PoisonError::into_inner).clone();
        match native {
            Some(native) => self.mute_call(&native.call, muted).await,
            None => Ok(()),
        }
    }

    // didActivate can arrive before the call's connection exists
    pub async fn set_call_audio_active(&self, active: bool) -> Result<()> {
        self.call_audio_active.store(active, Ordering::SeqCst);
        let native = self.native_call.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let Some(native) = native else { return Ok(()) };
        let Err(error) = native.voice.set_session_active(active).await else { return Ok(()) };
        if !active {
            return Err(error);
        }
        // retry the audio unit once, then fail the call rather than go on in silence
        tokio::time::sleep(AUDIO_RETRY).await;
        if let Err(error) = native.voice.set_session_active(true).await {
            let _ = self.end_call(&native.call, true).await;
            return Err(error);
        }
        Ok(())
    }

    pub async fn call_device_running(&self) -> bool {
        let native = self.native_call.lock().unwrap_or_else(PoisonError::into_inner).clone();
        match native {
            Some(native) => native.voice.is_running().await,
            None => false,
        }
    }

    pub async fn end_current_call(&self) -> Result<()> {
        self.early_answer.lock().unwrap_or_else(PoisonError::into_inner).take();
        let active = self.active_call.lock().unwrap_or_else(PoisonError::into_inner).clone();
        match active {
            Some(call) => self.end_call(&call, false).await,
            None => Ok(()),
        }
    }

    pub async fn decline_ringing_call(&self) -> Result<()> {
        self.decline_ringing_call_within(EARLY_ANSWER).await
    }

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

    fn answer_early(&self, answer: EarlyAnswer) {
        *self.early_answer.lock().unwrap_or_else(PoisonError::into_inner) = Some(answer);
    }

    fn mark_answering(&self, call: &str) -> bool {
        let mut early = self.early_answer.lock().unwrap_or_else(PoisonError::into_inner);
        if matches!(&*early, Some(EarlyAnswer::Accepting { call: accepting }) if accepting == call) {
            return false;
        }
        *early = Some(EarlyAnswer::Accepting { call: call.to_owned() });
        true
    }

    fn answered_early(&self, call: &str) -> bool {
        match &*self.early_answer.lock().unwrap_or_else(PoisonError::into_inner) {
            Some(EarlyAnswer::Accepting { call: accepting }) => accepting == call,
            Some(EarlyAnswer::Accept { until }) => now() <= *until,
            _ => false,
        }
    }

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

    pub(crate) fn answer_offered_early(&self, call: &str) {
        let Some(core) = self.this.upgrade() else { return };
        let call = call.to_owned();
        tokio::spawn(async move {
            let routing = core.call_routing().await;
            let _ = core.answer_native_call(&call, routing).await;
        });
    }

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

    async fn build_native(&self, call: &str, routing: CallRouting, platform: AudioPlatform, peer_media: Option<u16>) -> Result<Arc<NativeCall>> {
        let Some(record) = self.store.call(call).await? else { bail!("no such call") };
        let mut config = self.transport.fresh_media_config().await;
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
            presenting: Mutex::new(PresentState::default()),
            present_seq: AtomicU32::new(0),
        });
        Ok(native)
    }

    async fn install_native(self: &Arc<Self>, native: Arc<NativeCall>) -> Result<Option<Arc<NativeCall>>> {
        let call = native.call.as_str();
        self.call_shown.store(true, Ordering::SeqCst);
        let replaced = self.native_call.lock().unwrap_or_else(PoisonError::into_inner).replace(native.clone());
        if let Some(replaced) = replaced {
            self.shut_native(&replaced).await;
        }
        // Read only after the call is in place, so a later activation reaches it directly
        native.voice.set_session_active(self.call_audio_active.load(Ordering::SeqCst)).await?;
        // hung up while connecting, close_call had nothing to stop then
        if self.store.call(call).await?.is_none_or(|record| record.ended_at.is_some()) {
            self.drop_native(call).await;
            return Ok(None);
        }
        self.follow(&native);
        Ok(Some(native))
    }

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
        if plan_changes_on_connect(native.presenting().as_ref()) {
            let _ = self.events.send(Event::PlanChanged);
        }
        self.mark_call_stage(CallStage::Connected);
        if native.voice.connected().await.is_err() {
            let _ = self.end_call(&native.call, true).await;
            return;
        }
        self.announce(native, CallUpdate::Connected);
        self.video_connected(native).await;
    }

    async fn video_connected(&self, native: &Arc<NativeCall>) {
        let Some(video) = &native.video else { return };
        if video.connected().await.is_err() {
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
                    native.camera.store(false, Ordering::SeqCst);
                    self.announce(native, CallUpdate::CameraFailed);
                }
            } else {
                native.camera.store(false, Ordering::SeqCst);
            }
        }
    }

    // A new watch receiver treats the current state as seen, so send it first
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

    pub(crate) async fn call_media_received(&self, contact: &ft_storage::Contact, call: MessageId, seq: u32, video: bool, paused: bool) -> Result<()> {
        let Some(native) = self.native_of(&call.to_string()) else { return Ok(()) };
        if native.contact != contact.device_id || native.video.is_none() {
            return Ok(());
        }
        let newer = native.remote.lock().unwrap_or_else(PoisonError::into_inner).hear(seq, video, paused);
        if !newer || !native.video_ready.load(Ordering::SeqCst) {
            return Ok(());
        }
        self.apply_remote(&native).await
    }

    pub async fn present_in_call(&self, call: &str, plugin: &str, file: Option<MessageId>) -> Result<()> {
        let Some(native) = self.native_of(call) else { bail!("no native call") };
        ensure!(native.connected_at().is_some(), "the call is not on yet");
        ensure!(native.can_present(), crate::PEER_CANNOT_PRESENT);
        let kind = self.plugin_manifest(plugin).map_or(ft_plugins::Kind::Tool, |manifest| manifest.kind);
        self.usable(kind).await?;
        ensure!(!plugin.is_empty(), "no plugin to present");
        if let Some(file) = file {
            ensure!(self.file_belongs_to(&file, &native.contact).await?, "not a file of this conversation");
        }
        let seq = {
            let mut state = native.presenting.lock().unwrap_or_else(PoisonError::into_inner);
            ensure!(state.now.as_ref().is_none_or(|now| now.by == PresentedBy::Me), "the other side is presenting");
            state.now = Some(Presenting { plugin: plugin.to_owned(), file, by: PresentedBy::Me });
            self.announce_presenting(&native, state.now.as_ref());
            native.next_present_seq()
        };
        self.send_presenting(&native, seq, Some(plugin.to_owned()), file);
        Ok(())
    }

    pub async fn file_belongs_to(&self, file: &MessageId, contact: &str) -> Result<bool> {
        let id = file.to_string();
        let Some(message) = self.store.message(&id).await? else { return Ok(false) };
        Ok(message.contact == contact && self.store.file(&id).await?.is_some())
    }

    #[doc(hidden)]
    pub async fn say_call_present(&self, contact: &str, call: &str, seq: u32, plugin: Option<String>, file: Option<MessageId>) -> Result<bool> {
        let contact = self.contact(contact).await?;
        let packet = Packet::new(Body::CallPresent { call: MessageId::parse(call)?, seq, plugin, file });
        self.transmit_direct(&contact, &packet).await
    }

    pub async fn stop_presenting(&self, call: &str) -> Result<()> {
        let Some(native) = self.native_of(call) else { bail!("no native call") };
        let seq = {
            let mut state = native.presenting.lock().unwrap_or_else(PoisonError::into_inner);
            let ours = state.now.as_ref().is_some_and(|now| now.by == PresentedBy::Me);
            if ours {
                state.now = None;
                self.announce_presenting(&native, None);
            }
            ours.then(|| native.next_present_seq())
        };
        if let Some(seq) = seq {
            self.send_presenting(&native, seq, None, None);
        }
        Ok(())
    }

    // Decided from the core's own call state only
    pub(crate) async fn following(&self, plugin: &str) -> Result<bool> {
        let Some(current) = self.current_call().await? else { return Ok(false) };
        Ok(current.phase == CallPhase::Active && followed(current.presenting.as_ref()).as_deref() == Some(plugin))
    }

    fn send_presenting(&self, native: &Arc<NativeCall>, seq: u32, plugin: Option<String>, file: Option<MessageId>) {
        let Some(core) = self.this.upgrade() else { return };
        let native = native.clone();
        tokio::spawn(async move { core.deliver_presenting(&native, seq, plugin, file).await });
    }

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

    // Ignore it from anyone but this call's contact, before the answer, or with a file from another chat
    pub(crate) async fn call_present_received(&self, contact: &ft_storage::Contact, call: MessageId, seq: u32, plugin: Option<String>, file: Option<MessageId>) -> Result<()> {
        let Some(native) = self.native_of(&call.to_string()) else { return Ok(()) };
        if native.contact != contact.device_id || !presents(native.media()) {
            return Ok(());
        }
        let answered = self.store.call(&native.call).await?.is_some_and(|record| record.answered_at.is_some());
        if !answered {
            return Ok(());
        }
        if let Some(file) = file {
            let id = file.to_string();
            let found = match self.store.message(&id).await? {
                Some(message) => Some((message.contact, self.store.file(&id).await?.is_some())),
                None => None,
            };
            if !presentable_file(found.as_ref().map(|(of, is_file)| (of.as_str(), *is_file)), &native.contact) {
                return Ok(());
            }
        }
        let mine_wins = mine_wins_clash(self.device_id().as_str(), &native.contact);
        let withdrawal = {
            let mut state = native.presenting.lock().unwrap_or_else(PoisonError::into_inner);
            if !newer_present(&mut state.heard, seq) {
                return Ok(());
            }
            let before = followed(state.now.as_ref());
            let (changed, withdrawal) = take_their_present(&mut state.now, &native.present_seq, plugin, file, mine_wins);
            let after = followed(state.now.as_ref());
            let serve = after != before;
            if serve && after.is_some() {
                let _ = self.events.send(Event::PlanChanged);
            }
            if changed {
                self.announce_presenting(&native, state.now.as_ref());
            }
            if serve && after.is_none() {
                let _ = self.events.send(Event::PlanChanged);
            }
            withdrawal
        };
        if let Some(seq) = withdrawal {
            // We lost the clash, withdraw ours so a late copy never shows on their side
            self.send_presenting(&native, seq, None, None);
        }
        Ok(())
    }

    // Anything sent on the old link may be lost, and a lost stop leaves them showing ours till the call ends
    pub(crate) fn link_opened_in_call(&self, contact: &str) {
        let native = self.native_call.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let Some(native) = native.filter(|native| native.contact == contact) else { return };
        let again = {
            let state = native.presenting.lock().unwrap_or_else(PoisonError::into_inner);
            let ever_said = native.present_seq.load(Ordering::SeqCst) > 0;
            said_again(state.now.as_ref(), ever_said).map(|(plugin, file)| (native.next_present_seq(), plugin, file))
        };
        if let Some((seq, plugin, file)) = again {
            self.send_presenting(&native, seq, plugin, file);
        }
    }

    fn announce_presenting(&self, native: &NativeCall, now: Option<&Presenting>) {
        let update = CallUpdate::Presenting {
            plugin: now.map(|now| now.plugin.clone()),
            file: now.and_then(|now| now.file),
            by: now.map(|now| now.by),
        };
        self.announce(native, update);
    }

    async fn apply_remote(&self, native: &NativeCall) -> Result<()> {
        let Some(video) = &native.video else { return Ok(()) };
        let _one_at_a_time = native.video_lock.lock().await;
        let Some((on, held)) = native.heard() else { return Ok(()) };
        video.set_remote(on, held).await?;
        Ok(())
    }

    async fn apply_camera(&self, native: &NativeCall) -> Result<VideoState> {
        let Some(video) = &native.video else { bail!("no video on this device") };
        let _one_at_a_time = native.video_lock.lock().await;
        video.set_paused(self.camera_held()).await?;
        video.set_camera(native.camera.load(Ordering::SeqCst)).await
    }

    async fn apply_hold(&self) -> Result<()> {
        let native = self.native_call.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let Some(native) = native else { return Ok(()) };
        let Some(video) = &native.video else { return Ok(()) };
        if !native.video_ready.load(Ordering::SeqCst) {
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
            if followed(native.presenting().as_ref()).is_some() {
                let _ = self.events.send(Event::PlanChanged);
            }
            if let Some(video) = &native.video {
                self.announce(&native, CallUpdate::Video(video.state()));
            }
            // CallKit takes the audio session back at the end, the next call waits for its own
            self.call_audio_active.store(false, Ordering::SeqCst);
        }
    }

    pub fn set_call_video(&self, platform: Option<VideoPlatform>) {
        *self.call_video.write().unwrap_or_else(PoisonError::into_inner) = platform;
    }

    #[doc(hidden)]
    pub fn set_call_media_version(&self, version: u16) {
        self.call_media_ceiling.store(version, Ordering::SeqCst);
    }

    // On iOS the layers belong to the devices, so the views have to go first. May block.
    pub fn set_video_detach(&self, detach: Option<VideoDetach>) {
        *self.video_detach.write().unwrap_or_else(PoisonError::into_inner) = detach;
    }

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

    pub async fn switch_call_camera(&self, call: &str) -> Result<VideoState> {
        let Some(native) = self.native_of(call) else { bail!("no native call") };
        let Some(video) = &native.video else { bail!("no video on this device") };
        let state = video.switch_camera().await?;
        Ok(shown(state, native.peer_media(), native.video_call))
    }

    pub async fn set_call_shown(&self, shown: bool) -> Result<()> {
        self.call_shown.store(shown, Ordering::SeqCst);
        self.apply_hold().await
    }

    pub async fn set_app_visible(&self, visible: bool) -> Result<()> {
        self.app_visible.store(visible, Ordering::SeqCst);
        self.apply_hold().await
    }

    pub async fn request_call_video(&self) -> Result<()> {
        let native = self.native_call.lock().unwrap_or_else(PoisonError::into_inner).clone();
        match native {
            Some(native) => self.set_call_camera(&native.call, true).await.map(|_| ()),
            None => Ok(()),
        }
    }

    pub fn call_video(&self, call: &str) -> Option<VideoState> {
        self.native_of(call)?.view()
    }

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

// highest seq wins, so repeats and late arrivals change nothing
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RemoteCamera {
    seq: Option<u32>,
    video: bool,
    paused: bool,
}

impl RemoteCamera {
    fn hear(&mut self, seq: u32, video: bool, paused: bool) -> bool {
        if self.seq.is_some_and(|kept| seq <= kept) {
            return false;
        }
        *self = Self { seq: Some(seq), video, paused };
        true
    }

    fn said(&self) -> Option<(bool, bool)> {
        self.seq.map(|_| (self.video, self.paused))
    }
}

fn media_version(has_video: bool) -> u16 {
    if has_video {
        ft_protocol::CALL_MEDIA_VERSION
    } else {
        0
    }
}

fn speaks_camera_state(peer_media: u16) -> bool {
    peer_media >= ft_protocol::CALL_MEDIA_CAMERA
}

fn presents(media: u16) -> bool {
    media >= ft_protocol::CALL_MEDIA_PRESENT
}

fn newer_present(kept: &mut Option<u32>, seq: u32) -> bool {
    if kept.is_some_and(|kept| seq <= kept) {
        return false;
    }
    *kept = Some(seq);
    true
}

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

fn take_their_present(now: &mut Option<Presenting>, our_seq: &AtomicU32, plugin: Option<String>, file: Option<MessageId>, mine_wins: bool) -> (bool, Option<u32>) {
    let Some(next) = after_their_present(now.as_ref(), plugin, file, mine_wins) else { return (false, None) };
    let ours = now.as_ref().is_some_and(|now| now.by == PresentedBy::Me);
    *now = next;
    (true, ours.then(|| next_seq(our_seq)))
}

fn next_seq(counter: &AtomicU32) -> u32 {
    counter.fetch_add(1, Ordering::SeqCst).wrapping_add(1)
}

// When both press Present at once, the lower device id keeps it on both phones
fn mine_wins_clash(me: &str, them: &str) -> bool {
    me < them
}

fn followed(now: Option<&Presenting>) -> Option<String> {
    now.filter(|now| now.by == PresentedBy::Them).map(|now| now.plugin.clone())
}

fn presentable_file(found: Option<(&str, bool)>, contact: &str) -> bool {
    found.is_none_or(|(of, is_file)| of == contact && is_file)
}

fn said_again(now: Option<&Presenting>, ever_said: bool) -> Option<(Option<String>, Option<MessageId>)> {
    if !ever_said {
        return None;
    }
    Some(now.filter(|now| now.by == PresentedBy::Me).map_or((None, None), |now| (Some(now.plugin.clone()), now.file)))
}

fn plan_changes_on_connect(now: Option<&Presenting>) -> bool {
    followed(now).is_some()
}

fn camera_allowed(peer_media: u16, video_call: bool) -> bool {
    video_call || speaks_camera_state(peer_media)
}

// Older apps send video from the start and never send CallMedia
fn remote_on_at_connect(peer_media: u16, video_call: bool) -> bool {
    video_call && !speaks_camera_state(peer_media)
}

fn camera_said(state: &VideoState) -> (bool, bool) {
    (state.camera, state.camera && state.paused)
}

fn held(app_visible: bool, call_shown: bool) -> bool {
    !(app_visible && call_shown)
}

// The engine reports available a moment before the core can apply the camera, so a camera turned on in between stays a wish
fn before_ready(state: VideoState, ready: bool, wish: bool) -> VideoState {
    if ready {
        state
    } else {
        VideoState { camera: wish, available: false, ..state }
    }
}

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

    #[test]
    fn the_first_camera_state_is_taken_whatever_its_seq() {
        let mut remote = RemoteCamera::default();
        assert!(remote.hear(0, true, false));
        assert_eq!(remote.said(), Some((true, false)));
        assert!(!remote.hear(0, false, false));
    }

    #[test]
    fn camera_states_go_only_to_a_side_at_media_version_one() {
        assert_eq!(media_version(true), ft_protocol::CALL_MEDIA_VERSION);
        assert_eq!(media_version(false), 0, "a phone without native video says what an older app says");
        assert!(!speaks_camera_state(0));
        assert!(speaks_camera_state(1));
        assert!(speaks_camera_state(2), "a newer app still reads it");
    }

    #[test]
    fn presentations_go_only_to_a_side_at_media_version_two() {
        assert!(!presents(0), "an older app's WebView");
        assert!(!presents(1), "apps 1.2 to 1.5");
        assert!(presents(2));
        assert!(presents(3), "a newer app still shows them");
    }

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

    #[test]
    fn before_the_video_is_ready_the_camera_is_the_wish_and_not_available() {
        let engine = VideoState { available: true, ..VideoState::default() };
        let waiting = before_ready(engine, false, true);
        assert!(waiting.camera, "the user's wish");
        assert!(!waiting.available, "not available until the video is ready");
        assert_eq!(before_ready(engine, true, true), engine, "once ready, the engine's state as it is");
    }

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

    #[test]
    fn the_camera_is_held_unless_the_app_and_the_call_screen_show_it() {
        assert!(!held(true, true));
        assert!(held(false, true), "the app is in the background or the phone locked");
        assert!(held(true, false), "the user left the call screen");
        assert!(held(false, false));
    }

    #[test]
    fn the_newest_presentation_wins_and_a_repeat_or_a_late_one_changes_nothing() {
        let mut kept = None;
        assert!(newer_present(&mut kept, 0), "the first is taken whatever its seq");
        assert!(!newer_present(&mut kept, 0), "a repeat");
        assert!(newer_present(&mut kept, 3), "seq may skip");
        assert!(!newer_present(&mut kept, 2), "a late one");
        assert_eq!(kept, Some(3));
    }

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

    #[test]
    fn in_a_clash_the_device_id_that_sorts_first_keeps_its_presentation() {
        assert!(mine_wins_clash("ft_a", "ft_b"));
        assert!(!mine_wins_clash("ft_b", "ft_a"));
    }

    #[test]
    fn losing_a_clash_withdraws_ours_with_a_newer_seq() {
        let board = || Some("com.flickertalk.board".to_owned());
        let mine = Presenting { plugin: "com.flickertalk.pdfviewer".to_owned(), file: None, by: PresentedBy::Me };
        let theirs = Presenting { plugin: "com.flickertalk.board".to_owned(), file: None, by: PresentedBy::Them };
        let ours_sent = AtomicU32::new(1);

        let mut now = Some(mine.clone());
        assert_eq!(take_their_present(&mut now, &ours_sent, board(), None, false), (true, Some(2)), "ours lost: withdrawn with seq 2");
        assert_eq!(now, Some(theirs.clone()));

        let mut now = Some(mine.clone());
        assert_eq!(take_their_present(&mut now, &ours_sent, board(), None, true), (false, None), "ours won: nothing to withdraw");
        assert_eq!(now, Some(mine.clone()));

        let mut now = Some(mine.clone());
        assert_eq!(take_their_present(&mut now, &ours_sent, None, None, false), (false, None), "their stop leaves ours");

        let mut now = None;
        assert_eq!(take_their_present(&mut now, &ours_sent, board(), None, false), (true, None), "nothing of ours to withdraw");
        assert_eq!(take_their_present(&mut now, &ours_sent, None, None, false), (true, None), "their stop");
        assert_eq!(ours_sent.load(Ordering::SeqCst), 2, "only the withdrawal took a seq");
    }

    #[test]
    fn only_what_the_other_side_presents_is_followed() {
        let theirs = Presenting { plugin: "com.example.board".to_owned(), file: None, by: PresentedBy::Them };
        let mine = Presenting { by: PresentedBy::Me, ..theirs.clone() };
        assert_eq!(followed(Some(&theirs)), Some("com.example.board".to_owned()));
        assert_eq!(followed(Some(&mine)), None, "presenting is using the tool");
        assert_eq!(followed(None), None);
    }

    #[test]
    fn connecting_while_the_other_side_presents_changes_what_may_open() {
        let theirs = Presenting { plugin: "com.example.board".to_owned(), file: None, by: PresentedBy::Them };
        let mine = Presenting { by: PresentedBy::Me, ..theirs.clone() };
        assert!(plan_changes_on_connect(Some(&theirs)), "the followed tool is served from now on");
        assert!(!plan_changes_on_connect(Some(&mine)), "ours changes nothing");
        assert!(!plan_changes_on_connect(None));
    }

    #[test]
    fn a_presented_file_must_be_a_file_of_the_call_s_chat() {
        assert!(presentable_file(Some(("ft_alice", true)), "ft_alice"), "a file of this chat");
        assert!(!presentable_file(Some(("ft_carol", true)), "ft_alice"), "a file of another chat");
        assert!(!presentable_file(Some(("ft_alice", false)), "ft_alice"), "a text of this chat");
        assert!(presentable_file(None, "ft_alice"), "not here yet");
    }

    #[test]
    fn a_side_that_spoke_says_its_presentation_again() {
        let board = || Some("com.example.board".to_owned());
        let file = MessageId::new();
        let mine = Presenting { plugin: "com.example.board".to_owned(), file: Some(file), by: PresentedBy::Me };
        let theirs = Presenting { by: PresentedBy::Them, ..mine.clone() };
        assert_eq!(said_again(Some(&mine), true), Some((board(), Some(file))), "ours");
        assert_eq!(said_again(None, true), Some((None, None)), "our stop, maybe lost");
        assert_eq!(said_again(Some(&theirs), true), Some((None, None)), "we present nothing");
        assert_eq!(said_again(None, false), None, "nothing ever said");
        assert_eq!(said_again(Some(&theirs), false), None);
    }
}
