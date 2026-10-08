//! The free days and the subscription, all of it on this phone (Plan §39–§47).
//!
//! Nothing here talks to anyone: it decides, from what the phone already knows, what the user may
//! do. The money is the Store's business (§47) and the server never learns who pays (§45–§46).
//!
//! Ioan, 2026-10-08: chat, voice notes, files, calls, new conversations, circles and games are
//! free forever. The premium part —the tools (plugins of kind `tool`) and the extra sessions with a
//! PIN— is free for 15 days from the install, then needs the subscription; nothing else is ever
//! limited. There is no age rule any more.

use std::time::Duration;

/// How long the premium part is free from the first time the app opened here (§41, strategy A;
/// 15 days since 2026-10-08, a year before).
pub const FREE_PERIOD: Duration = Duration::from_secs(15 * 24 * 60 * 60);

/// How long a paid date that has passed still counts while the Store has not said anything since
/// (seen on an iPhone, 2026-10-08: the Store renewed at the end of the period and the open app heard
/// nothing). The core asks the Store when the date passes; only the Store's "nothing" ends it early.
pub const GRACE_MS: i64 = 3 * 24 * 60 * 60 * 1000;

/// What this phone knows about its own plan. All of it local.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    /// When the free days end (ms).
    pub free_until: i64,
    /// When the subscription runs out (ms), as the Store last said it; 0 when there is none (§45).
    pub paid_until: i64,
}

impl Plan {
    /// The next instant (ms) at which the access changes with nothing said: the end of the free
    /// days, the paid date (when the Store must be asked again), the end of the grace.
    pub fn next_change(&self, now: i64) -> Option<i64> {
        if now < self.free_until {
            return Some(self.free_until);
        }
        if self.paid_until <= 0 {
            return None;
        }
        if now < self.paid_until {
            return Some(self.paid_until);
        }
        let grace_ends = self.paid_until + GRACE_MS;
        (now < grace_ends).then_some(grace_ends)
    }

    /// Whether the paid date has passed with no word from the Store since: the Store must be asked.
    pub fn paid_date_passed(&self, now: i64) -> bool {
        self.paid_until > 0 && self.paid_until <= now
    }
}

/// Where the user stands right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Inside the free days.
    Trial { until: i64 },
    /// Paid up (§45).
    Subscribed { until: i64 },
    /// The free days are over and there is no subscription: everything goes on but the premium
    /// part.
    Limited,
}

/// The things access decides about. Writing, calling, sending files, circles and games are never
/// among them: they are free (Ioan, 2026-10-08), and a message that arrives is always delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Doing {
    /// Opening or installing a tool (a plugin of kind `tool`), the ones the app carries included.
    UseTool,
    /// Opening a hidden session with a PIN, a new one or one already there (§108). The main list
    /// always works.
    UseSession,
}

impl Access {
    /// What the phone is entitled to at `now`.
    pub fn of(now: i64, plan: Plan) -> Access {
        if now < plan.free_until {
            return Access::Trial { until: plan.free_until };
        }
        if plan.paid_until > 0 && now < plan.paid_until + GRACE_MS {
            return Access::Subscribed { until: plan.paid_until };
        }
        Access::Limited
    }

    /// Whether the user may do that now.
    pub fn may(self, doing: Doing) -> bool {
        match doing {
            // Everything `Doing` names is premium.
            Doing::UseTool | Doing::UseSession => self != Access::Limited,
        }
    }

