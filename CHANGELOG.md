# Changelog
All notable changes. Format based on [Keep a Changelog](https://keepachangelog.com/); versions follow [SemVer](https://semver.org/).
Versions before 1.0.1 were private builds, listed here for history.

## [1.0.1] - 2026-11-24
### Changed
- The app is now named **allAround**. Application ID `dev.facetilt` and internal names are unchanged.
- versionCode 2.
### Added
- LICENSE (Apache-2.0), NOTICE, THIRD_PARTY_NOTICES, CONTRIBUTING, CODE_OF_CONDUCT, SECURITY, issue and PR templates.
- Full README; Play Console answer sheet.

## [1.0.0] - 2026-11-21
### Changed
- Simplified UI: one switch on Home, short consent and permission screens, shorter Privacy screen, technical options under "Advanced".
- Target SDK 36 (Google Play requirement).
### Added
- Signed release builds (`keystore.properties`), native debug symbols in the bundle, R8 keep rules for the JNI callbacks.

## [0.2.0] - 2026-11-20
### Added
- Orientation proposer ported from Android's `WindowOrientationListener`: overhead detection, per-orientation tilt tolerance,
  22.5 degree hysteresis, flat / swing / acceleration gating.
### Changed
- Classifier input 32x32 to 48x48 (about 1.5 points more accurate in every lighting condition), 47 KB int8.

## [0.1.x] - 2026-11-20
### Added
- 0.1.2: **Only intervene when lying down** (assist mode).
- 0.1.1: optional **2nd verification** of portrait-to-landscape changes.
- 0.1.0: first version: Rust core (sensors, trigger state machine, NDK Camera2, 32x32 int8 classifier, decision engine), Kotlin shell
  (foreground service, Quick Settings tile, rotation appliers, consent and privacy screens), training pipeline, CI.
