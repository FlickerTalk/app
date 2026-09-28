//! Native voice calls (2026-09-28): on the phones, a voice call's media runs in Rust (`ft-media`)
//! instead of the WebView. On a locked iPhone a call answered from CallKit has no WebView at all,
//! and WKWebView's media is muted in the background, so the voice has to live here.
//!
//! The signalling is the same as the WebView's calls (`calls.rs`): `CallOffer`, `CallAnswer` and
//! `CallEnd` over the direct connection, with standard SDP, so a native phone and a WebView phone
//! (an older app, or a video call) still talk. Video calls and the desktop keep the WebView.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, PoisonError};
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use ft_media::{AudioPlatform, CallRouting, LinkState, MediaSession, Voice};

use crate::calls::stale;
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
}

/// The media of a native call: its connection and its voice.
pub(crate) struct NativeCall {
    call: String,
    contact: String,
    session: MediaSession,
    voice: Voice,
    /// When the media connected (ms); 0 until then.
    connected_at: AtomicI64,
}

impl NativeCall {
    fn connected_at(&self) -> Option<i64> {
        Some(self.connected_at.load(Ordering::SeqCst)).filter(|at| *at > 0)
    }

    async fn shut(&self) {
        self.voice.stop().await;
        self.session.close().await;
    }
}

/// How long the OS's answer waits for an offer that has not arrived yet.
pub const EARLY_ANSWER: Duration = Duration::from_secs(30);

/// Before an audio device that would not start is tried again.
const AUDIO_RETRY: Duration = Duration::from_millis(300);

/// Settings key: the call routing, as Settings writes it (`direct`, `auto`, `always`).
const CALL_ROUTING: &str = "call_routing";

impl Core {
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
    pub async fn start_native_call(self: &Arc<Self>, contact: &str, routing: CallRouting) -> Result<String> {
        let platform = self.audio_platform()?;
        self.set_call_routing(routing).await?;
        let call = self.place_call(contact, false).await?;
        let (core, id) = (self.clone(), call.clone());
        tokio::spawn(async move {
            if core.offer_native(&id, routing, platform).await.is_err() {
                let _ = core.end_call(&id, true).await;
            }
        });
        Ok(call)
    }

    async fn offer_native(self: &Arc<Self>, call: &str, routing: CallRouting, platform: AudioPlatform) -> Result<()> {
        let Some(native) = self.open_native(call, routing, platform).await? else { return Ok(()) };
        let sdp = native.session.offer().await?;
        self.offer_call(call, &sdp).await
    }

    /// Answers the ringing call with our voice. Answering a call already answered does nothing:
    /// CallKit and the WebView may both answer it.
    pub async fn answer_native_call(self: &Arc<Self>, call: &str, routing: CallRouting) -> Result<()> {
        let platform = self.audio_platform()?;
        let _one_at_a_time = self.native_setup.lock().await;
        let Some(record) = self.store.call(call).await? else { bail!("no such call") };
        if record.outgoing || record.ended_at.is_some() {
            bail!("no such ringing call");
        }
        if record.answered_at.is_some() {
            return Ok(());
        }
        let Some(offer) = self.offer_of(call) else { bail!("no offer for this call") };
        let answered = self.answer_native(call, &offer, routing, platform).await;
        if answered.is_err() {
            let _ = self.end_call(call, true).await;
        }
        answered
    }

    async fn answer_native(self: &Arc<Self>, call: &str, offer: &str, routing: CallRouting, platform: AudioPlatform) -> Result<()> {
        let Some(native) = self.open_native(call, routing, platform).await? else { bail!("the call is over") };
        let sdp = native.session.answer(offer).await?;
        self.answer_call(call, &sdp).await
    }

