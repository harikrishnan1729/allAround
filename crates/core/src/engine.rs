//! The tilt-trigger / capture / decide state machine.
//!
//! ```text
//!            onset (>15°, moving, in-plane >10°)          settle (new band | 120 ms stable)
//!   Idle ───────────────────────────────────► Tilting ─────────────────────────► AwaitingFrame
//!    ▲   [OpenCamera if allowed]                │  │      [CaptureFrame / wait for open]   │
//!    │                                          │  └─ reverse / 1.5 s timeout ─► Idle     │
//!    │                                          │      [CloseCamera]                       │
//!    └──────────────────────────────────────────┴──── frame classified | capture timeout ◄┘
//!                                                     [Apply? + CloseCamera]
//! ```
//!
//! A second entry into evaluation exists for slow rotations that never look
//! like a tilt onset ("stable mismatch"): the device rests in an orientation
//! that differs from the screen and was not evaluated yet.

use facetilt_decision::{verify, Decision, DecisionEngine, HeadObservation, OrientationProposer, Proposal, Rotation};

use crate::config::EngineConfig;
use crate::filter::{angle_between_deg, circular_distance, inplane_angle, LowPass};
use crate::metrics::{ActivationRecord, Metrics, Outcome, StatsSnapshot};
use crate::ratelimit::RateLimiter;

/// An empty proposal (nothing proposed, no gating flags set).
const NO_PROPOSAL: Proposal = Proposal {
    rotation: None,
    predicted: None,
    tilt_deg: 0.0,
    overhead: false,
    flat: false,
    swinging: false,
    accelerating: false,
};

/// Inputs to the engine. Every event is accompanied by `now_ms` (a monotonic
/// clock; on Android `CLOCK_BOOTTIME`, the sensor timestamp base).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    /// A gravity (or raw accelerometer) sample in m/s², device frame.
    Gravity {
        /// Vector pointing up.
        v: [f32; 3],
    },
    /// Proximity sensor state changed.
    Proximity {
        /// Something is close to the sensor.
        covered: bool,
    },
    /// Hinge angle (foldables).
    Hinge {
        /// 0 = closed, 180 = flat open.
        degrees: f32,
    },
    /// The camera for `session` is open and streaming.
    CameraOpened {
        /// Session id from [`Command::OpenCamera`].
        session: u32,
    },
    /// The camera for `session` failed (could not open, disconnected, error,
    /// privacy toggle, in use by another app…). The camera layer has already
    /// released it.
    CameraError {
        /// Session id.
        session: u32,
    },
    /// The frame requested by [`Command::CaptureFrame`] was classified.
    FrameClassified {
        /// Session id.
        session: u32,
        /// Classifier output, or `None` if the frame was unusable
        /// (e.g. uniform black frame from the camera privacy toggle).
        head: Option<HeadObservation>,
        /// Inference time.
        inference_us: u32,
    },
    /// A deadline from [`Engine::next_deadline`] passed.
    Tick,
    /// Screen off / locked / paused / user switched away: abandon everything.
    Stop,
    /// The display rotation changed outside the engine.
    DisplayRotation(Rotation),
    /// Kotlin finished applying the last rotation in `apply_us`.
    ApplyReported {
        /// Microseconds.
        apply_us: u32,
    },
}

/// Outputs of the engine, executed by the platform layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Open the front camera and start streaming into the (hidden) reader.
    OpenCamera {
        /// Session id echoed in camera events.
        session: u32,
    },
    /// Classify the next frame that arrives.
    CaptureFrame {
        /// Session id.
        session: u32,
    },
    /// Close the camera now (also valid while it is still opening).
    CloseCamera {
        /// Session id.
        session: u32,
    },
    /// Show this orientation (normal mode), or pin it against gravity (assist mode).
    Apply(Decision),
    /// Assist mode only: take rotation control from the system (`Seize`, so the
    /// system auto-rotate cannot flip before FaceTilt has decided) or hand it
    /// back (`Release`, re-enabling the system's own auto-rotate).
    Control(ControlAction),
}

/// Payload of [`Command::Control`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlAction {
    /// Disable the system auto-rotate and lock the current orientation while
    /// FaceTilt evaluates.
    Seize,
    /// Re-enable the system auto-rotate; FaceTilt is no longer holding anything.
    Release,
}

