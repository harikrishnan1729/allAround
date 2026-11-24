//! End-to-end scenarios: sensor trace → engine → simulated camera → decision.
//! Traces live in `tests/traces/` (synthetic, see `tools/gen_traces.py`).
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use common::{load, seq, synth, CamModel, Head, Sim, SimResult};
use facetilt_core::decision::{HeadClass, HeadObservation, Reason, Rotation};
use facetilt_core::{AbortReason, Command, Event, Outcome};

fn run(name: &str) -> SimResult {
    let t = load(name);
    let head = Head::from_meta(t.meta("head").unwrap());
    Sim::new(head).run(&t)
}

fn expect_rot(t: &facetilt_core::trace::Trace) -> Option<Rotation> {
    match t.meta("expect")? {
        "R0" => Some(Rotation::R0),
        "R90" => Some(Rotation::R90),
        "R180" => Some(Rotation::R180),
        "R270" => Some(Rotation::R270),
        _ => None,
    }
}

/// Invariants that must hold for *every* trace and camera model.
fn check_invariants(name: &str, r: &SimResult) {
    assert_eq!(r.open_at_end, 0, "{name}: camera left open");
    // The camera is never open longer than settle timeout + capture timeout.
    assert!(r.max_camera_session_ms <= 1_500 + 700, "{name}: camera session {} ms", r.max_camera_session_ms);
    // Opens are at least 800 ms apart.
    let opens: Vec<u64> =
        r.commands.iter().filter(|(_, c)| matches!(c, Command::OpenCamera { .. })).map(|(t, _)| *t).collect();
    for w in opens.windows(2) {
        assert!(w[1] - w[0] >= 800, "{name}: opens {} ms apart", w[1] - w[0]);
    }
}

#[test]
fn all_traces_respect_invariants_and_expectations() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/traces");
    let mut n = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        let t = load(&name);
        let mut sim = Sim::new(Head::from_meta(t.meta("head").unwrap()));
        let r = sim.run(&t);
        check_invariants(&name, &r);
        if let Some(want) = expect_rot(&t) {
            // Upside-down is disabled by default; covered separately.
            if want != Rotation::R180 {
                assert_eq!(r.final_rotation, Some(want), "{name}: {:#?}", r.records);
            }
        }
        // Same trace with a slow camera and with a failing camera: invariants hold.
        sim.cam = CamModel { open_ms: 450, first_frame_ms: 120, ..CamModel::default() };
        check_invariants(&name, &sim.run(&t));
        sim.cam = CamModel { fail_open: true, ..CamModel::default() };
        check_invariants(&name, &sim.run(&t));
        // With the 2nd verification enabled: same invariants and expected outcome.
        let mut sim = Sim::new(Head::from_meta(t.meta("head").unwrap()));
        sim.cfg.second_verification = true;
        let r = sim.run(&t);
        check_invariants(&name, &r);
        if let Some(want) = expect_rot(&t) {
            if want != Rotation::R180 {
                assert_eq!(r.final_rotation, Some(want), "{name} (2nd verification): {:#?}", r.records);
            }
        }
        n += 1;
    }
    assert!(n >= 15, "only {n} traces found");
}

