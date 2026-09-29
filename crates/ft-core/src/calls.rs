//! Voice and video calls (Plan §66, §106 M6), one to one. The media is the WebView's WebRTC, or
//! Rust's on the phones (`native_calls.rs`); the core carries its descriptions (offer, answer) and the end of the call, encrypted with Olm and
//! only over a direct connection (opened on demand through the router): never through the mailbox.
//! It also keeps the call history, which never leaves the phone.
//!
//! One call at a time: an offer that arrives during another call is answered "busy" and logged as
//! missed.

use std::time::Duration;

use anyhow::{bail, Result};
use ft_protocol::{Body, EndReason, MessageId, Packet};
use ft_storage::{CallOutcome, CallRecord, Contact};

use crate::native_calls::EarlyOutcome;
use crate::timings::CallStage;
use crate::{now, Core, Event};

/// How long a call keeps trying to reach a phone that may be asleep (the router wakes it).
pub const CALL_REACH: Duration = Duration::from_secs(40);
/// Between attempts.
const CALL_RETRY: Duration = Duration::from_secs(2);

/// A call nobody answers stops ringing after this long.
pub const RING_LIMIT: Duration = Duration::from_secs(60);

/// A call that rang unanswered for longer than it can: its caller is gone.
pub(crate) fn stale(record: &CallRecord, now: i64) -> bool {
    record.answered_at.is_none() && now - record.started_at > RING_LIMIT.as_millis() as i64
}

/// What happened to a call, for the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallUpdate {
    /// The contact is calling: the UI rings and, if accepted, answers `sdp`.
    Incoming { video: bool, sdp: String },
    /// The contact answered our call.
    Answered { sdp: String },
    /// The call ringing here is being answered (2026-09-29), from the app, the phone's own call
    /// screen or the notification: it rings no more, and connects next.
    Answering,
    Ended { outcome: CallOutcome },
    /// The media connected: the call is on (a native call; a WebView call knows by itself).
    Connected,
    /// Our voice was muted or unmuted (from the call screen, CallKit or the notification).
    Muted { muted: bool },
    /// The contact called while another call was going on (2026-09-29): refused as busy and
    /// logged as missed. Only the history changes: the phone's call screen stays with the call
    /// going on (an `Ended` here used to end that one in CallKit, and its audio).
    MissedWhileBusy,
    /// The call's video changed (native video, 2026-09-29): the whole state, never a change.
    Video(ft_media::VideoState),
    /// Our camera was wanted and could not start (native video, 2026-09-29): a video call's
    /// camera as the call connected, an encoder that cannot be set up, say. The call goes on as
    /// voice; the video state that follows has the camera off.
    CameraFailed,
}

impl Core {
    /// Logs a new outgoing call and returns its id; the offer follows with `offer_call`.
    pub async fn place_call(&self, contact: &str, video: bool) -> Result<String> {
        // A call is something new: it needs the subscription once the free year is over (§42).
        self.allowed(ft_billing::Doing::Call).await?;
        let stored = self.chosen(contact).await?;
        if stored.blocked {
            bail!("the contact is blocked");
        }
        self.drop_stale_call().await?;
        let call = MessageId::new().to_string();
        {
            let mut active = self.active_call.lock().expect("active call poisoned");
            if active.is_some() {
                bail!("already in a call");
            }
            *active = Some(call.clone());
        }
        self.store
            .insert_call(&CallRecord {
                call_id: call.clone(),
                contact: contact.to_owned(),
                outgoing: true,
                video,
                started_at: now(),
                answered_at: None,
                ended_at: None,
                outcome: None,
            })
            .await?;
        Ok(call)
    }

    /// Sends our offer, trying for a while: the contact's phone may be asleep, and each attempt
    /// has the router wake it (M4). If it cannot be reached, the call ends as unreachable.
    pub async fn offer_call(&self, call: &str, sdp: &str) -> Result<()> {
        self.offer_call_within(call, sdp, CALL_REACH).await
    }

