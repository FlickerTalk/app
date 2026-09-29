//! Call setup timings (2026-09-29, temporary diagnostics): how long each step between a call's
//! start (or its push) and its first audio took, to find where the seconds between answering and
//! being connected go, the direct connection's (the DataChannel's) steps included. Stage names and
//! milliseconds only: never who, never an address.

use std::sync::PoisonError;
use std::time::{Duration, Instant};

use crate::Core;

/// A step of a call's setup, on either side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallStage {
    /// The router's push for a call reached the phone (the bridge says so).
    PushReceived,
    /// We started a call.
    CallStarted,
    /// Our direct connection's offer, for a call, was ready to go through the router.
    LinkOffered,
    /// Our direct connection's description (offer or answer) was made and set: its gathering began.
    LinkGatheringStarted,
    /// That description was taken to be sent, with the candidates gathered by then.
    LinkGatheringDone,
    /// The router took our direct connection's offer: the other side is connected to it.
    LinkOfferSent,
    /// The other side is not connected: the router (0.4.0) woke it and keeps our offer for it.
    LinkOfferRetained,
    /// The other side's answer to our direct connection offer arrived.
    LinkAnswerReceived,
    /// The other side's direct connection offer arrived (a call's or a message's: this side
    /// cannot tell).
    LinkOfferReceived,
    /// We answered the other side's direct connection offer.
    LinkAnswered,
    /// A direct connection with the contact opened.
    LinkOpened,
    /// Their call's offer arrived.
    OfferReceived,
    /// The phone rings (the UI heard of the call).
    Ringing,
    /// The user answered (the bridge says so; the core hears it after the permissions).
    AnswerTapped,
    /// The core was asked to answer.
    AnswerRequested,
    /// The call's connection, with its tracks, exists.
    ConnectionBuilt,
    GatheringStarted,
    /// The description was taken to be sent, with the candidates gathered by then.
    GatheringDone,
    OfferBuilt,
    OfferSent,
    AnswerBuilt,
    AnswerSent,
    AnswerReceived,
    Connected,
    AudioDeviceStarted,
    FirstAudioPacket,
}

impl CallStage {
    pub fn name(self) -> &'static str {
        match self {
            Self::PushReceived => "push received",
            Self::CallStarted => "call started",
            Self::LinkOffered => "link offered",
            Self::LinkAnswered => "link answered",
            Self::LinkGatheringStarted => "link gathering started",
            Self::LinkGatheringDone => "link gathering done",
            Self::LinkOfferSent => "link offer sent",
            Self::LinkOfferRetained => "link offer retained",
            Self::LinkAnswerReceived => "link answer received",
            Self::LinkOfferReceived => "link offer received",
            Self::LinkOpened => "link open",
            Self::OfferReceived => "offer received",
            Self::Ringing => "ringing",
            Self::AnswerTapped => "answer tapped",
            Self::AnswerRequested => "answer requested",
            Self::ConnectionBuilt => "connection built",
            Self::GatheringStarted => "gathering started",
            Self::GatheringDone => "gathering done",
            Self::OfferBuilt => "offer built",
            Self::OfferSent => "offer sent",
            Self::AnswerBuilt => "answer built",
            Self::AnswerSent => "answer sent",
            Self::AnswerReceived => "answer received",
            Self::Connected => "connected",
            Self::AudioDeviceStarted => "audio device started",
            Self::FirstAudioPacket => "first audio packet",
        }
    }
}

/// The candidates our description went out with, by type.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Candidates {
    pub host: u16,
    pub srflx: u16,
    pub relay: u16,
    /// Whether gathering had completed, or the wait was cut short.
    pub complete: bool,
}

/// Where a call's setup stands: each stage reached, in milliseconds from the first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CallTimings {
    pub stages: Vec<(CallStage, u64)>,
    /// The call's media description.
    pub candidates: Option<Candidates>,
    /// The direct connection's (the DataChannel's) description, when this call opened one.
    pub link: Option<Candidates>,
}

