use std::sync::atomic::{AtomicU64, Ordering};

/// Slow-frame threshold in microseconds (4 ms). Frames whose render
/// preparation takes longer than this are logged at WARN level.
pub(crate) const SLOW_FRAME_THRESHOLD_US: u64 = 4_000;

/// Last frame render time in microseconds, stored atomically so the status bar
/// can read it without requiring a reference to `AppRoot`.
static LAST_FRAME_TIME_US: AtomicU64 = AtomicU64::new(0);

/// Store the most recent frame render time (called from `AppRoot::render`).
pub(crate) fn store_frame_time(elapsed_us: u64) {
    LAST_FRAME_TIME_US.store(elapsed_us, Ordering::Relaxed);
}

/// Returns the most recent frame render time in microseconds.
///
/// Used by the status bar to display a dev-only frame-time readout.
pub fn last_frame_time_us() -> u64 {
    LAST_FRAME_TIME_US.load(Ordering::Relaxed)
}

/// Returns the slow-frame threshold in microseconds.
///
/// Exposed so the status bar can colour-code the readout relative to the
/// threshold.
pub fn slow_frame_threshold_us() -> u64 {
    SLOW_FRAME_THRESHOLD_US
}

/// Returns `true` when the elapsed render-preparation time exceeds the
/// slow-frame threshold.
pub(crate) fn is_slow_frame(elapsed_us: u64) -> bool {
    elapsed_us > SLOW_FRAME_THRESHOLD_US
}
