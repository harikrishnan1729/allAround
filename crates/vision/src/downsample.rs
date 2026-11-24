//! Integer box-filter downsampling of a luma plane to 48×48.
//!
//! Algorithm (mirrored exactly in `training/facetilt/boxfilter.py`):
//!
//! * `k = min(width, height) / 32` (integer division, must be ≥ 1);
//! * crop a centred square of side `32·k`, origin
//!   `((width − 32k) / 2, (height − 32k) / 2)` (integer division);
//! * every output pixel is the rounded mean of a `k×k` block:
//!   `(sum + k²/2) / k²` in integer arithmetic.
//!
//! The source is only read; nothing is allocated.

use crate::{Thumb, SIDE};

/// Why a plane could not be downsampled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownsampleError {
    /// Shorter side below 32 pixels.
    TooSmall,
    /// Row stride smaller than the width.
    BadStride,
    /// The buffer is shorter than `stride × height` requires.
    BufferTooShort,
}

/// Downsamples the Y plane `y` (`width × height`, `row_stride` bytes per row,
/// pixel stride 1 — guaranteed for the Y plane of `YUV_420_888`) into `out`.
pub fn box_downsample(
    y: &[u8],
    width: usize,
    height: usize,
    row_stride: usize,
    out: &mut Thumb,
) -> Result<(), DownsampleError> {
    let k = width.min(height) / SIDE;
    if k == 0 {
        return Err(DownsampleError::TooSmall);
    }
    if row_stride < width {
        return Err(DownsampleError::BadStride);
    }
    let crop = k * SIDE;
    let x0 = (width - crop) / 2;
    let y0 = (height - crop) / 2;
    // Last byte read: row y0+crop-1, column x0+crop-1.
    let needed = (y0 + crop - 1) * row_stride + x0 + crop;
    if y.len() < needed {
        return Err(DownsampleError::BufferTooShort);
    }
    let n = (k * k) as u32;
    let half = n / 2;
    let mut acc = [0u32; SIDE];
    for oy in 0..SIDE {
        acc.fill(0);
        for dy in 0..k {
            let row_start = (y0 + oy * k + dy) * row_stride + x0;
            let row = &y[row_start..row_start + crop];
            for (a, block) in acc.iter_mut().zip(row.chunks_exact(k)) {
                *a += block.iter().map(|&p| u32::from(p)).sum::<u32>();
            }
        }
        for (o, a) in out[oy * SIDE..(oy + 1) * SIDE].iter_mut().zip(acc.iter()) {
            // (sum + n/2) / n ≤ 255 because sum ≤ 255·n.
            *o = ((a + half) / n) as u8;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PIXELS;

    #[test]
    fn constant_plane() {
        let y = vec![77u8; 176 * 144];
        let mut out = [0u8; PIXELS];
        box_downsample(&y, 176, 144, 176, &mut out).unwrap_or_else(|e| panic!("{e:?}"));
        assert!(out.iter().all(|&p| p == 77));
    }

    #[test]
    fn block_means_and_rounding() {
        // w = h = 2·SIDE, so k = 2: each output pixel averages a 2×2 block.
        let w = 2 * SIDE;
        let mut y = vec![0u8; w * w];
        y[0] = 1; // block (0,0) = {1,0,0,0} → (1+2)/4 = 0
        y[2] = 2; // block (0,1) = {2,0,0,0} → (2+2)/4 = 1
        y[4] = 255;
        y[5] = 255;
        y[w + 4] = 255;
        y[w + 5] = 254; // block (0,2) → (1019+2)/4 = 255
        let mut out = [9u8; PIXELS];
        box_downsample(&y, w, w, w, &mut out).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(&out[..3], &[0, 1, 255]);
    }

    #[test]
    fn centre_crop_and_stride() {
        // 100×70 with stride 128: k = 2, crop 64, x0 = 18, y0 = 3.
        let (w, h, s) = (100usize, 70usize, 128usize);
        let mut y = vec![0u8; s * h];
        for r in 0..h {
            for c in 0..w {
                y[r * s + c] = if (18..82).contains(&c) && (3..67).contains(&r) { 200 } else { 10 };
            }
            for c in w..s {
                y[r * s + c] = 255; // padding must never be read
            }
        }
        let mut out = [0u8; PIXELS];
        box_downsample(&y, w, h, s, &mut out).unwrap_or_else(|e| panic!("{e:?}"));
        assert!(out.iter().all(|&p| p == 200));
    }

    #[test]
    fn errors() {
        let mut out = [0u8; PIXELS];
        // Shorter side below SIDE → too small.
        let small = SIDE - 1;
        assert_eq!(
            box_downsample(&vec![0u8; small * SIDE], SIDE, small, SIDE, &mut out),
            Err(DownsampleError::TooSmall)
        );
        // Row stride below width → bad stride.
        assert_eq!(
            box_downsample(&vec![0u8; SIDE * SIDE], SIDE, SIDE, SIDE - 1, &mut out),
            Err(DownsampleError::BadStride)
        );
        // Buffer one byte short of what the crop needs.
        let need = (SIDE - 1) * SIDE + SIDE; // last byte read for a SIDE×SIDE, stride SIDE frame
        assert_eq!(
            box_downsample(&vec![0u8; need - 1], SIDE, SIDE, SIDE, &mut out),
            Err(DownsampleError::BufferTooShort)
        );
        // The last row may be shorter than the stride (common for camera buffers): ok.
        let stride = SIDE + 16;
        let buf = vec![0u8; stride * (SIDE - 1) + SIDE];
        assert_eq!(box_downsample(&buf, SIDE, SIDE, stride, &mut out), Ok(()));
    }
}