/// Why an activation was abandoned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbortReason {
    /// The tilt reversed before reaching a new orientation.
    Reversed,
    /// Nothing settled within `settle_timeout_ms`.
    SettleTimeout,
    /// Screen off / paused.
    Stopped,
    /// Settled during the decision cooldown.
    Cooldown,
}

/// Small fixed-capacity command list (no allocation on the hot path).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Commands {
    buf: [Option<Command>; 4],
    len: usize,
}

impl Commands {
    fn push(&mut self, c: Command) {
        // The engine never emits more than 3 commands per event; the debug
        // assertion documents that invariant, release builds drop extras.
        debug_assert!(self.len < self.buf.len());
        if let Some(slot) = self.buf.get_mut(self.len) {
            *slot = Some(c);
            self.len += 1;
        }
    }

    /// Number of commands.
    pub fn len(&self) -> usize {
        self.len
    }

    /// True if empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Iterates the commands in order.
    pub fn iter(&self) -> impl Iterator<Item = Command> + '_ {
        self.buf[..self.len].iter().flatten().copied()
    }
}

/// Externally visible phase (for tests and the debug screen).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    /// Monitoring.
    Idle,
    /// A tilt is in progress.
    Tilting {
        /// Onset time.
        onset_ms: u64,
        /// Largest angle from the stable reference seen so far.
        peak_deg: f32,
    },
    /// Settled; waiting for the camera and/or a classified frame.
    AwaitingFrame {
        /// Give up (gravity fallback) at this time.
        deadline_ms: u64,
        /// CaptureFrame was issued.
        capture_requested: bool,
        /// Gravity rotation at settle time.
        gravity: Rotation,
    },
}

/// A scheduled "2nd verification" of a portrait → landscape change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Verify {
    /// Orientation before the change (portrait).
    from: Rotation,
    /// Orientation being verified (landscape).
    to: Rotation,
    /// Earliest time for the second look.
    due_ms: u64,
    /// Give up if the phone has not rested in `to` by then.
    expires_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cam {
    Closed,
    Opening(u32),
    Open(u32),
}

/// The FaceTilt engine. See the module docs.
#[derive(Debug, Clone)]
pub struct Engine {
    cfg: EngineConfig,
    decision: DecisionEngine,
    lpf: LowPass,
    last_sample_ms: Option<u64>,
    omega_dps: f32,
    filtered: Option<[f32; 3]>,
    /// AOSP-grade gravity proposal (overhead/flat/swing/acceleration gating,
    /// per-orientation tilt tolerance, hysteresis). Replaces the naive reader.
    proposer: OrientationProposer,
    proposal: Proposal,
    stable_ref: Option<[f32; 3]>,
    stable_since: Option<u64>,
    proximity_covered: bool,
    half_folded: bool,
    phase: Phase,
    cam: Cam,
    next_session: u32,
    opened_this_activation: bool,
    limiter: RateLimiter,
    last_evaluated: Option<Rotation>,
    last_change: Option<(u64, Rotation, Rotation)>,
    /// 2nd verification scheduled after a portrait → landscape change.
    pending_verify: Option<Verify>,
    /// The activation in progress is a 2nd verification.
    verifying: Option<Verify>,
    /// Assist mode: FaceTilt has taken rotation control from the system.
    assist_controlling: bool,
    /// Assist mode: FaceTilt is holding portrait against gravity (user lying).
    assist_holding: bool,
    record: Option<ActivationRecord>,
    metrics: Metrics,
}

impl Engine {
    /// New engine. `current` is the rotation on screen at start.
    pub fn new(cfg: EngineConfig, current: Rotation, now_ms: u64) -> Self {
        Engine {
            decision: DecisionEngine::new(cfg.decision, current),
            lpf: LowPass::new(cfg.lpf_tau_ms),
            limiter: RateLimiter::new(cfg.min_open_interval_ms, cfg.max_backoff_ms, cfg.backoff_reset_ms),
            cfg,
            last_sample_ms: None,
            omega_dps: 0.0,
            filtered: None,
            proposer: OrientationProposer::new(cfg.proposer, current),
            proposal: NO_PROPOSAL,
            stable_ref: None,
            stable_since: None,
            proximity_covered: false,
            half_folded: false,
            phase: Phase::Idle,
            cam: Cam::Closed,
            next_session: 0,
            opened_this_activation: false,
            last_evaluated: None,
            last_change: None,
            record: None,
            pending_verify: None,
            verifying: None,
            assist_controlling: false,
            assist_holding: false,
            metrics: Metrics::new(now_ms),
        }
    }

