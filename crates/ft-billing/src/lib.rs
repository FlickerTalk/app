//! The free year, the age class and the subscription, all of it on this phone (Plan §39–§47).
//!
//! Nothing here talks to anyone: it decides, from what the phone already knows, what the user may
//! do. The money is the Store's business (§47) and the server never learns who pays (§45–§46).

use std::time::Duration;

/// How long the app is free from the first time it opened here (§41, strategy A).
pub const FREE_PERIOD: Duration = Duration::from_secs(365 * 24 * 60 * 60);

/// What the price depends on. Never a date of birth, and it never leaves the phone (§30, §43).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgeClass {
    /// A minor, as the phone's system says it (Ioan, 2026-10-07): always free.
    Minor,
    Adult,
    /// The system could not say (no age API, an older system, the user declined to share). Pays
    /// after the free year like an adult; never assumed to be a minor.
    #[default]
    Unknown,
}

impl AgeClass {
    pub fn as_str(self) -> &'static str {
        match self {
            AgeClass::Minor => "minor",
            AgeClass::Adult => "adult",
            AgeClass::Unknown => "unknown",
        }
    }

    /// What the phone keeps once the system answered `heard` (2026-10-07): an unknown age takes
    /// any answer and a minor may grow up, but an adult is never taken back and a system that
    /// cannot answer any more forgets nothing.
    pub fn then_heard(self, heard: AgeClass) -> AgeClass {
        match (self, heard) {
            (AgeClass::Unknown, heard) => heard,
            (AgeClass::Minor, AgeClass::Adult) => AgeClass::Adult,
            (kept, _) => kept,
        }
    }

    /// Reads back what `as_str` wrote. Anything else is `Unknown`: a date, a number, nonsense.
    pub fn of(value: &str) -> Self {
        match value {
            "minor" => AgeClass::Minor,
            "adult" => AgeClass::Adult,
            _ => AgeClass::Unknown,
        }
    }
}

/// What this phone knows about its own plan. All of it local.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    /// When the free year ends (ms).
    pub free_until: i64,
    pub age: AgeClass,
    /// When the subscription runs out (ms); 0 when there is none (§45).
    pub paid_until: i64,
}

/// Where the user stands right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Inside the free year.
    Trial { until: i64 },
    /// A minor: free, for as long as the system says so (§40).
    Young,
    /// Paid up (§45).
    Subscribed { until: i64 },
    /// The year is over and there is no subscription: the conversation goes on, but nothing new
    /// starts (Ioan, 2026-09-23).
    Limited,
}

/// The things access decides about. Receiving is never one of them: a message that arrives is
/// delivered, whatever the plan says (§1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Doing {
    /// Answering in a conversation that already exists.
    Reply,
    /// Writing to someone for the first time.
    Start,
    Call,
    SendFile,
}

impl Access {
    /// What the phone is entitled to at `now`.
    pub fn of(now: i64, plan: Plan) -> Access {
        if now < plan.free_until {
            return Access::Trial { until: plan.free_until };
        }
        if plan.age == AgeClass::Minor {
            return Access::Young;
        }
        if plan.paid_until > now {
            return Access::Subscribed { until: plan.paid_until };
        }
        Access::Limited
    }

    /// Whether the user may do that now.
    pub fn may(self, doing: Doing) -> bool {
        match self {
            Access::Limited => doing == Doing::Reply,
            _ => true,
        }
    }

