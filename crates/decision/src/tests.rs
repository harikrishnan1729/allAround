//! Scenario tests for the decision rule. Device poses are derived from physical
//! descriptions rather than hard-coded rotations so the tests also check the
//! sign conventions (see the crate docs).

use super::*;

const T: f32 = 0.7;

fn obs(class: HeadClass, confidence: f32) -> Option<HeadObservation> {
    Some(HeadObservation { class, confidence })
}

fn gravity_for(up_in_device_frame: [f32; 3], current: Rotation) -> Rotation {
    read_gravity(up_in_device_frame, current, &GravityConfig::default()).rotation().expect("pose is not flat")
}

#[test]
fn class_to_rotation_mapping() {
    assert_eq!(HeadClass::Upright.content_rotation(), Some(Rotation::R0));
    assert_eq!(HeadClass::RotatedRight.content_rotation(), Some(Rotation::R90));
    assert_eq!(HeadClass::UpsideDown.content_rotation(), Some(Rotation::R180));
    assert_eq!(HeadClass::RotatedLeft.content_rotation(), Some(Rotation::R270));
    assert_eq!(HeadClass::NoFace.content_rotation(), None);
    for i in 0..HeadClass::COUNT {
        assert_eq!(HeadClass::from_index(i).map(|c| c as usize), Some(i));
    }
    assert_eq!(HeadClass::from_index(5), None);
}

/// User lies down on their LEFT side holding the phone in portrait relative to
/// their face. Their right side faces the ceiling; the phone's +x (screen right,
/// as the user sees it) points up, so gravity proposes R90. The face still
/// appears upright on the device → keep portrait.
#[test]
fn lying_on_left_side_keeps_portrait() {
    let g = gravity_for([9.81, 0.0, 0.0], Rotation::R0);
    assert_eq!(g, Rotation::R90);
    let d = decide(Rotation::R0, g, obs(HeadClass::Upright, 0.95), T);
    assert_eq!(d, Decision { rotation: Rotation::R0, reason: Reason::FaceKeptOrientation });
}

/// Mirror image of the above: lying on the RIGHT side → gravity says R270.
#[test]
fn lying_on_right_side_keeps_portrait() {
    let g = gravity_for([-9.81, 0.0, 0.0], Rotation::R0);
    assert_eq!(g, Rotation::R270);
    let d = decide(Rotation::R0, g, obs(HeadClass::Upright, 0.9), T);
    assert_eq!(d, Decision { rotation: Rotation::R0, reason: Reason::FaceKeptOrientation });
}

/// Sitting upright and turning the phone CCW: the head top now points to the
/// device's +x edge → RotatedRight → R90, matching gravity → rotate.
#[test]
fn sitting_and_turning_phone_rotates() {
    let g = gravity_for([9.6, 0.5, 1.5], Rotation::R0);
    assert_eq!(g, Rotation::R90);
    let d = decide(Rotation::R0, g, obs(HeadClass::RotatedRight, 0.88), T);
    assert_eq!(d, Decision { rotation: Rotation::R90, reason: Reason::FaceConfirmedRotation });
    // ...and back to portrait from landscape.
    let g = gravity_for([0.3, 9.7, 1.0], Rotation::R90);
    let d = decide(Rotation::R90, g, obs(HeadClass::Upright, 0.9), T);
    assert_eq!(d, Decision { rotation: Rotation::R0, reason: Reason::FaceConfirmedRotation });
}

/// Lying on the left side and watching a video in landscape: the phone is
/// turned so the face is RotatedRight relative to the device… while gravity
/// says portrait. Gravity == current? No: current is R90 (landscape kept
/// because of the face), the user now tilts the phone back to R0 by gravity,
/// but the face still says R90 → keep landscape.
#[test]
fn lying_on_side_with_landscape_content_keeps_landscape() {
    let d = decide(Rotation::R90, Rotation::R0, obs(HeadClass::RotatedRight, 0.9), T);
    assert_eq!(d, Decision { rotation: Rotation::R90, reason: Reason::FaceKeptOrientation });
}

#[test]
fn upside_down_phone() {
    let g = gravity_for([0.0, -9.81, 0.0], Rotation::R0);
    assert_eq!(g, Rotation::R180);
    // Pure rule: face upside down too → rotate to R180.
    let d = decide(Rotation::R0, g, obs(HeadClass::UpsideDown, 0.93), T);
    assert_eq!(d, Decision { rotation: Rotation::R180, reason: Reason::FaceConfirmedRotation });
    // User lying with head "below" the phone (e.g. on their back, looking up,
    // phone inverted relative to gravity but upright relative to the face): keep.
    let d = decide(Rotation::R0, g, obs(HeadClass::Upright, 0.93), T);
    assert_eq!(d.rotation, Rotation::R0);
    assert_eq!(d.reason, Reason::FaceKeptOrientation);

    // Engine: upside-down disallowed by default (like stock Android phones).
    let mut e = DecisionEngine::new(DecisionConfig::default(), Rotation::R0);
    let d = e.decide(1_000, g, obs(HeadClass::UpsideDown, 0.93));
    assert_eq!(d, Decision { rotation: Rotation::R0, reason: Reason::UpsideDownDisallowed });
    let mut e = DecisionEngine::new(DecisionConfig { allow_upside_down: true, ..Default::default() }, Rotation::R0);
    assert_eq!(e.decide(1_000, g, obs(HeadClass::UpsideDown, 0.93)).rotation, Rotation::R180);
}

