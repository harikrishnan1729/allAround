//! `float[]` configuration passed from Kotlin (`EngineConfig.toArray()` in
//! `app/.../engine/NativeBridge.kt`). Fixed layout, versioned, validated and
//! clamped here so a bad value from Java can never put the engine in an
//! unsafe state (e.g. a zero rate limit).

use facetilt_core::EngineConfig;

/// Layout version understood by this library.
pub const VERSION: f32 = 3.0;
/// Number of floats in a v3 config.
pub const LEN: usize = 15;

/// Indices into the config array. Keep in sync with `NativeBridge.kt`.
#[allow(missing_docs)]
pub mod idx {
    pub const VERSION: usize = 0;
    pub const CONFIDENCE_THRESHOLD: usize = 1;
    pub const ONSET_DEG: usize = 2;
    pub const STABLE_MS: usize = 3;
    pub const SETTLE_TIMEOUT_MS: usize = 4;
    pub const MIN_OPEN_INTERVAL_MS: usize = 5;
    pub const COOLDOWN_MS: usize = 6;
    pub const HYSTERESIS_DEG: usize = 7;
    pub const FLAT_TILT_DEG: usize = 8;
    pub const ALLOW_UPSIDE_DOWN: usize = 9;
    pub const USE_PROXIMITY: usize = 10;
    pub const SENSOR_ORIENTATION_OVERRIDE: usize = 11;
    pub const DEBUG_TRACE: usize = 12;
    pub const SECOND_VERIFICATION: usize = 13;
    pub const ASSIST_MODE: usize = 14;
}

/// Engine configuration plus platform-layer options.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BridgeConfig {
    /// Engine tunables.
    pub engine: EngineConfig,
    /// Camera sensor orientation reported by the Java `CameraCharacteristics`
    /// API (which applies foldable device-state overrides on API 32+), or
    /// `None` to use the NDK value.
    pub sensor_orientation_override: Option<i32>,
    /// Log gravity samples (debug builds; never camera data).
    pub debug_trace: bool,
}

/// Invalid configuration array.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// Wrong length.
    Length(usize),
    /// Unknown layout version.
    Version,
}

fn clampf(v: f32, lo: f32, hi: f32, default: f32) -> f32 {
    if v.is_finite() {
        v.clamp(lo, hi)
    } else {
        default
    }
}

fn clampms(v: f32, lo: u64, hi: u64, default: u64) -> u64 {
    if v.is_finite() && v >= 0.0 {
        (v as u64).clamp(lo, hi)
    } else {
        default
    }
}

/// Parses and validates a config array.
pub fn parse(a: &[f32]) -> Result<BridgeConfig, ConfigError> {
    if a.len() != LEN {
        return Err(ConfigError::Length(a.len()));
    }
    if a[idx::VERSION] != VERSION {
        return Err(ConfigError::Version);
    }
    let d = EngineConfig::default();
    let mut e = d;
    e.decision.confidence_threshold = clampf(a[idx::CONFIDENCE_THRESHOLD], 0.5, 0.99, d.decision.confidence_threshold);
    e.onset_deg = clampf(a[idx::ONSET_DEG], 8.0, 40.0, d.onset_deg);
    e.stable_ms = clampms(a[idx::STABLE_MS], 40, 500, d.stable_ms);
    e.settle_timeout_ms = clampms(a[idx::SETTLE_TIMEOUT_MS], 500, 3000, d.settle_timeout_ms);
    // Never below 500 ms between opens, whatever Java says.
    e.min_open_interval_ms = clampms(a[idx::MIN_OPEN_INTERVAL_MS], 500, 10_000, d.min_open_interval_ms);
    e.decision.cooldown_ms = clampms(a[idx::COOLDOWN_MS], 0, 5_000, d.decision.cooldown_ms);
    e.gravity.hysteresis_deg = clampf(a[idx::HYSTERESIS_DEG], 0.0, 25.0, d.gravity.hysteresis_deg);
    e.gravity.flat_tilt_deg = clampf(a[idx::FLAT_TILT_DEG], 45.0, 85.0, d.gravity.flat_tilt_deg);
    e.decision.allow_upside_down = a[idx::ALLOW_UPSIDE_DOWN] >= 0.5;
    e.use_proximity = a[idx::USE_PROXIMITY] >= 0.5;
    e.second_verification = a[idx::SECOND_VERIFICATION] >= 0.5;
    e.assist_mode = a[idx::ASSIST_MODE] >= 0.5;
    let o = a[idx::SENSOR_ORIENTATION_OVERRIDE];
    let sensor_orientation_override =
        (o.is_finite() && o >= 0.0).then(|| ((o.round() as i32).rem_euclid(360) + 45) / 90 * 90 % 360);
    Ok(BridgeConfig { engine: e, sensor_orientation_override, debug_trace: a[idx::DEBUG_TRACE] >= 0.5 })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> [f32; LEN] {
        [3.0, 0.7, 15.0, 120.0, 1500.0, 800.0, 600.0, 10.0, 70.0, 0.0, 1.0, -1.0, 0.0, 0.0, 0.0]
    }

    #[test]
    fn default_array_matches_engine_defaults() {
        let c = parse(&defaults()).unwrap_or_default();
        assert_eq!(c.engine, EngineConfig::default());
        assert_eq!(c.sensor_orientation_override, None);
        assert!(!c.debug_trace);
    }

    #[test]
    fn rejects_bad_shape_and_version() {
        assert_eq!(parse(&[1.0; 3]), Err(ConfigError::Length(3)));
        let mut a = defaults();
        a[0] = 1.0;
        assert_eq!(parse(&a), Err(ConfigError::Version));
    }

    #[test]
    fn optional_flags() {
        let mut a = defaults();
        assert!(!parse(&a).unwrap_or_default().engine.second_verification);
        assert!(!parse(&a).unwrap_or_default().engine.assist_mode);
        a[idx::SECOND_VERIFICATION] = 1.0;
        a[idx::ASSIST_MODE] = 1.0;
        let c = parse(&a).unwrap_or_default().engine;
        assert!(c.second_verification && c.assist_mode);
    }

    #[test]
    fn clamps_dangerous_values() {
        let mut a = defaults();
        a[idx::MIN_OPEN_INTERVAL_MS] = 0.0;
        a[idx::CONFIDENCE_THRESHOLD] = f32::NAN;
        a[idx::SETTLE_TIMEOUT_MS] = 1e9;
        a[idx::SENSOR_ORIENTATION_OVERRIDE] = 268.0;
        let c = parse(&a).unwrap_or_default();
        assert_eq!(c.engine.min_open_interval_ms, 500);
        assert_eq!(c.engine.decision.confidence_threshold, 0.7);
        assert_eq!(c.engine.settle_timeout_ms, 3000);
        assert_eq!(c.sensor_orientation_override, Some(270));
    }
}
