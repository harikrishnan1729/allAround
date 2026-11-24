//! Gravity → display rotation, with hysteresis and flat / free-fall rejection.

use crate::Rotation;

/// Tunables for [`read_gravity`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GravityConfig {
    /// Extra degrees beyond 45° the device must rotate before the proposed
    /// rotation leaves the current band. Prevents flip-flopping near 45°.
    pub hysteresis_deg: f32,
    /// Tilt (angle between the screen plane and the horizontal) above which the
    /// device counts as flat and no orientation is proposed.
    pub flat_tilt_deg: f32,
    /// Accepted range of |g| in m/s². Outside it the reading is dominated by
    /// linear acceleration (shaking, free fall) and is ignored.
    pub min_magnitude: f32,
    /// See `min_magnitude`.
    pub max_magnitude: f32,
}

impl Default for GravityConfig {
    fn default() -> Self {
        GravityConfig { hysteresis_deg: 10.0, flat_tilt_deg: 70.0, min_magnitude: 5.8, max_magnitude: 13.8 }
    }
}

/// Interpretation of one (filtered) gravity vector.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GravityReading {
    /// Device lies (nearly) flat: no orientation can be inferred from gravity.
    Flat {
        /// Tilt angle in degrees (90 = perfectly flat).
        tilt_deg: f32,
    },
    /// Magnitude implausible for gravity (shaking, free fall, sensor glitch).
    Unreliable,
    /// A usable orientation.
    Oriented {
        /// Proposed rotation after hysteresis relative to the current rotation.
        rotation: Rotation,
        /// In-plane orientation angle, degrees in `[0, 360)`.
        angle_deg: f32,
        /// Tilt angle in degrees.
        tilt_deg: f32,
    },
}

impl GravityReading {
    /// The proposed rotation, if any.
    pub fn rotation(&self) -> Option<Rotation> {
        match self {
            GravityReading::Oriented { rotation, .. } => Some(*rotation),
            _ => None,
        }
    }
}

/// In-plane orientation angle of an "up" vector, in degrees `[0, 360)`.
///
/// 0° = device upright, 90° = device `+x` points up (→ [`Rotation::R90`]).
/// This is the same quantity `WindowOrientationListener` computes in AOSP
/// (`-atan2(-x, y)`), which equals `atan2(x, y)`.
pub fn orientation_angle_deg(x: f32, y: f32) -> f32 {
    let a = x.atan2(y).to_degrees();
    if a < 0.0 {
        a + 360.0
    } else {
        a
    }
}

/// Circular distance between two angles in degrees, in `[0, 180]`.
pub(crate) fn angular_distance(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(360.0);
    if d > 180.0 {
        360.0 - d
    } else {
        d
    }
}

/// Interprets a gravity vector `g` given the rotation currently on screen.
pub fn read_gravity(g: [f32; 3], current: Rotation, cfg: &GravityConfig) -> GravityReading {
    let [x, y, z] = g;
    let mag = (x * x + y * y + z * z).sqrt();
    if !mag.is_finite() || mag < cfg.min_magnitude || mag > cfg.max_magnitude {
        return GravityReading::Unreliable;
    }
    let tilt_deg = (z / mag).clamp(-1.0, 1.0).asin().to_degrees().abs();
    if tilt_deg > cfg.flat_tilt_deg {
        return GravityReading::Flat { tilt_deg };
    }
    let angle_deg = orientation_angle_deg(x, y);
    // Nearest quarter turn.
    let nearest = Rotation::from_index(((angle_deg + 45.0) / 90.0) as u32);
    let rotation =
        if nearest != current && angular_distance(angle_deg, current.center_deg()) < 45.0 + cfg.hysteresis_deg {
            current
        } else {
            nearest
        };
    GravityReading::Oriented { rotation, angle_deg, tilt_deg }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn up(angle_deg: f32, tilt_deg: f32) -> [f32; 3] {
        let (a, t) = (angle_deg.to_radians(), tilt_deg.to_radians());
        let inplane = 9.81 * t.cos();
        [inplane * a.sin(), inplane * a.cos(), 9.81 * t.sin()]
    }

    #[test]
    fn upright_and_quarter_turns() {
        let cfg = GravityConfig::default();
        for (deg, expect) in
            [(0.0, Rotation::R0), (90.0, Rotation::R90), (180.0, Rotation::R180), (270.0, Rotation::R270)]
        {
            let r = read_gravity(up(deg, 10.0), Rotation::R0, &cfg);
            // From R0 every pure quarter-turn is beyond the hysteresis band.
            assert_eq!(r.rotation(), Some(expect), "angle {deg}");
        }
    }

    #[test]
    fn counter_clockwise_turn_reads_r90() {
        // Phone turned CCW: its right edge (+x) points up, accelerometer reads +x.
        let r = read_gravity([9.81, 0.0, 0.0], Rotation::R0, &GravityConfig::default());
        assert_eq!(r.rotation(), Some(Rotation::R90));
    }

    #[test]
    fn hysteresis_holds_near_45() {
        let cfg = GravityConfig::default();
        // 50° is nearer to R90 but inside the 45+10 band of R0: stay.
        assert_eq!(read_gravity(up(50.0, 0.0), Rotation::R0, &cfg).rotation(), Some(Rotation::R0));
        // ...and coming from R90 the same pose stays R90.
        assert_eq!(read_gravity(up(50.0, 0.0), Rotation::R90, &cfg).rotation(), Some(Rotation::R90));
        assert_eq!(read_gravity(up(40.0, 0.0), Rotation::R90, &cfg).rotation(), Some(Rotation::R90));
        // Past the band it switches.
        assert_eq!(read_gravity(up(56.0, 0.0), Rotation::R0, &cfg).rotation(), Some(Rotation::R90));
        assert_eq!(read_gravity(up(34.0, 0.0), Rotation::R90, &cfg).rotation(), Some(Rotation::R0));
    }

    #[test]
    fn flat_and_unreliable() {
        let cfg = GravityConfig::default();
        assert!(matches!(read_gravity([0.0, 0.3, 9.8], Rotation::R0, &cfg), GravityReading::Flat { .. }));
        assert!(matches!(read_gravity(up(0.0, 75.0), Rotation::R0, &cfg), GravityReading::Flat { .. }));
        assert!(matches!(read_gravity(up(90.0, 60.0), Rotation::R0, &cfg), GravityReading::Oriented { .. }));
        assert_eq!(read_gravity([0.0, 1.0, 0.0], Rotation::R0, &cfg), GravityReading::Unreliable);
        assert_eq!(read_gravity([0.0, 25.0, 0.0], Rotation::R0, &cfg), GravityReading::Unreliable);
        assert_eq!(read_gravity([f32::NAN, 9.8, 0.0], Rotation::R0, &cfg), GravityReading::Unreliable);
    }

    #[test]
    fn angle_is_in_range() {
        for i in 0..3600 {
            let a = i as f32 / 10.0;
            let g = up(a, 0.0);
            let got = orientation_angle_deg(g[0], g[1]);
            assert!((0.0..360.0).contains(&got));
            assert!(angular_distance(got, a) < 0.05, "{a} -> {got}");
        }
    }
}
