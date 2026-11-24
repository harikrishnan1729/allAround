//! Tests of the AOSP-faithful orientation proposer. Device poses are described
//! physically (in-plane roll + tilt) and converted to a gravity vector, so the
//! tests also pin the sign conventions.

use super::*;

/// Gravity "up" vector for a pose: `roll` = in-plane orientation angle (deg,
/// 90 = device +x up), `tilt` = angle of the screen from vertical (deg,
/// +90 flat facing up, −90 overhead facing down).
fn pose(roll_deg: f32, tilt_deg: f32) -> [f32; 3] {
    let (a, t) = (roll_deg.to_radians(), tilt_deg.to_radians());
    let inplane = STANDARD_GRAVITY * t.cos();
    [inplane * a.sin(), inplane * a.cos(), STANDARD_GRAVITY * t.sin()]
}

/// Feeds a steady pose for `dur_ms` at 20 ms and returns the final proposal.
fn hold(p: &mut OrientationProposer, start: u64, dur_ms: u64, roll: f32, tilt: f32) -> (u64, Proposal) {
    let v = pose(roll, tilt);
    let mut now = start;
    let mut last = p.update(now, v);
    let end = start + dur_ms;
    while now < end {
        now += 20;
        last = p.update(now, v);
    }
    (now, last)
}

fn new() -> OrientationProposer {
    OrientationProposer::new(ProposerConfig::default(), Rotation::R0)
}

#[test]
fn upright_portrait_proposes_r0() {
    let mut p = new();
    let (_, prop) = hold(&mut p, 0, 1000, 0.0, 5.0);
    assert_eq!(prop.rotation, Some(Rotation::R0));
    assert!(!prop.overhead && !prop.flat);
}

#[test]
fn quarter_turns_from_r0() {
    for (roll, want) in [(90.0, Rotation::R90), (270.0, Rotation::R270)] {
        let mut p = new();
        let (_, prop) = hold(&mut p, 0, 1000, roll, 10.0);
        assert_eq!(prop.rotation, Some(want), "roll {roll}");
    }
}

#[test]
fn overhead_lying_on_back_proposes_nothing() {
    // Phone held over the face, screen facing down (tilt ≈ −60°): overhead mode,
    // no rotation however it is turned.
    let mut p = new();
    for roll in [0.0, 90.0, 180.0, 270.0] {
        let (_, prop) = hold(&mut p, 0, 1200, roll, -60.0);
        assert!(prop.overhead, "roll {roll}: {prop:?}");
        assert_eq!(prop.rotation, None, "roll {roll}");
        p.reset();
    }
}

#[test]
fn overhead_has_hysteresis() {
    let mut p = new();
    hold(&mut p, 0, 600, 90.0, -50.0); // enter overhead
                                       // Tilt back up to −30° (above enter −40 but below exit −15): stays overhead.
    let (_, prop) = hold(&mut p, 600, 400, 90.0, -30.0);
    assert!(prop.overhead);
    // Up past −15: leaves overhead and (after settle) proposes landscape.
    let (_, prop) = hold(&mut p, 1000, 800, 90.0, 10.0);
    assert!(!prop.overhead);
    assert_eq!(prop.rotation, Some(Rotation::R90));
}

#[test]
fn flat_on_table_proposes_nothing() {
    let mut p = new();
    let (_, prop) = hold(&mut p, 0, 1500, 0.0, 88.0);
    assert!(prop.flat, "{prop:?}");
    assert_eq!(prop.rotation, None);
}

#[test]
fn upside_down_needs_more_vertical_than_landscape() {
    // Tilt 62°: within R90's tolerance (≤65) but outside R180's (≤60).
    let mut p = OrientationProposer::new(ProposerConfig::default(), Rotation::R90);
    let (_, prop) = hold(&mut p, 0, 1000, 180.0, 62.0);
    assert_eq!(prop.rotation, None, "upside-down rejected when too tilted: {prop:?}");
    // At 55° it is accepted.
    let mut p = OrientationProposer::new(ProposerConfig::default(), Rotation::R90);
    let (_, prop) = hold(&mut p, 0, 1000, 180.0, 55.0);
    assert_eq!(prop.rotation, Some(Rotation::R180));
}

