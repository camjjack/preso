//! Presenter talk timer.
//!
//! Per AGENTS.md, no `Instant::now()` in logic: the current time is always
//! injected, so tests drive a synthetic clock and `App` supplies the real
//! one via [`Clock`].

use std::time::{Duration, Instant};

pub trait Clock {
    fn now(&self) -> Instant;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

const WARN_THRESHOLD: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Copy)]
pub struct Timer {
    started: Option<Instant>,
    /// Total talk duration for countdown display (`--duration`).
    countdown: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimerDisplay {
    /// `MM:SS` since the first navigation.
    pub elapsed: String,
    /// `(MM:SS remaining, warn)` when a countdown is configured.
    /// `warn` flips at five minutes left (clamps at 00:00).
    pub remaining: Option<(String, bool)>,
}

impl Timer {
    pub fn new(countdown_minutes: Option<u64>) -> Self {
        Self {
            started: None,
            countdown: countdown_minutes.map(|m| Duration::from_secs(m * 60)),
        }
    }

    /// The talk starts at the first navigation.
    pub fn start_if_needed(&mut self, now: Instant) {
        self.started.get_or_insert(now);
    }

    pub fn running(&self) -> bool {
        self.started.is_some()
    }

    pub fn reset(&mut self) {
        self.started = None;
    }

    pub fn display(&self, now: Instant) -> Option<TimerDisplay> {
        let started = self.started?;
        let elapsed = now.saturating_duration_since(started);
        let remaining = self.countdown.map(|total| {
            let left = total.saturating_sub(elapsed);
            (format_mmss(left), left <= WARN_THRESHOLD)
        });
        Some(TimerDisplay {
            elapsed: format_mmss(elapsed),
            remaining,
        })
    }
}

fn format_mmss(d: Duration) -> String {
    let secs = d.as_secs();
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_running_until_first_navigation() {
        let t = Timer::new(None);
        assert!(!t.running());
        assert!(t.display(Instant::now()).is_none());
    }

    #[test]
    fn elapsed_formats_mmss() {
        let t0 = Instant::now();
        let mut t = Timer::new(None);
        t.start_if_needed(t0);
        t.start_if_needed(t0 + Duration::from_secs(999)); // no restart
        let d = t.display(t0 + Duration::from_secs(125)).unwrap();
        assert_eq!(d.elapsed, "02:05");
        assert_eq!(d.remaining, None);
    }

    #[test]
    fn countdown_warns_at_five_minutes_and_clamps() {
        let t0 = Instant::now();
        let mut t = Timer::new(Some(30));
        t.start_if_needed(t0);

        let early = t.display(t0 + Duration::from_secs(10 * 60)).unwrap();
        assert_eq!(early.remaining, Some(("20:00".into(), false)));

        let warn = t.display(t0 + Duration::from_secs(26 * 60)).unwrap();
        assert_eq!(warn.remaining, Some(("04:00".into(), true)));

        let over = t.display(t0 + Duration::from_secs(31 * 60)).unwrap();
        assert_eq!(over.remaining, Some(("00:00".into(), true)));
    }

    #[test]
    fn reset_stops_the_timer() {
        let t0 = Instant::now();
        let mut t = Timer::new(None);
        t.start_if_needed(t0);
        t.reset();
        assert!(!t.running());
    }
}