impl CallTimings {
    /// When `stage` was reached, if it was.
    pub fn at(&self, stage: CallStage) -> Option<u64> {
        self.stages.iter().find(|(reached, _)| *reached == stage).map(|(_, ms)| *ms)
    }

    /// One line for the device log, in the order the stages were reached.
    pub fn line(&self) -> String {
        if self.stages.is_empty() {
            return "no call timings".to_owned();
        }
        let stages: Vec<String> = self.stages.iter().map(|(stage, ms)| format!("{} {ms} ms", stage.name())).collect();
        let mut line = stages.join(", ");
        for (what, found) in [("candidates", self.candidates), ("link candidates", self.link)] {
            if let Some(found) = found {
                let how = if found.complete { "complete" } else { "cut short" };
                line.push_str(&format!("; {what} host {} srflx {} relay {}, {how}", found.host, found.srflx, found.relay));
            }
        }
        line
    }
}

impl Core {
    /// A direct connection's description went out: when its gathering began and ended, and what
    /// it gave.
    pub(crate) fn mark_link_gathering(&self, gathering: &ft_webrtc::Gathering) {
        let mut clock = self.call_clock.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(started) = gathering.started {
            CallClock::mark(&mut clock, CallStage::LinkGatheringStarted, started);
        }
        if let Some(finished) = gathering.finished {
            CallClock::mark(&mut clock, CallStage::LinkGatheringDone, finished);
            let found = Candidates { host: gathering.host, srflx: gathering.srflx, relay: gathering.relay, complete: gathering.complete };
            CallClock::link_gathered(&mut clock, found, finished);
        }
    }
}

/// A clock is only joined this long after it started: a call rings for a minute at most.
const CLOCK_WINDOW: Duration = Duration::from_secs(120);

/// A clock started by a direct connection's offer is the call's only if the call's offer comes
/// this soon after it: the connection opens within 12 s, and the call's offer goes over it at once.
const LINK_WINDOW: Duration = Duration::from_secs(15);

/// The stages of the call being set up, as they happen.
#[derive(Debug, Clone)]
pub(crate) struct CallClock {
    start: Instant,
    marks: Vec<(CallStage, Instant)>,
    link: Option<Candidates>,
}

impl CallClock {
    /// `stage` was reached `at`. A push or our own call starts a new clock; their offer starts one
    /// unless it follows a push or a direct connection's offer that just started one; a direct
    /// connection's offer starts one unless a push or a call is being timed; anything else joins a
    /// fresh clock, and only its first time counts.
    pub(crate) fn mark(clock: &mut Option<CallClock>, stage: CallStage, at: Instant) {
        let fresh = clock.as_ref().filter(|current| at.saturating_duration_since(current.start) < CLOCK_WINDOW);
        let starts = match stage {
            CallStage::PushReceived | CallStage::CallStarted => true,
            CallStage::OfferReceived => fresh.is_none_or(|current| {
                current.reached(CallStage::OfferReceived)
                    || current.reached(CallStage::CallStarted)
                    || (current.started_by(CallStage::LinkOfferReceived) && at.saturating_duration_since(current.start) >= LINK_WINDOW)
            }),
            // A newer connection replaces an older one no call came over.
            CallStage::LinkOfferReceived => fresh.is_none_or(|current| {
                current.started_by(CallStage::LinkOfferReceived) && !current.reached(CallStage::OfferReceived)
            }),
            _ => false,
        };
        if starts {
            *clock = Some(CallClock { start: at, marks: vec![(stage, at)], link: None });
            return;
        }
        if fresh.is_none() {
            return;
        }
        if let Some(current) = clock.as_mut().filter(|current| !current.reached(stage)) {
            current.marks.push((stage, at));
        }
    }

