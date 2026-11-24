//! Times the full per-frame image path (downsample → orient → classify) on
//! synthetic 320×240 luma planes. Single-threaded, like the app.
//!
//! Host:    cargo run --release -p facetilt-vision --example bench_inference
//! Device:  cargo ndk -t arm64-v8a build --release -p facetilt-vision --example bench_inference
//!          adb push target/aarch64-linux-android/release/examples/bench_inference /data/local/tmp/
//!          adb shell /data/local/tmp/bench_inference
#![allow(clippy::unwrap_used)]

use std::time::Instant;

use facetilt_vision::{box_downsample, to_mirror_view, Classifier, PIXELS};

fn main() {
    let t0 = Instant::now();
    let c = Classifier::load_embedded().unwrap();
    println!("model load + optimise: {:.1} ms", t0.elapsed().as_secs_f64() * 1e3);
    let (w, h) = (320usize, 240usize);
    let mut plane = vec![0u8; w * h];
    let mut seed = 0x1234_5678u32;
    let mut ds = Vec::new();
    let mut inf = Vec::new();
    for i in 0..500 {
        for p in plane.iter_mut() {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            *p = (seed >> 24) as u8 / 2 + (i % 100) as u8;
        }
        let t = Instant::now();
        let mut thumb = [0u8; PIXELS];
        box_downsample(&plane, w, h, w, &mut thumb).unwrap();
        let mut view = [0u8; PIXELS];
        to_mirror_view(&thumb, 270, true, &mut view);
        ds.push(t.elapsed().as_secs_f64() * 1e6);
        let t = Instant::now();
        let _ = c.classify(&mut view).unwrap();
        inf.push(t.elapsed().as_secs_f64() * 1e6);
    }
    for (name, v) in [("downsample+orient", &mut ds), ("inference", &mut inf)] {
        v.sort_by(|a, b| a.total_cmp(b));
        let p = |q: f64| v[((v.len() - 1) as f64 * q) as usize];
        println!("{name:>18}: p50 {:7.1} µs  p95 {:7.1} µs  max {:7.1} µs", p(0.5), p(0.95), p(1.0));
    }
}
