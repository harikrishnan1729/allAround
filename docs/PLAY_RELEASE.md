# Publishing to Google Play

Bundle: `dist/allAround-1.0.1.aab` (signed with the upload key, `release/upload-keystore.jks`).

## Keep safe (or you cannot ship updates)
* `release/upload-keystore.jks` and `keystore.properties` are **not in git**. Back both up somewhere private (password manager).
* In Play Console, enrol in **Play App Signing** (the default). Google then holds the real app-signing key and the upload key is
  replaceable if lost. The app ID **`dev.facetilt` is permanent** once uploaded.

## Store listing (fits Play limits)
* **Title** (9/30): allAround. The title is short, so the short description below carries the search keywords.
* **Short description** (80/80): Keeps your screen upright for your face, so lying down doesn't flip it sideways.
* **Full description:**
  Rotates your screen the way Pixel's face detection does: only when your head turns with the phone. Lying on your side no
  longer flips the screen to landscape. When you rotate the phone, the front camera opens for a split second, a tiny
  black-and-white image is checked on your device and erased, and the screen rotates only if your head turned too. If
  the camera isn't available, normal auto-rotate is used. Nothing is saved or sent, and the app has no internet permission.
  Off until you turn it on. Android's camera indicator shows whenever the camera is used.
* Needed assets: 512x512 icon, 1024x500 feature graphic, at least 2 phone screenshots (take them on a real device).

## Play Console forms
1. **Privacy policy URL**: host `PRIVACY.md` at a public URL and paste it.
2. **Data safety**: answers are in `DATA_SAFETY.md` (no data collected or shared).
3. **Foreground service permissions** (App content): declare type **Camera**; Play asks for a description and a short video
   showing the feature (turn it on, rotate the phone while lying down, camera indicator visible).
4. **Sensitive permissions**: `SYSTEM_ALERT_WINDOW` and `WRITE_SETTINGS` may need a justification: they are only used to
   rotate the screen, and the user picks one.
5. **Content rating** questionnaire and **Target audience**: not designed for children.
6. Start with an **internal / closed test**. Some new developer accounts must run a closed test before production, and Play
   Console shows the current rule.

## Checks already done
Target SDK 36 (Play's requirement since 2026-08-31), arm64 library 16 KB-aligned, no network permissions, R8 keep rules for
the JNI callbacks, native debug symbols bundled. **Not done: running on a real device.** Test before the first rollout.
