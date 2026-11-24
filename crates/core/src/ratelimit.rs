/// Camera-open rate limiter with exponential backoff.
///
/// * At most one open per `min_interval_ms`.
/// * Every "wasted" open (aborted tilt, timeout, camera error, settle without
///   an orientation change) and every flap (a change that reverts the previous
///   change within the flap window) doubles the interval, up to `max_backoff_ms`.
/// * A decision that sticks halves it again; `reset_after_ms` without any
///   camera use resets it completely.
#[derive(Debug, Clone)]
pub struct RateLimiter {
    min_interval_ms: u64,
    max_backoff_ms: u64,
    reset_after_ms: u64,
    level: u32,
    last_open_ms: Option<u64>,
}

const MAX_LEVEL: u32 = 6;

impl RateLimiter {
    /// New limiter.
    pub fn new(min_interval_ms: u64, max_backoff_ms: u64, reset_after_ms: u64) -> Self {
        RateLimiter { min_interval_ms, max_backoff_ms, reset_after_ms, level: 0, last_open_ms: None }
    }

    /// Current minimum interval between opens.
    pub fn interval_ms(&self) -> u64 {
        (self.min_interval_ms << self.level).min(self.max_backoff_ms.max(self.min_interval_ms))
    }

    /// Current backoff level (0 = no backoff).
    pub fn level(&self) -> u32 {
        self.level
    }

    fn maybe_reset(&mut self, now_ms: u64) {
        if let Some(t) = self.last_open_ms {
            if now_ms.saturating_sub(t) >= self.reset_after_ms {
                self.level = 0;
            }
        }
    }

    /// Whether the camera may be opened at `now_ms`.
    pub fn can_open(&mut self, now_ms: u64) -> bool {
        self.maybe_reset(now_ms);
        match self.last_open_ms {
            None => true,
            Some(t) => now_ms.saturating_sub(t) >= self.interval_ms(),
        }
    }

    /// Records an open.
    pub fn on_open(&mut self, now_ms: u64) {
        self.last_open_ms = Some(now_ms);
    }

    /// Records a wasted open or a flap.
    pub fn on_wasted(&mut self) {
        self.level = (self.level + 1).min(MAX_LEVEL);
    }

    /// Records a useful decision.
    pub fn on_useful(&mut self) {
        self.level = self.level.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_interval() {
        let mut r = RateLimiter::new(800, 8000, 15_000);
        assert!(r.can_open(0));
        r.on_open(0);
        assert!(!r.can_open(799));
        assert!(r.can_open(800));
    }

    #[test]
    fn backoff_doubles_caps_and_resets() {
        let mut r = RateLimiter::new(800, 8000, 15_000);
        r.on_open(0);
        r.on_wasted();
        assert_eq!(r.interval_ms(), 1600);
        r.on_wasted();
        r.on_wasted();
        assert_eq!(r.interval_ms(), 6400);
        r.on_wasted();
        assert_eq!(r.interval_ms(), 8000);
        for _ in 0..10 {
            r.on_wasted();
        }
        assert_eq!(r.interval_ms(), 8000);
        assert!(!r.can_open(7999));
        // Long quiet period resets.
        assert!(r.can_open(15_000));
        assert_eq!(r.level(), 0);
    }

    #[test]
    fn useful_decisions_relax_backoff() {
        let mut r = RateLimiter::new(800, 8000, 15_000);
        r.on_wasted();
        r.on_wasted();
        r.on_useful();
        assert_eq!(r.interval_ms(), 1600);
    }
}
