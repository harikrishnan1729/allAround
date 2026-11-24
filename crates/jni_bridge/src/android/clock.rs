//! Engine clock: `CLOCK_BOOTTIME` in milliseconds, the same time base Android
//! uses for sensor event timestamps, so sensor and camera events order correctly.

pub fn now_ms() -> u64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: `ts` is a valid, writable timespec on our stack and
    // CLOCK_BOOTTIME is supported by every Android kernel (API 17+).
    let rc = unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut ts) };
    if rc != 0 {
        return 0;
    }
    (ts.tv_sec as u64).saturating_mul(1000).saturating_add(ts.tv_nsec as u64 / 1_000_000)
}
