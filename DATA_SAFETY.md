# Data safety summary (Google Play "Data safety" form)

| Question | Answer |
|---|---|
| Does your app collect or share any of the required user data types? | **No** |
| Is all of the user data collected by your app encrypted in transit? | Not applicable — no data is collected or transmitted (the app has no network permission) |
| Do you provide a way for users to request that their data is deleted? | Not applicable — no data is collected. Locally stored settings can be erased in-app (Privacy → Delete all data) |
| Data shared with third parties | **None** |
| Data collected | **None** |

### Notes for the reviewer

* **Camera:** frames are processed ephemerally, on device, in memory, for well under a second,
  and are never stored or transmitted. Under Play's definitions, on-device processing that is
  not sent off the device is not "collection".
* **Photos and videos / Personal info / Device IDs / App activity / Diagnostics:** none
  collected, none shared.
* **Network:** the APK declares no `INTERNET`, `ACCESS_NETWORK_STATE` or Wi-Fi permissions;
  `app/build.gradle.kts` fails the build if the merged manifest ever contains one, and CI runs
  `scripts/check_no_internet.sh` on the final APK.
* **SDKs:** no analytics, advertising, crash-reporting or networking SDKs (audited by
  `cargo deny check` and `scripts/audit_deps.sh`).
* **Foreground service type `camera` justification:** the user explicitly enables face-aware
  auto-rotation; the service accesses the front camera only for the brief window between a
  device-tilt onset and the rotation decision, and shows a persistent notification with a Pause
  action while enabled.
