# FaceTilt classifier training pipeline

Separate from the app; produces `crates/vision/model/facetilt_orient_int8.onnx`.

```
facetilt/
  labels.py         class indices (must match HeadClass in crates/decision)
  boxfilter.py      bit-exact port of the app's image path (downsample, mirror view, standardise)
  dataset.py        FairFace decoding, camera simulator, augmentation, self-supervised rotation labels
  model.py          TinyOrientNet (3 conv layers, 28.5k params)
  train.py          data -> train -> ONNX fp32 -> static int8 QDQ -> evaluation reports -> copy model into the app
  report.py         reports/EVALUATION.md renderer
  export_golden.py  fixtures for crates/vision/tests/parity.rs (Rust == Python check)
tests/              python -m unittest discover -s tests
reports/            metrics.json, EVALUATION.md (generated)
MODEL_CARD.md       model card
DATA_LICENSES.md    data sources and licences
```

## Reproduce

```bash
python3 -m venv .venv && . .venv/bin/activate
pip install -r requirements.txt
# FairFace "padding=1.25" images + validation labels (CC BY 4.0), see DATA_LICENSES.md:
#   https://github.com/joojs/fairface  ->  unzip to /opt/data/fairface/{train,val}, val_labels.csv
python -m facetilt.train --fairface /opt/data/fairface        # ~45 min on 4 CPU cores
python -m facetilt.export_golden --fairface /opt/data/fairface
cd .. && cargo test -p facetilt-vision                          # parity + tract vs onnxruntime
```

Self-supervision: every face is rendered once through the camera simulator and then rotated by
0/90/180/270 degrees *after* the box filter (a lossless permutation of the 32x32 grid, exactly what
the phone does with the sensor orientation), which yields the four orientation labels for free.
No human labels are used for training; FairFace's demographic labels are used only to audit
accuracy per group.
