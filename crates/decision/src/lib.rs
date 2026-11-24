//! # facetilt-decision
//!
//! Pure, allocation-free orientation decision logic. No I/O, no clocks, no
//! `unsafe`: every input (including time) is passed in, so all behaviour is
//! reproducible in unit tests.
//!
//! ## Coordinate conventions (shared by every FaceTilt crate)
//!
//! * Device frame = Android sensor frame in the device's *natural* orientation:
//!   `+x` towards the right edge of the screen, `+y` towards the top edge,
//!   `+z` out of the screen towards the user.
//! * [`Rotation`] uses the numeric values of `android.view.Surface.ROTATION_*`.
//!   `R90` means "content up points towards device `+x`"; this is what Android
//!   selects when the phone is turned counter-clockwise (left edge down).
//! * Accelerometer / gravity readings at rest point *up* (a phone standing
//!   upright reads roughly `(0, +9.81, 0)`), so the "up" direction is the
//!   reading itself.
//! * Head orientation is the direction of the user's head-top in the *mirror
//!   view* of the front camera, expressed in the device frame (see the vision
//!   crate for the sensor-orientation / mirroring transform).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod gravity;
mod proposer;

pub use gravity::{orientation_angle_deg, read_gravity, GravityConfig, GravityReading};
pub use proposer::{OrientationProposer, Proposal, ProposerConfig};

/// Display rotation, numerically identical to `android.view.Surface.ROTATION_*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Rotation {
    /// Natural orientation (portrait on phones).
    R0 = 0,
    /// Content up = device `+x` (phone turned counter-clockwise).
    R90 = 1,
    /// Upside down.
    R180 = 2,
    /// Content up = device `-x` (phone turned clockwise).
    R270 = 3,
}

impl Rotation {
    /// All rotations in index order.
    pub const ALL: [Rotation; 4] = [Rotation::R0, Rotation::R90, Rotation::R180, Rotation::R270];

    /// Builds a rotation from a `Surface.ROTATION_*` value (taken modulo 4).
    pub const fn from_index(i: u32) -> Rotation {
        match i % 4 {
            0 => Rotation::R0,
            1 => Rotation::R90,
            2 => Rotation::R180,
            _ => Rotation::R270,
        }
    }

    /// Strict variant of [`Rotation::from_index`]; `None` for values outside 0..=3.
    pub const fn try_from_index(i: i32) -> Option<Rotation> {
        if i < 0 || i > 3 {
            None
        } else {
            Some(Rotation::from_index(i as u32))
        }
    }

    /// `Surface.ROTATION_*` value.
    pub const fn index(self) -> u8 {
        self as u8
    }

    /// Centre of this rotation's band on the orientation-angle circle, in degrees
    /// (see [`orientation_angle_deg`]).
    pub const fn center_deg(self) -> f32 {
        (self as u8 as f32) * 90.0
    }

    /// Number of quarter turns between two rotations (0..=2, symmetric).
    pub const fn quarter_turns_between(self, other: Rotation) -> u8 {
        let d = (self as u8 + 4 - other as u8) % 4;
        if d == 3 {
            1
        } else {
            d
        }
    }
}

/// Output classes of the on-device head-orientation classifier.
///
/// The classes describe how the face appears in the *mirror view* image
/// expressed in the device's natural orientation. The numeric values are the
/// classifier's output indices and must match `training/facetilt/labels.py`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum HeadClass {
    /// Head top points to the device's top edge.
    Upright = 0,
    /// Face rotated 90° counter-clockwise: head top points to the device's left edge.
    RotatedLeft = 1,
    /// Face rotated 90° clockwise: head top points to the device's right edge.
    RotatedRight = 2,
    /// Head top points to the device's bottom edge.
    UpsideDown = 3,
    /// No face, or the classifier is unsure.
    NoFace = 4,
}

impl HeadClass {
    /// Number of classifier outputs.
    pub const COUNT: usize = 5;

    /// Maps a classifier output index to a class.
    pub const fn from_index(i: usize) -> Option<HeadClass> {
        match i {
            0 => Some(HeadClass::Upright),
            1 => Some(HeadClass::RotatedLeft),
            2 => Some(HeadClass::RotatedRight),
            3 => Some(HeadClass::UpsideDown),
            4 => Some(HeadClass::NoFace),
            _ => None,
        }
    }

