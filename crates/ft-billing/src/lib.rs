//! The free year and the subscription, all of it on this phone (Plan §39–§47).
//!
//! Nothing here talks to anyone: it decides, from what the phone already knows, what the user may
//! do. The money is the Store's business (§47) and the server never learns who pays (§45–§46).
//!
//! Ioan, 2026-10-08: chat, voice notes, files, calls, new conversations, circles and games are
//! free forever. The subscription, after the first year, is for the tools (plugins of kind
//! `tool`), and nothing else is ever limited. There is no age rule any more.

use std::time::Duration;

/// How long the tools are free from the first time the app opened here (§41, strategy A).
pub const FREE_PERIOD: Duration = Duration::from_secs(365 * 24 * 60 * 60);

/// What this phone knows about its own plan. All of it local.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    /// When the free year ends (ms).
    pub free_until: i64,
    /// When the subscription runs out (ms); 0 when there is none (§45).
    pub paid_until: i64,
}

/// Where the user stands right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Inside the free year.
    Trial { until: i64 },
    /// Paid up (§45).
    Subscribed { until: i64 },
    /// The year is over and there is no subscription: everything goes on but the tools.
    Limited,
}

/// The things access decides about. Writing, calling, sending files, circles and games are never
/// among them: they are free (Ioan, 2026-10-08), and a message that arrives is always delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Doing {
    /// Opening or installing a tool (a plugin of kind `tool`), the ones the app carries included.
    UseTool,
}

impl Access {
    /// What the phone is entitled to at `now`.
    pub fn of(now: i64, plan: Plan) -> Access {
        if now < plan.free_until {
            return Access::Trial { until: plan.free_until };
        }
        if plan.paid_until > now {
            return Access::Subscribed { until: plan.paid_until };
        }
        Access::Limited
    }

    /// Whether the user may do that now.
    pub fn may(self, doing: Doing) -> bool {
        !matches!((self, doing), (Access::Limited, Doing::UseTool))
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

    // §41: a year from the first time the app opened here, no card, and the phone's own clock.
    #[test]
    fn the_first_year_is_free_tools_included() {
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
        // One that ran out is no subscription at all.
        assert_eq!(Access::of(NOW, plan(NOW - DAY, NOW - 1)), Access::Limited);
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

    #[test]
    fn the_free_period_is_a_year() {
        assert_eq!(FREE_PERIOD.as_secs(), 365 * 24 * 60 * 60);
    }
}