    /// Current phase.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Rotation the engine believes is on screen.
    pub fn current(&self) -> Rotation {
        self.decision.current()
    }

    /// True while the engine wants the camera open.
    pub fn camera_wanted(&self) -> bool {
        self.cam != Cam::Closed
    }

    /// Active configuration.
    pub fn config(&self) -> &EngineConfig {
        &self.cfg
    }

    /// Replaces the configuration (takes effect for the next activation).
    pub fn set_config(&mut self, cfg: EngineConfig) {
        self.cfg = cfg;
        self.decision.set_config(cfg.decision);
        self.lpf = LowPass::new(cfg.lpf_tau_ms);
        self.proposer.set_config(cfg.proposer);
        self.limiter = RateLimiter::new(cfg.min_open_interval_ms, cfg.max_backoff_ms, cfg.backoff_reset_ms);
    }

    /// Metrics.
    pub fn metrics(&self) -> &Metrics {
        &self.metrics
    }

    /// Stats snapshot for the debug screen.
    pub fn stats(&self, now_ms: u64) -> StatsSnapshot {
        self.metrics.snapshot(now_ms)
    }

    /// Next time at which [`Event::Tick`] must be delivered, if any.
    pub fn next_deadline(&self) -> Option<u64> {
        match self.phase {
            Phase::Idle => None,
            Phase::Tilting { onset_ms, .. } => Some(onset_ms + self.cfg.settle_timeout_ms),
            Phase::AwaitingFrame { deadline_ms, .. } => Some(deadline_ms),
        }
    }

    /// Processes one event.
    pub fn handle(&mut self, now_ms: u64, event: Event) -> Commands {
        let mut out = Commands::default();
        match event {
            Event::Gravity { v } => self.on_gravity(now_ms, v, &mut out),
            Event::Proximity { covered } => self.proximity_covered = covered,
            Event::Hinge { degrees } => {
                let (lo, hi) = self.cfg.half_fold_range;
                self.half_folded = degrees > lo && degrees < hi;
            }
            Event::CameraOpened { session } => self.on_camera_opened(now_ms, session, &mut out),
            Event::CameraError { session } => self.on_camera_error(now_ms, session, &mut out),
            Event::FrameClassified { session, head, inference_us } => {
                self.on_frame(now_ms, session, head, inference_us, &mut out);
            }
            Event::Tick => self.on_tick(now_ms, &mut out),
            Event::Stop => self.on_stop(now_ms, &mut out),
            Event::DisplayRotation(r) => {
                if r != self.decision.current() {
                    self.pending_verify = None;
                    self.decision.set_current(r);
                    self.last_evaluated = None;
                }
            }
            Event::ApplyReported { apply_us } => self.metrics.report_apply(apply_us),
        }
        out
    }

    // ----- sensor path ---------------------------------------------------

    fn on_gravity(&mut self, now: u64, v: [f32; 3], out: &mut Commands) {
        if !(v[0].is_finite() && v[1].is_finite() && v[2].is_finite()) {
            return;
        }
        let dt = self.last_sample_ms.map_or(20, |t| now.saturating_sub(t).clamp(1, 200)) as f32;
        self.last_sample_ms = Some(now);
        let prev = self.lpf.value();
        let f = self.lpf.update(v, dt);
        if let Some(p) = prev {
            let inst = angle_between_deg(p, f) * 1000.0 / dt;
            let a = dt / (self.cfg.omega_tau_ms.max(0.0) + dt);
            self.omega_dps += a * (inst - self.omega_dps);
        }
        self.filtered = Some(f);
        // The AOSP proposer owns its own (200 ms) filter; feed it the raw sample.
        self.proposer.set_current(self.decision.current());
        self.proposal = self.proposer.update(now, v);

        if self.omega_dps < self.cfg.stable_dps {
            self.stable_since.get_or_insert(now);
        } else {
            self.stable_since = None;
        }
        let stable = self.stable_since.is_some_and(|s| now.saturating_sub(s) >= self.cfg.stable_ms);

        match self.phase {
            Phase::Idle => self.idle_sample(now, f, stable, out),
            Phase::Tilting { onset_ms, peak_deg } => self.tilting_sample(now, f, stable, onset_ms, peak_deg, out),
            Phase::AwaitingFrame { .. } => {}
        }
    }