#[test]
fn sitting_rotation_is_confirmed_by_face_within_budget() {
    // The TYPE_GRAVITY path (essentially every mid-range+ phone) must meet the
    // <250 ms budget; the raw-accelerometer fallback is noisier and crosses the
    // AOSP hysteresis dead-zone later, so it is allowed a little more.
    for (name, budget_ms) in [("rotate_ccw_sitting", 250u64), ("rotate_ccw_sitting_accel", 320)] {
        let r = run(name);
        assert_eq!(r.applied.len(), 1, "{name}: {:?}", r.applied);
        let (_, d) = r.applied[0];
        assert_eq!(d.rotation, Rotation::R90);
        assert_eq!(d.reason, Reason::FaceConfirmedRotation);
        assert_eq!(r.opens, 1, "{name}");
        let rec = r.records.iter().find(|x| matches!(x.outcome, Outcome::Decided { .. })).unwrap();
        let lat = rec.onset_to_decision_ms().unwrap();
        assert!(lat < budget_ms, "{name}: onset→decision {lat} ms (budget {budget_ms}): {rec:#?}");
        // The camera was pre-warmed at onset, not after settling.
        assert_eq!(rec.open_cmd_ms, Some(rec.onset_ms), "{name}");
        assert!(r.max_camera_session_ms < 400, "{name}: camera on {} ms", r.max_camera_session_ms);
        println!("{name}: onset→decision {lat} ms, camera on {} ms, record {rec:?}", r.max_camera_session_ms);
    }
}

#[test]
fn clockwise_rotation() {
    let r = run("rotate_cw_sitting");
    assert_eq!(r.applied.len(), 1);
    assert_eq!(r.applied[0].1.rotation, Rotation::R270);
}

#[test]
fn lying_down_keeps_portrait() {
    for name in ["lie_down_left", "lie_down_right"] {
        let r = run(name);
        assert!(r.applied.is_empty(), "{name}: {:?}", r.applied);
        assert_eq!(r.final_rotation, Some(Rotation::R0));
        assert!(r.opens <= 1, "{name}: {} opens", r.opens);
        let kept = r
            .records
            .iter()
            .any(|x| matches!(x.outcome, Outcome::Decided { changed: false, reason: Reason::FaceKeptOrientation }));
        assert!(kept, "{name}: {:#?}", r.records);
    }
}

#[test]
fn lying_down_without_face_behaves_like_auto_rotate() {
    let t = load("lie_down_left");
    let r = Sim::new(Head::Fixed(HeadClass::NoFace, 0.97)).run(&t);
    assert_eq!(r.final_rotation, Some(Rotation::R90));
    assert_eq!(r.applied[0].1.reason, Reason::FallbackNoFace);
}

#[test]
fn low_confidence_falls_back_to_gravity() {
    let t = load("lie_down_left");
    let r = Sim::new(Head::Fixed(HeadClass::Upright, 0.55)).run(&t);
    assert_eq!(r.final_rotation, Some(Rotation::R90));
    assert_eq!(r.applied[0].1.reason, Reason::FallbackLowConfidence);
}

#[test]
fn unusable_frame_falls_back_to_gravity() {
    // e.g. camera privacy toggle → black frames → vision returns None.
    let t = load("lie_down_left");
    let r = Sim::new(Head::Unusable).run(&t);
    assert_eq!(r.final_rotation, Some(Rotation::R90));
    assert_eq!(r.applied[0].1.reason, Reason::FallbackCameraUnavailable);
}

#[test]
fn sitting_up_after_lying_down_does_not_need_the_camera_again() {
    let r = run("lie_down_then_sit_up");
    assert!(r.applied.is_empty(), "{:?}", r.applied);
    assert_eq!(r.opens, 1, "{:#?}", r.records);
}

#[test]
fn reversed_tilt_is_discarded() {
    let r = run("tilt_and_return");
    assert!(r.applied.is_empty());
    assert!(r.opens <= 1);
    assert!(
        r.records.iter().all(|x| matches!(x.outcome, Outcome::Aborted(AbortReason::Reversed) | Outcome::NoChange)),
        "{:#?}",
        r.records
    );
}

#[test]
fn small_drift_never_opens_the_camera() {
    let r = run("small_drift");
    assert_eq!(r.opens, 0, "{:#?}", r.records);
    assert!(r.records.is_empty());
}

#[test]
fn flat_device_never_opens_the_camera() {
    for name in ["flat_spin", "pickup_from_table"] {
        let r = run(name);
        assert_eq!(r.opens, 0, "{name}: {:#?}", r.records);
        assert!(r.applied.is_empty(), "{name}");
    }
}