    /// Like `offer_call`, trying for `reach`.
    pub async fn offer_call_within(&self, call: &str, sdp: &str, reach: Duration) -> Result<()> {
        // The WebView's calls speak media version 0: no switching between voice and video.
        self.offer_call_media(call, sdp, 0, reach).await
    }

    /// Like `offer_call_within`, saying our call media version (`CALL_MEDIA_VERSION`).
    pub(crate) async fn offer_call_media(&self, call: &str, sdp: &str, media: u16, reach: Duration) -> Result<()> {
        let Some((record, contact)) = self.open_call(call, true).await? else { bail!("no such call") };
        let body = Body::CallOffer { call: MessageId::parse(call)?, sdp: sdp.to_owned(), video: record.video, media };
        let deadline = std::time::Instant::now() + reach;
        loop {
            // The caller may have given up meanwhile.
            if self.store.call(call).await?.is_none_or(|current| current.ended_at.is_some()) {
                return Ok(());
            }
            if self.transmit_direct_call(&contact, &Packet::new(body.clone())).await? {
                self.mark_call_stage(CallStage::OfferSent);
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return self.close_call(&record, CallOutcome::Unreachable).await;
            }
            tokio::time::sleep(CALL_RETRY.min(reach)).await;
        }
    }

    /// Accepts an incoming call with our answer (the WebView's: media version 0).
    pub async fn answer_call(&self, call: &str, sdp: &str) -> Result<()> {
        self.answer_call_media(call, sdp, 0).await
    }

    /// Like `answer_call`, saying our call media version.
    pub(crate) async fn answer_call_media(&self, call: &str, sdp: &str, media: u16) -> Result<()> {
        let Some((record, contact)) = self.open_call(call, false).await? else { bail!("no such call") };
        // Answered here or in the WebView: nothing more is prepared for it.
        self.discard_prepared(call).await;
        self.store.answer_call(call, now()).await?;
        let body = Body::CallAnswer { call: MessageId::parse(call)?, sdp: sdp.to_owned(), media };
        if self.transmit_direct(&contact, &Packet::new(body)).await? {
            self.mark_call_stage(CallStage::AnswerSent);
        } else {
            self.close_call(&record, CallOutcome::Failed).await?;
        }
        Ok(())
    }

    /// Hangs up, declines or gives up, whichever it is by now; `failed` when the media could not
    /// connect.
    pub async fn end_call(&self, call: &str, failed: bool) -> Result<()> {
        let Some(record) = self.store.call(call).await? else { bail!("no such call") };
        if record.ended_at.is_some() {
            return Ok(());
        }
        let (reason, outcome) = match (failed, record.answered_at.is_some(), record.outgoing) {
            (true, _, _) => (EndReason::Failed, CallOutcome::Failed),
            (false, true, _) => (EndReason::Hangup, CallOutcome::Answered),
            (false, false, true) => (EndReason::Cancelled, CallOutcome::Cancelled),
            (false, false, false) => (EndReason::Declined, CallOutcome::Declined),
        };
        self.close_call(&record, outcome).await?;
        let contact = self.contact(&record.contact).await?;
        let packet = Packet::new(Body::CallEnd { call: MessageId::parse(call)?, reason });
        if !self.transmit_direct(&contact, &packet).await.unwrap_or(false) {
            // The other phone may be asleep, its socket to the router gone (2026-09-28): the end
            // keeps trying while a call would, so it stops ringing as soon as it is back.
            if let Some(core) = self.this.upgrade() {
                tokio::spawn(async move { core.deliver_call_end(&contact, &packet, CALL_REACH).await });
            }
        }
        Ok(())
    }

    /// Sends a call's end until it gets through or `reach` passes.
    async fn deliver_call_end(&self, contact: &Contact, packet: &Packet, reach: Duration) {
        let deadline = std::time::Instant::now() + reach;
        while std::time::Instant::now() < deadline {
            tokio::time::sleep(CALL_RETRY).await;
            if self.transmit_direct(contact, packet).await.unwrap_or(false) {
                return;
            }
        }
    }