#[test]
fn no_face_falls_back_to_gravity() {
    let d = decide(Rotation::R0, Rotation::R90, obs(HeadClass::NoFace, 0.99), T);
    assert_eq!(d, Decision { rotation: Rotation::R90, reason: Reason::FallbackNoFace });
}

#[test]
fn camera_unavailable_falls_back_to_gravity() {
    let d = decide(Rotation::R0, Rotation::R270, None, T);
    assert_eq!(d, Decision { rotation: Rotation::R270, reason: Reason::FallbackCameraUnavailable });
}

#[test]
fn low_confidence_falls_back_to_gravity() {
    // Even when the (weak) face vote says "keep", low confidence means gravity wins.
    let d = decide(Rotation::R0, Rotation::R90, obs(HeadClass::Upright, 0.69), T);
    assert_eq!(d, Decision { rotation: Rotation::R90, reason: Reason::FallbackLowConfidence });
    // Exactly at threshold is trusted.
    let d = decide(Rotation::R0, Rotation::R90, obs(HeadClass::Upright, 0.7), T);
    assert_eq!(d.reason, Reason::FaceKeptOrientation);
    // NaN is never trusted.
    let d = decide(Rotation::R0, Rotation::R90, obs(HeadClass::Upright, f32::NAN), T);
    assert_eq!(d.reason, Reason::FallbackLowConfidence);
}

#[test]
fn ambiguous_face_falls_back_to_gravity() {
    // Current R0, gravity R90, face says R270: matches neither → gravity.
    let d = decide(Rotation::R0, Rotation::R90, obs(HeadClass::RotatedLeft, 0.95), T);
    assert_eq!(d, Decision { rotation: Rotation::R90, reason: Reason::FallbackAmbiguous });
}

#[test]
fn gravity_unchanged_is_a_no_op_regardless_of_face() {
    for c in [HeadClass::Upright, HeadClass::RotatedLeft, HeadClass::NoFace] {
        let d = decide(Rotation::R90, Rotation::R90, obs(c, 1.0), T);
        assert_eq!(d, Decision { rotation: Rotation::R90, reason: Reason::GravityUnchanged });
    }
}

/// The device lying flat proposes no orientation, so the engine is never asked;
/// the reading itself must say Flat.
#[test]
fn flat_on_a_table_proposes_nothing() {
    let r = read_gravity([0.2, -0.1, 9.79], Rotation::R0, &GravityConfig::default());
    assert!(matches!(r, GravityReading::Flat { .. }));
    assert_eq!(r.rotation(), None);
}

/// A pose oscillating around 45° must not flip repeatedly: hysteresis keeps
/// the reading stable, and the engine cooldown rate-limits genuine flips.
#[test]
fn rapid_flapping_is_damped() {
    let cfg = GravityConfig::default();
    let mut e = DecisionEngine::new(DecisionConfig::default(), Rotation::R0);
    let mut changes = 0;
    let mut t = 0u64;
    for i in 0..40 {
        // Oscillate 38° ↔ 52° every 50 ms.
        let a: f32 = if i % 2 == 0 { 38.0 } else { 52.0 };
        let g = [9.81 * a.to_radians().sin(), 9.81 * a.to_radians().cos(), 0.0];
        let grav = read_gravity(g, e.current(), &cfg).rotation().expect("oriented");
        // Worst case: the face always agrees with gravity.
        let head = obs(if grav == Rotation::R90 { HeadClass::RotatedRight } else { HeadClass::Upright }, 0.9);
        let before = e.current();
        e.decide(t, grav, head);
        if e.current() != before {
            changes += 1;
        }
        t += 50;
    }
    assert_eq!(changes, 0, "hysteresis should absorb ±7° around 45°");

    // Wider oscillation (30° ↔ 60°) passes hysteresis; the cooldown limits flips
    // to at most one per cooldown window.
    let mut e = DecisionEngine::new(DecisionConfig::default(), Rotation::R0);
    let mut changes = 0;
    let mut t = 0u64;
    for i in 0..40 {
        let a: f32 = if i % 2 == 0 { 60.0 } else { 30.0 };
        let g = [9.81 * a.to_radians().sin(), 9.81 * a.to_radians().cos(), 0.0];
        let grav = read_gravity(g, e.current(), &cfg).rotation().expect("oriented");
        let head = obs(if grav == Rotation::R90 { HeadClass::RotatedRight } else { HeadClass::Upright }, 0.9);
        let before = e.current();
        let d = e.decide(t, grav, head);
        if e.current() != before {
            changes += 1;
        } else if grav != before {
            assert_eq!(d.reason, Reason::Cooldown);
        }
        t += 50;
    }
    // 40 samples * 50 ms = 2 s; cooldown 600 ms → at most 4 changes.
    assert!(changes <= 4, "changes = {changes}");
    assert!(changes >= 2);
}

