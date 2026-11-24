//! Discrete-event simulator: replays a sensor trace through the engine with a
//! simulated camera (open latency, first-frame delay, frame interval,
//! inference time) and records every command.
#![allow(dead_code, clippy::unwrap_used, clippy::panic)]

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use facetilt_core::decision::{orientation_angle_deg, Decision, HeadClass, HeadObservation, Rotation};
use facetilt_core::trace::{self, Sample, Trace};
use facetilt_core::{ActivationRecord, Command, ControlAction, Engine, EngineConfig, Event};

pub fn load(name: &str) -> Trace {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/traces").join(format!("{name}.csv"));
    let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    trace::parse(&text).unwrap()
}

#[derive(Debug, Clone, Copy)]
pub struct CamModel {
    /// OpenCamera → CameraOpened (device open + session + repeating request).
    pub open_ms: u64,
    /// CameraOpened → first frame.
    pub first_frame_ms: u64,
    pub frame_interval_ms: u64,
    pub inference_us: u32,
    pub fail_open: bool,
}

impl Default for CamModel {
    /// Conservative mid-range numbers (see README "Latency budget").
    fn default() -> Self {
        CamModel { open_ms: 110, first_frame_ms: 45, frame_interval_ms: 33, inference_us: 2_500, fail_open: false }
    }
}

pub enum Head {
    Fixed(HeadClass, f32),
    /// The user sits upright: the head always looks "upright in the world",
    /// i.e. it matches the nearest gravity orientation.
    FollowGravity,
    Unusable,
    /// One answer per camera look, in order (the last one repeats).
    Sequence(Vec<Option<HeadObservation>>, Cell<usize>),
}

impl Head {
    pub fn from_meta(s: &str) -> Head {
        match s {
            "Upright" => Head::Fixed(HeadClass::Upright, 0.93),
            "RotatedLeft" => Head::Fixed(HeadClass::RotatedLeft, 0.9),
            "RotatedRight" => Head::Fixed(HeadClass::RotatedRight, 0.9),
            "UpsideDown" => Head::Fixed(HeadClass::UpsideDown, 0.9),
            "NoFace" => Head::Fixed(HeadClass::NoFace, 0.95),
            "follow" => Head::FollowGravity,
            other => panic!("unknown head {other}"),
        }
    }

    fn observe(&self, g: [f32; 3]) -> Option<HeadObservation> {
        match *self {
            Head::Fixed(class, confidence) => Some(HeadObservation { class, confidence }),
            Head::Unusable => None,
            Head::Sequence(ref v, ref i) => {
                let k = i.get();
                i.set(k + 1);
                v.get(k).or(v.last()).copied().flatten()
            }
            Head::FollowGravity => {
                let a = orientation_angle_deg(g[0], g[1]);
                let class = match Rotation::from_index(((a + 45.0) / 90.0) as u32) {
                    Rotation::R0 => HeadClass::Upright,
                    Rotation::R90 => HeadClass::RotatedRight,
                    Rotation::R180 => HeadClass::UpsideDown,
                    Rotation::R270 => HeadClass::RotatedLeft,
                };
                Some(HeadObservation { class, confidence: 0.9 })
            }
        }
    }
}

pub struct Sim {
    pub cfg: EngineConfig,
    pub cam: CamModel,
    pub head: Head,
    pub initial: Rotation,
    /// (time, event) injected as-is (Stop, Proximity, …).
    pub injected: Vec<(u64, Event)>,
}

#[derive(Debug, Default)]
pub struct SimResult {
    pub applied: Vec<(u64, Decision)>,
    pub commands: Vec<(u64, Command)>,
    pub final_rotation: Option<Rotation>,
    pub opens: usize,
    pub open_at_end: usize,
    pub camera_on_ms: u64,
    pub records: Vec<ActivationRecord>,
    pub max_camera_session_ms: u64,
    /// Assist mode: the system auto-rotate is on (FaceTilt released control).
    pub system_autorotate: bool,
    /// Assist mode: timeline of Seize (true) / Release (false) commands.
    pub control: Vec<(u64, bool)>,
}

impl Sim {
    pub fn new(head: Head) -> Self {
        Sim { cfg: EngineConfig::default(), cam: CamModel::default(), head, initial: Rotation::R0, injected: vec![] }
    }