    /// Ends the active call if it rang unanswered for too long.
    async fn drop_stale_call(&self) -> Result<()> {
        let active = self.active_call.lock().expect("active call poisoned").clone();
        let Some(active) = active else { return Ok(()) };
        match self.store.call(&active).await? {
            Some(record) if record.ended_at.is_none() && stale(&record, now()) => {
                let outcome = if record.outgoing { CallOutcome::Cancelled } else { CallOutcome::Missed };
                self.close_call(&record, outcome).await
            }
            Some(record) if record.ended_at.is_none() => Ok(()),
            // Gone or already over: nothing holds the line.
            _ => {
                let mut current = self.active_call.lock().expect("active call poisoned");
                if current.as_deref() == Some(active.as_str()) {
                    *current = None;
                }
                Ok(())
            }
        }
    }

    /// The contact calls us, at call media version `media`.
    pub(crate) async fn call_offered(&self, contact: &Contact, call: MessageId, sdp: String, video: bool, media: u16) -> Result<()> {
        let call_id = call.to_string();
        if !contact.rules.accepts_calls || !contact.accepted {
            // Calls off (app#5), or a stranger still in the requests (A5): busy for them, and not
            // a trace on this phone.
            let _ = self.transmit_direct(contact, &Packet::new(Body::CallEnd { call, reason: EndReason::Busy })).await;
            return Ok(());
        }
        if self.silent(contact) {
            // A closed hidden session takes no calls: busy is neutral for the caller, and the
            // phone neither rings nor says anything. The session's history keeps it as missed.
            let record = CallRecord {
                call_id: call_id.clone(),
                contact: contact.device_id.clone(),
                outgoing: false,
                video,
                started_at: now(),
                answered_at: None,
                ended_at: Some(now()),
                outcome: Some(CallOutcome::Missed),
            };
            self.store.insert_call(&record).await?;
            let _ = self.transmit_direct(contact, &Packet::new(Body::CallEnd { call, reason: EndReason::Busy })).await;
            return Ok(());
        }
        self.drop_stale_call().await?;
        let busy = {
            let mut active = self.active_call.lock().expect("active call poisoned");
            match active.as_deref() {
                Some(current) if current != call_id => true,
                Some(_) => return Ok(()),
                None => {
                    *active = Some(call_id.clone());
                    false
                }
            }
        };
        let record = CallRecord {
            call_id: call_id.clone(),
            contact: contact.device_id.clone(),
            outgoing: false,
            video,
            started_at: now(),
            answered_at: None,
            ended_at: busy.then(now),
            outcome: busy.then_some(CallOutcome::Missed),
        };
        let fresh = self.store.insert_call(&record).await?;
        if busy {
            let _ = self.transmit_direct(contact, &Packet::new(Body::CallEnd { call, reason: EndReason::Busy })).await;
            if fresh {
                // For the history: the UI follows only the call it shows.
                self.announce_call(&record, CallUpdate::MissedWhileBusy);
            }
            return Ok(());
        }
        if fresh {
            self.mark_call_stage(CallStage::OfferReceived);
            self.remember_offer(&call_id, &sdp, media);
            // Answered or declined on the phone's own screen before the offer came (2026-09-29):
            // decided before the UI hears of it, so it never rings again.
            let early = self.take_early_answer(&call_id);
            match early {
                EarlyOutcome::Decline => return self.end_call(&call_id, false).await,
                EarlyOutcome::Answer => self.answer_offered_early(&call_id),
                EarlyOutcome::Ring => {}
            }
            self.announce_call(&record, CallUpdate::Incoming { video, sdp });
            self.mark_call_stage(CallStage::Ringing);
            // Its answer is prepared while it rings; one answered already goes at once, without
            // waiting for a preparation (2026-09-29).
            if early == EarlyOutcome::Ring {
                self.prepare_while_ringing(&call_id);
            }
        }
        Ok(())
    }

