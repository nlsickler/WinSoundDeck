use std::time::{Duration, Instant};

/// Time-based burst coalescer that schedules a single action after `delay`
/// elapses without new triggers.
pub struct Debouncer {
    delay: Duration,
    deadline: Option<Instant>,
}

impl Debouncer {
    pub fn new(delay: Duration) -> Self {
        Self {
            delay,
            deadline: None,
        }
    }

    pub fn trigger(&mut self, now: Instant) {
        self.deadline = Some(now + self.delay);
    }

    pub fn next_timeout(&self, now: Instant, max_wait: Duration) -> Duration {
        match self.deadline {
            Some(dl) => dl.saturating_duration_since(now).min(max_wait),
            None => max_wait,
        }
    }

    pub fn take_if_due(&mut self, now: Instant) -> bool {
        if self.deadline.is_some_and(|dl| now >= dl) {
            self.deadline = None;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Debouncer;
    use std::time::{Duration, Instant};

    #[test]
    fn debouncer_coalesces_bursts_and_fires_once_settled() {
        let t0 = Instant::now();
        let mut d = Debouncer::new(Duration::from_millis(300));
        assert!(!d.take_if_due(t0));
        assert_eq!(
            d.next_timeout(t0, Duration::from_millis(500)),
            Duration::from_millis(500)
        );

        d.trigger(t0);
        assert!(!d.take_if_due(t0 + Duration::from_millis(150)));

        d.trigger(t0 + Duration::from_millis(150));
        assert!(!d.take_if_due(t0 + Duration::from_millis(350)));
        assert_eq!(
            d.next_timeout(t0 + Duration::from_millis(350), Duration::from_millis(500)),
            Duration::from_millis(100)
        );

        assert!(d.take_if_due(t0 + Duration::from_millis(450)));
        assert!(!d.take_if_due(t0 + Duration::from_millis(500)));
    }
}