    fn idle_sample(&mut self, now: u64, f: [f32; 3], stable: bool, out: &mut Commands) {
        let reference = *self.stable_ref.get_or_insert(f);
        let current = self.decision.current();
        if stable {
            self.stable_ref = Some(f);
            // Assist mode: we are holding portrait against gravity (user lying)
            // and the phone has now come to rest in that same orientation (user
            // sat up). Hand control back to the system auto-rotate — no camera.
            if self.assist_holding && self.proposal.predicted == Some(current) {
                self.assist_release(out);
                return;
            }
            if self.try_second_verification(now, out) {
                return;
            }
            match self.proposal.predicted {
                Some(g) if g == current => self.last_evaluated = None,
                Some(g) if Some(g) != self.last_evaluated && !self.decision.in_cooldown(now) => {
                    // Came to rest in a new, not-yet-evaluated orientation
                    // without a detectable onset (slow rotation, pick-up from flat).
                    self.begin(now, false, out);
                    self.settle(now, out);
                }
                _ => {}
            }
            return;
        }
        let angle = angle_between_deg(f, reference);
        let inplane = match (inplane_angle(f), inplane_angle(reference)) {
            (Some(a), Some(b)) => circular_distance(a, b),
            _ => 0.0,
        };
        if angle > self.cfg.onset_deg && self.omega_dps > self.cfg.moving_dps && inplane > self.cfg.onset_inplane_deg {
            // Moving back towards the orientation already on screen (e.g. sitting
            // up after lying on one side): the likely outcome needs no camera. If
            // the motion continues past it, `settle` opens the camera late.
            let prewarm = !self.returning_to_current(f, reference);
            self.begin(now, prewarm, out);
        }
    }

    fn tilting_sample(&mut self, now: u64, f: [f32; 3], stable: bool, onset: u64, peak: f32, out: &mut Commands) {
        let reference = self.stable_ref.unwrap_or(f);
        let angle = angle_between_deg(f, reference);
        let peak = peak.max(angle);
        self.phase = Phase::Tilting { onset_ms: onset, peak_deg: peak };
        let current = self.decision.current();
        // The proposer's predicted rotation reaching a new orientation is the
        // "tilt has arrived" signal (hysteresis already applied); settling also
        // happens on a plain stability timeout.
        let in_new_band = self.proposal.predicted.is_some_and(|r| r != current);
        if in_new_band || stable {
            self.settle(now, out);
        } else if peak - angle > self.cfg.reverse_deg && self.proposal.predicted.is_none_or(|g| g == current) {
            self.abort(now, AbortReason::Reversed, out);
        } else if now.saturating_sub(onset) >= self.cfg.settle_timeout_ms {
            self.abort(now, AbortReason::SettleTimeout, out);
        }
    }

    // ----- activation lifecycle -------------------------------------------

    fn camera_allowed(&mut self, now: u64) -> bool {
        self.cam == Cam::Closed
            && !(self.cfg.use_proximity && self.proximity_covered)
            && !self.half_folded
            // AOSP-grade suppression: never spend the camera while the device is
            // flat (on a table) or overhead (screen facing the user lying on
            // their back). Swing/acceleration are the very motion we respond to,
            // so they do not block the pre-warm; the camera confirm debounces.
            && !self.proposal.flat
            && !self.proposal.overhead
            && !self.decision.in_cooldown(now)
            && self.limiter.can_open(now)
    }

