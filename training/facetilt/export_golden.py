"""Writes golden fixtures for crates/vision/tests/parity.rs.

  python -m facetilt.export_golden --fairface /opt/data/fairface

* downsample.bin  — random luma planes (several sizes / strides) with the expected
  32x32 box-filter output, the 8 mirror-view variants, and the standardised floats.
* classifier.bin  — 32x32 inputs with onnxruntime logits of the shipped int8 model:
  the *mean face* of 2,000 FairFace validation images (an average of thousands of
  people; it identifies no one) at all 4 rotations, plus procedural no-face scenes.
"""
import argparse
import os
import struct

import numpy as np

from . import boxfilter, dataset, labels
from .train import REPO, ort_logits

FIX = os.path.join(REPO, "crates", "vision", "tests", "fixtures")
MODEL = os.path.join(REPO, "crates", "vision", "model", "facetilt_orient_int8.onnx")


def write_downsample(path, rng):
    cases = [(176, 144, 192), (320, 240, 320), (352, 288, 384), (640, 480, 640), (100, 70, 128), (144, 176, 192)]
    with open(path, "wb") as f:
        f.write(struct.pack("<I", len(cases)))
        for w, h, stride in cases:
            plane = rng.integers(0, 256, size=(h, stride), dtype=np.uint8)
            # Add structure so rotations are distinguishable.
            plane[: h // 3, : w // 2] //= 3
            y = np.ascontiguousarray(plane[:, :w])
            thumb = boxfilter.box_downsample(y)
            f.write(struct.pack("<III", w, h, stride))
            f.write(plane.tobytes())
            f.write(thumb.tobytes())
            for o in (0, 90, 180, 270):
                for m in (False, True):
                    f.write(boxfilter.to_mirror_view(thumb, o, m).tobytes())
            std = boxfilter.standardize(thumb)
            f.write(std.astype("<f4").tobytes())


def mean_face(root, n=2000):
    acc = np.zeros((dataset.SRC, dataset.SRC), np.float64)
    for i in range(1, n + 1):
        acc += dataset._decode(os.path.join(root, "val", f"{i}.jpg"))
    return np.clip(acc / n + 0.5, 0, 255).astype(np.uint8)


def mean_face_thumb(img):
    from PIL import Image
    side = dataset.FACE_W / 0.45
    im, cxy = dataset._view(img, None, 0.0, (dataset.FACE_CX, dataset.FACE_CY), side, False)
    frame = dataset._camera_frame(im, cxy, side, (320, 240), None)
    return boxfilter.box_downsample(np.clip(frame + 0.5, 0, 255).astype(np.uint8))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--fairface", default="/opt/data/fairface")
    args = ap.parse_args()
    boxfilter.SIDE = 48  # must match crates/vision SIDE and the shipped model
    os.makedirs(FIX, exist_ok=True)
    rng = np.random.default_rng(1234)
    write_downsample(os.path.join(FIX, "downsample.bin"), rng)

    face = mean_face_thumb(mean_face(args.fairface))
    inputs, expected = [], []
    for k in range(4):
        inputs.append(np.ascontiguousarray(np.rot90(face, k)))
        expected.append(labels.ROT90_K_TO_LABEL[k])
    for i in range(12):
        t = boxfilter.box_downsample(dataset.procedural_scene(np.random.default_rng(500 + i), (320, 240)).astype(np.uint8))
        if boxfilter.standardize(t) is not None:
            inputs.append(t)
            expected.append(labels.NO_FACE)
    X = np.stack(inputs)
    logits = np.concatenate([ort_logits(MODEL, X[i:i + 1]) for i in range(len(X))])  # shipped model: batch 1
    # Layout: u32 n; n x { u8 expected_class, u8 ort_argmax, 5 x f32 ort_logits, 1024 x u8 pixels }
    with open(os.path.join(FIX, "classifier.bin"), "wb") as f:
        f.write(struct.pack("<I", len(X)))
        for x, e, lg in zip(X, expected, logits):
            f.write(struct.pack("<BB", e, int(lg.argmax())))
            f.write(lg.astype("<f4").tobytes())
            f.write(x.astype(np.uint8).tobytes())
    print("mean-face predictions:", logits[:4].argmax(1).tolist(), "expected", expected[:4])
    print("scene predictions:", logits[4:].argmax(1).tolist())


if __name__ == "__main__":
    main()
