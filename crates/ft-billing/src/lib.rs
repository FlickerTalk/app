//! Runs on the phone only. The server never learns who pays.

use std::time::Duration;

pub const FREE_PERIOD: Duration = Duration::from_secs(15 * 24 * 60 * 60);

// the Store can renew at period end without telling the open app, so a passed date counts this long
pub const GRACE_MS: i64 = 3 * 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    pub free_until: i64,
    // ms, 0 = none
    pub paid_until: i64,
}

impl Plan {
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

    pub fn paid_date_passed(&self, now: i64) -> bool {
        self.paid_until > 0 && self.paid_until <= now
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Trial { until: i64 },
    Subscribed { until: i64 },
    Limited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Doing {
    UseTool,
    UseSession,
}

impl Access {
    pub fn of(now: i64, plan: Plan) -> Access {
        if now < plan.free_until {
            return Access::Trial { until: plan.free_until };
        }
        if plan.paid_until > 0 && now < plan.paid_until + GRACE_MS {
            return Access::Subscribed { until: plan.paid_until };
        }
        Access::Limited
    }

    pub fn may(self, doing: Doing) -> bool {
        match doing {
            Doing::UseTool | Doing::UseSession => self != Access::Limited,
        }
    }

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
        assert_eq!(Access::of(NOW, plan(NOW - DAY, NOW - GRACE_MS - 1)), Access::Limited);
    }

    #[test]
    fn a_date_that_passed_counts_for_the_grace_while_the_store_says_nothing() {
        assert_eq!(GRACE_MS, 3 * DAY);
        assert_eq!(Access::of(NOW, plan(NOW - DAY, NOW - 1)), Access::Subscribed { until: NOW - 1 });
        assert_eq!(Access::of(NOW, plan(NOW - DAY, NOW - GRACE_MS + 1)), Access::Subscribed { until: NOW - GRACE_MS + 1 });
        assert_eq!(Access::of(NOW, plan(NOW - DAY, NOW - GRACE_MS)), Access::Limited);
    }

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

    #[test]
    fn without_the_subscription_only_the_tools_are_closed() {
        let access = Access::of(NOW, plan(NOW - DAY, 0));
        assert_eq!(access, Access::Limited);
        assert!(access.asks_to_pay());
        assert!(!access.may(Doing::UseTool));
    }

    #[test]
    fn the_free_period_is_fifteen_days() {
        assert_eq!(FREE_PERIOD.as_secs(), 15 * 24 * 60 * 60);
    }

    #[test]
    fn the_extra_sessions_are_premium_like_the_tools() {
        for doing in [Doing::UseTool, Doing::UseSession] {
            assert!(Access::of(NOW, plan(NOW + DAY, 0)).may(doing));
            assert!(Access::of(NOW, plan(NOW - DAY, NOW + DAY)).may(doing));
            assert!(!Access::of(NOW, plan(NOW - DAY, 0)).may(doing));
        }
    }
}