    /// "2nd verification": once the phone rests in the landscape orientation a
    /// previous decision chose, look once more and switch back to portrait if
    /// the face is still aligned with it (user lying on their side).
    /// Returns true if a verification activation was started.
    fn try_second_verification(&mut self, now: u64, out: &mut Commands) -> bool {
        let Some(v) = self.pending_verify else { return false };
        let still_there = self.decision.current() == v.to && self.proposal.predicted == Some(v.to);
        if now >= v.expires_ms || !still_there {
            self.pending_verify = None;
            return false;
        }
        if now < v.due_ms || !self.camera_allowed(now) {
            return false; // retried on the next sample until it expires
        }
        self.pending_verify = None;
        self.verifying = Some(v);
        self.record = Some(ActivationRecord { onset_ms: now, settle_ms: Some(now), ..Default::default() });
        self.opened_this_activation = false;
        self.open_camera(now, out);
        self.phase = Phase::AwaitingFrame {
            deadline_ms: now + self.cfg.capture_timeout_ms,
            capture_requested: false,
            gravity: v.to,
        };
        true
    }

    fn returning_to_current(&self, f: [f32; 3], reference: [f32; 3]) -> bool {
        let c = self.decision.current().center_deg();
        match (inplane_angle(f), inplane_angle(reference)) {
            (Some(a), Some(b)) => circular_distance(a, c) + 5.0 < circular_distance(b, c),
            _ => false,
        }
    }

    /// Starts an activation. `prewarm`: open the camera right away (tilt onset)
    /// instead of only once the motion has settled.
    fn begin(&mut self, now: u64, prewarm: bool, out: &mut Commands) {
        self.phase = Phase::Tilting { onset_ms: now, peak_deg: 0.0 };
        self.record = Some(ActivationRecord { onset_ms: now, ..Default::default() });
        self.opened_this_activation = false;
        // Assist mode: take control the moment a tilt starts (≈15°), before the
        // system's own auto-rotate would flip at ≈45-60°, so FaceTilt can decide
        // whether the user is lying down first.
        if self.cfg.assist_mode && !self.assist_controlling {
            self.assist_controlling = true;
            out.push(Command::Control(ControlAction::Seize));
        }
        if prewarm && self.camera_allowed(now) {
            self.open_camera(now, out);
        }
    }

    /// Assist mode: hand rotation control back to the system (used when the user
    /// sits back up, and on stop). The held orientation is already `current`, so
    /// nothing else need change.
    fn assist_release(&mut self, out: &mut Commands) {
        self.assist_holding = false;
        if self.assist_controlling {
            self.assist_controlling = false;
            out.push(Command::Control(ControlAction::Release));
        }
    }

    fn open_camera(&mut self, now: u64, out: &mut Commands) {
        self.next_session = self.next_session.wrapping_add(1);
        let session = self.next_session;
        self.cam = Cam::Opening(session);
        self.opened_this_activation = true;
        self.limiter.on_open(now);
        self.metrics.camera_opened_cmd(now);
        if let Some(r) = self.record.as_mut() {
            r.open_cmd_ms = Some(now);
        }
        out.push(Command::OpenCamera { session });
    }

    fn close_camera(&mut self, now: u64, out: &mut Commands) {
        match self.cam {
            Cam::Opening(session) | Cam::Open(session) => {
                out.push(Command::CloseCamera { session });
                self.metrics.camera_closed_cmd(now);
                self.cam = Cam::Closed;
            }
            Cam::Closed => {}
        }
    }

    fn finish(&mut self, now: u64, outcome: Outcome, out: &mut Commands) {
        self.verifying = None;
        self.close_camera(now, out);
        if let Some(mut r) = self.record.take() {
            r.outcome = outcome;
            self.metrics.finish(r);
        }
        self.phase = Phase::Idle;
        // Assist mode: every activation that seized control but is not holding
        // an orientation hands control back to the system here (covers aborts,
        // no-change / flat settles and decisions that just follow gravity).
        if self.cfg.assist_mode && self.assist_controlling && !self.assist_holding {
            self.assist_controlling = false;
            out.push(Command::Control(ControlAction::Release));
        }
        // The current pose becomes the new reference; fresh stability window.
        self.stable_ref = self.filtered;
    }

