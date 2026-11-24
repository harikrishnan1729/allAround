# allAround: privacy policy

*Applies to version 1.0.1. Plain-language version first; technical details follow.*

## In one paragraph

allAround is **off until you turn it on**. When it is on and you start rotating your phone, it
switches on the front camera for a fraction of a second, shrinks one frame to a 48 × 48
black-and-white thumbnail **in memory**, asks a tiny on-device model "which way is this head
turned?", erases the thumbnail, and switches the camera off. **No photo or video is ever saved,
nothing is sent anywhere — the app cannot even access the internet — and allAround cannot
recognise who you are.**

## What allAround collects

**Nothing.** allAround has no servers, no accounts, no analytics, no advertising, no crash
reporting. The app does not request the `INTERNET` permission (or any network permission);
the build fails if one is ever added, including by a library.

## What allAround stores on your phone

| Item | Why | Deleted by |
|---|---|---|
| Whether you accepted the explanation screen | Not showing it every time | "Delete all data" |
| Your settings (on/off, rotation method, threshold, upside-down, start after reboot) | Remembering your choices | "Delete all data" |
| Number of camera activations **today** (one integer and the date) | Shown on the Privacy screen | "Delete all data"; resets daily |
| Your original auto-rotate setting, while allAround replaces it | Restoring it when allAround is turned off | Automatically, when allAround turns off |

All of it is excluded from Android backup and device-to-device transfer. **No image, no
thumbnail, no face data, no sensor history and no timestamps of activations are ever stored.**

## What happens to camera data, step by step

1. **When:** the camera opens only between the start of a rotation of the phone and the
   decision (typically under 0.4 s), only while the screen is on and unlocked, and never more
   than once every 0.8 s (longer if you keep tilting back and forth). If you turn on
   **2nd verification**, the camera opens once more (same processing) after the screen has
   turned to landscape and the phone is held still.
2. **Resolution:** the smallest image size the camera offers (usually 176 × 144 to 320 × 240),
   with no preview on screen. Only the brightness (Y, "luma") channel is read; colour is never
   read or converted.
3. **Shrinking in place:** the brightness channel is averaged down to 48 × 48 = 2,304 values
   directly into a small stack buffer. The full-resolution frame is never copied, stored,
   logged or passed to the Java/Kotlin part of the app.
4. **Erasing the camera buffer:** the camera's buffer is overwritten with zeros (on devices
   that allow writable camera buffers; on the rest it is released immediately and the camera
   overwrites it with the next frame) and handed back to the camera before step 5.
5. **One question:** the on-device model (≈ 35,000 parameters, 47 KB) outputs one of five
   answers — head up, rotated left, rotated right, upside down, or no face — plus a
   confidence. It has no way to produce identity, landmarks or any "face print".
6. **Erasing the thumbnail:** the thumbnail and the model's normalised input are overwritten
   with zeros; the model's transient internal values are freed.
7. **Only the answer survives:** a single small number crosses from the native engine to the
   app, which rotates the screen (or not). The camera is closed.

## Your controls

* Master switch in the app, a **Pause** button in the (minimised, silent) notification, and a
  **Quick Settings tile**.
* Android's camera privacy indicator lights up every time — allAround never tries to hide it,
  and the status notification cannot be, and is not, hidden either. These are how Android tells
  you the camera may be in use, and allAround keeps them visible on purpose.
* Android 12+'s camera privacy toggle is respected: with camera access off, allAround behaves
  like normal auto-rotate.
* Denying or revoking the camera permission keeps allAround off; the phone uses normal
  auto-rotate.
* **Delete all data** (Privacy screen) turns allAround off, restores normal auto-rotate, and
  erases everything listed above.

## Permissions

| Permission | Used for |
|---|---|
| `CAMERA` | The front camera, only as described above |
| `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_CAMERA` | Android requires a visible foreground service for any app that may use the camera while you are in another app |
| `POST_NOTIFICATIONS` (Android 13+, optional) | The service notification with the Pause button |
| `WRITE_SETTINGS` **or** `SYSTEM_ALERT_WINDOW` (you choose one) | Rotating the screen: via the system rotation setting, or via an invisible 1 × 1 pixel window |
| `RECEIVE_BOOT_COMPLETED` | Only if you enable "start after reboot" (off by default) |

Not requested: internet, network state, location, microphone, contacts, storage, accounts,
advertising ID.

## Children

allAround collects no data from anyone, including children.

## Changes

Any change to this policy ships with the app's source and is listed in the release notes.