#[test]
fn walking_does_not_trigger() {
    let r = run("walking_portrait");
    assert_eq!(r.opens, 0, "{:#?}", r.records);
}

#[test]
fn flapping_is_rate_limited() {
    let t = load("flap_45");
    let r = Sim::new(Head::FollowGravity).run(&t);
    let duration_s = (t.samples.last().unwrap().t_ms - t.samples[0].t_ms) as f64 / 1000.0;
    // 20 swings across 45° in ~8.5 s. Without limiting that would be ~20 opens.
    assert!(r.opens <= 7, "{} opens in {duration_s:.1} s", r.opens);
    assert!(r.applied.len() <= 6, "{} changes: {:?}", r.applied.len(), r.applied);
    println!("flap_45: {} opens, {} changes in {duration_s:.1} s", r.opens, r.applied.len());
}

#[test]
fn never_settling_motion_is_abandoned() {
    let r = run("never_settles");
    assert!(r.applied.is_empty());
    assert!(r.opens <= 3, "{} opens", r.opens);
}

#[test]
fn upside_down_respects_setting() {
    // A seated user flips the phone: the 0→180° motion passes through the R90
    // band, where the head (which stays upright in the world) confirms R90.
    let t = load("upside_down");
    let r = Sim::new(Head::FollowGravity).run(&t);
    // Upside-down disabled (stock phone default): never shows R180, and the
    // final at-rest evaluation is explicitly refused.
    assert_ne!(r.final_rotation, Some(Rotation::R180));
    assert!(r.applied.iter().all(|(_, d)| d.rotation != Rotation::R180));
    assert!(
        r.records.iter().any(|x| matches!(x.outcome, Outcome::Decided { reason: Reason::UpsideDownDisallowed, .. })),
        "{:#?}",
        r.records
    );
    let mut sim = Sim::new(Head::FollowGravity);
    sim.cfg.decision.allow_upside_down = true;
    let r = sim.run(&t);
    assert_eq!(r.final_rotation, Some(Rotation::R180), "{:#?}", r.records);
    assert_eq!(r.applied.last().map(|(_, d)| d.reason), Some(Reason::FaceConfirmedRotation));
}

#[test]
fn slow_rotation_is_evaluated_once_at_rest() {
    let r = run("slow_rotate");
    assert_eq!(r.final_rotation, Some(Rotation::R90));
    assert!(r.opens <= 2, "{} opens: {:#?}", r.opens, r.records);
}

#[test]
fn screen_off_mid_capture_discards() {
    let t = load("rotate_ccw_sitting");
    // Find the onset time from a clean run, then switch the screen off at
    // several points: while opening, after the capture request, just before
    // the classified frame arrives.
    let clean = Sim::new(Head::Fixed(HeadClass::RotatedRight, 0.9)).run(&t);
    let rec = clean.records.iter().find(|x| matches!(x.outcome, Outcome::Decided { .. })).unwrap();
    let frame = rec.frame_ms.unwrap();
    for stop_at in [rec.onset_ms + 20, rec.capture_cmd_ms.unwrap(), frame - 1] {
        let mut sim = Sim::new(Head::Fixed(HeadClass::RotatedRight, 0.9));
        // Screen goes off and stays off: no samples are delivered afterwards
        // (the platform layer unregisters the sensors).
        let mut cut = t.clone();
        cut.samples.retain(|s| s.t_ms < stop_at);
        sim.injected.push((stop_at, Event::Stop));
        let r = sim.run(&cut);
        assert!(r.applied.is_empty(), "stop at {stop_at}: {:?}", r.applied);
        assert_eq!(r.open_at_end, 0, "stop at {stop_at}");
        assert!(r.records.iter().any(|x| x.outcome == Outcome::Aborted(AbortReason::Stopped)));
        assert_eq!(r.final_rotation, Some(Rotation::R0));
    }
}