    fn settle(&mut self, now: u64, out: &mut Commands) {
        if let Some(r) = self.record.as_mut() {
            r.settle_ms = Some(now);
        }
        let current = self.decision.current();
        let Some(g) = self.proposal.predicted else {
            // Flat or unreliable: nothing to decide (Android also keeps the
            // orientation while flat).
            if self.opened_this_activation {
                self.limiter.on_wasted();
            }
            return self.finish(now, Outcome::NoChange, out);
        };
        if g == current {
            self.last_evaluated = None;
            if self.opened_this_activation {
                self.limiter.on_wasted();
            }
            return self.finish(now, Outcome::NoChange, out);
        }
        if Some(g) == self.last_evaluated {
            // This exact proposal was already judged (typically: the face said
            // "keep" while the user was still lying down and the motion kept
            // going). Re-deciding without a fresh frame would fall back to
            // gravity and undo the face verdict.
            if self.opened_this_activation {
                self.limiter.on_wasted();
            }
            return self.finish(now, Outcome::NoChange, out);
        }
        if self.decision.in_cooldown(now) {
            // Re-evaluated later through the stable-mismatch path.
            return self.finish(now, Outcome::Aborted(AbortReason::Cooldown), out);
        }
        let deadline_ms = now + self.cfg.capture_timeout_ms;
        match self.cam {
            Cam::Open(session) => {
                self.request_capture(now, session, out);
                self.phase = Phase::AwaitingFrame { deadline_ms, capture_requested: true, gravity: g };
            }
            Cam::Opening(_) => {
                self.phase = Phase::AwaitingFrame { deadline_ms, capture_requested: false, gravity: g };
            }
            Cam::Closed => {
                if self.camera_allowed(now) {
                    self.open_camera(now, out);
                    self.phase = Phase::AwaitingFrame { deadline_ms, capture_requested: false, gravity: g };
                } else {
                    self.phase = Phase::AwaitingFrame { deadline_ms, capture_requested: false, gravity: g };
                    self.conclude(now, None, out);
                }
            }
        }
    }

    fn request_capture(&mut self, now: u64, session: u32, out: &mut Commands) {
        out.push(Command::CaptureFrame { session });
        if let Some(r) = self.record.as_mut() {
            r.capture_cmd_ms = Some(now);
        }
    }

    /// Takes the decision with whatever head information is available.
    fn conclude(&mut self, now: u64, head: Option<HeadObservation>, out: &mut Commands) {
        let Phase::AwaitingFrame { gravity: settle_gravity, .. } = self.phase else {
            return;
        };
        if let Some(v) = self.verifying.take() {
            return self.conclude_verification(now, v, head, out);
        }
        // Prefer the freshest reading; fall back to the one at settle time.
        let g = self.proposal.predicted.unwrap_or(settle_gravity);
        let before = self.decision.current();
        let d = self.decision.decide(now, g, head);
        let changed = d.rotation != before;
        if let Some(r) = self.record.as_mut() {
            r.decision_ms = Some(now);
        }
        if self.cfg.assist_mode {
            // Only ever act to hold an orientation against gravity (the lying
            // case: the decision differs from what gravity alone would choose).
            // Anything else: give control back to the system auto-rotate.
            if d.rotation != g {
                out.push(Command::Apply(d));
                self.assist_holding = true;
                self.last_change = Some((now, before, d.rotation));
                self.limiter.on_useful();
            } else {
                // Follows gravity: let the system auto-rotate take over (finish
                // emits Release). Keep the engine's `current` in step with it.
                self.decision.set_current(g);
                self.assist_holding = false;
                if head.is_some() {
                    self.limiter.on_useful();
                }
            }
            self.last_evaluated = Some(g);
            return self.finish(now, Outcome::Decided { changed, reason: d.reason }, out);
        }
        if changed {
            out.push(Command::Apply(d));
            let flap = matches!(self.last_change, Some((t, from, _))
                if now.saturating_sub(t) < self.cfg.flap_window_ms && from == d.rotation);
            if flap {
                self.limiter.on_wasted();
            } else {
                self.limiter.on_useful();
            }
            self.last_change = Some((now, before, d.rotation));
            if self.cfg.second_verification && d.rotation.is_landscape() && !before.is_landscape() {
                self.pending_verify = Some(Verify {
                    from: before,
                    to: d.rotation,
                    due_ms: now + self.cfg.verify_delay_ms,
                    expires_ms: now + self.cfg.verify_window_ms,
                });
            }
        } else if head.is_some() {
            self.limiter.on_useful();
        }
        self.last_evaluated = Some(g);
        self.finish(now, Outcome::Decided { changed, reason: d.reason }, out);
    }