    /// The display rotation that would make content upright *for this head*,
    /// or `None` for [`HeadClass::NoFace`].
    pub const fn content_rotation(self) -> Option<Rotation> {
        match self {
            HeadClass::Upright => Some(Rotation::R0),
            HeadClass::RotatedRight => Some(Rotation::R90),
            HeadClass::UpsideDown => Some(Rotation::R180),
            HeadClass::RotatedLeft => Some(Rotation::R270),
            HeadClass::NoFace => None,
        }
    }
}

/// One classifier result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeadObservation {
    /// Arg-max class.
    pub class: HeadClass,
    /// Softmax probability of `class`, in `0.0..=1.0`.
    pub confidence: f32,
}

/// Why a decision was taken. Values cross JNI as plain ints; keep them stable
/// (mirrored in `app/.../engine/DecisionReason.kt`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Reason {
    /// Gravity proposes the orientation that is already shown.
    GravityUnchanged = 0,
    /// The face agrees with the new gravity orientation: rotate.
    FaceConfirmedRotation = 1,
    /// The face agrees with the *current* orientation (e.g. lying on one side): keep.
    FaceKeptOrientation = 2,
    /// Classifier said "no face": behave like standard auto-rotate.
    FallbackNoFace = 3,
    /// Classifier confidence below threshold: behave like standard auto-rotate.
    FallbackLowConfidence = 4,
    /// Camera unavailable (denied, in use, privacy toggle, proximity covered, rate-limited, timeout).
    FallbackCameraUnavailable = 5,
    /// The face matches neither the old nor the new orientation: standard auto-rotate.
    FallbackAmbiguous = 6,
    /// A change happened too recently; keep the current orientation.
    Cooldown = 7,
    /// The proposed rotation is upside-down and upside-down is disabled.
    UpsideDownDisallowed = 8,
    /// 2nd verification: the face still matches the previous (portrait)
    /// orientation, e.g. the user is lying on their side → switch back.
    SecondCheckReverted = 9,
    /// 2nd verification: the face matches the landscape orientation → keep it.
    SecondCheckConfirmed = 10,
    /// 2nd verification: no usable/confident answer → keep the current orientation.
    SecondCheckInconclusive = 11,
}

impl Rotation {
    /// R90 or R270.
    pub const fn is_landscape(self) -> bool {
        matches!(self, Rotation::R90 | Rotation::R270)
    }
}

/// Result of [`decide`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    /// Orientation to show.
    pub rotation: Rotation,
    /// Why.
    pub reason: Reason,
}

impl Decision {
    /// True if `rotation` differs from `current`.
    pub fn changes(&self, current: Rotation) -> bool {
        self.rotation != current
    }
}

/// Stateless core rule.
///
/// * `current`: orientation currently on screen.
/// * `gravity`: orientation proposed by gravity (already hysteresis-filtered).
/// * `head`: classifier result, or `None` if no usable frame was obtained.
/// * `threshold`: minimum confidence to trust the classifier (default 0.7).
///
/// Rules, in order:
/// 1. gravity == current → no change.
/// 2. no usable frame → gravity (standard auto-rotate).
/// 3. class == NoFace → gravity.
/// 4. confidence < threshold → gravity.
/// 5. head matches the *new* gravity orientation → rotate.
/// 6. head matches the *current* orientation → keep (lying-on-your-side case).
/// 7. head matches neither → gravity (we never invent a third orientation).
pub fn decide(current: Rotation, gravity: Rotation, head: Option<HeadObservation>, threshold: f32) -> Decision {
    if gravity == current {
        return Decision { rotation: current, reason: Reason::GravityUnchanged };
    }
    let Some(obs) = head else {
        return Decision { rotation: gravity, reason: Reason::FallbackCameraUnavailable };
    };
    let Some(head_rot) = obs.class.content_rotation() else {
        return Decision { rotation: gravity, reason: Reason::FallbackNoFace };
    };
    // NaN confidence must never be trusted (`partial_cmp` is `None` for NaN).
    if !matches!(
        obs.confidence.partial_cmp(&threshold),
        Some(core::cmp::Ordering::Greater | core::cmp::Ordering::Equal)
    ) {
        return Decision { rotation: gravity, reason: Reason::FallbackLowConfidence };
    }
    if head_rot == gravity {
        Decision { rotation: gravity, reason: Reason::FaceConfirmedRotation }
    } else if head_rot == current {
        Decision { rotation: current, reason: Reason::FaceKeptOrientation }
    } else {
        Decision { rotation: gravity, reason: Reason::FallbackAmbiguous }
    }
}

