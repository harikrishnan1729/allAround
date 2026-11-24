//! # facetilt-jni (`libfacetilt.so`)
//!
//! The JNI surface of FaceTilt. Kotlin (`dev.facetilt.engine.NativeBridge`)
//! sees only these functions:
//!
//! | Kotlin                                   | Purpose                                    |
//! |------------------------------------------|--------------------------------------------|
//! | `nativeStart(listener, pkg, rot, cfg)`   | register sensors, start the engine thread  |
//! | `nativeStop()`                           | close camera, unregister sensors, join     |
//! | `nativeSetConfig(cfg)`                   | update tunables                            |
//! | `nativeSetDisplayRotation(rot)`          | the display rotated for another reason     |
//! | `nativeReportApplied(us)`                | apply-stage timing for the debug screen    |
//! | `nativeGetStats(): long[]`               | counters + stage timings (no image data)   |
//! | `nativeFrontCameraId(): String?`         | lets Kotlin read the Java-side orientation |
//!
//! and the callback `DecisionListener.onOrientationDecision(rotation, reason)`
//! with two small ints. **No image data ever crosses JNI.**
//!
//! Every export runs inside [`guard::ffi_guard`]: no panic can cross into the JVM.
//! The Android implementation lives in [`android`] (compiled only for
//! `target_os = "android"`); on other targets the exports compile against a
//! stub so the crate can be linted and unit-tested on a workstation.

#![warn(missing_docs)]

pub mod config;
pub mod guard;

#[cfg(target_os = "android")]
mod android;

#[cfg(not(target_os = "android"))]
mod host_stub;
#[cfg(not(target_os = "android"))]
use host_stub as platform;

#[cfg(target_os = "android")]
use android as platform;

use jni::objects::{JClass, JFloatArray, JObject, JString};
use jni::sys::{jboolean, jint, jlong, jlongArray, jstring, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;

use crate::guard::ffi_guard;

fn read_config(env: &mut JNIEnv<'_>, arr: &JFloatArray<'_>) -> Option<config::BridgeConfig> {
    let len = env.get_array_length(arr).ok()?;
    let len = usize::try_from(len).ok()?;
    if len != config::LEN {
        log::warn!("config array has {len} entries, expected {}", config::LEN);
        return None;
    }
    let mut buf = [0f32; config::LEN];
    env.get_float_array_region(arr, 0, &mut buf).ok()?;
    match config::parse(&buf) {
        Ok(c) => Some(c),
        Err(e) => {
            log::warn!("invalid config: {e:?}");
            None
        }
    }
}

/// `static boolean nativeStart(DecisionListener listener, String packageName, int rotation, float[] config)`
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_facetilt_engine_NativeBridge_nativeStart<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    listener: JObject<'l>,
    package: JString<'l>,
    rotation: jint,
    cfg: JFloatArray<'l>,
) -> jboolean {
    ffi_guard("nativeStart", JNI_FALSE, || {
        let Some(cfg) = read_config(&mut env, &cfg) else { return JNI_FALSE };
        let Ok(package) = env.get_string(&package).map(String::from) else { return JNI_FALSE };
        let Ok(vm) = env.get_java_vm() else { return JNI_FALSE };
        let Ok(listener) = env.new_global_ref(listener) else { return JNI_FALSE };
        let rotation = facetilt_decision::Rotation::try_from_index(rotation).unwrap_or(facetilt_decision::Rotation::R0);
        if platform::start(vm, listener, &package, rotation, cfg) {
            JNI_TRUE
        } else {
            JNI_FALSE
        }
    })
}

/// `static void nativeStop()`
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_facetilt_engine_NativeBridge_nativeStop<'l>(_env: JNIEnv<'l>, _class: JClass<'l>) {
    ffi_guard("nativeStop", (), platform::stop);
}

/// `static boolean nativeSetConfig(float[] config)`
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_facetilt_engine_NativeBridge_nativeSetConfig<'l>(
    mut env: JNIEnv<'l>,
    _class: JClass<'l>,
    cfg: JFloatArray<'l>,
) -> jboolean {
    ffi_guard("nativeSetConfig", JNI_FALSE, || match read_config(&mut env, &cfg) {
        Some(c) => {
            platform::set_config(c);
            JNI_TRUE
        }
        None => JNI_FALSE,
    })
}

/// `static void nativeSetDisplayRotation(int rotation)`
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_facetilt_engine_NativeBridge_nativeSetDisplayRotation<'l>(
    _env: JNIEnv<'l>,
    _class: JClass<'l>,
    rotation: jint,
) {
    ffi_guard("nativeSetDisplayRotation", (), || {
        if let Some(r) = facetilt_decision::Rotation::try_from_index(rotation) {
            platform::set_display_rotation(r);
        }
    });
}

/// `static void nativeReportApplied(long applyMicros)`
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_facetilt_engine_NativeBridge_nativeReportApplied<'l>(
    _env: JNIEnv<'l>,
    _class: JClass<'l>,
    apply_us: jlong,
) {
    ffi_guard("nativeReportApplied", (), || {
        platform::report_applied(u32::try_from(apply_us.max(0)).unwrap_or(u32::MAX));
    });
}

/// `static long[] nativeGetStats()` — layout: `facetilt_core::StatsSnapshot::to_array`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_facetilt_engine_NativeBridge_nativeGetStats<'l>(
    env: JNIEnv<'l>,
    _class: JClass<'l>,
) -> jlongArray {
    ffi_guard("nativeGetStats", std::ptr::null_mut(), || {
        let stats = platform::stats();
        let Ok(len) = i32::try_from(stats.len()) else { return std::ptr::null_mut() };
        let Ok(arr) = env.new_long_array(len) else { return std::ptr::null_mut() };
        if env.set_long_array_region(&arr, 0, &stats).is_err() {
            return std::ptr::null_mut();
        }
        arr.into_raw()
    })
}

/// `static String nativeFrontCameraId()` — the camera the engine uses, or null.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_facetilt_engine_NativeBridge_nativeFrontCameraId<'l>(
    env: JNIEnv<'l>,
    _class: JClass<'l>,
) -> jstring {
    ffi_guard("nativeFrontCameraId", std::ptr::null_mut(), || match platform::front_camera_id() {
        Some(id) => env.new_string(id).map(JString::into_raw).unwrap_or(std::ptr::null_mut()),
        None => std::ptr::null_mut(),
    })
}
