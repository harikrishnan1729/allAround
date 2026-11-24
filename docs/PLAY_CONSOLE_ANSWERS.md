# Play Console: answers to paste in

App: **allAround** · ID `dev.facetilt` · file `allAround-1.0.1.aab`
(Field names below follow Play Console as I know it; wording in the Console may differ slightly.)

## 0. Before you start
1. Create the app: name above, **App**, **Free**, language English. Accept the declarations.
2. Host `PRIVACY.md` at a public URL (e.g. GitHub Pages or a Gist) and keep the link for step 3.
3. **Test the `-test.apk` on a real phone first.** It is the same build as the AAB.
4. Upload the AAB to **Testing → Internal testing** first. Keep **Play App Signing** on (default).

## 1. Store listing
* **App name:** allAround
* **Short description (80):** Keeps your screen upright for your face, so lying down doesn't flip it sideways.
* **Full description:** (in `PLAY_RELEASE.md`)
* **Category:** Tools · **Tags:** utility · **Contact email:** yours · **Privacy policy URL:** from step 0.2
* **Graphics:** 512×512 icon, 1024×500 feature graphic, 2+ phone screenshots (take them on your phone).

## 2. App content
| Section | Answer |
|---|---|
| Privacy policy | URL from step 0.2 |
| Ads | **No**, the app contains no ads |
| App access | **All functionality is available without special access** (no login) |
| Content rating (IARC) | Category **Utility**. Answer **No** to violence, sexual content, language, drugs, gambling, user-generated content, location sharing and purchases |
| Target audience | **18 and over** (keeps you out of the Families policy) |
| News app / Government / Financial / Health | **No** to all |
| Data safety | See section 3 |
| Advertising ID | **No**, the app does not use it |

## 3. Data safety
* Does your app collect or share any of the required user data types? **No**
* Because nothing is collected or shared, the data-type, encryption-in-transit and deletion-request questions do not apply.
* Camera images are processed on the device, in memory, for milliseconds, never stored or sent. The app has no `INTERNET` permission.
* Preview the summary and **Submit**.

## 4. Foreground service declaration (App content → Foreground service permissions)
* **Type:** Camera
* **Description:**
  > The user turns on face-aware auto-rotate in the app. While it is on, a foreground service lets the app open the front camera for a fraction of a second (typically under 0.4 s) when the user rotates the phone, to check which way the user's head is turned. The frame is reduced to a tiny grayscale image in memory, used once, and erased. Nothing is stored or transmitted. A persistent notification with a Pause button is shown while the feature is on, and the user can turn it off from the app, the notification, or a Quick Settings tile.
* **Demo video** (30–60 s, screen recording): open the app → turn the switch on → allow camera → show the notification → lie on your side and rotate the phone, with the camera indicator visible → turn it off.

## 5. Permission justifications (if Play asks)
* **`SYSTEM_ALERT_WINDOW` (display over other apps):** optional rotation method the user chooses. Draws an invisible 1×1 px, non-interactive window whose only purpose is to set the screen orientation. It shows no content and intercepts no touches.
* **`WRITE_SETTINGS` (modify system settings):** used only to set the system screen-rotation setting to apply the decided orientation. The user's original setting is restored when the feature is turned off.
* **`RECEIVE_BOOT_COMPLETED`:** only if the user enables "Start after reboot" (off by default). It posts a notification to tap, and nothing else.
* **`POST_NOTIFICATIONS`:** the required foreground-service notification (optional for the user to allow).

## 6. Release
**Testing → Internal testing → Create release →** upload the AAB → release name `1.0.0` → notes "First release" → Save → Roll out. Add yourself as a tester and install from the opt-in link.
Then promote to **Closed testing** or **Production** as Play Console allows. Some new personal accounts must complete a closed test first, and Play Console shows the current requirement.
