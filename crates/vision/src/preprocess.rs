//! Per-image standardisation (mirrored in `training/facetilt/boxfilter.py`).

use crate::{Thumb, PIXELS};

/// Frames whose luma standard deviation is below this (in 8-bit levels) carry
/// no usable structure: lens covered, camera privacy toggle (black frames),
/// pitch-dark room. They are reported as "unusable" instead of being classified.
pub const MIN_STD: f32 = 3.0;

/// Writes `(p − mean) / std` into `out`. Returns `false` (and leaves `out`
/// zeroed) for near-uniform frames.
///
/// Arithmetic: mean is exact (`sum / 1024` with `sum < 2²⁴`), variance is
/// accumulated in f64 and rounded once to f32, normalisation is f32 — the
/// same sequence numpy uses in the Python port.
pub fn standardize(t: &Thumb, out: &mut [f32; PIXELS]) -> bool {
    let sum: u32 = t.iter().map(|&p| u32::from(p)).sum();
    let mean = sum as f32 / PIXELS as f32;
    let var: f64 = t
        .iter()
        .map(|&p| {
            let d = f64::from(p) - f64::from(mean);
            d * d
        })
        .sum::<f64>()
        / PIXELS as f64;
    let std = var.sqrt() as f32;
    if std < MIN_STD {
        out.fill(0.0);
        return false;
    }
    for (o, &p) in out.iter_mut().zip(t.iter()) {
        *o = (f32::from(p) - mean) / std;
    }
    true
}

/// Numerically stable softmax over logits.
pub fn softmax<const N: usize>(logits: &[f32; N]) -> [f32; N] {
    let m = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut e = [0.0f32; N];
    let mut s = 0.0f32;
    for (o, &l) in e.iter_mut().zip(logits.iter()) {
        *o = (l - m).exp();
        s += *o;
    }
    for o in &mut e {
        *o /= s;
    }
    e
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_frames_are_rejected() {
        let mut out = [1.0f32; PIXELS];
        assert!(!standardize(&[0u8; PIXELS], &mut out));
        assert!(out.iter().all(|&v| v == 0.0));
        let mut t = [100u8; PIXELS];
        t[0] = 102; // tiny noise
        assert!(!standardize(&t, &mut out));
    }

    #[test]
    fn zero_mean_unit_std() {
        let mut t = [0u8; PIXELS];
        for (i, p) in t.iter_mut().enumerate() {
            *p = (i % 256) as u8;
        }
        let mut out = [0.0f32; PIXELS];
        assert!(standardize(&t, &mut out));
        let mean: f32 = out.iter().sum::<f32>() / PIXELS as f32;
        let var: f32 = out.iter().map(|v| v * v).sum::<f32>() / PIXELS as f32;
        assert!(mean.abs() < 1e-5 && (var - 1.0).abs() < 1e-4);
    }

    #[test]
    fn softmax_sums_to_one() {
        let p = softmax(&[1.0, 2.0, 3.0, -50.0, 1000.0]);
        assert!((p.iter().sum::<f32>() - 1.0).abs() < 1e-6);
        assert!(p[4] > 0.999);
    }
}
