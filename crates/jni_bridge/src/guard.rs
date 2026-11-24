//! Panic firewall for every FFI entry point (JNI exports and NDK callbacks).
//! Unwinding across an `extern` boundary is undefined behaviour (or an abort),
//! so every boundary calls [`ffi_guard`], which turns a panic into a logged
//! error and a safe default return value.

use std::panic::{catch_unwind, AssertUnwindSafe};

/// Runs `f`, returning `default` if it panics.
///
/// `AssertUnwindSafe` is sound here because after a caught panic we never
/// touch state the closure may have left half-updated: shared state lives
/// behind `Mutex`es (poisoning is handled by the callers) or atomics.
pub fn ffi_guard<R>(name: &'static str, default: R, f: impl FnOnce() -> R) -> R {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(payload) => {
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "<non-string panic>".into());
            // Panic messages in this crate never contain image data.
            log::error!("panic caught at FFI boundary `{name}`: {msg}");
            default
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_value_or_default() {
        assert_eq!(ffi_guard("ok", 0, || 5), 5);
        #[allow(clippy::panic)]
        let r = ffi_guard("boom", -1, || -> i32 { panic!("boom") });
        assert_eq!(r, -1);
    }
}