    /// The contact answered our call, at call media version `media`.
    pub(crate) async fn call_answered(&self, contact: &Contact, call: MessageId, sdp: String, media: u16) -> Result<()> {
        let Some((record, _)) = self.open_call(&call.to_string(), true).await? else { return Ok(()) };
        if record.contact != contact.device_id {
            return Ok(());
        }
        self.mark_call_stage(CallStage::AnswerReceived);
        self.store.answer_call(&record.call_id, now()).await?;
        // A native call takes the answer itself; the UI only learns that it was answered.
        if let Err(error) = self.accept_native_answer(&record.call_id, &sdp, media).await {
            self.end_call(&record.call_id, true).await?;
            return Err(error);
        }
        self.announce_call(&record, CallUpdate::Answered { sdp });
        Ok(())
    }

    /// The contact ended the call.
    pub(crate) async fn call_ended(&self, contact: &Contact, call: MessageId, reason: EndReason) -> Result<()> {
        let Some(record) = self.store.call(&call.to_string()).await? else { return Ok(()) };
        if record.contact != contact.device_id || record.ended_at.is_some() {
            return Ok(());
        }
        let outcome = match (record.answered_at.is_some(), record.outgoing, reason) {
            (_, _, EndReason::Failed) => CallOutcome::Failed,
            (true, _, _) => CallOutcome::Answered,
            (false, true, EndReason::Busy) => CallOutcome::Busy,
            (false, true, _) => CallOutcome::Declined,
            (false, false, _) => CallOutcome::Missed,
        };
        self.close_call(&record, outcome).await
    }

    /// The call, if it is still going on, goes the given way, and its contact.
    async fn open_call(&self, call: &str, outgoing: bool) -> Result<Option<(CallRecord, Contact)>> {
        let Some(record) = self.store.call(call).await? else { return Ok(None) };
        if record.ended_at.is_some() || record.outgoing != outgoing {
            return Ok(None);
        }
        let contact = self.contact(&record.contact).await?;
        Ok(Some((record, contact)))
    }

    pub(crate) async fn close_call(&self, record: &CallRecord, outcome: CallOutcome) -> Result<()> {
        self.store.finish_call(&record.call_id, now(), outcome).await?;
        // The voice stops with the call, whichever side ended it.
        self.drop_native(&record.call_id).await;
        {
            let mut active = self.active_call.lock().expect("active call poisoned");
            if active.as_deref() == Some(record.call_id.as_str()) {
                *active = None;
            }
        }
        self.announce_call(record, CallUpdate::Ended { outcome });
        Ok(())
    }

    pub(crate) fn announce_call(&self, record: &CallRecord, update: CallUpdate) {
        let _ = self.events.send(Event::Call { contact: record.contact.clone(), call: record.call_id.clone(), update });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ringing(started_at: i64) -> CallRecord {
        CallRecord {
            call_id: "c".to_owned(),
            contact: "ft_bob".to_owned(),
            outgoing: false,
            video: false,
            started_at,
            answered_at: None,
            ended_at: None,
            outcome: None,
        }
    }

    // A call left ringing (the other phone died, say) must not keep us busy for ever.
    #[test]
    fn an_unanswered_call_goes_stale_after_ringing_too_long() {
        let limit = RING_LIMIT.as_millis() as i64;
        assert!(!stale(&ringing(1_000), 1_000 + limit - 1));
        assert!(stale(&ringing(1_000), 1_000 + limit + 1));
        let answered = CallRecord { answered_at: Some(2_000), ..ringing(1_000) };
        assert!(!stale(&answered, 1_000 + 10 * limit), "a call in progress is never stale");
    }

    #[test]
    fn call_updates_compare_by_content() {
        let ended = CallUpdate::Ended { outcome: CallOutcome::Busy };
        assert_eq!(ended.clone(), ended);
        assert_ne!(CallUpdate::Answered { sdp: "a".to_owned() }, CallUpdate::Answered { sdp: "b".to_owned() });
    }
}