#[test]
fn camera_failure_falls_back_to_gravity() {
    let t = load("lie_down_left");
    let mut sim = Sim::new(Head::Fixed(HeadClass::Upright, 0.95));
    sim.cam.fail_open = true;
    let r = sim.run(&t);
    // Without the camera the app behaves exactly like normal auto-rotate.
    assert_eq!(r.final_rotation, Some(Rotation::R90));
    assert_eq!(r.applied[0].1.reason, Reason::FallbackCameraUnavailable);
}

#[test]
fn proximity_covered_skips_camera() {
    let t = load("lie_down_left");
    let mut sim = Sim::new(Head::Fixed(HeadClass::Upright, 0.95));
    sim.injected.push((0, Event::Proximity { covered: true }));
    let r = sim.run(&t);
    assert_eq!(r.opens, 0);
    assert_eq!(r.final_rotation, Some(Rotation::R90));
}

#[test]
fn half_folded_skips_camera() {
    let t = load("lie_down_left");
    let mut sim = Sim::new(Head::Fixed(HeadClass::Upright, 0.95));
    sim.injected.push((0, Event::Hinge { degrees: 95.0 }));
    assert_eq!(sim.run(&t).opens, 0);
}

#[test]
fn very_slow_camera_times_out_to_gravity() {
    let t = load("lie_down_left");
    let mut sim = Sim::new(Head::Fixed(HeadClass::Upright, 0.95));
    sim.cam.open_ms = 2_000;
    let r = sim.run(&t);
    assert_eq!(r.final_rotation, Some(Rotation::R90));
    assert_eq!(r.applied[0].1.reason, Reason::FallbackCameraUnavailable);
    assert_eq!(r.open_at_end, 0);
}

#[test]
fn display_rotation_from_outside_is_respected() {
    // An app forces landscape; the user then lies down... the engine starts from R90.
    let t = load("rotate_ccw_sitting");
    let mut sim = Sim::new(Head::Fixed(HeadClass::RotatedRight, 0.9));
    sim.injected.push((0, Event::DisplayRotation(Rotation::R90)));
    let r = sim.run(&t);
    // Already showing what gravity proposes: nothing to do, no camera needed.
    assert!(r.applied.is_empty());
    assert_eq!(r.final_rotation, Some(Rotation::R90));
}

// ----- 2nd verification -------------------------------------------------------

fn face(class: HeadClass, confidence: f32) -> Option<HeadObservation> {
    Some(HeadObservation { class, confidence })
}

fn verifications(r: &SimResult) -> Vec<Reason> {
    r.records
        .iter()
        .filter_map(|x| match x.outcome {
            Outcome::Decided { reason, .. }
                if matches!(
                    reason,
                    Reason::SecondCheckReverted | Reason::SecondCheckConfirmed | Reason::SecondCheckInconclusive
                ) =>
            {
                Some(reason)
            }
            _ => None,
        })
        .collect()
}

/// The reported bug: lying down, the first look fails (blurred / dark frame →
/// gravity fallback) and the screen wrongly turns landscape. With the option on,
/// the second look at rest sees the face still upright and switches back.
#[test]
fn second_verification_switches_back_when_user_is_lying() {
    let t = load("lie_down_left");
    let answers = vec![None, face(HeadClass::Upright, 0.92)];

    let off = Sim::new(seq(answers.clone())).run(&t);
    assert_eq!(off.final_rotation, Some(Rotation::R90), "without the option the mistake stays");

    let mut sim = Sim::new(seq(answers));
    sim.cfg.second_verification = true;
    let r = sim.run(&t);
    check_invariants("lie_down_left + 2nd verification", &r);
    let rotations: Vec<_> = r.applied.iter().map(|(_, d)| (d.rotation, d.reason)).collect();
    assert_eq!(
        rotations,
        vec![(Rotation::R90, Reason::FallbackCameraUnavailable), (Rotation::R0, Reason::SecondCheckReverted)]
    );
    assert_eq!(r.final_rotation, Some(Rotation::R0));
    assert_eq!(r.opens, 2);
    let (t1, t2) = (r.applied[0].0, r.applied[1].0);
    assert!(t2 - t1 >= 700, "second look only once the phone rests in landscape ({} ms)", t2 - t1);
    println!("lying: wrong landscape at {t1} ms, switched back at {t2} ms");
    // No third look and no flip back to landscape afterwards.
    assert_eq!(verifications(&r), vec![Reason::SecondCheckReverted]);
}

