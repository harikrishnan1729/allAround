//! AOSP-grade gravity → orientation proposer.
//!
//! A faithful port of Android's `WindowOrientationListener.SensorEventListenerImpl`
//! (the logic that makes stock Android auto-rotate feel "flawless"), which
//! Pixel's face-aware auto-rotate rides on top of. The constants are AOSP's
//! (frameworks/base, android10 branch; verified against source) converted from
//! nanoseconds to milliseconds.
//!
//! What this adds over a naive "nearest quarter-turn" rule:
//! * **Adaptive low-pass filter** (200 ms time constant), reset after a long gap.
//! * **Overhead detection** — when the screen faces the floor (tilt ≤ −40°, the
//!   "lying on my back holding the phone over my face" case) orientation is
//!   indeterminate, so no rotation is proposed (hysteresis exit at −15°).
//! * **Per-orientation tilt tolerance** — a rotation is only proposed when the
//!   device is close enough to vertical, with stricter limits on unnatural
//!   orientations (upside-down hardest).
//! * **22.5° hysteresis dead-zone** around 45° so orientation never flip-flops.
//! * **Flat / swing / acceleration gating** — no rotation while the phone rests
//!   flat, is being picked up ("swinging"), or is accelerating, plus a settle
//!   delay after each of those ends.
//!
//! Feed it a gravity vector (`TYPE_GRAVITY`, preferred) or a raw accelerometer
//! vector; the acceleration gate is only active for the raw accelerometer (a
//! gravity vector never leaves the plausible magnitude band), which is correct.

use crate::Rotation;

const RAD_TO_DEG: f32 = 180.0 / core::f32::consts::PI;
const STANDARD_GRAVITY: f32 = 9.80665;

/// AOSP constants (ms). See the module docs; names match the Java source.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProposerConfig {
    /// Low-pass filter time constant.
    pub filter_time_constant_ms: f32,
    /// Reset the filter if the gap between samples exceeds this.
    pub max_filter_delta_ms: u64,
    /// Below this |g|, orientation is indeterminate.
    pub near_zero_magnitude: f32,
    /// A reading whose magnitude is more than this from gravity means the device
    /// is accelerating (only meaningful for a raw accelerometer).
    pub acceleration_tolerance: f32,
    /// Maximum tilt (deg from vertical) at which a rotation may be proposed.
    pub max_tilt_deg: i32,
    /// Enter overhead mode at or below this tilt (screen facing the floor).
    pub tilt_overhead_enter_deg: i32,
    /// Leave overhead mode at or above this tilt.
    pub tilt_overhead_exit_deg: i32,
    /// Hysteresis gap between adjacent orientations.
    pub adjacent_gap_deg: i32,
    /// Per-rotation `[min, max]` tilt tolerance (indexed by rotation).
    pub tilt_tolerance_deg: [[i32; 2]; 4],
    /// Predicted rotation must hold this long before it is proposed.
    pub settle_ms: u64,
    /// Delay after the device stops being flat.
    pub min_time_since_flat_ended_ms: u64,
    /// Delay after the device stops swinging.
    pub min_time_since_swing_ended_ms: u64,
    /// Delay after acceleration stops.
    pub min_time_since_acceleration_ended_ms: u64,
    /// Tilt above which the device counts as flat.
    pub flat_angle_deg: f32,
    /// How long tilt must stay above `flat_angle_deg` to count as flat.
    pub flat_time_ms: u64,
    /// A tilt increase of this much within `swing_time_ms` is a swing (pick-up).
    pub swing_away_angle_delta_deg: f32,
    /// Window for swing detection.
    pub swing_time_ms: u64,
}

impl Default for ProposerConfig {
    fn default() -> Self {
        ProposerConfig {
            filter_time_constant_ms: 200.0,
            max_filter_delta_ms: 1000,
            near_zero_magnitude: 1.0,
            acceleration_tolerance: 4.0,
            max_tilt_deg: 80,
            tilt_overhead_enter_deg: -40,
            tilt_overhead_exit_deg: -15,
            adjacent_gap_deg: 45,
            // ROTATION_0 / 90 / 180 / 270 (AOSP mTiltToleranceConfig).
            tilt_tolerance_deg: [[-25, 70], [-25, 65], [-25, 60], [-25, 65]],
            settle_ms: 40,
            min_time_since_flat_ended_ms: 500,
            min_time_since_swing_ended_ms: 300,
            min_time_since_acceleration_ended_ms: 500,
            flat_angle_deg: 80.0,
            flat_time_ms: 1000,
            swing_away_angle_delta_deg: 20.0,
            swing_time_ms: 300,
        }
    }
}

