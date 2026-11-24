use facetilt_decision::{DecisionConfig, GravityConfig, ProposerConfig};

/// All tunables of the trigger / decision pipeline. Defaults are the values
/// documented in the README; Kotlin can override a subset via `setConfig`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EngineConfig {
    /// Decision rule (confidence threshold, cooldown, upside-down).
    pub decision: DecisionConfig,
    /// Gravity interpretation (hysteresis, flat threshold). Legacy; the engine
    /// now drives rotation from `proposer` (AOSP WindowOrientationListener port).
    pub gravity: GravityConfig,
    /// AOSP-grade orientation proposer (overhead/flat/swing/accel gating, tilt
    /// tolerance, hysteresis). The filter time constant is lowered from AOSP's
    /// 200 ms because FaceTilt pre-warms the camera and confirms with it, so a
    /// faster (slightly noisier) proposal still meets the <250 ms latency budget.
    pub proposer: ProposerConfig,
    /// Low-pass time constant applied to the gravity vector.
    pub lpf_tau_ms: f32,
    /// Time constant of the angular-speed smoother.
    pub omega_tau_ms: f32,
    /// Angle between the filtered gravity vector and the last stable one that
    /// counts as a tilt onset ("~15°").
    pub onset_deg: f32,
    /// Minimum *in-plane* (screen-plane) angle change at onset. Filters out
    /// pitch-only motions (raising the phone) that can never change orientation.
    pub onset_inplane_deg: f32,
    /// Angular speed above which the device is "still moving".
    pub moving_dps: f32,
    /// Angular speed below which the device is considered at rest.
    pub stable_dps: f32,
    /// How long the device must stay below `stable_dps` to count as settled.
    pub stable_ms: u64,
    /// Settle as soon as the in-plane angle is within this many degrees of a
    /// *new* rotation's centre ("reaches the new orientation band").
    pub band_deg: f32,
    /// Abort when the tilt falls back this many degrees below its peak before
    /// reaching a new orientation.
    pub reverse_deg: f32,
    /// Abort if nothing settles within this time after onset.
    pub settle_timeout_ms: u64,
    /// After settling, give up waiting for a classified frame after this long
    /// and fall back to gravity.
    pub capture_timeout_ms: u64,
    /// Minimum time between two camera opens.
    pub min_open_interval_ms: u64,
    /// Upper bound of the exponential backoff.
    pub max_backoff_ms: u64,
    /// Backoff resets after this long without any camera use.
    pub backoff_reset_ms: u64,
    /// A decision reverting the previous change within this window counts as flapping.
    pub flap_window_ms: u64,
    /// Skip the camera while the proximity sensor is covered.
    pub use_proximity: bool,
    /// Hinge angles (degrees) strictly inside this range mean "half folded":
    /// camera geometry is ambiguous, so the camera is skipped.
    pub half_fold_range: (f32, f32),
    /// "2nd verification": after a portrait → landscape change, look again once
    /// the phone rests in landscape and switch back if the user is lying down.
    pub second_verification: bool,
    /// Assist mode: keep the system auto-rotate running normally and only take
    /// control to hold portrait when the user is lying down; release it again
    /// when they are back to normal. When false, FaceTilt controls rotation the
    /// whole time it is on (the original behaviour).
    pub assist_mode: bool,
    /// Earliest second look after the change (≥ the decision cooldown, and long
    /// enough for the rotation animation to finish).
    pub verify_delay_ms: u64,
    /// The second look is dropped if the phone has not rested in landscape by then.
    pub verify_window_ms: u64,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            decision: DecisionConfig::default(),
            gravity: GravityConfig::default(),
            proposer: ProposerConfig { filter_time_constant_ms: 90.0, ..ProposerConfig::default() },
            lpf_tau_ms: 40.0,
            omega_tau_ms: 60.0,
            onset_deg: 15.0,
            onset_inplane_deg: 10.0,
            moving_dps: 25.0,
            stable_dps: 12.0,
            stable_ms: 120,
            band_deg: 30.0,
            reverse_deg: 12.0,
            settle_timeout_ms: 1500,
            capture_timeout_ms: 700,
            min_open_interval_ms: 800,
            max_backoff_ms: 8000,
            backoff_reset_ms: 15_000,
            flap_window_ms: 3000,
            use_proximity: true,
            half_fold_range: (30.0, 150.0),
            second_verification: false,
            assist_mode: false,
            verify_delay_ms: 700,
            verify_window_ms: 5000,
        }
    }
}
