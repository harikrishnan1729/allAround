# Contributing to allAround

Thanks for helping. The biggest need right now is **testing on real phones**: latency, battery, and real-camera accuracy are
all unmeasured (see the Status section of the [README](README.md)).

By contributing you agree your contribution is licensed under the [Apache License 2.0](LICENSE), as in section 5 of that licence.
Please follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## Ground rules (these protect users, so they are not negotiable)
1. **No network.** Never add `INTERNET` or any network permission, analytics, ads, crash reporters, or network-capable crates.
   The build and CI fail if you do.
2. **Camera data stays in native memory.** Frames are reduced in place to the 48x48 thumbnail, the camera buffer is zeroed and
   released *before* inference, and the thumbnail is zeroed after. Never store, log, cache, or send image data, and never pass it
   across JNI (only the small class enum crosses).
3. **Never hide the camera indicator or the foreground-service notification.** Pull requests that try will be closed.
4. **Fail safe.** If the camera is unavailable or the model is unsure, behave like normal gravity auto-rotate.
5. **No panics across FFI** (`catch_unwind` at every boundary). `unsafe` belongs in `crates/jni_bridge` only, each block with a
   `SAFETY:` comment.
6. **The model only outputs head orientation.** No face recognition, identity embeddings, or landmarks.

## Getting set up
Follow [Building from source](README.md#building-from-source). `./gradlew assembleDebug` builds Rust and Kotlin together.

## Before you open a pull request
Run what CI runs:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo ndk -t arm64-v8a -P 29 clippy -p facetilt-jni --all-targets -- -D warnings
cargo test --workspace
cargo deny check
./gradlew testDebugUnitTest lintDebug
```

* Add or update tests. State-machine changes need a scenario in `crates/core/tests/scenarios.rs`.
* **Sensor traces are welcome:** `scripts/record_trace.sh` records gravity vectors only (no camera data) from a debug build.
* Keep the Kotlin `NativeBridge` layouts in step with the Rust side (config and stats arrays, reason codes); the contract tests
  pin them.
* Rust is formatted with the repo's `rustfmt.toml`; Kotlin follows the official style.

## Changing the model
Retrain with `training/` (see the README), then update together: the embedded model, the parity fixtures
(`python -m facetilt.export_golden`), `training/MODEL_CARD.md`, and the evaluation report. Keep the int8 model under 100 KB and report
accuracy per lighting condition. **Never commit datasets or images.** Respect the data licences in `training/DATA_LICENSES.md`.

## Pull requests and commits
Keep changes focused. Explain *why* in the description. Mention how you tested, and on which device and Android version if
you ran it on hardware.

## Reporting bugs
Use the issue templates. Include the app version, device model, Android version, and `adb logcat -s FaceTilt` output.
**Do not attach photos or screenshots of your face.** For security issues, see [SECURITY.md](SECURITY.md).