/// Also catches a confident-but-wrong first frame taken mid-motion.
#[test]
fn second_verification_fixes_a_wrong_first_frame() {
    let t = load("lie_down_right");
    let mut sim = Sim::new(seq(vec![face(HeadClass::RotatedLeft, 0.8), face(HeadClass::Upright, 0.9)]));
    sim.cfg.second_verification = true;
    let r = sim.run(&t);
    assert_eq!(r.applied.first().map(|(_, d)| d.rotation), Some(Rotation::R270));
    assert_eq!(r.final_rotation, Some(Rotation::R0));
}

#[test]
fn second_verification_keeps_landscape_for_seated_user() {
    let t = load("rotate_ccw_sitting");
    let mut sim = Sim::new(Head::Fixed(HeadClass::RotatedRight, 0.9));
    sim.cfg.second_verification = true;
    let r = sim.run(&t);
    assert_eq!(r.applied.len(), 1);
    assert_eq!(r.final_rotation, Some(Rotation::R90));
    assert_eq!(verifications(&r), vec![Reason::SecondCheckConfirmed]);
    assert_eq!(r.opens, 2, "one extra camera look per landscape rotation");
}

#[test]
fn second_verification_needs_a_confident_face_to_revert() {
    let t = load("lie_down_left");
    for second in [None, face(HeadClass::NoFace, 0.95), face(HeadClass::Upright, 0.5)] {
        let mut sim = Sim::new(seq(vec![None, second]));
        sim.cfg.second_verification = true;
        let r = sim.run(&t);
        assert_eq!(r.final_rotation, Some(Rotation::R90), "{second:?}");
        assert_eq!(verifications(&r), vec![Reason::SecondCheckInconclusive], "{second:?}");
    }
}

#[test]
fn second_verification_is_dropped_when_phone_turns_back() {
    // Portrait → landscape, back to portrait 300 ms later (before the 700 ms delay).
    let t = synth(&[(1000, 0.0), (450, 90.0), (300, 90.0), (450, 0.0), (2000, 0.0)], 0.0, 30.0);
    let mut sim = Sim::new(Head::FollowGravity);
    sim.cfg.second_verification = true;
    let r = sim.run(&t);
    check_invariants("turn back", &r);
    assert!(verifications(&r).is_empty(), "{:#?}", r.records);
    assert_eq!(r.final_rotation, Some(Rotation::R0));
}

#[test]
fn second_verification_only_for_portrait_to_landscape() {
    // Landscape → portrait needs no second look.
    let t = synth(&[(1000, 90.0), (450, 0.0), (2500, 0.0)], 90.0, 30.0);
    let mut sim = Sim::new(Head::FollowGravity);
    sim.cfg.second_verification = true;
    sim.initial = Rotation::R90;
    let r = sim.run(&t);
    assert_eq!(r.final_rotation, Some(Rotation::R0));
    assert!(verifications(&r).is_empty());
    assert_eq!(r.opens, 1);
}

#[test]
fn screen_off_cancels_pending_second_verification() {
    let t = load("rotate_ccw_sitting");
    let mut sim = Sim::new(Head::Fixed(HeadClass::RotatedRight, 0.9));
    sim.cfg.second_verification = true;
    let first = sim.run(&t);
    let decided = first.applied[0].0;
    sim.injected.push((decided + 10, Event::Stop));
    let r = sim.run(&t);
    assert_eq!(r.opens, 1, "{:#?}", r.records);
    assert!(verifications(&r).is_empty());
}