    /// Whether the app should ask for the subscription.
    pub fn asks_to_pay(self) -> bool {
        self == Access::Limited
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 24 * 60 * 60 * 1000;
    const NOW: i64 = 1_800_000_000_000;

    fn plan(free_until: i64, paid_until: i64) -> Plan {
        Plan { free_until, paid_until }
    }

    // §41: 15 days from the first time the app opened here, no card, and the phone's own clock.
    #[test]
    fn the_first_days_are_free_tools_included() {
        let access = Access::of(NOW, plan(NOW + DAY, 0));
        assert_eq!(access, Access::Trial { until: NOW + DAY });
        assert!(!access.asks_to_pay());
        assert!(access.may(Doing::UseTool));
    }

    #[test]
    fn a_paid_year_is_a_paid_year() {
        let access = Access::of(NOW, plan(NOW - DAY, NOW + DAY));
        assert_eq!(access, Access::Subscribed { until: NOW + DAY });
        assert!(!access.asks_to_pay());
        assert!(access.may(Doing::UseTool));
        // One that ran out longer ago than the grace is no subscription at all.
        assert_eq!(Access::of(NOW, plan(NOW - DAY, NOW - GRACE_MS - 1)), Access::Limited);
    }

    // Seen on an iPhone (2026-10-08): the Store renewed at the end of the period, but the open app
    // heard nothing. The core asks the Store when the date passes, and a date nobody has denied
    // still counts for a few days while the Store cannot be heard.
    #[test]
    fn a_date_that_passed_counts_for_the_grace_while_the_store_says_nothing() {
        assert_eq!(GRACE_MS, 3 * DAY);
        assert_eq!(Access::of(NOW, plan(NOW - DAY, NOW - 1)), Access::Subscribed { until: NOW - 1 });
        assert_eq!(Access::of(NOW, plan(NOW - DAY, NOW - GRACE_MS + 1)), Access::Subscribed { until: NOW - GRACE_MS + 1 });
        assert_eq!(Access::of(NOW, plan(NOW - DAY, NOW - GRACE_MS)), Access::Limited);
    }

    // When the access changes by itself, with nothing said: the free days end, the paid date
    // passes (the Store is asked then), the grace ends.
    #[test]
    fn the_plan_knows_when_it_changes_by_itself() {
        assert_eq!(plan(NOW + DAY, 0).next_change(NOW), Some(NOW + DAY));
        assert_eq!(plan(NOW + DAY, NOW + 5 * DAY).next_change(NOW), Some(NOW + DAY));
        assert_eq!(plan(NOW - DAY, NOW + 5 * DAY).next_change(NOW), Some(NOW + 5 * DAY));
        assert_eq!(plan(NOW - DAY, NOW - DAY).next_change(NOW), Some(NOW - DAY + GRACE_MS));
        assert_eq!(plan(NOW - DAY, 0).next_change(NOW), None);
        assert_eq!(plan(NOW - DAY, NOW - GRACE_MS).next_change(NOW), None);
    }

    #[test]
    fn the_paid_date_passed_says_when_the_store_must_be_asked() {
        assert!(!plan(NOW - DAY, NOW + 1).paid_date_passed(NOW));
        assert!(plan(NOW - DAY, NOW).paid_date_passed(NOW));
        assert!(plan(NOW - DAY, NOW - GRACE_MS - DAY).paid_date_passed(NOW));
        assert!(!plan(NOW - DAY, 0).paid_date_passed(NOW));
    }

    // Ioan, 2026-10-08: chat, calls, files, circles and games are free forever. After the free
    // year, only the tools ask for the subscription.
    #[test]
    fn without_the_subscription_only_the_tools_are_closed() {
        let access = Access::of(NOW, plan(NOW - DAY, 0));
        assert_eq!(access, Access::Limited);
        assert!(access.asks_to_pay());
        assert!(!access.may(Doing::UseTool));
    }

    // Ioan, 2026-10-08 (01:00): the premium part is free for 15 days from the install, not a year.
    #[test]
    fn the_free_period_is_fifteen_days() {
        assert_eq!(FREE_PERIOD.as_secs(), 15 * 24 * 60 * 60);
    }

    // Extra sessions with a PIN are premium too; everything else never asks.
    #[test]
    fn the_extra_sessions_are_premium_like_the_tools() {
        for doing in [Doing::UseTool, Doing::UseSession] {
            assert!(Access::of(NOW, plan(NOW + DAY, 0)).may(doing));
            assert!(Access::of(NOW, plan(NOW - DAY, NOW + DAY)).may(doing));
            assert!(!Access::of(NOW, plan(NOW - DAY, 0)).may(doing));
        }
    }
}