    /// The direct connection's description went out `at` with these candidates: kept by a fresh
    /// clock (the last one counts: it is the connection that was used).
    pub(crate) fn link_gathered(clock: &mut Option<CallClock>, candidates: Candidates, at: Instant) {
        if let Some(current) = clock.as_mut().filter(|current| at.saturating_duration_since(current.start) < CLOCK_WINDOW) {
            current.link = Some(candidates);
        }
    }

    fn started_by(&self, stage: CallStage) -> bool {
        self.marks.first().is_some_and(|(first, _)| *first == stage)
    }

    fn reached(&self, stage: CallStage) -> bool {
        self.marks.iter().any(|(reached, _)| *reached == stage)
    }

    /// The timings so far, with what the media knows (`later`: stages reached out of the core's
    /// sight, such as the gathering and the first audio).
    pub(crate) fn timings(&self, later: &[(CallStage, Option<Instant>)], candidates: Option<Candidates>) -> CallTimings {
        let known = later.iter().filter_map(|(stage, at)| at.map(|at| (*stage, at)));
        let mut stages: Vec<(CallStage, Instant)> = self.marks.iter().copied().chain(known).collect();
        stages.sort_by_key(|(_, at)| *at);
        let since = |at: Instant| u64::try_from(at.saturating_duration_since(self.start).as_millis()).unwrap_or(u64::MAX);
        CallTimings { stages: stages.into_iter().map(|(stage, at)| (stage, since(at))).collect(), candidates, link: self.link }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn after(start: Instant, ms: u64) -> Instant {
        start + Duration::from_millis(ms)
    }

    #[test]
    fn a_push_or_a_call_starts_the_clock_and_later_stages_count_from_it() {
        let start = Instant::now();
        let mut clock = None;
        CallClock::mark(&mut clock, CallStage::Ringing, start);
        assert!(clock.is_none(), "nothing to join");
        CallClock::mark(&mut clock, CallStage::PushReceived, start);
        CallClock::mark(&mut clock, CallStage::LinkAnswered, after(start, 300));
        CallClock::mark(&mut clock, CallStage::OfferReceived, after(start, 450));
        CallClock::mark(&mut clock, CallStage::Ringing, after(start, 460));
        let timings = clock.as_ref().expect("a clock").timings(&[], None);
        assert_eq!(timings.at(CallStage::PushReceived), Some(0));
        assert_eq!(timings.at(CallStage::LinkAnswered), Some(300));
        assert_eq!(timings.at(CallStage::OfferReceived), Some(450), "the offer joins the push's clock");
        assert_eq!(timings.at(CallStage::Ringing), Some(460));

        CallClock::mark(&mut clock, CallStage::CallStarted, after(start, 5_000));
        let timings = clock.as_ref().expect("a clock").timings(&[], None);
        assert_eq!(timings.stages, [(CallStage::CallStarted, 0)], "our own call starts afresh");
    }

    #[test]
    fn a_second_offer_or_a_stale_clock_starts_a_new_one() {
        let start = Instant::now();
        let mut clock = None;
        CallClock::mark(&mut clock, CallStage::OfferReceived, start);
        CallClock::mark(&mut clock, CallStage::OfferReceived, after(start, 1_000));
        let timings = clock.as_ref().expect("a clock").timings(&[], None);
        assert_eq!(timings.stages, [(CallStage::OfferReceived, 0)], "the next call's offer");

        let mut clock = None;
        CallClock::mark(&mut clock, CallStage::PushReceived, start);
        CallClock::mark(&mut clock, CallStage::OfferReceived, after(start, 200_000));
        let timings = clock.as_ref().expect("a clock").timings(&[], None);
        assert_eq!(timings.stages, [(CallStage::OfferReceived, 0)], "a push long ago is another call's");
        CallClock::mark(&mut clock, CallStage::Ringing, after(start, 400_000));
        assert_eq!(clock.as_ref().expect("a clock").timings(&[], None).at(CallStage::Ringing), None, "too late to join");
    }

    #[test]
    fn a_stage_keeps_its_first_time_and_media_stages_count_from_the_start() {
        let start = Instant::now();
        let mut clock = None;
        CallClock::mark(&mut clock, CallStage::CallStarted, start);
        CallClock::mark(&mut clock, CallStage::LinkOpened, after(start, 100));
        CallClock::mark(&mut clock, CallStage::LinkOpened, after(start, 900));
        let later = [
            (CallStage::GatheringStarted, Some(after(start, 20))),
            (CallStage::GatheringDone, Some(after(start, 70))),
            (CallStage::FirstAudioPacket, None),
        ];
        let candidates = Candidates { host: 2, srflx: 1, relay: 0, complete: false };
        let timings = clock.as_ref().expect("a clock").timings(&later, Some(candidates));
        assert_eq!(
            timings.stages,
            [(CallStage::CallStarted, 0), (CallStage::GatheringStarted, 20), (CallStage::GatheringDone, 70), (CallStage::LinkOpened, 100)],
            "in the order they were reached"
        );
        assert_eq!(timings.candidates, Some(candidates));
        let before = [(CallStage::ConnectionBuilt, Some(start - Duration::from_millis(5)))];
        assert_eq!(clock.as_ref().expect("a clock").timings(&before, None).at(CallStage::ConnectionBuilt), Some(0));
    }

    // DataChannel setup timings (2026-09-29): the answering side cannot tell a call's direct
    // connection from a message's, so a link offer starts its clock; the call's offer that comes
    // over that connection joins it, and so the link's stages count from its offer.
    #[test]
    fn a_link_offer_starts_the_answering_side_s_clock_and_the_call_s_offer_joins_it() {
        let start = Instant::now();
        let mut clock = None;
        CallClock::mark(&mut clock, CallStage::LinkOfferReceived, start);
        CallClock::mark(&mut clock, CallStage::LinkGatheringStarted, after(start, 2));
        CallClock::mark(&mut clock, CallStage::LinkGatheringDone, after(start, 1_002));
        CallClock::mark(&mut clock, CallStage::LinkAnswered, after(start, 1_005));
        CallClock::mark(&mut clock, CallStage::LinkOpened, after(start, 1_300));
        CallClock::mark(&mut clock, CallStage::OfferReceived, after(start, 1_310));
        let timings = clock.as_ref().expect("a clock").timings(&[], None);
        assert_eq!(timings.at(CallStage::LinkOfferReceived), Some(0));
        assert_eq!(timings.at(CallStage::LinkGatheringDone), Some(1_002));
        assert_eq!(timings.at(CallStage::OfferReceived), Some(1_310), "the call's offer joins the link's clock");

        // After a push, the link's offer joins the push's clock.
        let mut clock = None;
        CallClock::mark(&mut clock, CallStage::PushReceived, start);
        CallClock::mark(&mut clock, CallStage::LinkOfferReceived, after(start, 200));
        CallClock::mark(&mut clock, CallStage::OfferReceived, after(start, 1_500));
        let timings = clock.as_ref().expect("a clock").timings(&[], None);
        assert_eq!(timings.stages, [(CallStage::PushReceived, 0), (CallStage::LinkOfferReceived, 200), (CallStage::OfferReceived, 1_500)]);
    }

    // A message's connection long before the call is not the call's; nor is an older link offer
    // once a newer one comes.
    #[test]
    fn only_a_recent_link_offer_is_the_call_s() {
        let start = Instant::now();
        let mut clock = None;
        CallClock::mark(&mut clock, CallStage::LinkOfferReceived, start);
        CallClock::mark(&mut clock, CallStage::OfferReceived, after(start, 30_000));
        let timings = clock.as_ref().expect("a clock").timings(&[], None);
        assert_eq!(timings.stages, [(CallStage::OfferReceived, 0)], "a message's connection, long ago");

        let mut clock = None;
        CallClock::mark(&mut clock, CallStage::LinkOfferReceived, start);
        CallClock::mark(&mut clock, CallStage::LinkOfferReceived, after(start, 5_000));
        CallClock::mark(&mut clock, CallStage::OfferReceived, after(start, 6_000));
        let timings = clock.as_ref().expect("a clock").timings(&[], None);
        assert_eq!(timings.stages, [(CallStage::LinkOfferReceived, 0), (CallStage::OfferReceived, 1_000)], "the newer link");

        // A call going on keeps its clock when its connection is made again.
        let mut clock = None;
        CallClock::mark(&mut clock, CallStage::CallStarted, start);
        CallClock::mark(&mut clock, CallStage::LinkOfferReceived, after(start, 9_000));
        assert_eq!(clock.as_ref().expect("a clock").timings(&[], None).at(CallStage::CallStarted), Some(0));
    }

    // The direct connection's description went out with these candidates: they go to the line
    // after the media's.
    #[test]
    fn the_link_s_candidates_go_with_the_timings() {
        let start = Instant::now();
        let mut clock = None;
        let link = Candidates { host: 1, srflx: 0, relay: 0, complete: false };
        CallClock::link_gathered(&mut clock, link, start);
        assert!(clock.is_none(), "nothing to join");
        CallClock::mark(&mut clock, CallStage::CallStarted, start);
        CallClock::link_gathered(&mut clock, link, after(start, 1_000));
        let media = Candidates { host: 2, srflx: 1, relay: 1, complete: true };
        let timings = clock.as_ref().expect("a clock").timings(&[], Some(media));
        assert_eq!(timings.link, Some(link));
        assert_eq!(
            timings.line(),
            "call started 0 ms; candidates host 2 srflx 1 relay 1, complete; link candidates host 1 srflx 0 relay 0, cut short"
        );
        let without_media = clock.as_ref().expect("a clock").timings(&[], None);
        assert_eq!(without_media.line(), "call started 0 ms; link candidates host 1 srflx 0 relay 0, cut short");
    }

    // The device log gets names and numbers, nothing else.
    #[test]
    fn the_line_names_stages_and_milliseconds_only() {
        let timings = CallTimings {
            stages: vec![(CallStage::OfferReceived, 0), (CallStage::AnswerSent, 1_250), (CallStage::Connected, 1_900)],
            candidates: Some(Candidates { host: 2, srflx: 1, relay: 1, complete: false }),
            link: None,
        };
        assert_eq!(
            timings.line(),
            "offer received 0 ms, answer sent 1250 ms, connected 1900 ms; candidates host 2 srflx 1 relay 1, cut short"
        );
        let complete = CallTimings { candidates: Some(Candidates { complete: true, ..Candidates::default() }), ..timings };
        assert!(complete.line().ends_with("candidates host 0 srflx 0 relay 0, complete"));
        assert_eq!(CallTimings::default().line(), "no call timings");
    }

    #[test]
    fn every_stage_has_a_name() {
        use CallStage::*;
        let all = [
            PushReceived, CallStarted, LinkGatheringStarted, LinkGatheringDone, LinkOffered, LinkOfferSent,
            LinkOfferRetained, LinkAnswerReceived, LinkOfferReceived, LinkAnswered, LinkOpened, OfferReceived, Ringing, AnswerTapped,
            AnswerRequested, ConnectionBuilt, GatheringStarted, GatheringDone, OfferBuilt, OfferSent, AnswerBuilt,
            AnswerSent, AnswerReceived, Connected, AudioDeviceStarted, FirstAudioPacket,
        ];
        let names: std::collections::BTreeSet<&str> = all.iter().map(|stage| stage.name()).collect();
        assert_eq!(names.len(), all.len(), "distinct");
        assert!(names.iter().all(|name| !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c == ' ')));
        assert_eq!(PushReceived.name(), "push received");
        assert_eq!(FirstAudioPacket.name(), "first audio packet");
        assert_eq!(LinkOfferRetained.name(), "link offer retained");
    }
}