// ----- assist mode ("only intervene when lying down") -------------------------

/// Assist mode releases control after a decision that just follows gravity, so
/// an invariant for normal mode (camera closed, etc.) must still hold, plus:
/// after every activation FaceTilt is either holding (controlling) or has
/// handed control back.
fn control_log(r: &SimResult) -> Vec<(u64, bool)> {
    r.control.clone()
}

/// Sitting and rotating to landscape: FaceTilt seizes at the tilt onset (so the
/// system cannot flip early), confirms the face, then hands control back to the
/// system auto-rotate instead of driving landscape itself.
#[test]
fn assist_seizes_then_releases_for_a_seated_user() {
    let t = load("rotate_ccw_sitting");
    let mut sim = Sim::new(Head::Fixed(HeadClass::RotatedRight, 0.9));
    sim.cfg.assist_mode = true;
    let r = sim.run(&t);
    // One Seize at onset, one Release at the decision; no pinned rotation.
    let seizes = r.control.iter().filter(|(_, s)| *s).count();
    let releases = r.control.iter().filter(|(_, s)| !*s).count();
    assert!(seizes >= 1 && seizes == releases, "balanced seize/release: {:?}", r.control);
    for w in r.control.windows(2) {
        assert_ne!(w[0].1, w[1].1, "seize/release must alternate: {:?}", r.control);
    }
    assert!(r.applied.is_empty(), "assist mode must not drive the landscape rotation itself");
    assert!(r.system_autorotate, "system auto-rotate must be back on: {:?}", r.control);
    assert_eq!(r.open_at_end, 0);
}

/// Lying on your side: FaceTilt seizes, sees the face is still upright, and
/// pins portrait (holds against gravity) — the system stays overridden.
#[test]
fn assist_holds_portrait_when_lying() {
    let t = load("lie_down_left");
    let mut sim = Sim::new(Head::Fixed(HeadClass::Upright, 0.92));
    sim.cfg.assist_mode = true;
    let r = sim.run(&t);
    assert_eq!(r.applied.len(), 1, "{:?}", r.applied);
    assert_eq!(r.applied[0].1.rotation, Rotation::R0);
    assert_eq!(r.applied[0].1.reason, Reason::FaceKeptOrientation);
    assert!(!r.system_autorotate, "while holding, the system auto-rotate stays off");
    // A hold leaves FaceTilt in control: the last control event is a Seize.
    assert_eq!(r.control.last().map(|(_, s)| *s), Some(true), "{:?}", r.control);
}

/// Lying, FaceTilt holds portrait, then the user sits up: gravity returns to
/// portrait and FaceTilt hands control back (re-enables auto-rotate) with no camera.
#[test]
fn assist_releases_when_user_sits_back_up() {
    let t = load("lie_down_then_sit_up");
    let mut sim = Sim::new(Head::Fixed(HeadClass::Upright, 0.92));
    sim.cfg.assist_mode = true;
    let r = sim.run(&t);
    assert_eq!(r.applied.len(), 1, "one pin (portrait) only: {:?}", r.applied);
    assert_eq!(r.applied[0].1.rotation, Rotation::R0);
    // Ends released: system auto-rotate back on once sat up.
    assert!(r.system_autorotate, "auto-rotate must be re-enabled after sitting up: {:?}", r.control);
    assert_eq!(r.opens, 1, "sitting up is detected from gravity alone, no extra camera");
    assert_eq!(r.open_at_end, 0);
}