    /// Whether the app should ask for the euro.
    pub fn asks_to_pay(self) -> bool {
        self == Access::Limited
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 24 * 60 * 60 * 1000;
    const NOW: i64 = 1_800_000_000_000;

    fn plan(free_until: i64, age: AgeClass, paid_until: i64) -> Plan {
        Plan { free_until, age, paid_until }
    }

    // §41: a year from the first time the app opened here, no card, and the phone's own clock.
    #[test]
    fn the_first_year_is_free_whoever_you_are() {
        for age in [AgeClass::Minor, AgeClass::Adult, AgeClass::Unknown] {
            let access = Access::of(NOW, plan(NOW + DAY, age, 0));
            assert_eq!(access, Access::Trial { until: NOW + DAY });
            assert!(!access.asks_to_pay());
            for doing in [Doing::Reply, Doing::Start, Doing::Call, Doing::SendFile] {
                assert!(access.may(doing));
            }
        }
    }

    // §40: minors, always free. Not "free for a while": free.
    #[test]
    fn a_minor_is_always_free() {
        let access = Access::of(NOW, plan(NOW - DAY, AgeClass::Minor, 0));
        assert_eq!(access, Access::Young);
        assert!(!access.asks_to_pay());
        assert!(access.may(Doing::Start) && access.may(Doing::Call));
    }

    #[test]
    fn a_paid_year_is_a_paid_year() {
        let access = Access::of(NOW, plan(NOW - DAY, AgeClass::Adult, NOW + DAY));
        assert_eq!(access, Access::Subscribed { until: NOW + DAY });
        assert!(access.may(Doing::Start));
        // One that ran out is no subscription at all.
        assert_eq!(Access::of(NOW, plan(NOW - DAY, AgeClass::Adult, NOW - 1)), Access::Limited);
    }

    // An adult who does not pay keeps receiving and keeps answering: nobody loses a message
    // because of the euro. What stops is starting something new (Ioan, 2026-09-23).
    #[test]
    fn without_the_euro_the_conversation_goes_on_but_nothing_new_starts() {
        let access = Access::of(NOW, plan(NOW - DAY, AgeClass::Adult, 0));
        assert_eq!(access, Access::Limited);
        assert!(access.asks_to_pay());
        assert!(access.may(Doing::Reply), "answering is always allowed");
        assert!(!access.may(Doing::Start));
        assert!(!access.may(Doing::Call));
        assert!(!access.may(Doing::SendFile));
    }

    // Not knowing is not being a minor: it is priced as an adult unless the system says otherwise.
    #[test]
    fn an_age_nobody_asked_about_is_not_a_free_pass() {
        assert_eq!(Access::of(NOW, plan(NOW - DAY, AgeClass::Unknown, 0)), Access::Limited);
        assert_eq!(AgeClass::default(), AgeClass::Unknown);
    }

    #[test]
    fn the_age_class_travels_as_a_word_and_never_as_a_date() {
        for age in [AgeClass::Minor, AgeClass::Adult, AgeClass::Unknown] {
            assert_eq!(AgeClass::of(age.as_str()), age);
        }
        assert_eq!(AgeClass::of("1999-04-01"), AgeClass::Unknown);
    }

    // Ioan, 2026-10-07: minor or adult is what the phone's system says (Declared Age Range on
    // iOS, Play Age Signals on Android), never what the user declares. A later answer may only
    // fill in what was unknown or say that a minor has grown up; it never takes back an adult,
    // and a system that cannot answer any more forgets nothing.
    #[test]
    fn the_platform_may_only_fill_in_the_age_or_say_a_minor_grew_up() {
        use AgeClass::*;
        assert_eq!(Unknown.then_heard(Minor), Minor);
        assert_eq!(Unknown.then_heard(Adult), Adult);
        assert_eq!(Unknown.then_heard(Unknown), Unknown);
        assert_eq!(Minor.then_heard(Adult), Adult, "a minor grows up");
        assert_eq!(Minor.then_heard(Unknown), Minor, "a system that cannot answer forgets nothing");
        assert_eq!(Minor.then_heard(Minor), Minor);
        assert_eq!(Adult.then_heard(Minor), Adult, "an adult never becomes a minor again");
        assert_eq!(Adult.then_heard(Unknown), Adult);
        assert_eq!(Adult.then_heard(Adult), Adult);
    }

    #[test]
    fn the_free_period_is_a_year() {
        assert_eq!(FREE_PERIOD.as_secs(), 365 * 24 * 60 * 60);
    }
}
