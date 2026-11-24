# Model card — FaceTilt head-orientation classifier (`facetilt_orient_int8.onnx`)

## Model details

| | |
|---|---|
| Task | 5-way classification of **head orientation relative to the phone**: upright, rotated 90° left, rotated 90° right, upside down, no face / unsure |
| Input | 48 × 48 grayscale (camera luma, box-filtered), per-image standardised, in the front camera's mirror view in the device's natural orientation |
| Output | 5 logits → softmax (computed in Rust); the app uses arg-max + its probability |
| Architecture | TinyOrientNet: 3 × [conv 3×3 → BN → ReLU → max-pool 2] (16/32/64 channels) → linear 2304→5. **34,933 parameters** |
| Format | ONNX opset 17, static int8 **QDQ** (per-channel int8 weights, uint8 activations), BN folded; **46.8 KB**; fp32 reference 138.5 KB |
| Runtime | tract 0.23 (pure Rust), single thread; logits identical to onnxruntime 1.29 on the parity fixtures |
| Version / date | 0.1.0, trained 2026-11-20 (48×48) (CPU, ~40 min incl. data generation) |
| Licence | Code Apache-2.0. Weights are derived from FairFace (CC BY 4.0): attribution in `DATA_LICENSES.md` |

## Intended use

* On-device, in-memory decision support for screen auto-rotation inside FaceTilt: "is the
  user's head turned the same way as the phone?". One inference per phone rotation.
* Its output is advisory: the app only trusts it when confidence ≥ 0.7 (configurable) and
  otherwise falls back to standard gravity auto-rotate.

## Out of scope / must not be used for

* Face detection or localisation, face recognition or verification, identity, age, gender,
  race, emotion, attention or liveness estimation. **The model cannot express any of these**:
  its only outputs are the five classes; there is no embedding head, no landmark head, and
  at 32 × 32 with 28k parameters it has neither the input resolution nor the capacity.
* Safety-critical decisions of any kind.

## Training data

* **Faces:** 47,500 FairFace training images (padding = 1.25 crops; CC BY 4.0) → each rendered
  twice through a camera simulator → each render used at 4 rotations (self-supervised labels:
  the rotation applied *is* the label). 373,100 face samples (93,275 per class after removing
  near-uniform frames, which the app never classifies).
* **No face:** 28,872 samples (7.2 % of the 401,972-sample set): FairFace background corners
  and views with ≤ ~25 % of a face visible; 12 face-free scikit-image photos (CC0 / public
  domain); procedural indoor-like scenes.
* **Camera simulator / augmentation:** random face size (30–65 % of the view), ±17 % off-centre,
  ±25° roll, horizontal flip (before rotation, so labels stay correct), real YUV frame sizes
  (176×144, 320×240, 352×288, 640×480), auto-exposure-like gain with a 20 % low-light tail,
  gamma, contrast, backlight, shot + read noise with 8-bit quantisation, Gaussian and motion
  blur, limited-range luma, synthetic glasses / sunglasses, hair fringe and hand occluders.
* Downsampling uses `boxfilter.py`, a bit-exact port of the app's Rust code (verified by
  `crates/vision/tests/parity.rs`).
* Validation: 2,500 held-out FairFace training images (model selection only).

## Evaluation data

FairFace **validation** split (10,954 images, disjoint from training), rendered through the
same simulator at fixed lighting conditions and all 4 rotations, plus no-face samples from
held-out scikit-image photos (brick, chelsea, coins, gravel, immunohistochemistry, page, text —
a third of the set, never used in training), FairFace-validation backgrounds and procedural scenes
with unseen seeds.

## Results (int8 model, from `reports/EVALUATION.md`)

| Condition | Accuracy | Face coverage @ 0.7 | Precision when trusted | Confident-wrong (faces) | No-face trusted |
|---|---|---|---|---|---|
| Random | 93.9% | 83.1% | 99.2% | 0.6% | 1.7% |
| Normal | 95.7% | 87.7% | 99.5% | 0.4% | 0.9% |
| Dim | 95.1% | 86.0% | 99.3% | 0.5% | 1.7% |
| Very low light | 86.2% | 67.9% | 97.6% | 1.5% | 2.9% |
| Overexposed | 93.4% | 81.9% | 99.1% | 0.6% | 1.7% |
| Backlit | 93.4% | 78.2% | 99.4% | 0.4% | 1.1% |
| Motion Blur | 95.3% | 86.0% | 99.4% | 0.5% | 1.4% |


Per class (random lighting): upright 94.5 %, rotated-left 94.8 %, rotated-right 94.6 %,
upside-down 94.3 %, no-face 83.9 %.

**Quantisation:** int8 vs fp32 differs by ≤ 0.3 points in every condition (e.g. random lighting 93.9% vs 94.0%).

**Group audit** (FairFace perceived-race / gender annotations; face samples, random lighting):
Black 92.5 %, White 94.0 %, Middle Eastern 94.3 %, Indian 94.8 %, East Asian 95.3 %,
Latino/Hispanic 95.5 %, Southeast Asian 95.7 %; female 95.3 %, male 93.8 %. The largest gap is 3.2
points (Black vs Southeast Asian). Low light is the condition most likely to widen it, since
it reduces contrast on darker skin; this was not separately measured. Because the app falls
back to normal auto-rotate whenever confidence is low, a lower score shows up mostly as
"face-aware rotation helps less often", not as wrong rotations (confident-wrong stays ≤ 1.6 %
in every condition).

**Runtime:** single-thread inference p50 0.34 ms / p95 0.55 ms on an x86-64 Xeon @ 2.1 GHz.
Not yet measured on ARM phones (see README "Latency").

## Limitations and caveats

* **Domain gap.** Evaluation uses *simulated* camera frames rendered from photographs taken by
  other people (mostly frontal, well-framed portraits). Real front-camera frames — a face in a
  pillow at 25 cm, extreme angles, partial faces at the frame edge, IR-lit night scenes —
  are not represented in the test set, so on-device accuracy is unknown until measured.
* Profile (side-on) faces, faces looking away, and multiple faces are under-represented.
* "No face" is the weakest class (81 %); about 1.5 % of non-face frames are trusted as a face.
  When that happens the app either does what gravity says anyway or (if the false answer
  matches the current orientation) keeps the current orientation once.
* Children and very old faces follow FairFace's age distribution; not separately audited.
* The model knows nothing about *who* is in front of the camera; with several people it
  answers for whatever dominates the thumbnail.

## Recommendations

* Collect consented, on-device evaluation frames (not training data) on real phones and
  report accuracy per lighting condition before any release beyond testing.
* Keep the confidence threshold ≥ 0.7; lowering it trades fewer fallbacks for more wrong
  rotations.