/// What the proposer currently believes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Proposal {
    /// The rotation to show, if one is confidently proposed. `None` means
    /// "orientation indeterminate — keep the current rotation" (overhead, flat,
    /// too tilted, still settling, or gated by recent motion).
    pub rotation: Option<Rotation>,
    /// The nearest acceptable rotation before the settle/gating barriers (the
    /// "a tilt towards this is in progress" signal, used to pre-warm the camera).
    pub predicted: Option<Rotation>,
    /// Tilt angle in degrees (−90 overhead … 0 vertical … +90 flat up).
    pub tilt_deg: f32,
    /// Screen faces the floor: orientation cannot be inferred from gravity.
    pub overhead: bool,
    /// Resting flat (e.g. on a table).
    pub flat: bool,
    /// Being picked up / swung.
    pub swinging: bool,
    /// Undergoing external acceleration (raw accelerometer only).
    pub accelerating: bool,
}

const HISTORY: usize = 80; // ~1.6 s at 50 Hz, covers flat (1 s) and swing (0.3 s)

/// Stateful orientation proposer. Clone-able and `Send`; no I/O, no `unsafe`.
#[derive(Debug, Clone)]
pub struct OrientationProposer {
    cfg: ProposerConfig,
    current: Rotation,
    filtered: Option<[f32; 3]>,
    last_ms: Option<u64>,
    predicted: Option<u8>,
    predicted_since_ms: u64,
    proposed: Option<Rotation>,
    overhead: bool,
    flat_ms: Option<u64>,
    swing_ms: Option<u64>,
    accel_ms: Option<u64>,
    tilt_hist: [(u64, f32); HISTORY],
    tilt_len: usize,
    tilt_head: usize,
}

impl OrientationProposer {
    /// New proposer; `current` is the rotation currently shown.
    pub fn new(cfg: ProposerConfig, current: Rotation) -> Self {
        OrientationProposer {
            cfg,
            current,
            filtered: None,
            last_ms: None,
            predicted: None,
            predicted_since_ms: 0,
            proposed: None,
            overhead: false,
            flat_ms: None,
            swing_ms: None,
            accel_ms: None,
            tilt_hist: [(0, 0.0); HISTORY],
            tilt_len: 0,
            tilt_head: 0,
        }
    }

    /// Replaces the configuration (keeps state).
    pub fn set_config(&mut self, cfg: ProposerConfig) {
        self.cfg = cfg;
    }

    /// The rotation currently shown (for hysteresis). Call when it changes.
    pub fn set_current(&mut self, current: Rotation) {
        self.current = current;
    }

    /// Forgets all filter / history state (e.g. after the screen turned off).
    pub fn reset(&mut self) {
        self.filtered = None;
        self.last_ms = None;
        self.predicted = None;
        self.proposed = None;
        self.overhead = false;
        self.flat_ms = None;
        self.swing_ms = None;
        self.accel_ms = None;
        self.tilt_len = 0;
        self.tilt_head = 0;
    }

    fn push_tilt(&mut self, now: u64, tilt: f32) {
        self.tilt_hist[self.tilt_head] = (now, tilt);
        self.tilt_head = (self.tilt_head + 1) % HISTORY;
        self.tilt_len = (self.tilt_len + 1).min(HISTORY);
    }

