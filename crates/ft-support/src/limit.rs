//! In memory only, so a restart forgives everyone and nothing about who wrote is kept.

use std::collections::HashMap;
use std::time::Duration;

const DAY_MS: u64 = 24 * 60 * 60 * 1000;

const TRACKED_BEFORE_PRUNING: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub gap: Duration,
    pub per_day: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self { gap: Duration::from_secs(1), per_day: 200 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Go,
    Wait(Duration),
    Spent { notice: bool },
}

#[derive(Debug, Clone, Copy)]
struct Tally {
    day: u64,
    count: u32,
    last_ms: Option<u64>,
    noticed: bool,
}

#[derive(Debug)]
pub struct RateLimiter {
    limits: Limits,
    tallies: HashMap<String, Tally>,
}

impl RateLimiter {
    pub fn new(limits: Limits) -> Self {
        Self { limits, tallies: HashMap::new() }
    }

    pub fn check(&mut self, contact: &str, now_ms: u64) -> Verdict {
        self.prune(now_ms);
        let Limits { gap, per_day } = self.limits;
        let Some(tally) = self.tally(contact, now_ms) else { return Verdict::Go };
        if tally.count >= per_day {
            let notice = !tally.noticed;
            tally.noticed = true;
            return Verdict::Spent { notice };
        }
        let gap = u64::try_from(gap.as_millis()).unwrap_or(u64::MAX);
        match tally.last_ms.map(|last| now_ms.saturating_sub(last)) {
            Some(elapsed) if elapsed < gap => Verdict::Wait(Duration::from_millis(gap - elapsed)),
            _ => Verdict::Go,
        }
    }

    pub fn record(&mut self, contact: &str, now_ms: u64) {
        let day = now_ms / DAY_MS;
        let tally = self.tallies.entry(contact.to_owned()).or_insert(Tally { day, count: 0, last_ms: None, noticed: false });
        Self::roll(tally, day);
        tally.count = tally.count.saturating_add(1);
        tally.last_ms = Some(now_ms);
    }

    fn tally(&mut self, contact: &str, now_ms: u64) -> Option<&mut Tally> {
        let tally = self.tallies.get_mut(contact)?;
        Self::roll(tally, now_ms / DAY_MS);
        Some(tally)
    }

    fn roll(tally: &mut Tally, day: u64) {
        if tally.day != day {
            *tally = Tally { day, count: 0, last_ms: tally.last_ms, noticed: false };
        }
    }

    fn prune(&mut self, now_ms: u64) {
        if self.tallies.len() >= TRACKED_BEFORE_PRUNING {
            let today = now_ms / DAY_MS;
            self.tallies.retain(|_, tally| tally.day == today);
        }
    }

    pub fn tracked(&self) -> usize {
        self.tallies.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOON: u64 = 20_000 * DAY_MS + 12 * 60 * 60 * 1000;

    fn limiter() -> RateLimiter {
        RateLimiter::new(Limits::default())
    }

    #[test]
    fn the_first_answer_goes_at_once() {
        assert_eq!(limiter().check("ft_a", NOON), Verdict::Go);
    }

    #[test]
    fn a_second_answer_within_the_gap_waits_for_the_rest_of_it() {
        let mut limiter = limiter();
        limiter.record("ft_a", NOON);
        assert_eq!(limiter.check("ft_a", NOON + 300), Verdict::Wait(Duration::from_millis(700)));
        assert_eq!(limiter.check("ft_a", NOON + 1_000), Verdict::Go);
    }

    #[test]
    fn contacts_are_counted_apart() {
        let mut limiter = limiter();
        limiter.record("ft_a", NOON);
        assert_eq!(limiter.check("ft_b", NOON + 10), Verdict::Go);
    }

    #[test]
    fn the_day_s_answers_run_out_and_that_is_said_once() {
        let mut limiter = RateLimiter::new(Limits { gap: Duration::ZERO, per_day: 3 });
        for n in 0..3 {
            assert_eq!(limiter.check("ft_a", NOON + n), Verdict::Go);
            limiter.record("ft_a", NOON + n);
        }
        assert_eq!(limiter.check("ft_a", NOON + 10), Verdict::Spent { notice: true });
        assert_eq!(limiter.check("ft_a", NOON + 20), Verdict::Spent { notice: false });
        assert_eq!(limiter.check("ft_b", NOON + 20), Verdict::Go, "someone else still gets answers");
    }

    #[test]
    fn a_new_utc_day_starts_afresh() {
        let mut limiter = RateLimiter::new(Limits { gap: Duration::from_secs(1), per_day: 1 });
        limiter.record("ft_a", NOON);
        assert_eq!(limiter.check("ft_a", NOON + 5_000), Verdict::Spent { notice: true });
        assert_eq!(limiter.check("ft_a", NOON + DAY_MS), Verdict::Go);
    }

    #[test]
    fn the_gap_holds_across_midnight() {
        let mut limiter = limiter();
        let midnight = 20_001 * DAY_MS;
        limiter.record("ft_a", midnight - 200);
        assert_eq!(limiter.check("ft_a", midnight + 100), Verdict::Wait(Duration::from_millis(700)));
    }

    #[test]
    fn many_contacts_from_another_day_are_forgotten() {
        let mut limiter = limiter();
        for n in 0..TRACKED_BEFORE_PRUNING {
            limiter.record(&format!("ft_{n}"), NOON);
        }
        limiter.record("ft_today", NOON + DAY_MS);
        let _ = limiter.check("ft_new", NOON + DAY_MS);
        assert_eq!(limiter.tracked(), 1, "only today's contact is kept");
    }
}