/// A reversed tilt in assist mode must still release control (auto-rotate back on).
#[test]
fn assist_releases_after_an_aborted_tilt() {
    let t = load("tilt_and_return");
    let mut sim = Sim::new(Head::Fixed(HeadClass::Upright, 0.9));
    sim.cfg.assist_mode = true;
    let r = sim.run(&t);
    assert!(r.applied.is_empty());
    assert!(r.system_autorotate, "control handed back after the tilt reversed: {:?}", r.control);
    for w in control_log(&r).windows(2) {
        assert_ne!(w[0].1, w[1].1, "seize/release must alternate: {:?}", r.control);
    }
}

/// No camera (privacy toggle / failure) in assist mode: behave like plain
/// auto-rotate — never hold, always release.
#[test]
fn assist_without_camera_follows_system_autorotate() {
    let t = load("lie_down_left");
    let mut sim = Sim::new(Head::Unusable);
    sim.cfg.assist_mode = true;
    let r = sim.run(&t);
    assert!(r.applied.is_empty(), "no face → never pin portrait");
    assert!(r.system_autorotate);
}

/// Screen off while holding portrait: control is released so the user's own
/// auto-rotate is restored.
#[test]
fn assist_releases_on_stop() {
    let t = load("lie_down_left");
    let mut sim = Sim::new(Head::Fixed(HeadClass::Upright, 0.92));
    sim.cfg.assist_mode = true;
    let decided = {
        let mut s2 = Sim::new(Head::Fixed(HeadClass::Upright, 0.92));
        s2.cfg.assist_mode = true;
        s2.run(&t).applied[0].0
    };
    sim.injected.push((decided + 200, Event::Stop));
    let mut cut = t.clone();
    cut.samples.retain(|x| x.t_ms < decided + 200);
    let r = sim.run(&cut);
    assert!(r.system_autorotate, "auto-rotate restored on screen-off: {:?}", r.control);
}

// ----- AOSP-grade gravity robustness (proposer integration) -------------------

/// Lying on your back holding the phone over your face (screen facing down,
/// tilt ≈ −55°): this is "overhead" — gravity cannot tell the orientation, so
/// FaceTilt proposes no rotation and never opens the camera, however the phone
/// is turned. (Pixel/AOSP behave the same via WindowOrientationListener.)
#[test]
fn overhead_in_bed_never_rotates_or_opens_camera() {
    // Rotate from portrait to landscape while held overhead at tilt −55°.
    let t = synth(&[(800, 0.0), (500, 90.0), (2000, 90.0)], 0.0, -55.0);
    let sim = Sim::new(Head::FollowGravity);
    let r = sim.run(&t);
    assert_eq!(r.opens, 0, "overhead must not open the camera: {:#?}", r.records);
    assert!(r.applied.is_empty(), "overhead must not rotate: {:?}", r.applied);
    assert_eq!(r.final_rotation, Some(Rotation::R0));
}

/// The strong AOSP hysteresis dead-zone (22.5°) means a steady pose a bit past
/// 45° does not rotate (it would under a naive nearest-quarter rule).
#[test]
fn pose_just_past_45_degrees_does_not_rotate() {
    // Hold at 55° from portrait (past 45°, inside the 67.5° dead-zone).
    let t = synth(&[(800, 0.0), (400, 55.0), (2000, 55.0)], 0.0, 15.0);
    let sim = Sim::new(Head::FollowGravity);
    let r = sim.run(&t);
    assert!(r.applied.is_empty(), "55° is inside the hysteresis dead-zone: {:?}", r.applied);
    assert!(r.opens <= 1, "at most a single pre-warm, no rotation: {}", r.opens);
}

/// Device resting flat on a table and spun around: no camera, no rotation.
#[test]
fn flat_and_spun_stays_put() {
    let t = synth(&[(1000, 0.0), (800, 90.0), (800, 180.0), (1000, 180.0)], 0.0, 86.0);
    let sim = Sim::new(Head::FollowGravity);
    let r = sim.run(&t);
    assert_eq!(r.opens, 0, "{:#?}", r.records);
    assert!(r.applied.is_empty());
}