    /// Iterates tilt history newest-first.
    fn hist_iter(&self) -> impl Iterator<Item = (u64, f32)> + '_ {
        (0..self.tilt_len).map(move |k| self.tilt_hist[(self.tilt_head + HISTORY - 1 - k) % HISTORY])
    }

    fn is_flat(&self, now: u64) -> bool {
        // Tilt has stayed above flat_angle for the whole flat_time window.
        for (ts, tilt) in self.hist_iter() {
            if tilt < self.cfg.flat_angle_deg {
                return false;
            }
            if ts + self.cfg.flat_time_ms <= now {
                return true;
            }
        }
        false
    }

    fn is_swinging(&self, now: u64, tilt: f32) -> bool {
        for (ts, past) in self.hist_iter() {
            if ts + self.cfg.swing_time_ms < now {
                break;
            }
            if past + self.cfg.swing_away_angle_delta_deg <= tilt {
                return true;
            }
        }
        false
    }

    fn tilt_acceptable(&self, rotation: u8, tilt: i32) -> bool {
        let [lo, hi] = self.cfg.tilt_tolerance_deg[rotation as usize];
        tilt >= lo && tilt <= hi
    }

    /// AOSP `isOrientationAngleAcceptableLocked`: a hysteresis dead-zone of
    /// `adjacent_gap/2` on the side adjacent to the current rotation.
    fn orientation_acceptable(&self, rotation: u8, angle: i32) -> bool {
        let cur = self.current.index() as i32;
        let r = rotation as i32;
        let gap = self.cfg.adjacent_gap_deg;
        // Lower bound (counter-clockwise adjacent or same).
        if r == cur || r == (cur + 1) % 4 {
            let lower = r * 90 - 45 + gap / 2;
            if r == 0 {
                if angle >= 315 && angle < lower + 360 {
                    return false;
                }
            } else if angle < lower {
                return false;
            }
        }
        // Upper bound (clockwise adjacent or same).
        if r == cur || r == (cur + 3) % 4 {
            let upper = r * 90 + 45 - gap / 2;
            if r == 0 {
                if angle <= 45 && angle > upper {
                    return false;
                }
            } else if angle > upper {
                return false;
            }
        }
        true
    }

    fn barrier_passed(&self, now: u64) -> bool {
        let since = |t: Option<u64>, d: u64| t.is_none_or(|t| now >= t + d);
        now >= self.predicted_since_ms + self.cfg.settle_ms
            && since(self.flat_ms, self.cfg.min_time_since_flat_ended_ms)
            && since(self.swing_ms, self.cfg.min_time_since_swing_ended_ms)
            && since(self.accel_ms, self.cfg.min_time_since_acceleration_ended_ms)
    }

    /// Feeds one gravity/accelerometer sample and returns the current proposal.
    pub fn update(&mut self, now: u64, v: [f32; 3]) -> Proposal {
        // 1. Adaptive low-pass filter (reset on a long gap).
        let reset = self.last_ms.is_none_or(|t| now.saturating_sub(t) > self.cfg.max_filter_delta_ms);
        let dt = self.last_ms.map_or(0, |t| now.saturating_sub(t)) as f32;
        self.last_ms = Some(now);
        let f = match (self.filtered, reset) {
            (Some(prev), false) => {
                let a = dt / (self.cfg.filter_time_constant_ms + dt);
                [prev[0] + a * (v[0] - prev[0]), prev[1] + a * (v[1] - prev[1]), prev[2] + a * (v[2] - prev[2])]
            }
            _ => v,
        };
        self.filtered = Some(f);
        if reset {
            self.tilt_len = 0;
            self.tilt_head = 0;
            self.predicted = None;
        }

        let [x, y, z] = f;
        let magnitude = (x * x + y * y + z * z).sqrt();
        if !magnitude.is_finite() || magnitude < self.cfg.near_zero_magnitude {
            self.predicted = None;
            return self.emit(now, f32::NAN, false, false);
        }
        // External acceleration (raw accelerometer only; gravity vectors never trip this).
        let min_mag = STANDARD_GRAVITY - self.cfg.acceleration_tolerance;
        let max_mag = STANDARD_GRAVITY + self.cfg.acceleration_tolerance;
        if magnitude < min_mag || magnitude > max_mag {
            self.accel_ms = Some(now);
        }

        // 2. Tilt angle and history-based flat / swing detection.
        let tilt = (z / magnitude).clamp(-1.0, 1.0).asin() * RAD_TO_DEG;
        let tilt_i = tilt.round() as i32;
        self.push_tilt(now, tilt);
        let flat = self.is_flat(now);
        if flat {
            self.flat_ms = Some(now);
        }
        let swinging = self.is_swinging(now, tilt);
        if swinging {
            self.swing_ms = Some(now);
        }

        // 3. Overhead hysteresis.
        if tilt_i <= self.cfg.tilt_overhead_enter_deg {
            self.overhead = true;
        } else if tilt_i >= self.cfg.tilt_overhead_exit_deg {
            self.overhead = false;
        }

        if self.overhead || tilt_i.abs() > self.cfg.max_tilt_deg {
            self.predicted = None;
            return self.emit(now, tilt, flat, swinging);
        }

        // 4. Orientation angle → nearest acceptable rotation.
        let mut angle = (-(-x).atan2(y) * RAD_TO_DEG).round() as i32;
        if angle < 0 {
            angle += 360;
        }
        let nearest = (((angle + 45) / 90) % 4) as u8;
        if self.tilt_acceptable(nearest, tilt_i) && self.orientation_acceptable(nearest, angle) {
            if self.predicted != Some(nearest) {
                self.predicted = Some(nearest);
                self.predicted_since_ms = now;
            }
        } else {
            self.predicted = None;
        }
        self.emit(now, tilt, flat, swinging)
    }

    fn emit(&mut self, now: u64, tilt: f32, flat: bool, swinging: bool) -> Proposal {
        // A predicted rotation becomes the proposal only after every barrier.
        let proposed = match self.predicted {
            Some(r) if self.barrier_passed(now) => Some(Rotation::from_index(r as u32)),
            _ => None,
        };
        if proposed.is_some() {
            self.proposed = proposed;
        }
        Proposal {
            rotation: proposed,
            predicted: self.predicted.map(|r| Rotation::from_index(r as u32)),
            tilt_deg: tilt,
            overhead: self.overhead,
            flat,
            swinging,
            accelerating: self.accel_ms == Some(now),
        }
    }
}

#[cfg(test)]
mod tests;
