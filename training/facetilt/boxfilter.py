"""Exact Python port of the app's image path (crates/vision/src/{downsample,orient,preprocess}.rs).

Train/runtime parity matters: every training sample goes through these exact
functions, and tests/parity.rs in the vision crate checks the Rust code
against golden outputs produced here.
"""
import numpy as np

SIDE = 32
MIN_STD = 3.0


def box_downsample(y: np.ndarray) -> np.ndarray:
    """Centred-square integer box filter to 32x32. `y` is a 2-D uint8 luma plane.

    k = min(w, h) // 32; crop 32k centred (integer offsets); each output is
    (sum + k*k//2) // (k*k).
    """
    if y.dtype != np.uint8 or y.ndim != 2:
        raise ValueError("expected 2-D uint8")
    h, w = y.shape
    k = min(w, h) // SIDE
    if k == 0:
        raise ValueError("too small")
    crop = k * SIDE
    x0 = (w - crop) // 2
    y0 = (h - crop) // 2
    c = y[y0:y0 + crop, x0:x0 + crop].astype(np.uint32)
    s = c.reshape(SIDE, k, SIDE, k).sum(axis=(1, 3))
    n = k * k
    return ((s + n // 2) // n).astype(np.uint8)


def to_mirror_view(t: np.ndarray, sensor_orientation_deg: int, mirror: bool) -> np.ndarray:
    """Rotate clockwise by the sensor orientation, then mirror horizontally."""
    quarter = ((sensor_orientation_deg % 360) + 45) // 90 % 4
    # np.rot90 rotates counter-clockwise for positive k.
    r = np.rot90(t, k=-quarter)
    if mirror:
        r = r[:, ::-1]
    return np.ascontiguousarray(r)


def standardize(t: np.ndarray):
    """Returns float32 (32,32) standardised image, or None for near-uniform frames."""
    t = t.astype(np.uint8)
    s = int(t.astype(np.uint32).sum())
    mean = np.float32(s) / np.float32(t.size)
    d = t.astype(np.float64) - np.float64(mean)
    std = np.float32(np.sqrt((d * d).sum() / t.size))
    if std < MIN_STD:
        return None
    return (t.astype(np.float32) - mean) / std


def standardize_batch(batch: np.ndarray, chunk: int = 8192) -> np.ndarray:
    """Vectorised standardize for training. Uniform frames map to zeros
    (the app never classifies them; they are kept out of the training set).

    Processed in row-chunks so peak memory stays bounded: the full-batch float64
    variance array would be gigabytes at 48x48. Arithmetic matches the per-image
    `standardize` (float64 variance) bit-for-bit, verified by the Rust parity test.
    """
    n = len(batch)
    out = np.empty((n, batch.shape[1], batch.shape[2]), dtype=np.float32)
    npix = np.float32(batch.shape[1] * batch.shape[2])
    for i in range(0, n, chunk):
        sub = batch[i:i + chunk]
        b = sub.astype(np.float32)
        flat = sub.reshape(len(sub), -1)
        mean = flat.astype(np.uint32).sum(axis=1).astype(np.float32) / npix
        d = flat.astype(np.float64) - mean[:, None].astype(np.float64)
        std = np.sqrt((d * d).sum(axis=1) / np.float64(flat.shape[1])).astype(np.float32)
        ok = std >= MIN_STD
        std_safe = np.where(ok, std, np.float32(1.0))
        o = (b - mean[:, None, None]) / std_safe[:, None, None]
        o[~ok] = 0.0
        out[i:i + chunk] = o
    return out


def rgb_to_luma(rgb: np.ndarray) -> np.ndarray:
    """BT.601 full-range luma (what JPEG and most camera Y planes carry)."""
    r, g, b = rgb[..., 0].astype(np.float32), rgb[..., 1].astype(np.float32), rgb[..., 2].astype(np.float32)
    return np.clip(0.299 * r + 0.587 * g + 0.114 * b + 0.5, 0, 255).astype(np.uint8)