    fn conclude_verification(&mut self, now: u64, v: Verify, head: Option<HeadObservation>, out: &mut Commands) {
        let before = self.decision.current();
        let d = verify(before, v.from, head, self.cfg.decision.confidence_threshold);
        let changed = d.rotation != before;
        if let Some(r) = self.record.as_mut() {
            r.decision_ms = Some(now);
        }
        if changed {
            self.decision.commit(now, d.rotation);
            out.push(Command::Apply(d));
            self.last_change = Some((now, before, d.rotation));
        }
        if head.is_some() {
            self.limiter.on_useful();
        }
        // Gravity still proposes landscape; it has now been judged twice, so
        // do not re-evaluate it until the phone moves to another orientation.
        self.last_evaluated = Some(self.proposal.predicted.unwrap_or(v.to));
        self.finish(now, Outcome::Decided { changed, reason: d.reason }, out);
    }

    fn abort(&mut self, now: u64, reason: AbortReason, out: &mut Commands) {
        if self.opened_this_activation && reason != AbortReason::Stopped {
            self.limiter.on_wasted();
        }
        let keep_ref = self.stable_ref;
        self.finish(now, Outcome::Aborted(reason), out);
        if reason == AbortReason::Reversed {
            // The device went back towards where it was: keep the old reference.
            self.stable_ref = keep_ref;
        }
    }

    // ----- camera / timer events -------------------------------------------

    fn on_camera_opened(&mut self, now: u64, session: u32, out: &mut Commands) {
        if self.cam != Cam::Opening(session) {
            return; // stale: the camera layer already got a CloseCamera for it
        }
        self.cam = Cam::Open(session);
        if let Some(r) = self.record.as_mut() {
            r.opened_ms = Some(now);
        }
        if let Phase::AwaitingFrame { deadline_ms, capture_requested: false, gravity } = self.phase {
            self.request_capture(now, session, out);
            self.phase = Phase::AwaitingFrame { deadline_ms, capture_requested: true, gravity };
        }
    }

    fn on_camera_error(&mut self, now: u64, session: u32, out: &mut Commands) {
        if !matches!(self.cam, Cam::Opening(s) | Cam::Open(s) if s == session) {
            return;
        }
        // The camera layer already released the device; just account for it.
        self.metrics.camera_closed_cmd(now);
        self.cam = Cam::Closed;
        self.limiter.on_wasted();
        if matches!(self.phase, Phase::AwaitingFrame { .. }) {
            self.conclude(now, None, out);
        }
    }

    fn on_frame(
        &mut self,
        now: u64,
        session: u32,
        head: Option<HeadObservation>,
        inference_us: u32,
        out: &mut Commands,
    ) {
        if self.cam != Cam::Open(session) {
            return;
        }
        if !matches!(self.phase, Phase::AwaitingFrame { capture_requested: true, .. }) {
            return;
        }
        self.metrics.inference(inference_us);
        if let Some(r) = self.record.as_mut() {
            r.frame_ms = Some(now);
            r.inference_us = Some(inference_us);
        }
        self.conclude(now, head, out);
    }

    fn on_tick(&mut self, now: u64, out: &mut Commands) {
        match self.phase {
            Phase::Tilting { onset_ms, .. } if now.saturating_sub(onset_ms) >= self.cfg.settle_timeout_ms => {
                self.abort(now, AbortReason::SettleTimeout, out);
            }
            Phase::AwaitingFrame { deadline_ms, .. } if now >= deadline_ms => {
                // Camera too slow / no frame: behave like normal auto-rotate.
                self.conclude(now, None, out);
            }
            _ => {}
        }
    }

    fn on_stop(&mut self, now: u64, out: &mut Commands) {
        if self.phase != Phase::Idle {
            self.abort(now, AbortReason::Stopped, out);
        }
        self.close_camera(now, out);
        if self.cfg.assist_mode && self.assist_controlling {
            self.assist_release(out);
        }
        self.assist_holding = false;
        self.pending_verify = None;
        self.verifying = None;
        self.lpf.reset();
        self.filtered = None;
        self.last_sample_ms = None;
        self.omega_dps = 0.0;
        self.stable_ref = None;
        self.stable_since = None;
        self.proposer.reset();
        self.proposal = NO_PROPOSAL;
    }
}