    pub fn run(&self, trace: &Trace) -> SimResult {
        let t0 = trace.samples.first().map_or(0, |s| s.t_ms);
        let mut e = Engine::new(self.cfg, self.initial, t0);
        let mut res = SimResult { system_autorotate: self.cfg.assist_mode, ..Default::default() };
        // Pending camera events keyed by (time, seq).
        let mut pending: BTreeMap<(u64, u64), Event> = BTreeMap::new();
        let mut seq = 0u64;
        let mut opened_at: BTreeMap<u32, u64> = BTreeMap::new();
        let mut alive: BTreeSet<u32> = BTreeSet::new();
        let mut session_start: BTreeMap<u32, u64> = BTreeMap::new();
        let mut injected = self.injected.clone();
        injected.sort_by_key(|(t, _)| *t);
        let mut inj = injected.into_iter().peekable();
        let mut samples = trace.samples.iter().peekable();
        let mut last_g = [0.0, 9.81, 0.0];
        let end = trace.samples.last().map_or(0, |s| s.t_ms) + 3_000;

        loop {
            let ts = samples.peek().map(|s| s.t_ms);
            let tc = pending.keys().next().map(|k| k.0);
            let td = e.next_deadline();
            let ti = inj.peek().map(|(t, _)| *t);
            let Some(now) = [ts, tc, td, ti].into_iter().flatten().min() else { break };
            if now > end {
                break;
            }
            let ev = if tc == Some(now) {
                let k = *pending.keys().next().unwrap();
                pending.remove(&k).unwrap()
            } else if ti == Some(now) {
                inj.next().unwrap().1
            } else if td == Some(now) {
                Event::Tick
            } else {
                let s = samples.next().unwrap();
                last_g = s.g;
                Event::Gravity { v: s.g }
            };
            for c in e.handle(now, ev).iter() {
                res.commands.push((now, c));
                match c {
                    Command::OpenCamera { session } => {
                        res.opens += 1;
                        alive.insert(session);
                        session_start.insert(session, now);
                        seq += 1;
                        if self.cam.fail_open {
                            pending.insert((now + 40, seq), Event::CameraError { session });
                            alive.remove(&session);
                        } else {
                            let t = now + self.cam.open_ms;
                            opened_at.insert(session, t);
                            pending.insert((t, seq), Event::CameraOpened { session });
                        }
                    }
                    Command::CaptureFrame { session } => {
                        let first = opened_at[&session] + self.cam.first_frame_ms;
                        let frame_t = if now <= first {
                            first
                        } else {
                            let k = (now - first).div_ceil(self.cam.frame_interval_ms);
                            first + k * self.cam.frame_interval_ms
                        };
                        let done = frame_t + u64::from(self.cam.inference_us).div_ceil(1000);
                        seq += 1;
                        let head = self.head.observe(last_g);
                        pending.insert(
                            (done, seq),
                            Event::FrameClassified { session, head, inference_us: self.cam.inference_us },
                        );
                    }
                    Command::CloseCamera { session } => {
                        alive.remove(&session);
                        if let Some(s) = session_start.remove(&session) {
                            res.max_camera_session_ms = res.max_camera_session_ms.max(now - s);
                        }
                    }
                    Command::Apply(d) => res.applied.push((now, d)),
                    Command::Control(a) => {
                        let seize = a == ControlAction::Seize;
                        res.system_autorotate = !seize;
                        res.control.push((now, seize));
                    }
                }
            }
        }
        res.final_rotation = Some(e.current());
        res.open_at_end = alive.len();
        res.camera_on_ms = e.stats(end).camera_on_ms;
        res.records = e.metrics().records().copied().collect();
        res
    }
}

pub fn seq(answers: Vec<Option<HeadObservation>>) -> Head {
    Head::Sequence(answers, Cell::new(0))
}

/// Noise-free trace: `(duration_ms, target_angle_deg)` segments with a
/// minimum-jerk profile, 20 ms sampling, constant tilt.
pub fn synth(segments: &[(u64, f32)], start_deg: f32, tilt_deg: f32) -> Trace {
    let mut samples = Vec::new();
    let (mut t, mut a) = (0u64, start_deg);
    for &(dur, target) in segments {
        let a0 = a;
        let mut u_t = 0;
        while u_t < dur {
            let u = u_t as f32 / dur as f32;
            let s = 10.0 * u.powi(3) - 15.0 * u.powi(4) + 6.0 * u.powi(5);
            a = a0 + (target - a0) * s;
            let (ar, tr) = (a.to_radians(), tilt_deg.to_radians());
            let g = [9.81 * tr.cos() * ar.sin(), 9.81 * tr.cos() * ar.cos(), 9.81 * tr.sin()];
            samples.push(Sample { t_ms: t, g });
            t += 20;
            u_t += 20;
        }
        a = target;
    }
    Trace { meta: vec![], samples }
}
