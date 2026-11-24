#![allow(clippy::unwrap_used)]
//! Train/runtime parity: the Rust image path must reproduce the Python
//! training pipeline exactly, and tract must agree with onnxruntime on the
//! shipped int8 model. Fixtures: `training/facetilt/export_golden.py`.

use facetilt_vision::preprocess::standardize;
use facetilt_vision::{box_downsample, to_mirror_view, Classifier, Thumb, PIXELS};

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> &'a [u8] {
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        a
    }
    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take(4).try_into().unwrap())
    }
    fn u8(&mut self) -> u8 {
        self.take(1)[0]
    }
    fn f32s(&mut self, n: usize) -> Vec<f32> {
        self.take(4 * n).chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect()
    }
    fn thumb(&mut self) -> Thumb {
        self.take(PIXELS).try_into().unwrap()
    }
}

#[test]
fn downsample_orient_standardize_match_python_bit_for_bit() {
    let data = include_bytes!("fixtures/downsample.bin");
    let mut r = Reader(data);
    let n = r.u32();
    assert!(n >= 5);
    for _ in 0..n {
        let (w, h, stride) = (r.u32() as usize, r.u32() as usize, r.u32() as usize);
        let plane = r.take(h * stride);
        let expected = r.thumb();
        let mut got = [0u8; PIXELS];
        box_downsample(plane, w, h, stride, &mut got).unwrap();
        assert_eq!(got, expected, "box filter {w}x{h} stride {stride}");
        for o in [0, 90, 180, 270] {
            for m in [false, true] {
                let want = r.thumb();
                let mut v = [0u8; PIXELS];
                to_mirror_view(&got, o, m, &mut v);
                assert_eq!(v, want, "orientation {o} mirror {m} ({w}x{h})");
            }
        }
        let want = r.f32s(PIXELS);
        let mut f = [0f32; PIXELS];
        assert!(standardize(&got, &mut f));
        let max = f.iter().zip(&want).map(|(a, b)| (a - b).abs()).fold(0f32, f32::max);
        assert!(max <= 1e-6, "standardize differs by {max}");
    }
    assert!(r.0.is_empty());
}

#[test]
fn tract_matches_onnxruntime_and_reads_the_mean_face() {
    let data = include_bytes!("fixtures/classifier.bin");
    let mut r = Reader(data);
    let n = r.u32() as usize;
    let c = Classifier::load_embedded().unwrap();
    let mut worst = 0f32;
    for i in 0..n {
        let expected = r.u8();
        let ort_pred = r.u8();
        let ort_logits = r.f32s(5);
        let mut thumb = r.thumb();
        let mut norm = [0f32; PIXELS];
        assert!(standardize(&thumb, &mut norm));
        let logits = c.logits_for_test(&norm).unwrap();
        let diff = logits.iter().zip(&ort_logits).map(|(a, b)| (a - b).abs()).fold(0f32, f32::max);
        worst = worst.max(diff);
        let p = c.classify(&mut thumb).unwrap().unwrap();
        assert_eq!(p.observation.class as u8, ort_pred, "sample {i}: tract vs onnxruntime argmax");
        assert!(thumb.iter().all(|&v| v == 0), "thumbnail not zeroed");
        if i < 4 {
            // Mean face at the 4 rotations must be classified correctly and confidently.
            assert_eq!(p.observation.class as u8, expected, "mean face rotation {i}: {:?}", p.probs);
            assert!(p.observation.confidence >= 0.7, "mean face rotation {i}: {:?}", p.probs);
        }
    }
    // Both runtimes execute the same QDQ graph; small requantisation rounding
    // differences are allowed, class decisions must agree (checked above).
    assert!(worst < 0.25, "max logit difference tract vs onnxruntime: {worst}");
    println!("max |tract - onnxruntime| logit difference: {worst:.5}");
}