#[test]
fn hysteresis_dead_zone_around_45() {
    // From R0, a roll of 60° is past the naive 45° boundary but inside the
    // 45+22.5 = 67.5° dead-zone, so it must NOT propose R90.
    let mut p = OrientationProposer::new(ProposerConfig::default(), Rotation::R0);
    let (_, prop) = hold(&mut p, 0, 1000, 60.0, 10.0);
    assert_ne!(prop.rotation, Some(Rotation::R90), "60° is inside the dead-zone: {prop:?}");
    // 70° is past it.
    let mut p = OrientationProposer::new(ProposerConfig::default(), Rotation::R0);
    let (_, prop) = hold(&mut p, 0, 1000, 70.0, 10.0);
    assert_eq!(prop.rotation, Some(Rotation::R90));
}

#[test]
fn predicted_appears_before_proposed_settles() {
    // The predicted (pre-settle) rotation is available immediately; the proposal
    // only after the settle barrier.
    let mut p = new();
    let v = pose(90.0, 10.0);
    let first = p.update(0, v);
    assert_eq!(first.predicted, Some(Rotation::R90));
    assert_eq!(first.rotation, None, "not settled on the first sample");
    let later = p.update(60, v);
    assert_eq!(later.rotation, Some(Rotation::R90));
}

#[test]
fn swing_pickup_delays_the_proposal() {
    // Pick the phone up from flat into landscape: the tilt swings from ~88° down
    // to ~10°, which is a swing, so the proposal is delayed ~300 ms after the
    // motion settles rather than firing immediately.
    let mut p = new();
    // Resting flat first.
    let mut now = hold(&mut p, 0, 400, 90.0, 88.0).0;
    // Swing up to landscape over ~200 ms.
    let v_target = pose(90.0, 10.0);
    for tilt in [70.0, 50.0, 30.0, 10.0] {
        now += 50;
        p.update(now, pose(90.0, tilt));
        let _ = v_target;
    }
    // Immediately after the swing, not yet proposed.
    let right_after = p.update(now + 20, pose(90.0, 10.0));
    assert_eq!(right_after.rotation, None, "proposal must wait after a swing: {right_after:?}");
    // After the swing-ended delay it settles.
    let (_, prop) = hold(&mut p, now + 40, 500, 90.0, 10.0);
    assert_eq!(prop.rotation, Some(Rotation::R90));
}

#[test]
fn acceleration_delays_proposal_for_raw_accelerometer() {
    // A reading well above gravity magnitude (shaking) counts as acceleration
    // and blocks the proposal until it subsides.
    let mut p = new();
    hold(&mut p, 0, 400, 90.0, 10.0); // steady landscape (settled)
                                      // Shaking: several high-magnitude samples (one spike is filtered out, as in AOSP).
    let big = {
        let v = pose(90.0, 10.0);
        [v[0] * 2.5, v[1] * 2.5, v[2] * 2.5]
    };
    let mut during = p.update(420, big);
    for t in [440, 460, 480, 500] {
        during = p.update(t, big);
    }
    assert!(during.accelerating, "{during:?}");
    assert_eq!(during.rotation, None, "no proposal while accelerating");
    // Recovers after the acceleration-ended delay.
    let (_, prop) = hold(&mut p, 520, 700, 90.0, 10.0);
    assert_eq!(prop.rotation, Some(Rotation::R90));
}

#[test]
fn gravity_vector_never_flags_acceleration() {
    // A TYPE_GRAVITY vector always has magnitude ≈ 9.81, so the acceleration
    // gate stays inactive (correctly — linear acceleration is already removed).
    let mut p = new();
    let (_, prop) = hold(&mut p, 0, 1000, 90.0, 10.0);
    assert!(!prop.accelerating);
    assert_eq!(prop.rotation, Some(Rotation::R90));
}

#[test]
fn filter_resets_after_a_long_gap() {
    let mut p = new();
    hold(&mut p, 0, 500, 0.0, 5.0);
    // 3 s gap, then straight into landscape: the filter restarts from the new
    // sample (no slow drift from the stale portrait value).
    let later = p.update(3500, pose(90.0, 10.0));
    assert_eq!(later.predicted, Some(Rotation::R90));
}

#[test]
fn lying_on_side_still_reports_landscape_from_gravity() {
    // Gravity alone cannot tell "lying on your side" from "sitting and rotating":
    // both give a landscape proposal. (The camera layer disambiguates; this just
    // confirms the proposer behaves like stock Android here.)
    let mut p = new();
    let (_, prop) = hold(&mut p, 0, 1000, 90.0, 12.0);
    assert_eq!(prop.rotation, Some(Rotation::R90));
}
