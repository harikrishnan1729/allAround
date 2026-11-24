"""Dataset construction: source decoding, camera simulation, augmentation,
self-supervised rotation labels.

Pipeline for one sample (mirrors what the phone does, see README):

  source image (FairFace aligned crop, decoded straight to JPEG luma)
    -> occluders (glasses / hair fringe / hand)            [faces only]
    -> random in-plane roll (+-25 deg), h-flip, off-centre square view
    -> camera frame at a real YUV size (176x144 .. 640x480), bilinear
    -> lighting (gain, gamma, contrast, backlight) + sensor noise + blur, 8-bit quantisation
    -> boxfilter.box_downsample()  (bit-exact port of the Rust code)
    -> np.rot90 by k in {0,1,2,3}  => label (self-supervised)

All geometry below is in the 160x160 source grid. Face geometry of the
FairFace "padding=1.25" dlib chips was measured from the mean of 2,000
validation images: eyes at (67,65)/(93,65), face x 55..105, hairline y~35,
chin y~108, face centre ~(80,75).
"""
import math
import os
from dataclasses import dataclass

import numpy as np
from PIL import Image, ImageFilter

from . import boxfilter, labels

SRC = 160
FACE_W = 50.0
FACE_CX, FACE_CY = 80.0, 75.0
FACE_BOX = (55, 33, 105, 110)  # x0, y0, x1, y1 (hair to chin)
EYES = ((67, 65), (93, 65))
CAMERA_SIZES = [(176, 144), (320, 240), (352, 288), (640, 480)]


# --------------------------------------------------------------------------- sources

def _decode(path):
    im = Image.open(path)
    im.draft("L", (2 * SRC, 2 * SRC))  # JPEG DCT-domain downscale, luma only
    im = im.convert("L")
    if im.size != (SRC, SRC):
        im = im.resize((SRC, SRC), Image.BOX)
    return np.asarray(im, dtype=np.uint8)


def load_fairface(root, split, limit, cache_dir, seed=0):
    """Returns (images uint8 [N,160,160], file names). Cached as .npy."""
    os.makedirs(cache_dir, exist_ok=True)
    cache = os.path.join(cache_dir, f"fairface_{split}_{limit}_{SRC}.npz")
    if os.path.exists(cache):
        z = np.load(cache, allow_pickle=False)
        return z["images"], list(z["names"])
    names = sorted(os.listdir(os.path.join(root, split)), key=lambda s: int(s.split(".")[0]))
    rng = np.random.default_rng(seed)
    if limit and limit < len(names):
        names = sorted(rng.choice(names, size=limit, replace=False).tolist(), key=lambda s: int(s.split(".")[0]))
    from multiprocessing import Pool
    with Pool(os.cpu_count()) as p:
        imgs = p.map(_decode, [os.path.join(root, split, n) for n in names], chunksize=256)
    images = np.stack(imgs)
    np.savez(cache, images=images, names=np.array(names))
    return images, names


def skimage_backgrounds():
    """Face-free sample images bundled with scikit-image (BSD-3 package; the
    individual images' licences are listed in training/DATA_LICENSES.md).
    Images containing people (astronaut, camera) are excluded."""
    import skimage.data as d
    from skimage.color import rgb2gray
    out = {}
    for name in ["brick", "coffee", "coins", "grass", "gravel", "horse", "moon", "page", "rocket",
                 "text", "chelsea", "checkerboard", "clock", "hubble_deep_field", "immunohistochemistry",
                 "logo", "motorcycle_left", "retina", "cell", "colorwheel"]:
        try:
            im = getattr(d, name)()
        except Exception:  # needs a download not available offline
            continue
        im = np.asarray(im)
        if im.ndim == 3:
            im = im[..., :3]
            im = (rgb2gray(im) * 255).astype(np.uint8) if im.dtype != np.uint8 else boxfilter.rgb_to_luma(im)
        elif im.dtype == bool:
            im = im.astype(np.uint8) * 255
        elif im.dtype != np.uint8:
            im = np.clip(im / max(1e-6, im.max()) * 255, 0, 255).astype(np.uint8)
        if min(im.shape) >= 64:
            out[name] = im
    return out


