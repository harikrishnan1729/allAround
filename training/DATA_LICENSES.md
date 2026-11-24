# Training data sources and licences

No image data is redistributed in this repository except the *mean* face thumbnail described
below. The training pipeline downloads/reads the datasets locally.

## Faces — FairFace (CC BY 4.0)

* **Dataset:** FairFace: Face Attribute Dataset for Balanced Race, Gender, and Age
  (Kärkkäinen & Joo, WACV 2021). <https://github.com/joojs/fairface>
* **Licence:** **CC BY 4.0** — verified on 2026-11-20 in the project's README
  (`License: CC BY 4.0`, <https://raw.githubusercontent.com/joojs/fairface/master/README.md>).
  Attribution: *K. Kärkkäinen and J. Joo, "FairFace: Face Attribute Dataset for Balanced Race,
  Gender, and Age for Bias Measurement and Mitigation", WACV 2021.*
* **Files used:** `fairface-img-margin125-trainval.zip` (padding = 1.25 crops, 448×448;
  86,744 train / 10,954 validation images), plus `fairface_label_val.csv` (age/gender/race
  annotations, used **only** to audit accuracy across groups).
* **Changes made:** decoded to luma, rescaled, cropped, rotated, re-lit, noised and box-filtered
  to 32×32 (see `facetilt/dataset.py`).
* The images originate from YFCC-100M (Flickr Creative Commons). We use them as distributed
  by the FairFace authors under CC BY 4.0.

### Redistributed derivative

`crates/vision/tests/fixtures/classifier.bin` contains the **mean** of 2,000 FairFace
validation images (a single blurred average face, 32×32, at four rotations). It is an
adaptation of CC BY 4.0 material — attribution above — and does not depict any individual.

## Non-face images

| Source | Images | Licence |
|---|---|---|
| scikit-image sample data (`skimage.data`, v0.26) | brick, grass, gravel | CC0 (CC0Textures) |
| | cell, retina | CC0 1.0 |
| | chelsea (cat), clock, coffee, horse | CC0 / public domain, by the photographers (per scikit-image docs) |
| | hubble_deep_field | NASA, public domain |
| | rocket | SpaceX, released into the public domain |
| | coins, immunohistochemistry, text | "No known copyright restrictions" (scikit-image docs) |
| | moon, page, logo, checkerboard, colorwheel | Part of scikit-image (BSD-3-Clause); checkerboard/colorwheel are synthetic |
| FairFace (above) | Background corners of the padded crops, and views showing ≤ ~25 % of a face | CC BY 4.0 |
| Procedural | Synthetic indoor-like scenes (gradients, edges, lamps, blobs) | Generated, no licence needed |

Images containing people from `skimage.data` (`astronaut`, `camera`) and the bundled
`lfw_subset` are **not** used.

## Datasets considered and rejected

| Dataset | Reason |
|---|---|
| CelebA, FFHQ (as a dataset), UTKFace, WIDER FACE | Non-commercial licences |
| LFW, Olivetti/AT&T | No explicit licence |
