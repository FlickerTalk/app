//! Voice and video calls (Plan §66, §106 M6), one to one. The media is the WebView's WebRTC; the
//! core carries its descriptions (offer, answer) and the end of the call, encrypted with Olm and
//! only over a direct connection (opened on demand through the router): never through the mailbox.
//! It also keeps the call history, which never leaves the phone.
//!
//! One call at a time: an offer that arrives during another call is answered "busy" and logged as
//! missed.

use std::time::Duration;

use anyhow::{bail, Result};
use ft_protocol::{Body, EndReason, MessageId, Packet};
use ft_storage::{CallOutcome, CallRecord, Contact};

use crate::{now, Core, Event};

/// A call nobody answers stops ringing after this long.
pub const RING_LIMIT: Duration = Duration::from_secs(60);

/// A call that rang unanswered for longer than it can: its caller is gone.
fn stale(record: &CallRecord, now: i64) -> bool {
    record.answered_at.is_none() && now - record.started_at > RING_LIMIT.as_millis() as i64
}

/// What happened to a call, for the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallUpdate {
    /// The contact is calling: the UI rings and, if accepted, answers `sdp`.
    Incoming { video: bool, sdp: String },
    /// The contact answered our call.
    Answered { sdp: String },
    Ended { outcome: CallOutcome },
}

impl Core {
    /// Logs a new outgoing call and returns its id; the offer follows with `offer_call`.
    pub async fn place_call(&self, contact: &str, video: bool) -> Result<String> {
        let stored = self.contact(contact).await?;
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

    /// Sends our offer; if the contact cannot be reached directly, the call ends as unreachable.
    pub async fn offer_call(&self, call: &str, sdp: &str) -> Result<()> {
        let Some((record, contact)) = self.open_call(call, true).await? else { bail!("no such call") };
        let body = Body::CallOffer { call: MessageId::parse(call)?, sdp: sdp.to_owned(), video: record.video };
        if !self.transmit_direct(&contact, &Packet::new(body)).await? {
            self.close_call(&record, CallOutcome::Unreachable).await?;
        }
        Ok(())
    }

    /// Accepts an incoming call with our answer.
    pub async fn answer_call(&self, call: &str, sdp: &str) -> Result<()> {
        let Some((record, contact)) = self.open_call(call, false).await? else { bail!("no such call") };
        self.store.answer_call(call, now()).await?;
        let body = Body::CallAnswer { call: MessageId::parse(call)?, sdp: sdp.to_owned() };
        if !self.transmit_direct(&contact, &Packet::new(body)).await? {
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
        let body = Body::CallEnd { call: MessageId::parse(call)?, reason };
        let _ = self.transmit_direct(&contact, &Packet::new(body)).await;
        Ok(())
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

    /// The contact calls us.
    pub(crate) async fn call_offered(&self, contact: &Contact, call: MessageId, sdp: String, video: bool) -> Result<()> {
        let call_id = call.to_string();
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
                self.announce_call(&record, CallUpdate::Ended { outcome: CallOutcome::Missed });
            }
            return Ok(());
        }
        if fresh {
            self.announce_call(&record, CallUpdate::Incoming { video, sdp });
        }
        Ok(())
    }

    /// The contact answered our call.
    pub(crate) async fn call_answered(&self, contact: &Contact, call: MessageId, sdp: String) -> Result<()> {
        let Some((record, _)) = self.open_call(&call.to_string(), true).await? else { return Ok(()) };
        if record.contact != contact.device_id {
            return Ok(());
        }
        self.store.answer_call(&record.call_id, now()).await?;
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

    async fn close_call(&self, record: &CallRecord, outcome: CallOutcome) -> Result<()> {
        self.store.finish_call(&record.call_id, now(), outcome).await?;
        {
            let mut active = self.active_call.lock().expect("active call poisoned");
            if active.as_deref() == Some(record.call_id.as_str()) {
                *active = None;
            }
        }
        self.announce_call(record, CallUpdate::Ended { outcome });
        Ok(())
    }

    fn announce_call(&self, record: &CallRecord, update: CallUpdate) {
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