# --------------------------------------------------------------------------- augmentation

@dataclass
class Lighting:
    gain: float = 1.0
    gamma: float = 1.0
    contrast: float = 1.0
    backlight: float = 0.0
    read_noise: float = 1.0
    shot: float = 0.15
    blur: float = 0.0
    motion: int = 0
    limited_range: bool = False

    @staticmethod
    def random(rng):
        return Lighting(
            # Auto-exposure keeps most frames near mid-grey; 20% low-light tail
            # where AE runs out of range (dark and noisy).
            gain=float(math.exp(rng.uniform(math.log(0.08), math.log(0.3)))) if rng.random() < 0.2
            else float(math.exp(rng.uniform(math.log(0.45), math.log(1.6)))),
            gamma=float(rng.uniform(0.7, 1.4)),
            contrast=float(rng.uniform(0.55, 1.25)),
            backlight=float(rng.uniform(0.3, 0.75)) if rng.random() < 0.15 else 0.0,
            read_noise=float(rng.uniform(0.5, 4.0)),
            shot=float(rng.uniform(0.05, 0.5)),
            blur=float(rng.uniform(0.3, 2.0)) if rng.random() < 0.5 else 0.0,
            motion=int(rng.integers(3, 10)) if rng.random() < 0.2 else 0,
            limited_range=bool(rng.random() < 0.3),
        )


# Fixed conditions used by the per-lighting evaluation report.
EVAL_CONDITIONS = {
    "normal": Lighting(gain=1.0, read_noise=1.0, shot=0.1),
    "dim": Lighting(gain=0.3, gamma=1.1, read_noise=2.0, shot=0.3),
    "very_low_light": Lighting(gain=0.1, gamma=1.2, read_noise=3.5, shot=0.5, blur=0.8),
    "overexposed": Lighting(gain=1.6, gamma=0.75, contrast=0.8, read_noise=1.0),
    "backlit": Lighting(gain=0.9, backlight=0.65, read_noise=1.5, shot=0.2),
    "motion_blur": Lighting(gain=0.8, motion=7, read_noise=1.5, shot=0.2),
}


def _occlude(img, rng):
    """Synthetic occluders in face coordinates (glasses, hair fringe, hand)."""
    img = img.astype(np.float32)
    yy, xx = np.mgrid[0:SRC, 0:SRC].astype(np.float32)
    if rng.random() < 0.25:  # glasses: two dark rings + bridge
        r = rng.uniform(6.5, 9.0)
        th = rng.uniform(1.0, 2.2)
        dark = rng.uniform(0.15, 0.6)
        mask = np.zeros_like(img, dtype=bool)
        for ex, ey in EYES:
            d = np.sqrt((xx - ex) ** 2 + (yy - ey) ** 2)
            mask |= np.abs(d - r) < th
        mask |= (np.abs(yy - EYES[0][1] + 1) < th) & (xx > EYES[0][0] + r - 1) & (xx < EYES[1][0] - r + 1)
        if rng.random() < 0.3:  # sunglasses: fill the lenses
            for ex, ey in EYES:
                mask |= np.sqrt((xx - ex) ** 2 + (yy - ey) ** 2) < r
        img[mask] *= dark
    if rng.random() < 0.2:  # hair fringe over the forehead, wavy lower edge
        edge = 50 + rng.uniform(0, 12) + 3 * np.sin(xx[0] / rng.uniform(3, 8) + rng.uniform(0, 6))
        tone = rng.uniform(10, 90)
        m = (yy < edge[None, :]) & (xx > 50) & (xx < 110) & (yy > 25)
        img[m] = 0.3 * img[m] + 0.7 * tone
    if rng.random() < 0.15:  # hand / object covering part of the face
        cx = rng.uniform(55, 105)
        cy = rng.uniform(70, 115)
        ax, ay = rng.uniform(10, 22), rng.uniform(14, 28)
        m = ((xx - cx) / ax) ** 2 + ((yy - cy) / ay) ** 2 < 1
        img[m] = rng.uniform(60, 200) + rng.normal(0, 6, size=int(m.sum()))
    return np.clip(img, 0, 255).astype(np.uint8)