    /// The OS answered (CallKit on a locked iPhone, with no WebView): the ringing voice call is
    /// answered here with the routing the core keeps. `false` when nothing rings, or it is a
    /// video call, which is the WebView's.
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
            let until = now() + i64::try_from(wait.as_millis()).unwrap_or(i64::MAX);
            *self.early_answer.lock().unwrap_or_else(PoisonError::into_inner) = Some(until);
        }
        let Some(current) = current else { return Ok(false) };
        if current.phase != CallPhase::Ringing || current.video {
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
            (false, false, _) => CallPhase::Ringing,
            (true, _, true) => CallPhase::Active,
            (true, _, false) => CallPhase::Connecting,
        };
        Ok(Some(CurrentCall {
            offer: (phase == CallPhase::Ringing).then(|| self.offer_of(&call)).flatten(),
            native: native.is_some(),
            muted: native.as_ref().is_some_and(|native| native.voice.is_muted()),
            connected_at,
            call,
            contact: record.contact,
            video: record.video,
            outgoing: record.outgoing,
            phase,
        }))
    }

    /// A voice call started ringing: if the OS answered it early (within its window), it is
    /// answered now, in the background, with the routing the core keeps.
    pub(crate) fn answer_if_answered_early(&self, call: &str, video: bool) {
        let until = self.early_answer.lock().unwrap_or_else(PoisonError::into_inner).take();
        let Some(until) = until else { return };
        if video || now() > until {
            return;
        }
        let Some(core) = self.this.upgrade() else { return };
        let call = call.to_owned();
        tokio::spawn(async move {
            let routing = core.call_routing().await;
            let _ = core.answer_native_call(&call, routing).await;
        });
    }

    /// Their offer, kept while the call rings here.
    pub(crate) fn remember_offer(&self, call: &str, sdp: &str) {
        *self.ringing_offer.lock().unwrap_or_else(PoisonError::into_inner) = Some((call.to_owned(), sdp.to_owned()));
    }

    fn offer_of(&self, call: &str) -> Option<String> {
        let ringing = self.ringing_offer.lock().unwrap_or_else(PoisonError::into_inner);
        ringing.as_ref().filter(|(id, _)| id == call).map(|(_, sdp)| sdp.clone())
    }

    fn native_of(&self, call: &str) -> Option<Arc<NativeCall>> {
        let native = self.native_call.lock().unwrap_or_else(PoisonError::into_inner);
        native.as_ref().filter(|native| native.call == call).cloned()
    }

    /// The media for the call: its connection, with our audio track, and its voice. `None` if
    /// the call ended meanwhile.
    async fn open_native(self: &Arc<Self>, call: &str, routing: CallRouting, platform: AudioPlatform) -> Result<Option<Arc<NativeCall>>> {
        let Some(record) = self.store.call(call).await? else { bail!("no such call") };
        let mut config = self.transport.media_config();
        config.routing = routing;
        let session = MediaSession::open(&config).await?;
        let voice = Voice::for_session(&session, platform);
        let native = Arc::new(NativeCall {
            call: call.to_owned(),
            contact: record.contact,
            session,
            voice,
            connected_at: AtomicI64::new(0),
        });
        let replaced = self.native_call.lock().unwrap_or_else(PoisonError::into_inner).replace(native.clone());
        if let Some(replaced) = replaced {
            replaced.shut().await;
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

    async fn native_connected(&self, native: &NativeCall) {
        if native.connected_at.compare_exchange(0, now(), Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return;
        }
        if native.voice.connected().await.is_err() {
            // No microphone or speaker: a call nobody can hear is a failed call.
            let _ = self.end_call(&native.call, true).await;
            return;
        }
        self.announce(native, CallUpdate::Connected);
    }

    /// The other side's answer to our native call. Nothing to do for a WebView call.
    pub(crate) async fn accept_native_answer(&self, call: &str, sdp: &str) -> Result<()> {
        match self.native_of(call) {
            Some(native) => native.session.accept(sdp).await,
            None => Ok(()),
        }
    }

    /// Stops the call's voice and closes its connection, if it has them.
    pub(crate) async fn drop_native(&self, call: &str) {
        {
            let mut ringing = self.ringing_offer.lock().unwrap_or_else(PoisonError::into_inner);
            if ringing.as_ref().is_some_and(|(id, _)| id == call) {
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
            native.shut().await;
            // CallKit gives the session back at the end; the next call waits for its own.
            self.call_audio_active.store(false, Ordering::SeqCst);
        }
    }

    fn announce(&self, native: &NativeCall, update: CallUpdate) {
        let _ = self.events.send(Event::Call { contact: native.contact.clone(), call: native.call.clone(), update });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases_compare() {
        assert_ne!(CallPhase::Ringing, CallPhase::Active);
    }
}
