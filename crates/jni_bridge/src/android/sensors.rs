//! NDK sensor access (`ASensorManager`). Registered only while the engine
//! runs, i.e. while the screen is on and unlocked (Kotlin starts/stops it).
//!
//! * Gravity (`TYPE_GRAVITY`, fused and already low-pass) if available,
//!   otherwise the raw accelerometer — at `SENSOR_DELAY_GAME` (20 ms), no batching.
//! * Proximity (non-wake-up variant when the device has one), on-change.
//! * Hinge angle (API 30+, foldables only), on-change.

use std::ffi::CString;
use std::fmt;
use std::ptr::{self, NonNull};

use facetilt_core::Event;
use ndk::looper::ThreadLooper;
use ndk_sys as ffi;

/// `SENSOR_DELAY_GAME`.
const GAME_US: i32 = 20_000;
/// On-change sensors: the rate is a hint only.
const ON_CHANGE_US: i32 = 200_000;

#[derive(Debug)]
pub struct SensorError(&'static str);

impl fmt::Display for SensorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

pub struct Sensors {
    queue: NonNull<ffi::ASensorEventQueue>,
    manager: NonNull<ffi::ASensorManager>,
    gravity: NonNull<ffi::ASensor>,
    gravity_is_accel: bool,
    proximity: Option<NonNull<ffi::ASensor>>,
    hinge: Option<NonNull<ffi::ASensor>>,
    /// Largest distance reported so far; binary proximity sensors report
    /// 0 (near) or their maximum range (far), and the NDK has no getMaxRange.
    proximity_far: f32,
}

// SAFETY (Send not implemented on purpose): the queue is bound to the looper
// of the thread that created it; `Sensors` never leaves the engine thread.

fn default_sensor(m: NonNull<ffi::ASensorManager>, ty: i32, wake_up: Option<bool>) -> Option<NonNull<ffi::ASensor>> {
    // SAFETY: `m` is a valid ASensorManager (process-lifetime singleton).
    let p = unsafe {
        match wake_up {
            Some(w) => ffi::ASensorManager_getDefaultSensorEx(m.as_ptr(), ty, w),
            None => ffi::ASensorManager_getDefaultSensor(m.as_ptr(), ty),
        }
    };
    NonNull::new(p.cast_mut())
}

impl Sensors {
    pub fn new(package: &str, looper: &ThreadLooper, ident: i32) -> Result<Self, SensorError> {
        let pkg = CString::new(package).map_err(|_| SensorError("package name contains NUL"))?;
        // SAFETY: `pkg` is a valid NUL-terminated string for the duration of the
        // call; the returned manager is a process-lifetime singleton.
        let manager = NonNull::new(unsafe { ffi::ASensorManager_getInstanceForPackage(pkg.as_ptr()) })
            .ok_or(SensorError("no sensor manager"))?;
        let (gravity, gravity_is_accel) = match default_sensor(manager, ffi::ASENSOR_TYPE_GRAVITY, None) {
            Some(g) => (g, false),
            None => (
                default_sensor(manager, ffi::ASENSOR_TYPE_ACCELEROMETER, None)
                    .ok_or(SensorError("no accelerometer"))?,
                true,
            ),
        };
        let proximity = default_sensor(manager, ffi::ASENSOR_TYPE_PROXIMITY, Some(false))
            .or_else(|| default_sensor(manager, ffi::ASENSOR_TYPE_PROXIMITY, None));
        let hinge = default_sensor(manager, ffi::ASENSOR_TYPE_HINGE_ANGLE, None);

        // SAFETY: manager valid; the looper belongs to this thread and outlives
        // the queue (the queue is destroyed in Drop, on this thread, before the
        // thread — and its looper — exit). No callback: events are read in `drain`.
        let queue = unsafe {
            ffi::ASensorManager_createEventQueue(
                manager.as_ptr(),
                looper.as_foreign().ptr().as_ptr(),
                ident,
                None,
                ptr::null_mut(),
            )
        };
        let queue = NonNull::new(queue).ok_or(SensorError("cannot create sensor queue"))?;
        let s = Sensors { queue, manager, gravity, gravity_is_accel, proximity, hinge, proximity_far: 0.0 };
        s.register(gravity, GAME_US)?;
        if let Some(p) = proximity {
            // Optional: failure only disables the proximity skip rule.
            let _ = s.register(p, ON_CHANGE_US);
        }
        if let Some(h) = hinge {
            let _ = s.register(h, ON_CHANGE_US);
        }
        Ok(s)
    }

    fn register(&self, sensor: NonNull<ffi::ASensor>, period_us: i32) -> Result<(), SensorError> {
        // SAFETY: queue and sensor are valid; maxBatchReportLatencyUs = 0
        // (deliver immediately) — batching would add latency.
        let rc = unsafe { ffi::ASensorEventQueue_registerSensor(self.queue.as_ptr(), sensor.as_ptr(), period_us, 0) };
        if rc < 0 {
            Err(SensorError("registerSensor failed"))
        } else {
            Ok(())
        }
    }

    pub fn describe(&self) -> &'static str {
        if self.gravity_is_accel {
            "accelerometer"
        } else {
            "gravity"
        }
    }

    /// Reads all pending events and hands them to `f` as engine events.
    pub fn drain(&mut self, mut f: impl FnMut(u64, Event)) {
        // ASensorEvent is a plain C struct; zeroed is a valid bit pattern.
        // SAFETY: all-zero is valid for this repr(C) POD type.
        let mut buf: [ffi::ASensorEvent; 16] = unsafe { std::mem::zeroed() };
        loop {
            // SAFETY: queue valid; buf has room for buf.len() events.
            let n = unsafe { ffi::ASensorEventQueue_getEvents(self.queue.as_ptr(), buf.as_mut_ptr(), buf.len()) };
            if n <= 0 {
                break;
            }
            for e in buf.iter().take(n as usize) {
                let t_ms = u64::try_from(e.timestamp / 1_000_000).unwrap_or(0);
                // SAFETY: every sensor type we registered reports its values in
                // the leading `float data[16]` member of the event union.
                let data = unsafe { e.__bindgen_anon_1.__bindgen_anon_1.data };
                match e.type_ {
                    ffi::ASENSOR_TYPE_GRAVITY | ffi::ASENSOR_TYPE_ACCELEROMETER => {
                        f(t_ms, Event::Gravity { v: [data[0], data[1], data[2]] });
                    }
                    ffi::ASENSOR_TYPE_PROXIMITY => {
                        let d = data[0];
                        self.proximity_far = self.proximity_far.max(d);
                        let covered = if self.proximity_far > 0.0 { d < self.proximity_far } else { d < 1.0 };
                        f(t_ms, Event::Proximity { covered });
                    }
                    ffi::ASENSOR_TYPE_HINGE_ANGLE => f(t_ms, Event::Hinge { degrees: data[0] }),
                    _ => {}
                }
            }
        }
    }
}

impl Drop for Sensors {
    fn drop(&mut self) {
        // SAFETY: queue/sensors valid; disabling an unregistered sensor is a
        // harmless error; the queue is destroyed exactly once, on its thread.
        unsafe {
            ffi::ASensorEventQueue_disableSensor(self.queue.as_ptr(), self.gravity.as_ptr());
            for s in [self.proximity, self.hinge].into_iter().flatten() {
                ffi::ASensorEventQueue_disableSensor(self.queue.as_ptr(), s.as_ptr());
            }
            ffi::ASensorManager_destroyEventQueue(self.manager.as_ptr(), self.queue.as_ptr());
        }
    }
}