def _view(img, rng, roll_deg, center, side, flip):
    """Square view of `side` source px centred at `center`, rolled by roll_deg."""
    pad = SRC
    big = np.pad(img, pad, mode="edge")
    im = Image.fromarray(big)
    if flip:
        im = im.transpose(Image.FLIP_LEFT_RIGHT)
        center = (SRC - 1 - center[0], center[1])
    cx, cy = center[0] + pad, center[1] + pad
    if roll_deg:
        im = im.rotate(roll_deg, resample=Image.BILINEAR, center=(cx, cy))
    return im, (cx, cy)


def _resize_box(im, box, size):
    """PIL resize of an arbitrary box; areas outside the image use edge pixels."""
    x0, y0, x1, y1 = box
    w, h = im.size
    pad = int(math.ceil(max(0.0, -x0, -y0, x1 - w, y1 - h))) + 2
    if pad > 2:
        im = Image.fromarray(np.pad(np.asarray(im), pad, mode="edge"))
        box = (x0 + pad, y0 + pad, x1 + pad, y1 + pad)
    return np.asarray(im.resize(size, Image.BILINEAR, box=box), dtype=np.float32)


def _camera_frame(im, cxy, side, size, rng):
    """Renders the camera frame (W x H) whose centred square is the view."""
    w, h = size
    half_h = side / 2.0
    half_w = side * w / h / 2.0
    cx, cy = cxy
    box = (cx - half_w, cy - half_h, cx + half_w, cy + half_h)
    return _resize_box(im, box, (w, h))


def _sensor(frame, L: Lighting, rng):
    x = frame / 255.0
    if L.backlight:
        h, w = x.shape
        yy, xx = np.mgrid[0:h, 0:w]
        r2 = ((xx - w / 2) / (0.3 * h)) ** 2 + ((yy - h / 2) / (0.35 * h)) ** 2
        x = x * (1 - L.backlight * np.exp(-r2)) + 0.35 * L.backlight * (1 - np.exp(-r2))
    m = x.mean()
    x = np.clip((x - m) * L.contrast + m, 0, 1) ** L.gamma
    x = x * L.gain
    img = Image.fromarray(np.clip(x * 255, 0, 255).astype(np.uint8))
    if L.blur:
        img = img.filter(ImageFilter.GaussianBlur(L.blur))
    x = np.asarray(img, dtype=np.float32)
    if L.motion:
        k = L.motion
        ang = rng.uniform(0, math.pi)
        acc = np.zeros_like(x)
        for i in range(k):
            dx = int(round((i - k / 2) * math.cos(ang)))
            dy = int(round((i - k / 2) * math.sin(ang)))
            acc += np.roll(np.roll(x, dx, axis=1), dy, axis=0)
        x = acc / k
    # Shot noise grows as light falls (camera raises analogue gain).
    shot_sigma = np.sqrt(np.maximum(x, 0) * L.shot / max(L.gain, 0.05))
    x = x + rng.normal(0, 1, x.shape).astype(np.float32) * shot_sigma
    x = x + rng.normal(0, L.read_noise, x.shape).astype(np.float32)
    if L.limited_range:
        x = 16 + x * (219 / 255)
    return np.clip(np.round(x), 0, 255).astype(np.uint8)