/// "2nd verification" rule, applied once the device rests in the orientation
/// chosen by a previous decision.
///
/// * `current`: orientation shown now (the one being verified, e.g. landscape).
/// * `previous`: orientation shown before the change (e.g. portrait).
///
/// Only a confident face that is aligned with `previous` reverts the change —
/// the typical case is a user lying on their side whose first frame was
/// blurred, badly exposed or missing (gravity fallback). Anything else keeps
/// `current`, so the second look can only undo a change, never invent one.
pub fn verify(current: Rotation, previous: Rotation, head: Option<HeadObservation>, threshold: f32) -> Decision {
    let keep = |reason| Decision { rotation: current, reason };
    let Some(obs) = head else { return keep(Reason::SecondCheckInconclusive) };
    let Some(head_rot) = obs.class.content_rotation() else { return keep(Reason::SecondCheckInconclusive) };
    if !matches!(
        obs.confidence.partial_cmp(&threshold),
        Some(core::cmp::Ordering::Greater | core::cmp::Ordering::Equal)
    ) {
        return keep(Reason::SecondCheckInconclusive);
    }
    if head_rot == previous && previous != current {
        Decision { rotation: previous, reason: Reason::SecondCheckReverted }
    } else {
        keep(Reason::SecondCheckConfirmed)
    }
}

/// Tunables for [`DecisionEngine`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecisionConfig {
    /// Minimum classifier confidence (default 0.7).
    pub confidence_threshold: f32,
    /// After an orientation change, further changes are suppressed for this long.
    pub cooldown_ms: u64,
    /// Whether R180 may be selected (Android phones disable it by default).
    pub allow_upside_down: bool,
}

impl Default for DecisionConfig {
    fn default() -> Self {
        DecisionConfig { confidence_threshold: 0.7, cooldown_ms: 600, allow_upside_down: false }
    }
}

/// Stateful wrapper around [`decide`] that adds a cooldown and remembers the
/// orientation currently on screen.
#[derive(Debug, Clone)]
pub struct DecisionEngine {
    cfg: DecisionConfig,
    current: Rotation,
    last_change_ms: Option<u64>,
}

impl DecisionEngine {
    /// New engine; `current` is the orientation on screen right now.
    pub fn new(cfg: DecisionConfig, current: Rotation) -> Self {
        DecisionEngine { cfg, current, last_change_ms: None }
    }

    /// Orientation currently on screen (as far as the engine knows).
    pub fn current(&self) -> Rotation {
        self.current
    }

    /// Active configuration.
    pub fn config(&self) -> &DecisionConfig {
        &self.cfg
    }

    /// Replaces the configuration.
    pub fn set_config(&mut self, cfg: DecisionConfig) {
        self.cfg = cfg;
    }

    /// Records an orientation change that happened outside the engine
    /// (user lock, an app forcing an orientation, …). Does not start a cooldown.
    pub fn set_current(&mut self, r: Rotation) {
        self.current = r;
    }

    /// Commits a change decided outside [`DecisionEngine::decide`] (the 2nd
    /// verification) and starts the cooldown.
    pub fn commit(&mut self, now_ms: u64, r: Rotation) {
        if r != self.current {
            self.current = r;
            self.last_change_ms = Some(now_ms);
        }
    }

    /// True while a recent change still suppresses new ones.
    pub fn in_cooldown(&self, now_ms: u64) -> bool {
        matches!(self.last_change_ms, Some(t) if now_ms.saturating_sub(t) < self.cfg.cooldown_ms)
    }

    /// Decides and, if the orientation changes, commits it and starts the cooldown.
    pub fn decide(&mut self, now_ms: u64, gravity: Rotation, head: Option<HeadObservation>) -> Decision {
        let current = self.current;
        if gravity == current {
            return Decision { rotation: current, reason: Reason::GravityUnchanged };
        }
        if self.in_cooldown(now_ms) {
            return Decision { rotation: current, reason: Reason::Cooldown };
        }
        let mut d = decide(current, gravity, head, self.cfg.confidence_threshold);
        if d.rotation == Rotation::R180 && !self.cfg.allow_upside_down && current != Rotation::R180 {
            d = Decision { rotation: current, reason: Reason::UpsideDownDisallowed };
        }
        if d.rotation != current {
            self.current = d.rotation;
            self.last_change_ms = Some(now_ms);
        }
        d
    }
}

#[cfg(test)]
mod tests;
