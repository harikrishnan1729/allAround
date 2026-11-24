# Battery and performance testing guide

Target: **< 0.5 % battery per hour of typical use** with FaceTilt on, and **tilt onset →
decision < 250 ms** on a mid-range phone.

FaceTilt's design keeps the power budget structural rather than tuned:

| Component | When active | Typical cost |
|---|---|---|
| Gravity sensor at 50 Hz (`SENSOR_DELAY_GAME`, no batching) | Screen on **and** unlocked | Sensor hub, ~0.1–0.5 mA on modern SoCs; the engine thread sleeps in `ALooper_pollOnce` between samples |
| Proximity / hinge sensors (on-change) | Same | negligible |
| Front camera | Only between tilt onset and decision (≤ 1.5 s hard cap, typically 0.15–0.4 s), ≤ 1 open / 800 ms, exponential backoff when flapping | camera ~150–350 mA *while on*; at 30 activations/h × 0.3 s ≈ 9 s/h → ~0.4–0.9 mAh/h |
| Inference | Once per activation, < 5 ms single thread | negligible |
| Screen off | **Nothing**: sensors unregistered, engine thread stopped, no wake lock, no alarm | 0 |

Rough budget on a 4,500 mAh battery: 0.5 %/h = 22.5 mAh/h. The camera term above is ~4 % of
that budget; sensors and the CPU wake-ups at 50 Hz dominate and stay well below it. **These are
estimates — measure on your device with the procedure below.**

## 1. Built-in counters (debug build)

Debug builds show **Debug: latency & battery** on the home screen:

* camera-on seconds per hour, activations per hour (since the engine started),
* inference p50 / p95,
* last activation: onset → camera open, → classified frame, inference, → decision, apply time,
* evaluations / changed / kept-by-face / fallbacks / aborted counters.

Release builds keep the counters (they are a few integers) but do not show the screen.

## 2. Battery Historian

```bash
adb shell dumpsys batterystats --reset
adb shell dumpsys batterystats --enable full-wake-history
# unplug USB (or: adb shell dumpsys battery unplug), use the phone normally for >= 1 h
adb shell dumpsys battery reset
adb bugreport bugreport.zip
```

Run Battery Historian locally (see <https://github.com/google/battery-historian>, e.g.
`docker run -p 9999:9999 gcr.io/android-battery-historian/stable:3.0 --port 9999`) and load
`bugreport.zip`. Check, for package `dev.facetilt`:

* **Camera** row: short slivers only during rotations; total camera-on time ≈ the debug
  screen's camera-on seconds.
* **Wakelocks**: none held by FaceTilt (the app never acquires one).
* **Alarms / Jobs**: none.
* **Sensors**: gravity (and proximity) registered only during screen-on-and-unlocked periods.
* **Estimated power use** for the app, as % of device drain per hour.

Compare against a baseline hour with FaceTilt off.

## 3. Perfetto (latency and CPU)

The native code emits ATrace sections (visible when the app is traced via `atrace_apps`):
`facetilt:camera_open`, `facetilt:camera_close`, `facetilt:frame` (downsample + zero + release)
and `facetilt:inference`. Record a system trace with sched, camera and app slices:

```bash
cat > facetilt.pbtx <<'EOF'
buffers { size_kb: 65536 fill_policy: RING_BUFFER }
data_sources { config { name: "linux.ftrace" ftrace_config {
  ftrace_events: "sched/sched_switch" ftrace_events: "power/cpu_frequency"
  ftrace_events: "power/suspend_resume"
  atrace_categories: "camera" atrace_categories: "view" atrace_categories: "wm"
  atrace_apps: "dev.facetilt" } } }
data_sources { config { name: "android.power" android_power_config {
  battery_poll_ms: 1000 collect_power_rails: true } } }
duration_ms: 30000
EOF
adb push facetilt.pbtx /data/local/tmp/
adb shell perfetto --txt -c /data/local/tmp/facetilt.pbtx -o /data/misc/perfetto-traces/facetilt.pftrace
# rotate the phone a few times during the 30 s
adb pull /data/misc/perfetto-traces/facetilt.pftrace
```

Open it in <https://ui.perfetto.dev>. Useful tracks:

* threads `facetilt-engine` (sensor processing — should be short slices at 50 Hz),
  `facetilt-camera` (open/configure/close), the `AImageReader` callback thread (downsample +
  inference — a single slice per activation),
* camera HAL / `cameraserver` slices for open and first-frame latency,
* `power rails` (on Pixels) for the camera rail during activations.

Per-stage latency from tilt onset can also be read from logcat:

```bash
adb logcat -s FaceTilt
```

and from the debug screen (stage timestamps are measured inside the engine on the same
`CLOCK_BOOTTIME` clock as the sensor events).

## 4. Inference micro-benchmark on device

```bash
cargo ndk -t arm64-v8a build --release -p facetilt-vision --example bench_inference
adb push target/aarch64-linux-android/release/examples/bench_inference /data/local/tmp/
adb shell /data/local/tmp/bench_inference
```

## 5. Checklist

- [ ] Screen off for 10 min: `adb shell dumpsys sensorservice` lists no FaceTilt connection.
- [ ] `adb shell dumpsys power | grep -i facetilt` shows no wake locks.
- [ ] `adb shell dumpsys media.camera` shows the camera closed between rotations.
- [ ] Debug screen: camera-on seconds/hour < 30 in normal use; inference p95 < 5 ms.
- [ ] Battery Historian: FaceTilt < 0.5 %/h over a 1 h session of typical use.
