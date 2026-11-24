## What and why

## How I tested
- [ ] `cargo fmt`, `cargo clippy -D warnings`, `cargo test --workspace`, `cargo deny check`
- [ ] `./gradlew testDebugUnitTest lintDebug`
- [ ] Tried on a device (model / Android version):

## Privacy checklist
- [ ] No new permissions, network access, telemetry, or logging of image data
- [ ] Camera frames still reduced in place, zeroed, and never passed across JNI
- [ ] Camera indicator and notification are untouched
