//! Stage timing and battery counters. Contains timestamps and counts only —
//! never image data.

use facetilt_decision::Reason;

use crate::engine::AbortReason;

/// How an activation (tilt onset or re-evaluation) ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Outcome {
    /// Still running.
    #[default]
    Pending,
    /// A decision was taken (`changed` = orientation changed).
    Decided {
        /// Whether the orientation changed.
        changed: bool,
        /// Decision reason.
        reason: Reason,
    },
    /// Settled without any orientation to decide (same orientation or flat).
    NoChange,
    /// Abandoned.
    Aborted(AbortReason),
}

/// Timestamps (engine clock, ms) of one activation's stages. `None` = stage
/// did not happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ActivationRecord {
    /// Tilt onset (or re-evaluation start).
    pub onset_ms: u64,
    /// OpenCamera issued.
    pub open_cmd_ms: Option<u64>,
    /// Camera device + session ready, frames flowing.
    pub opened_ms: Option<u64>,
    /// Tilt settled.
    pub settle_ms: Option<u64>,
    /// CaptureFrame issued.
    pub capture_cmd_ms: Option<u64>,
    /// Classified frame received.
    pub frame_ms: Option<u64>,
    /// Inference duration reported by the vision stage.
    pub inference_us: Option<u32>,
    /// Decision taken.
    pub decision_ms: Option<u64>,
    /// Time Kotlin needed to apply the rotation (reported back over JNI).
    pub apply_us: Option<u32>,
    /// Outcome.
    pub outcome: Outcome,
}

impl ActivationRecord {
    /// Onset → decision latency.
    pub fn onset_to_decision_ms(&self) -> Option<u64> {
        self.decision_ms.map(|d| d.saturating_sub(self.onset_ms))
    }
}

const RING: usize = 16;
const INFER_RING: usize = 128;

/// Rolling counters exported to the debug screen.
#[derive(Debug, Clone)]
pub struct Metrics {
    started_ms: u64,
    camera_open_since: Option<u64>,
    camera_on_ms_total: u64,
    camera_opens: u64,
    evaluations: u64,
    decisions_changed: u64,
    decisions_kept_by_face: u64,
    fallbacks: u64,
    aborts: u64,
    verifications: u64,
    reverts: u64,
    records: [ActivationRecord; RING],
    record_count: usize,
    inference_us: [u32; INFER_RING],
    inference_count: usize,
}

/// Plain-number snapshot for the debug screen (crosses JNI as a `long[]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StatsSnapshot {
    /// Time since the engine started.
    pub uptime_ms: u64,
    /// Total camera-on time.
    pub camera_on_ms: u64,
    /// Number of camera opens.
    pub camera_opens: u64,
    /// Number of evaluations (tilt onsets + re-evaluations).
    pub evaluations: u64,
    /// Decisions that changed the orientation.
    pub decisions_changed: u64,
    /// Decisions where the face kept the orientation.
    pub decisions_kept_by_face: u64,
    /// Decisions that fell back to gravity.
    pub fallbacks: u64,
    /// Aborted activations.
    pub aborts: u64,
    /// Inference p50 (µs).
    pub inference_p50_us: u64,
    /// Inference p95 (µs).
    pub inference_p95_us: u64,
    /// Last decided activation: onset → camera opened (ms), or -1.
    pub last_open_ms: i64,
    /// Last decided activation: onset → classified frame (ms), or -1.
    pub last_frame_ms: i64,
    /// Last decided activation: inference (µs), or -1.
    pub last_inference_us: i64,
    /// Last decided activation: onset → decision (ms), or -1.
    pub last_decision_ms: i64,
    /// Last decided activation: apply (µs), or -1.
    pub last_apply_us: i64,
    /// Last decision reason (as `Reason as i64`), or -1.
    pub last_reason: i64,
    /// 2nd verifications performed.
    pub verifications: u64,
    /// 2nd verifications that switched back to portrait.
    pub reverts: u64,
}

impl StatsSnapshot {
    /// Number of fields in [`StatsSnapshot::to_array`]. Must match `EngineStats.kt`.
    pub const LEN: usize = 18;

    /// Fixed-layout array for JNI.
    pub fn to_array(&self) -> [i64; Self::LEN] {
        let u = |v: u64| i64::try_from(v).unwrap_or(i64::MAX);
        [
            u(self.uptime_ms),
            u(self.camera_on_ms),
            u(self.camera_opens),
            u(self.evaluations),
            u(self.decisions_changed),
            u(self.decisions_kept_by_face),
            u(self.fallbacks),
            u(self.aborts),
            u(self.inference_p50_us),
            u(self.inference_p95_us),
            self.last_open_ms,
            self.last_frame_ms,
            self.last_inference_us,
            self.last_decision_ms,
            self.last_apply_us,
            self.last_reason,
            u(self.verifications),
            u(self.reverts),
        ]
    }
}

impl Metrics {
    /// New metrics starting at `now_ms`.
    pub fn new(now_ms: u64) -> Self {
        Metrics {
            started_ms: now_ms,
            camera_open_since: None,
            camera_on_ms_total: 0,
            camera_opens: 0,
            evaluations: 0,
            decisions_changed: 0,
            decisions_kept_by_face: 0,
            fallbacks: 0,
            aborts: 0,
            verifications: 0,
            reverts: 0,
            records: [ActivationRecord::default(); RING],
            record_count: 0,
            inference_us: [0; INFER_RING],
            inference_count: 0,
        }
    }

