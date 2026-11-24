//! Sensor-frame → mirror-view transform on 48×48 thumbnails.
//!
//! `CameraCharacteristics.SENSOR_ORIENTATION` is "the clockwise angle through
//! which the output image needs to be rotated to be upright on the device
//! screen in its native orientation". After that rotation a front-camera image
//! is upright but not mirrored; flipping it horizontally gives the selfie
//! "mirror view", in which the direction of the user's head-top on the image
//! equals its direction in the device frame (a plane mirror preserves in-plane
//! positions). The classifier is trained in that frame, so its class maps
//! directly to a display rotation (see `HeadClass::content_rotation`).

use crate::{Thumb, SIDE};

/// Rotates `src` clockwise by `sensor_orientation_deg` (rounded to a multiple
/// of 90°) and mirrors horizontally if `mirror` (front-facing camera).
pub fn to_mirror_view(src: &Thumb, sensor_orientation_deg: i32, mirror: bool, out: &mut Thumb) {
    const N: usize = SIDE;
    let quarter = ((sensor_orientation_deg.rem_euclid(360) + 45) / 90) % 4;
    for r in 0..N {
        for c in 0..N {
            // Column in the rotated (pre-mirror) image that lands at (r, c).
            let cc = if mirror { N - 1 - c } else { c };
            let (sr, sc) = match quarter {
                0 => (r, cc),
                1 => (N - 1 - cc, r),
                2 => (N - 1 - r, N - 1 - cc),
                _ => (cc, N - 1 - r),
            };
            out[r * N + c] = src[sr * N + sc];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PIXELS;

    fn marker(r: usize, c: usize) -> Thumb {
        let mut t = [0u8; PIXELS];
        t[r * SIDE + c] = 255;
        t
    }

    fn find(t: &Thumb) -> (usize, usize) {
        let i = t.iter().position(|&p| p == 255).unwrap_or(usize::MAX);
        (i / SIDE, i % SIDE)
    }

    #[test]
    fn clockwise_rotations() {
        // Top-left marker; rotating CW by 90° moves it to the top-right.
        let last = SIDE - 1;
        let src = marker(0, 0);
        let mut out = [0u8; PIXELS];
        to_mirror_view(&src, 90, false, &mut out);
        assert_eq!(find(&out), (0, last));
        to_mirror_view(&src, 180, false, &mut out);
        assert_eq!(find(&out), (last, last));
        to_mirror_view(&src, 270, false, &mut out);
        assert_eq!(find(&out), (last, 0));
        to_mirror_view(&src, 0, false, &mut out);
        assert_eq!(find(&out), (0, 0));
        // Non-multiples round to the nearest quarter turn.
        to_mirror_view(&src, 268, false, &mut out);
        assert_eq!(find(&out), (last, 0));
        to_mirror_view(&src, -90, false, &mut out);
        assert_eq!(find(&out), (last, 0));
    }

    #[test]
    fn mirror_is_applied_after_rotation() {
        // Marker at (0, 5): rotate CW 270 → (SIDE-1-5, 0); mirror → (SIDE-1-5, SIDE-1).
        let src = marker(0, 5);
        let mut out = [0u8; PIXELS];
        to_mirror_view(&src, 270, true, &mut out);
        assert_eq!(find(&out), (SIDE - 1 - 5, SIDE - 1));
    }

    #[test]
    fn is_a_permutation() {
        let mut src = [0u8; PIXELS];
        for (i, p) in src.iter_mut().enumerate() {
            *p = (i * 7 % 251) as u8;
        }
        let mut sorted_src = src.to_vec();
        sorted_src.sort_unstable();
        for o in [0, 90, 180, 270] {
            for m in [false, true] {
                let mut out = [0u8; PIXELS];
                to_mirror_view(&src, o, m, &mut out);
                let mut s = out.to_vec();
                s.sort_unstable();
                assert_eq!(s, sorted_src);
            }
        }
    }
}