def face_sample(img, rng, lighting=None, occlude=True):
    """One upright face thumbnail (32x32 uint8, mirror view, head top up)."""
    if occlude:
        img = _occlude(img, rng)
    frac = rng.uniform(0.30, 0.65)  # face width / view width
    side = FACE_W / frac
    off = rng.uniform(-0.17, 0.17, size=2) * side
    center = (FACE_CX + off[0], FACE_CY + off[1])
    roll = rng.uniform(-25, 25)
    flip = bool(rng.random() < 0.5)
    im, cxy = _view(img, rng, roll, center, side, flip)
    size = CAMERA_SIZES[rng.integers(len(CAMERA_SIZES))]
    frame = _camera_frame(im, cxy, side, size, rng)
    L = lighting or Lighting.random(rng)
    return boxfilter.box_downsample(_sensor(frame, L, rng))


def partial_face_sample(img, rng, lighting=None):
    """View where at most ~25% of the face is visible -> labelled NO_FACE."""
    side = FACE_W / rng.uniform(0.3, 0.6)
    ang = rng.uniform(0, 2 * math.pi)
    dist = side * rng.uniform(0.75, 0.95)
    center = (FACE_CX + dist * math.cos(ang), FACE_CY + dist * math.sin(ang))
    im, cxy = _view(img, rng, rng.uniform(-180, 180), center, side, False)
    size = CAMERA_SIZES[rng.integers(len(CAMERA_SIZES))]
    frame = _camera_frame(im, cxy, side, size, rng)
    return boxfilter.box_downsample(_sensor(frame, lighting or Lighting.random(rng), rng))


def background_sample(rng, fairface_img=None, sk_images=None, lighting=None):
    """A no-face thumbnail from one of: FairFace background corner, scikit-image
    photo crop, or a procedural indoor-like scene (ceiling, wall, lamp)."""
    L = lighting or Lighting.random(rng)
    kind = rng.random()
    if fairface_img is not None and kind < 0.35:
        s = rng.uniform(40, 52)
        corner = rng.integers(4)
        x = [0, SRC - s, 0, SRC - s][corner] + rng.uniform(0, 4)
        y = [0, 0, SRC - s, SRC - s][corner] + rng.uniform(0, 4)
        im = Image.fromarray(fairface_img).rotate(float(rng.choice([0, 90, 180, 270])))
        size = CAMERA_SIZES[rng.integers(len(CAMERA_SIZES))]
        w, h = size
        box = (x + s / 2 - s * w / h / 2, y, x + s / 2 + s * w / h / 2, y + s)
        frame = _resize_box(im, box, size)
    elif sk_images and kind < 0.65:
        keys = sorted(sk_images)
        src = sk_images[keys[rng.integers(len(keys))]]
        hh, ww = src.shape
        s = rng.uniform(0.25, 1.0) * min(hh, ww)
        x, y = rng.uniform(0, ww - s), rng.uniform(0, hh - s)
        im = Image.fromarray(src).rotate(float(rng.uniform(0, 360)), resample=Image.BILINEAR,
                                         center=(x + s / 2, y + s / 2))
        size = CAMERA_SIZES[rng.integers(len(CAMERA_SIZES))]
        w, h = size
        box = (x + s / 2 - s * w / h / 2, y, x + s / 2 + s * w / h / 2, y + s)
        frame = _resize_box(im, box, size)
    else:
        frame = procedural_scene(rng, CAMERA_SIZES[rng.integers(len(CAMERA_SIZES))])
    return boxfilter.box_downsample(_sensor(frame, L, rng))


def procedural_scene(rng, size):
    w, h = size
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float32)
    ang = rng.uniform(0, 2 * math.pi)
    base = rng.uniform(40, 200)
    img = base + rng.uniform(-60, 60) * ((xx * math.cos(ang) + yy * math.sin(ang)) / max(w, h))
    for _ in range(rng.integers(0, 5)):  # walls, door frames, furniture edges
        a = rng.uniform(0, math.pi)
        d = rng.uniform(-0.5, 0.5) * max(w, h)
        side = (xx - w / 2) * math.cos(a) + (yy - h / 2) * math.sin(a) > d
        img = np.where(side, img + rng.uniform(-50, 50), img)
    for _ in range(rng.integers(0, 3)):  # lamps / windows
        cx, cy = rng.uniform(0, w), rng.uniform(0, h)
        r = rng.uniform(0.05, 0.25) * h
        img = img + rng.uniform(40, 150) * np.exp(-(((xx - cx) ** 2 + (yy - cy) ** 2) / (2 * r * r)))
    for _ in range(rng.integers(0, 4)):  # blobs (objects, pillow folds)
        cx, cy = rng.uniform(0, w), rng.uniform(0, h)
        rx, ry = rng.uniform(0.05, 0.3) * h, rng.uniform(0.05, 0.3) * h
        m = ((xx - cx) / rx) ** 2 + ((yy - cy) / ry) ** 2 < 1
        img = np.where(m, rng.uniform(0, 255), img)
    img = img + rng.normal(0, rng.uniform(0, 8), img.shape)  # texture
    return np.clip(img, 0, 255).astype(np.float32)


