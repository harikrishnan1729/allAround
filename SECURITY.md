# Security policy

## Supported versions
Only the latest release receives fixes.

## Reporting a vulnerability
**Please do not open a public issue.** Use GitHub's private reporting instead: on the repository page open
**Security → Advisories → Report a vulnerability**. (Maintainers: enable *Private vulnerability reporting* under
Settings → Code security.)

Include the app version, your Android version and device, what you found, and how to reproduce it. Reports are handled
on a best-effort basis; there is no guaranteed response time.

## What counts as a security or privacy issue
This app uses the camera, so the bar is high. Please report anything that:

* lets camera data (the full frame or the 48x48 thumbnail) be **stored, logged, sent, or passed across JNI**;
* adds **network access** of any kind (the app must never request `INTERNET` or any network permission);
* **hides or bypasses** the Android camera indicator or the foreground-service notification;
* keeps the camera open longer than intended, or opens it when the feature is off, the screen is off, or permission is revoked;
* leaks the user's settings or enables unintended permission use.

## Not in scope
The model being wrong about a head orientation is a bug (open a normal issue), not a vulnerability: the app is designed to
fall back to normal auto-rotate when it is unsure.