    pub(crate) fn camera_opened_cmd(&mut self, now_ms: u64) {
        if self.camera_open_since.is_none() {
            self.camera_open_since = Some(now_ms);
            self.camera_opens += 1;
        }
    }

    pub(crate) fn camera_closed_cmd(&mut self, now_ms: u64) {
        if let Some(t) = self.camera_open_since.take() {
            self.camera_on_ms_total += now_ms.saturating_sub(t);
        }
    }

    pub(crate) fn inference(&mut self, us: u32) {
        self.inference_us[self.inference_count % INFER_RING] = us;
        self.inference_count += 1;
    }

    pub(crate) fn finish(&mut self, rec: ActivationRecord) {
        self.evaluations += 1;
        match rec.outcome {
            Outcome::Decided { changed, reason } => {
                if changed {
                    self.decisions_changed += 1;
                }
                match reason {
                    Reason::FaceKeptOrientation => self.decisions_kept_by_face += 1,
                    Reason::SecondCheckReverted => {
                        self.verifications += 1;
                        self.reverts += 1;
                    }
                    Reason::SecondCheckConfirmed | Reason::SecondCheckInconclusive => self.verifications += 1,
                    Reason::FallbackAmbiguous
                    | Reason::FallbackCameraUnavailable
                    | Reason::FallbackLowConfidence
                    | Reason::FallbackNoFace => self.fallbacks += 1,
                    _ => {}
                }
            }
            Outcome::Aborted(_) => self.aborts += 1,
            Outcome::NoChange | Outcome::Pending => {}
        }
        self.records[self.record_count % RING] = rec;
        self.record_count += 1;
    }

    /// Attaches the Kotlin-measured apply time to the most recent decided record.
    pub fn report_apply(&mut self, apply_us: u32) {
        if self.record_count == 0 {
            return;
        }
        let i = (self.record_count - 1) % RING;
        if matches!(self.records[i].outcome, Outcome::Decided { changed: true, .. }) {
            self.records[i].apply_us = Some(apply_us);
        }
    }

    /// Most recent records, newest last.
    pub fn records(&self) -> impl Iterator<Item = &ActivationRecord> {
        let n = self.record_count.min(RING);
        let start = self.record_count - n;
        (start..self.record_count).map(move |i| &self.records[i % RING])
    }

    fn percentile(&self, p: f32) -> u64 {
        let n = self.inference_count.min(INFER_RING);
        if n == 0 {
            return 0;
        }
        let mut v = self.inference_us;
        let s = &mut v[..n];
        s.sort_unstable();
        let idx = (((n - 1) as f32) * p).round() as usize;
        u64::from(s[idx.min(n - 1)])
    }

    /// Snapshot at `now_ms`.
    pub fn snapshot(&self, now_ms: u64) -> StatsSnapshot {
        let open_now = self.camera_open_since.map_or(0, |t| now_ms.saturating_sub(t));
        let last = self.records().filter(|r| matches!(r.outcome, Outcome::Decided { .. })).last().copied();
        let rel = |t: Option<u64>| -> i64 {
            match (last, t) {
                (Some(r), Some(t)) => i64::try_from(t.saturating_sub(r.onset_ms)).unwrap_or(-1),
                _ => -1,
            }
        };
        StatsSnapshot {
            uptime_ms: now_ms.saturating_sub(self.started_ms),
            camera_on_ms: self.camera_on_ms_total + open_now,
            camera_opens: self.camera_opens,
            evaluations: self.evaluations,
            decisions_changed: self.decisions_changed,
            decisions_kept_by_face: self.decisions_kept_by_face,
            fallbacks: self.fallbacks,
            aborts: self.aborts,
            inference_p50_us: self.percentile(0.50),
            inference_p95_us: self.percentile(0.95),
            last_open_ms: rel(last.and_then(|r| r.opened_ms)),
            last_frame_ms: rel(last.and_then(|r| r.frame_ms)),
            last_inference_us: last.and_then(|r| r.inference_us).map_or(-1, i64::from),
            last_decision_ms: rel(last.and_then(|r| r.decision_ms)),
            last_apply_us: last.and_then(|r| r.apply_us).map_or(-1, i64::from),
            last_reason: match last.map(|r| r.outcome) {
                Some(Outcome::Decided { reason, .. }) => reason as i64,
                _ => -1,
            },
            verifications: self.verifications,
            reverts: self.reverts,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_and_camera_time() {
        let mut m = Metrics::new(0);
        for us in 1..=100u32 {
            m.inference(us * 10);
        }
        let s = m.snapshot(0);
        assert_eq!(s.inference_p50_us, 510);
        assert_eq!(s.inference_p95_us, 950);
        m.camera_opened_cmd(100);
        m.camera_opened_cmd(150); // idempotent
        assert_eq!(m.snapshot(400).camera_on_ms, 300);
        m.camera_closed_cmd(500);
        assert_eq!(m.snapshot(9_000).camera_on_ms, 400);
        assert_eq!(m.snapshot(9_000).camera_opens, 1);
    }

    #[test]
    fn ring_keeps_latest() {
        let mut m = Metrics::new(0);
        for i in 0..40u64 {
            m.finish(ActivationRecord { onset_ms: i, outcome: Outcome::NoChange, ..Default::default() });
        }
        let v: Vec<u64> = m.records().map(|r| r.onset_ms).collect();
        assert_eq!(v.len(), RING);
        assert_eq!(v.first(), Some(&24));
        assert_eq!(v.last(), Some(&39));
        assert_eq!(m.snapshot(0).to_array().len(), StatsSnapshot::LEN);
    }
}
