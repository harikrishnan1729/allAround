/// First-order low-pass (exponential moving average) on a 3-vector with a
/// time-constant parameterisation, so it behaves the same at any sample rate.
#[derive(Debug, Clone)]
pub struct LowPass {
    tau_ms: f32,
    state: Option<[f32; 3]>,
}

impl LowPass {
    /// New filter with time constant `tau_ms` (0 disables filtering).
    pub fn new(tau_ms: f32) -> Self {
        LowPass { tau_ms: tau_ms.max(0.0), state: None }
    }

    /// Feeds one sample taken `dt_ms` after the previous one; returns the filtered value.
    pub fn update(&mut self, v: [f32; 3], dt_ms: f32) -> [f32; 3] {
        let out = match self.state {
            None => v,
            Some(s) => {
                let alpha = if self.tau_ms <= 0.0 { 1.0 } else { dt_ms / (self.tau_ms + dt_ms) };
                [s[0] + alpha * (v[0] - s[0]), s[1] + alpha * (v[1] - s[1]), s[2] + alpha * (v[2] - s[2])]
            }
        };
        self.state = Some(out);
        out
    }

    /// Current output, if any sample was seen.
    pub fn value(&self) -> Option<[f32; 3]> {
        self.state
    }

    /// Forgets the state.
    pub fn reset(&mut self) {
        self.state = None;
    }
}

/// Angle between two vectors in degrees (0 if either is ~zero).
pub fn angle_between_deg(a: [f32; 3], b: [f32; 3]) -> f32 {
    let dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let na = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    let nb = (b[0] * b[0] + b[1] * b[1] + b[2] * b[2]).sqrt();
    if na < 1e-6 || nb < 1e-6 {
        return 0.0;
    }
    (dot / (na * nb)).clamp(-1.0, 1.0).acos().to_degrees()
}

/// In-plane orientation angle of `v` if its screen-plane projection is large
/// enough to be meaningful (≥ 3 m/s², i.e. tilt below ~72°).
pub(crate) fn inplane_angle(v: [f32; 3]) -> Option<f32> {
    let p = (v[0] * v[0] + v[1] * v[1]).sqrt();
    (p >= 3.0).then(|| facetilt_decision::orientation_angle_deg(v[0], v[1]))
}

pub(crate) fn circular_distance(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(360.0);
    if d > 180.0 {
        360.0 - d
    } else {
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowpass_converges_and_is_rate_independent() {
        for dt in [5.0f32, 20.0, 50.0] {
            let mut f = LowPass::new(40.0);
            f.update([0.0, 9.81, 0.0], dt);
            let mut t = 0.0;
            let mut out = [0.0; 3];
            while t < 200.0 {
                out = f.update([9.81, 0.0, 0.0], dt);
                t += dt;
            }
            // After 5 time constants we are within ~2% (discretisation varies with dt).
            assert!(out[0] > 9.3 && out[1] < 0.6, "dt={dt}: {out:?}");
        }
    }

    #[test]
    fn angles() {
        assert!((angle_between_deg([0.0, 1.0, 0.0], [1.0, 0.0, 0.0]) - 90.0).abs() < 1e-3);
        assert_eq!(angle_between_deg([0.0; 3], [1.0, 0.0, 0.0]), 0.0);
        assert!(inplane_angle([0.0, 0.5, 9.8]).is_none());
        assert!((inplane_angle([9.8, 0.0, 0.0]).unwrap_or(0.0) - 90.0).abs() < 1e-3);
        assert!((circular_distance(350.0, 10.0) - 20.0).abs() < 1e-4);
    }
}
