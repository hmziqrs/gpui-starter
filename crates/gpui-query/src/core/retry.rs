//! Retry configuration for failed query and mutation requests.

use serde::{Deserialize, Serialize};

/// Retry configuration for failed requests.
///
/// # Defaults
///
/// 3 retries with exponential backoff, 1-second base delay, 30-second cap.
///
/// # Builder pattern
///
/// ```
/// use gpui_query::RetryPolicy;
///
/// let policy = RetryPolicy::new(5)
///     .with_delay(500)
///     .with_exponential_backoff()
///     .with_max_delay(10_000);
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts. 0 = no retries.
    pub max_retries: u32,
    /// Base delay in milliseconds between retries.
    pub retry_delay_ms: u64,
    /// Whether to use exponential backoff.
    pub exponential_backoff: bool,
    /// Maximum delay cap for exponential backoff (in milliseconds).
    pub max_retry_delay_ms: u64,
}

impl RetryPolicy {
    /// Create a policy that never retries.
    pub fn no_retries() -> Self {
        Self {
            max_retries: 0,
            retry_delay_ms: 0,
            exponential_backoff: false,
            max_retry_delay_ms: 0,
        }
    }

    /// Create a policy with the given maximum number of retries.
    pub fn new(max_retries: u32) -> Self {
        Self {
            max_retries,
            retry_delay_ms: 1000,
            exponential_backoff: false,
            max_retry_delay_ms: 30_000,
        }
    }

    /// Set the base delay between retries (in milliseconds).
    pub fn with_delay(mut self, delay_ms: u64) -> Self {
        self.retry_delay_ms = delay_ms;
        self
    }

    /// Enable exponential backoff for retries.
    pub fn with_exponential_backoff(mut self) -> Self {
        self.exponential_backoff = true;
        self
    }

    /// Set the maximum delay cap for exponential backoff (in milliseconds).
    pub fn with_max_delay(mut self, max_ms: u64) -> Self {
        self.max_retry_delay_ms = max_ms;
        self
    }

    /// Absolute ceiling for any single retry delay (1 hour in milliseconds).
    /// This prevents effectively-infinite sleeps even when `max_retry_delay_ms`
    /// is set to an unreasonably large value.
    const ABSOLUTE_MAX_DELAY_MS: u64 = 3_600_000;

    /// Calculate the delay for the given attempt number (0-based).
    ///
    /// With exponential backoff the delay is `retry_delay_ms * 2^attempt`,
    /// capped first by `max_retry_delay_ms` and then by a hard ceiling of
    /// one hour.  The shift amount is itself limited to 62 so the factor
    /// never exceeds `2^62`, preventing intermediate overflow.
    pub fn delay_for_attempt(&self, attempt: u32) -> u64 {
        if !self.exponential_backoff {
            return self.retry_delay_ms;
        }
        // Cap the shift so the factor stays well within u64 range.
        let shift = attempt.min(62);
        let factor = 1u64 << shift;
        let delay = self.retry_delay_ms.checked_mul(factor).unwrap_or(u64::MAX);
        delay
            .min(self.max_retry_delay_ms)
            .min(Self::ABSOLUTE_MAX_DELAY_MS)
    }

    /// Whether a retry should be attempted given the current retry count.
    pub fn should_retry(&self, current_retries: u32) -> bool {
        current_retries < self.max_retries
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::new(3)
            .with_exponential_backoff()
            .with_delay(1000)
            .with_max_delay(30_000)
    }
}