#[test]
fn cooldown_expires() {
    let mut e = DecisionEngine::new(DecisionConfig::default(), Rotation::R0);
    assert_eq!(e.decide(0, Rotation::R90, obs(HeadClass::RotatedRight, 0.9)).rotation, Rotation::R90);
    assert!(e.in_cooldown(599));
    let d = e.decide(599, Rotation::R0, obs(HeadClass::Upright, 0.9));
    assert_eq!(d, Decision { rotation: Rotation::R90, reason: Reason::Cooldown });
    assert!(!e.in_cooldown(600));
    assert_eq!(e.decide(600, Rotation::R0, obs(HeadClass::Upright, 0.9)).rotation, Rotation::R0);
}

#[test]
fn keeping_does_not_start_cooldown() {
    let mut e = DecisionEngine::new(DecisionConfig::default(), Rotation::R0);
    let d = e.decide(0, Rotation::R90, obs(HeadClass::Upright, 0.9));
    assert_eq!(d.reason, Reason::FaceKeptOrientation);
    assert!(!e.in_cooldown(1));
}

/// "Screen turns off mid-capture": the capture is discarded, so `decide` is
/// never called. The engine must therefore be unaffected by a capture that
/// never completed; the end-to-end version of this test lives in
/// `facetilt-core` (`tests/scenarios.rs::screen_off_mid_capture_discards`).
#[test]
fn discarded_capture_leaves_engine_untouched() {
    let e = DecisionEngine::new(DecisionConfig::default(), Rotation::R0);
    let snapshot = e.clone();
    // (capture aborted: no call)
    assert_eq!(e.current(), snapshot.current());
    assert!(!e.in_cooldown(0));
}

#[test]
fn external_rotation_updates_current_without_cooldown() {
    let mut e = DecisionEngine::new(DecisionConfig::default(), Rotation::R0);
    e.set_current(Rotation::R270);
    assert_eq!(e.current(), Rotation::R270);
    assert!(!e.in_cooldown(0));
}

#[test]
fn rotation_helpers() {
    assert_eq!(Rotation::R0.quarter_turns_between(Rotation::R270), 1);
    assert_eq!(Rotation::R90.quarter_turns_between(Rotation::R270), 2);
    assert_eq!(Rotation::try_from_index(4), None);
    assert_eq!(Rotation::try_from_index(-1), None);
    assert_eq!(Rotation::try_from_index(3), Some(Rotation::R270));
    for r in Rotation::ALL {
        assert_eq!(Rotation::from_index(r.index() as u32), r);
    }
}

// ----- 2nd verification -----------------------------------------------------

#[test]
fn second_check_reverts_when_user_is_lying() {
    // Screen went to landscape (R90) although the user lies on their side: at
    // rest, the face still appears upright relative to the device → back to R0.
    let d = verify(Rotation::R90, Rotation::R0, obs(HeadClass::Upright, 0.9), T);
    assert_eq!(d, Decision { rotation: Rotation::R0, reason: Reason::SecondCheckReverted });
    let d = verify(Rotation::R270, Rotation::R0, obs(HeadClass::Upright, 0.8), T);
    assert_eq!(d.rotation, Rotation::R0);
}

#[test]
fn second_check_confirms_landscape_for_seated_user() {
    let d = verify(Rotation::R90, Rotation::R0, obs(HeadClass::RotatedRight, 0.9), T);
    assert_eq!(d, Decision { rotation: Rotation::R90, reason: Reason::SecondCheckConfirmed });
}

#[test]
fn second_check_never_acts_without_a_confident_face() {
    for head in [None, obs(HeadClass::NoFace, 0.99), obs(HeadClass::Upright, 0.5), obs(HeadClass::Upright, f32::NAN)] {
        let d = verify(Rotation::R90, Rotation::R0, head, T);
        assert_eq!(d, Decision { rotation: Rotation::R90, reason: Reason::SecondCheckInconclusive }, "{head:?}");
    }
}

#[test]
fn second_check_never_invents_a_third_orientation() {
    // Face says R270 (neither the shown R90 nor the previous R0): keep.
    let d = verify(Rotation::R90, Rotation::R0, obs(HeadClass::RotatedLeft, 0.95), T);
    assert_eq!(d.rotation, Rotation::R90);
}

#[test]
fn commit_starts_cooldown() {
    let mut e = DecisionEngine::new(DecisionConfig::default(), Rotation::R90);
    e.commit(1_000, Rotation::R0);
    assert_eq!(e.current(), Rotation::R0);
    assert!(e.in_cooldown(1_500));
    assert!(!e.in_cooldown(1_600));
    assert!(Rotation::R90.is_landscape() && Rotation::R270.is_landscape() && !Rotation::R0.is_landscape());
}