# --------------------------------------------------------------------------- labelled sets

def rotations_of(thumb):
    """All 4 rotations with their self-supervised labels."""
    return [(np.ascontiguousarray(np.rot90(thumb, k)), labels.ROT90_K_TO_LABEL[k]) for k in range(4)]


def _worker(args):
    kind, payload, seed, lighting_name = args
    rng = np.random.default_rng(seed)
    L = EVAL_CONDITIONS[lighting_name] if lighting_name else None
    if kind == "face":
        return face_sample(payload, rng, lighting=L)
    if kind == "partial":
        return partial_face_sample(payload, rng, lighting=L)
    ff, sk = payload
    return background_sample(rng, fairface_img=ff, sk_images=sk, lighting=L)


_SK = None


def _init(sk):
    global _SK
    _SK = sk


def _worker_bg(args):
    ff, seed, lighting_name = args
    rng = np.random.default_rng(seed)
    L = EVAL_CONDITIONS[lighting_name] if lighting_name else None
    return background_sample(rng, fairface_img=ff, sk_images=_SK, lighting=L)


def build(faces, sk_images, variants, seed, lighting_name=None, nf_ratio=0.25, partial_ratio=0.2):
    """Returns X uint8 [N,32,32], y int64 [N], group int64 [N] (source index or -1).

    Every face camera-simulation yields 4 samples (one per rotation). Usable
    frames only (the app never classifies near-uniform frames).
    """
    from multiprocessing import Pool
    n_faces = len(faces)
    jobs = []
    for v in range(variants):
        for i in range(n_faces):
            jobs.append(("face", faces[i], seed * 1_000_003 + v * n_faces + i, lighting_name))
    n_nf = int(round(nf_ratio * len(jobs) * 4 / (1 - nf_ratio) / 4))  # ~nf_ratio of final samples
    n_partial = int(n_nf * partial_ratio)
    rng = np.random.default_rng(seed + 17)
    part_jobs = [("partial", faces[rng.integers(n_faces)], seed * 7_000_003 + j, lighting_name) for j in range(n_partial)]
    bg_jobs = [(faces[rng.integers(n_faces)], seed * 9_000_011 + j, lighting_name) for j in range(n_nf - n_partial)]
    with Pool(os.cpu_count(), initializer=_init, initargs=(sk_images,)) as p:
        face_th = p.map(_worker, jobs, chunksize=512)
        part_th = p.map(_worker, part_jobs, chunksize=512)
        bg_th = p.map(_worker_bg, bg_jobs, chunksize=512)
    X, y, g = [], [], []
    for (kind, _, _, _), th, idx in zip(jobs, face_th, [j % n_faces for j in range(len(jobs))]):
        for r, lab in rotations_of(th):
            X.append(r)
            y.append(lab)
            g.append(idx)
    for th in part_th + bg_th:
        k = int(rng.integers(4))
        X.append(np.ascontiguousarray(np.rot90(th, k)))
        y.append(labels.NO_FACE)
        g.append(-1)
    X = np.stack(X)
    y = np.array(y, np.int64)
    g = np.array(g, np.int64)
    keep = np.array([boxfilter.standardize(x) is not None for x in X])
    return X[keep], y[keep], g[keep]
