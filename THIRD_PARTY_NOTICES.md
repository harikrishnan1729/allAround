# Third-party notices

allAround is licensed under Apache-2.0 (see [LICENSE](LICENSE) and [NOTICE](NOTICE)). It builds on the open-source software below.
Licence texts are in each package's own source distribution.

## Rust crates (linked into `libfacetilt.so`)

121 crates, all under permissive licences (MIT, Apache-2.0, BSD, Zlib, Unlicense, BlueOak). Checked in CI by `cargo deny check` (see `deny.toml`). Generated from `cargo metadata` for the Android target; regenerate it when dependencies change.

| Crate | Version | Licence |
|---|---|---|
| android_log-sys | 0.3.2 | MIT OR Apache-2.0 |
| android_logger | 0.15.1 | MIT OR Apache-2.0 |
| anyhow | 1.0.104 | MIT OR Apache-2.0 |
| anymap3 | 1.1.0 | BlueOak-1.0.0 OR MIT OR Apache-2.0 |
| autocfg | 1.5.1 | Apache-2.0 OR MIT |
| bit-set | 0.11.1 | Apache-2.0 OR MIT |
| bit-vec | 0.10.2 | Apache-2.0 OR MIT |
| bitflags | 2.13.2 | MIT OR Apache-2.0 |
| borsh | 1.8.1 | MIT OR Apache-2.0 |
| borsh-derive | 1.8.1 | Apache-2.0 |
| byteorder | 1.5.0 | Unlicense OR MIT |
| bytes | 1.12.2 | MIT |
| cc | 1.11.2 | MIT OR Apache-2.0 |
| cesu8 | 1.1.0 | Apache-2.0/MIT |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 |
| cfg_aliases | 0.2.2 | MIT |
| chacha20 | 0.10.2 | MIT OR Apache-2.0 |
| combine | 4.6.9 | MIT |
| derive-new | 0.7.0 | MIT |
| downcast-rs | 2.0.2 | MIT OR Apache-2.0 |
| dyn-clone | 1.0.20 | MIT OR Apache-2.0 |
| dyn-hash | 1.0.0 | MIT OR Apache-2.0 |
| either | 1.19.0 | MIT OR Apache-2.0 |
| env_filter | 0.1.4 | MIT OR Apache-2.0 |
| equivalent | 1.0.3 | Apache-2.0 OR MIT |
| erased-serde | 0.4.11 | MIT OR Apache-2.0 |
| errno | 0.3.15 | MIT OR Apache-2.0 |
| filetime | 0.2.30 | MIT/Apache-2.0 |
| find-msvc-tools | 0.1.15 | MIT OR Apache-2.0 |
| foldhash | 0.2.0 | Zlib |
| getrandom | 0.4.3 | MIT OR Apache-2.0 |
| half | 2.7.1 | MIT OR Apache-2.0 |
| hashbrown | 0.16.1 | MIT OR Apache-2.0 |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 |
| indexmap | 2.14.2 | Apache-2.0 OR MIT |
| inventory | 0.3.25 | MIT OR Apache-2.0 |
| itertools | 0.15.0 | MIT OR Apache-2.0 |
| itoa | 1.0.19 | MIT OR Apache-2.0 |
| jni | 0.21.1 | MIT/Apache-2.0 |
| jni-sys | 0.3.1 | MIT OR Apache-2.0 |
| jni-sys | 0.4.1 | MIT OR Apache-2.0 |
| jni-sys-macros | 0.4.1 | MIT OR Apache-2.0 |
| lazy_static | 1.5.1 | MIT OR Apache-2.0 |
| libc | 0.2.191 | MIT OR Apache-2.0 |
| libm | 0.2.16 | MIT |
| linux-raw-sys | 0.12.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| lock_api | 0.5.0 | MIT OR Apache-2.0 |
| log | 0.4.35 | MIT OR Apache-2.0 |
| maplit | 1.0.2 | MIT/Apache-2.0 |
| matrixmultiply | 0.3.11 | MIT/Apache-2.0 |
| memchr | 2.8.3 | Unlicense OR MIT |
| memmap2 | 0.9.12 | MIT OR Apache-2.0 |
| metrics | 0.24.6 | MIT |
| minijinja | 3.1.0 | Apache-2.0 |
| ndarray | 0.17.2 | MIT OR Apache-2.0 |
| ndk | 0.9.0 | MIT OR Apache-2.0 |
| ndk-sys | 0.6.0+11769913 | MIT OR Apache-2.0 |
| nom | 8.0.0 | MIT |
| nom-language | 0.1.0 | MIT |
| num-bigint | 0.4.8 | MIT OR Apache-2.0 |
| num-complex | 0.4.6 | MIT OR Apache-2.0 |
| num-integer | 0.1.47 | MIT OR Apache-2.0 |
| num-rational | 0.4.2 | MIT OR Apache-2.0 |
| num-traits | 0.2.19 | MIT OR Apache-2.0 |
| num_enum | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 |
| num_enum_derive | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 |
| once_cell | 1.21.4 | MIT OR Apache-2.0 |
| parking_lot | 0.13.0 | MIT OR Apache-2.0 |
| parking_lot_core | 0.10.0 | MIT OR Apache-2.0 |
| pastey | 0.2.3 | MIT OR Apache-2.0 |
| primal-check | 0.3.4 | MIT OR Apache-2.0 |
| proc-macro-crate | 3.5.0 | MIT OR Apache-2.0 |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 |
| prost | 0.14.6 | Apache-2.0 |
| prost-derive | 0.14.6 | Apache-2.0 |
| quote | 1.0.47 | MIT OR Apache-2.0 |
| rand | 0.10.4 | MIT OR Apache-2.0 |
| rand_core | 0.10.1 | MIT OR Apache-2.0 |
| rand_distr | 0.6.0 | MIT OR Apache-2.0 |
| rapidhash | 4.5.1 | MIT OR Apache-2.0 |
| rawpointer | 0.2.1 | MIT/Apache-2.0 |
| rustfft | 6.4.1 | MIT OR Apache-2.0 |
| rustix | 1.1.6 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| rustversion | 1.0.23 | MIT OR Apache-2.0 |
| same-file | 1.0.6 | Unlicense/MIT |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 |
| serde | 1.0.229 | MIT OR Apache-2.0 |
| serde_core | 1.0.229 | MIT OR Apache-2.0 |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 |
| serde_json | 1.0.154 | MIT OR Apache-2.0 |
| shlex | 2.0.1 | MIT OR Apache-2.0 |
| simd-adler32 | 0.3.10 | MIT |
| smallvec | 1.16.6 | MIT OR Apache-2.0 |
| strength_reduce | 0.2.4 | MIT OR Apache-2.0 |
| string-interner | 0.20.0 | MIT/Apache-2.0 |
| syn | 2.0.119 | MIT OR Apache-2.0 |
| syn | 3.0.10 | MIT OR Apache-2.0 |
| tar | 0.4.46 | MIT OR Apache-2.0 |
| thiserror | 1.0.69 | MIT OR Apache-2.0 |
| thiserror-impl | 1.0.69 | MIT OR Apache-2.0 |
| toml_datetime | 1.1.3+spec-1.1.0 | MIT OR Apache-2.0 |
| toml_edit | 0.25.19+spec-1.1.0 | MIT OR Apache-2.0 |
| toml_parser | 1.1.5+spec-1.1.0 | MIT OR Apache-2.0 |
| tract-core | 0.23.10 | MIT OR Apache-2.0 |
| tract-data | 0.23.10 | MIT OR Apache-2.0 |
| tract-extra | 0.23.10 | MIT OR Apache-2.0 |
| tract-hir | 0.23.10 | MIT OR Apache-2.0 |
| tract-linalg | 0.23.10 | MIT OR Apache-2.0 |
| tract-nnef | 0.23.10 | MIT OR Apache-2.0 |
| tract-onnx | 0.23.10 | MIT OR Apache-2.0 |
| tract-onnx-opl | 0.23.10 | MIT OR Apache-2.0 |
| transpose | 0.2.3 | MIT OR Apache-2.0 |
| typeid | 1.0.3 | MIT OR Apache-2.0 |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| walkdir | 2.5.0 | Unlicense/MIT |
| winnow | 1.0.5 | MIT |
| xattr | 1.7.0 | MIT OR Apache-2.0 |
| zerocopy | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT |
| zerocopy-derive | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT |
| zeroize | 1.9.2 | Apache-2.0 OR MIT |
| zmij | 1.0.23 | MIT |

## Android / Kotlin libraries

AndroidX (core, activity, lifecycle, DataStore), Jetpack Compose and Material 3, the Kotlin standard library and kotlinx.coroutines are licensed under the Apache License 2.0. Versions are pinned in `gradle/libs.versions.toml`.

## Algorithms, data and models

* **Orientation proposer:** port of Android's `WindowOrientationListener` (Apache-2.0, The Android Open Source Project).
* **Model weights:** trained on FairFace (CC BY 4.0). Attribution and the other data sources: [training/DATA_LICENSES.md](training/DATA_LICENSES.md).
* **Training tools** (PyTorch, ONNX, onnxruntime, NumPy, Pillow, scikit-image) are used offline and are not shipped in the app.
