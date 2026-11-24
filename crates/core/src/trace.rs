//! Sensor trace format shared by the test fixtures, the trace generator
//! (`tools/gen_traces.py`) and the debug-build logcat recorder
//! (`scripts/record_trace.sh`).
//!
//! ```text
//! # comment lines start with '#'; "# key: value" lines are metadata
//! t_ms,gx,gy,gz
//! 0,0.012,8.51,4.87
//! 20,0.013,8.52,4.86
//! ```
//! Values are gravity "up" vectors in m/s² in the device frame. Contains no
//! camera data of any kind.

/// One sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// Milliseconds.
    pub t_ms: u64,
    /// Up vector (m/s²).
    pub g: [f32; 3],
}

/// A parsed trace.
#[derive(Debug, Clone, Default)]
pub struct Trace {
    /// `# key: value` metadata, in file order.
    pub meta: Vec<(String, String)>,
    /// Samples in time order.
    pub samples: Vec<Sample>,
}

impl Trace {
    /// Metadata lookup.
    pub fn meta(&self, key: &str) -> Option<&str> {
        self.meta.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

/// Parses a trace; returns a description of the first malformed line on error.
pub fn parse(text: &str) -> Result<Trace, String> {
    let mut trace = Trace::default();
    let mut last_t = None;
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(c) = line.strip_prefix('#') {
            if let Some((k, v)) = c.split_once(':') {
                trace.meta.push((k.trim().to_string(), v.trim().to_string()));
            }
            continue;
        }
        if line.starts_with("t_ms") {
            continue;
        }
        let mut it = line.split(',');
        let mut field = || -> Result<&str, String> { it.next().ok_or(format!("line {}: missing field", n + 1)) };
        let t_ms: u64 = field()?.trim().parse().map_err(|e| format!("line {}: {e}", n + 1))?;
        let mut g = [0.0f32; 3];
        for v in &mut g {
            *v = field()?.trim().parse().map_err(|e| format!("line {}: {e}", n + 1))?;
        }
        if last_t.is_some_and(|p| t_ms < p) {
            return Err(format!("line {}: time goes backwards", n + 1));
        }
        last_t = Some(t_ms);
        trace.samples.push(Sample { t_ms, g });
    }
    Ok(trace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses() {
        let t =
            parse("# scenario: x\n# head: Upright\nt_ms,gx,gy,gz\n0,0,9.8,0\n20, 0.1, 9.7, 0.2\n").unwrap_or_default();
        assert_eq!(t.meta("scenario"), Some("x"));
        assert_eq!(t.samples.len(), 2);
        assert_eq!(t.samples[1].g, [0.1, 9.7, 0.2]);
        assert!(parse("0,1,2\n").is_err());
        assert!(parse("20,0,0,0\n10,0,0,0\n").is_err());
    }
}
