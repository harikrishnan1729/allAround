"""End-to-end: data -> train -> ONNX fp32 -> int8 QDQ -> evaluation reports.

    python -m facetilt.train --fairface /opt/data/fairface --out artifacts

Writes:
  artifacts/facetilt_orient_fp32.onnx
  artifacts/facetilt_orient_int8.onnx   (copied to crates/vision/model/)
  reports/metrics.json, reports/EVALUATION.md
"""
import argparse
import csv
import json
import os
import shutil
import time

import numpy as np
import torch
import torch.nn.functional as F

from . import boxfilter, dataset, labels
from .model import TinyOrientNet, count_params, fuse_for_export

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REPO = os.path.dirname(HERE)


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, flush=True)


def to_tensor(X):
    return torch.from_numpy(boxfilter.standardize_batch(X)[:, None])


def train(model, Xtr, ytr, Xva, yva, epochs, batch, lr, seed):
    torch.manual_seed(seed)
    xt, yt = to_tensor(Xtr), torch.from_numpy(ytr)
    xv, yv = to_tensor(Xva), torch.from_numpy(yva)
    opt = torch.optim.AdamW(model.parameters(), lr=lr, weight_decay=1e-4)
    steps = epochs * ((len(xt) + batch - 1) // batch)
    sched = torch.optim.lr_scheduler.OneCycleLR(opt, max_lr=lr, total_steps=steps, pct_start=0.15)
    best, best_state = -1.0, None
    g = torch.Generator().manual_seed(seed)
    for ep in range(epochs):
        model.train()
        perm = torch.randperm(len(xt), generator=g)
        tot, n = 0.0, 0
        for i in range(0, len(xt), batch):
            idx = perm[i:i + batch]
            xb, yb = xt[idx], yt[idx]
            # Cheap online augmentation on the 32x32 tensor: +-1 px jitter.
            if True:
                dx, dy = torch.randint(-1, 2, (2,), generator=g).tolist()
                xb = torch.roll(xb, shifts=(dy, dx), dims=(2, 3))
            loss = F.cross_entropy(model(xb), yb, label_smoothing=0.05)
            opt.zero_grad(set_to_none=True)
            loss.backward()
            opt.step()
            sched.step()
            tot += loss.item() * len(idx)
            n += len(idx)
        acc = evaluate_torch(model, xv, yv)
        log(f"epoch {ep + 1}/{epochs} loss {tot / n:.4f} val_acc {acc:.4f}")
        if acc > best:
            best = acc
            best_state = {k: v.clone() for k, v in model.state_dict().items()}
    model.load_state_dict(best_state)
    return best


@torch.no_grad()
def evaluate_torch(model, x, y):
    model.eval()
    correct = 0
    for i in range(0, len(x), 4096):
        correct += (model(x[i:i + 4096]).argmax(1) == y[i:i + 4096]).sum().item()
    return correct / len(x)


def export_onnx(model, path):
    fused = fuse_for_export(model)
    import facetilt.boxfilter as _bf2
    dummy = torch.zeros(1, 1, _bf2.SIDE, _bf2.SIDE)
    torch.onnx.export(fused, dummy, path, input_names=["input"], output_names=["logits"],
                      dynamic_axes={"input": {0: "N"}, "logits": {0: "N"}}, opset_version=17, dynamo=False)
    return fused


def quantize(fp32_path, int8_path, calib_X):
    from onnxruntime.quantization import (CalibrationDataReader, CalibrationMethod, QuantFormat, QuantType,
                                          quantize_static)
    from onnxruntime.quantization.shape_inference import quant_pre_process

    class Reader(CalibrationDataReader):
        def __init__(self, X):
            self.it = iter([{"input": boxfilter.standardize_batch(X[i:i + 1])[:, None]} for i in range(len(X))])

        def get_next(self):
            return next(self.it, None)

    pre = fp32_path.replace(".onnx", "_pre.onnx")
    quant_pre_process(fp32_path, pre)
    quantize_static(pre, int8_path, Reader(calib_X), quant_format=QuantFormat.QDQ, per_channel=True,
                    weight_type=QuantType.QInt8, activation_type=QuantType.QUInt8,
                    calibrate_method=CalibrationMethod.Percentile,
                    extra_options={"CalibPercentile": 99.99})
    os.remove(pre)


def make_static_batch(src, dst):
    """Copy of `src` with the symbolic batch dim fixed to 1 everywhere (tract
    plans a fixed-shape graph; the app always classifies one frame)."""
    import onnx
    m = onnx.load(src)
    for vi in list(m.graph.input) + list(m.graph.output) + list(m.graph.value_info):
        for d in vi.type.tensor_type.shape.dim:
            if d.dim_param:
                d.Clear()
                d.dim_value = 1
    onnx.checker.check_model(m)
    onnx.save(m, dst)


def ort_logits(path, X):
    import onnxruntime as ort
    so = ort.SessionOptions()
    so.intra_op_num_threads = 4
    s = ort.InferenceSession(path, so, providers=["CPUExecutionProvider"])
    out = []
    for i in range(0, len(X), 2048):
        out.append(s.run(None, {"input": boxfilter.standardize_batch(X[i:i + 2048])[:, None]})[0])
    return np.concatenate(out)


def softmax(z):
    z = z - z.max(1, keepdims=True)
    e = np.exp(z)
    return e / e.sum(1, keepdims=True)


def metrics(logits, y, threshold=0.7):
    p = softmax(logits)
    pred = p.argmax(1)
    conf = p.max(1)
    out = {"n": int(len(y)), "accuracy": float((pred == y).mean())}
    out["per_class"] = {labels.NAMES[c]: float((pred[y == c] == c).mean()) if (y == c).any() else None
                        for c in range(labels.NUM_CLASSES)}
    cm = np.zeros((labels.NUM_CLASSES, labels.NUM_CLASSES), int)
    for t, q in zip(y, pred):
        cm[t, q] += 1
    out["confusion"] = cm.tolist()
    # What the app actually does: trust the face only if conf >= threshold and
    # class != no_face; otherwise fall back to gravity.
    accepted = (conf >= threshold) & (pred != labels.NO_FACE)
    face = y != labels.NO_FACE
    out["app_threshold"] = threshold
    out["face_coverage"] = float(accepted[face].mean()) if face.any() else None
    out["accepted_precision"] = float((pred[accepted] == y[accepted]).mean()) if accepted.any() else None
    # Dangerous error: a confident wrong orientation on a face (would rotate wrongly).
    out["confident_wrong_rate_faces"] = float((accepted & face & (pred != y)).sum() / max(1, face.sum()))
    # Non-face accepted as a face orientation.
    nf = y == labels.NO_FACE
    out["nonface_accepted_rate"] = float(accepted[nf].mean()) if nf.any() else None
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--fairface", default="/opt/data/fairface")
    ap.add_argument("--cache", default="/opt/data/cache")
    ap.add_argument("--out", default=os.path.join(HERE, "artifacts"))
    ap.add_argument("--train-images", type=int, default=50000)
    ap.add_argument("--variants", type=int, default=2)
    ap.add_argument("--epochs", type=int, default=14)
    ap.add_argument("--batch", type=int, default=512)
    ap.add_argument("--lr", type=float, default=4e-3)
    ap.add_argument("--seed", type=int, default=20261120)
    ap.add_argument("--side", type=int, default=48, help="classifier input side (box-filter target)")
    ap.add_argument("--resume", action="store_true", help="load tiny_orient_net.pt and skip training")
    ap.add_argument("--test-images", type=int, default=0, help="0 = all FairFace val images")
    args = ap.parse_args()
    os.makedirs(args.out, exist_ok=True)
    torch.set_num_threads(os.cpu_count())
    import facetilt.boxfilter as _bf
    _bf.SIDE = args.side  # box_downsample + standardize read this module global

    log("decoding FairFace train")
    tr_imgs, _ = dataset.load_fairface(args.fairface, "train", args.train_images, args.cache, seed=args.seed)
    n_val = max(500, len(tr_imgs) // 20)
    va_imgs, tr_imgs = tr_imgs[:n_val], tr_imgs[n_val:]
    log("decoding FairFace val (test split)")
    te_imgs, te_names = dataset.load_fairface(args.fairface, "val", args.test_images, args.cache, seed=args.seed)
    sk = dataset.skimage_backgrounds()
    sk_names = sorted(sk)
    # Hold out a third of the scikit-image photos for testing the no-face class.
    sk_test = {k: sk[k] for k in sk_names[::3]}
    sk_train = {k: sk[k] for k in sk_names if k not in sk_test}

    log(f"building train set from {len(tr_imgs)} faces x {args.variants} variants")
    Xtr, ytr, _ = dataset.build(tr_imgs, sk_train, args.variants, seed=args.seed)
    log(f"train: {len(Xtr)} samples, class counts {np.bincount(ytr).tolist()}")
    Xva, yva, _ = dataset.build(va_imgs, sk_train, 1, seed=args.seed + 1)
    log(f"val: {len(Xva)} samples")

    model = TinyOrientNet(side=args.side)
    n_params = count_params(model)
    log(f"params: {n_params}")
    ckpt = os.path.join(args.out, "tiny_orient_net.pt")
    if args.resume and os.path.exists(ckpt):
        model.load_state_dict(torch.load(ckpt, map_location="cpu"))
        model.eval()
        best = evaluate_torch(model, to_tensor(Xva), torch.from_numpy(yva))
        log(f"resumed from {ckpt}, val acc {best:.4f}")
    else:
        best = train(model, Xtr, ytr, Xva, yva, args.epochs, args.batch, args.lr, args.seed)
        log(f"best val acc {best:.4f}")
        torch.save(model.state_dict(), ckpt)

    fp32 = os.path.join(args.out, "facetilt_orient_fp32.onnx")
    int8 = os.path.join(args.out, "facetilt_orient_int8.onnx")
    export_onnx(model, fp32)
    calib = Xtr[np.random.default_rng(1).choice(len(Xtr), 3000, replace=False)]
    quantize(fp32, int8, calib)
    log(f"fp32 {os.path.getsize(fp32)} B, int8 {os.path.getsize(int8)} B")

    # ---------------------------------------------------------------- evaluation
    races = {}
    genders = {}
    with open(os.path.join(args.fairface, "val_labels.csv")) as f:
        for row in csv.DictReader(f):
            name = row["file"].split("/")[-1]
            races[name] = row["race"]
            genders[name] = row["gender"]
    report = {"params": n_params, "fp32_bytes": os.path.getsize(fp32), "int8_bytes": os.path.getsize(int8),
              "val_accuracy_torch": best, "train_samples": int(len(Xtr)), "test_faces": int(len(te_imgs)),
              "conditions": {}, "groups": {}}
    sk_test_names = sorted(sk_test)
    report["no_face_test_sources"] = {"skimage_held_out": sk_test_names,
                                      "fairface_val_backgrounds": True, "procedural": True}
    for cond in ["random"] + list(dataset.EVAL_CONDITIONS):
        log(f"test condition {cond}")
        Xte, yte, gte = dataset.build(te_imgs, sk_test, 1, seed=args.seed + 100 + len(cond),
                                      lighting_name=None if cond == "random" else cond, nf_ratio=0.2)
        res = {}
        for tag, path in (("fp32", fp32), ("int8", int8)):
            lg = ort_logits(path, Xte)
            res[tag] = metrics(lg, yte)
            if cond == "random" and tag == "int8":
                pred = softmax(lg).argmax(1)
                for key, table in (("race", races), ("gender", genders)):
                    groups = {}
                    for grp in sorted(set(table.values())):
                        sel = np.array([gi >= 0 and table[te_names[gi]] == grp for gi in gte])
                        groups[grp] = {"n": int(sel.sum()), "accuracy": float((pred[sel] == yte[sel]).mean())}
                    report["groups"][key] = groups
        report["conditions"][cond] = res
        log(f"  fp32 acc {res['fp32']['accuracy']:.4f}  int8 acc {res['int8']['accuracy']:.4f}")
    os.makedirs(os.path.join(HERE, "reports"), exist_ok=True)
    with open(os.path.join(HERE, "reports", "metrics.json"), "w") as f:
        json.dump(report, f, indent=2)
    from .report import write_markdown
    write_markdown(report, os.path.join(HERE, "reports", "EVALUATION.md"))
    dst = os.path.join(REPO, "crates", "vision", "model", "facetilt_orient_int8.onnx")
    make_static_batch(int8, dst)
    # Same weights; check the static copy is numerically identical.
    probe = Xtr[:256]
    assert np.array_equal(ort_logits(int8, probe), np.concatenate([ort_logits(dst, probe[i:i + 1]) for i in range(len(probe))]))
    log(f"copied int8 model to {dst}")


if __name__ == "__main__":
    main()
