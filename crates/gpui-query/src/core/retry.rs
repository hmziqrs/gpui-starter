//! Retry configuration for failed query and mutation requests.
//!
//! [`RetryPolicy`] controls whether and how failed requests are retried.
//! It supports fixed delays, exponential backoff with a configurable cap,
//! and a maximum retry count.

use serde::{Deserialize, Serialize};

/// Retry configuration for failed requests.
///
/// # Defaults
///
/// The [`Default`](RetryPolicy::default) implementation uses 3 retries with
/// exponential backoff, a 1-second base delay, and a 30-second cap.
///
/// # Builder pattern
///
/// ```
/// use gpui_query::core::RetryPolicy;
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
    ///
    /// Starts with sensible defaults: 1-second fixed delay, no backoff,
    /// no delay cap. Use the builder methods to customize further.
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
    ///
    /// Each subsequent retry doubles the delay, capped at
    /// [`max_retry_delay_ms`](RetryPolicy::max_retry_delay_ms).
    pub fn with_exponential_backoff(mut self) -> Self {
        self.exponential_backoff = true;
        self
    }

    /// Set the maximum delay cap for exponential backoff (in milliseconds).
    pub fn with_max_delay(mut self, max_ms: u64) -> Self {
        self.max_retry_delay_ms = max_ms;
        self
    }

    /// Calculate the delay for the given attempt number (0-based).
    ///
    /// - Without exponential backoff, returns [`retry_delay_ms`](RetryPolicy::retry_delay_ms).
    /// - With exponential backoff, returns `retry_delay_ms * 2^attempt`,
    ///   capped at [`max_retry_delay_ms`](RetryPolicy::max_retry_delay_ms).
    pub fn delay_for_attempt(&self, attempt: u32) -> u64 {
        if !self.exponential_backoff {
            return self.retry_delay_ms;
        }

        // Compute retry_delay_ms * 2^attempt, saturating at u64::MAX.
        let factor = 1u64.checked_shl(attempt).unwrap_or(u64::MAX);
        let delay = self
            .retry_delay_ms
            .checked_mul(factor)
            .unwrap_or(u64::MAX);

        delay.min(self.max_retry_delay_ms)
    }

    /// Whether a retry should be attempted given the current retry count.
    ///
    /// Returns `true` when `current_retries < max_retries`.
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

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_retries_disables_retry() {
        let policy = RetryPolicy::no_retries();
        assert_eq!(policy.max_retries, 0);
        assert!(!policy.should_retry(0));
        assert!(!policy.should_retry(1));
    }

    #[test]
    fn new_sets_max_retries() {
        let policy = RetryPolicy::new(5);
        assert_eq!(policy.max_retries, 5);
        assert!(policy.should_retry(4));
        assert!(!policy.should_retry(5));
    }

    #[test]
    fn should_retry_boundary() {
        let policy = RetryPolicy::new(3);
        assert!(policy.should_retry(0));
        assert!(policy.should_retry(1));
        assert!(policy.should_retry(2));
        assert!(!policy.should_retry(3));
        assert!(!policy.should_retry(10));
    }

    #[test]
    fn delay_for_attempt_fixed() {
        let policy = RetryPolicy::new(3).with_delay(500);
        assert_eq!(policy.delay_for_attempt(0), 500);
        assert_eq!(policy.delay_for_attempt(1), 500);
        assert_eq!(policy.delay_for_attempt(5), 500);
    }

    #[test]
    fn delay_for_attempt_exponential() {
        let policy = RetryPolicy::new(5)
            .with_delay(1000)
            .with_exponential_backoff()
            .with_max_delay(30_000);

        assert_eq!(policy.delay_for_attempt(0), 1000); // 1000 * 2^0
        assert_eq!(policy.delay_for_attempt(1), 2000); // 1000 * 2^1
        assert_eq!(policy.delay_for_attempt(2), 4000); // 1000 * 2^2
        assert_eq!(policy.delay_for_attempt(3), 8000); // 1000 * 2^3
        assert_eq!(policy.delay_for_attempt(4), 16000); // 1000 * 2^4
        assert_eq!(policy.delay_for_attempt(5), 30000); // capped at max
    }

    #[test]
    fn delay_for_attempt_exponential_capped() {
        let policy = RetryPolicy::new(10)
            .with_delay(500)
            .with_exponential_backoff()
            .with_max_delay(2000);

        assert_eq!(policy.delay_for_attempt(0), 500);
        assert_eq!(policy.delay_for_attempt(1), 1000);
        assert_eq!(policy.delay_for_attempt(2), 2000); // 2000 exact
        assert_eq!(policy.delay_for_attempt(3), 2000); // capped
        assert_eq!(policy.delay_for_attempt(10), 2000); // still capped
    }

    #[test]
    fn default_policy() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_retries, 3);
        assert!(policy.exponential_backoff);
        assert_eq!(policy.retry_delay_ms, 1000);
        assert_eq!(policy.max_retry_delay_ms, 30_000);
    }

    #[test]
    fn builder_chain() {
        let policy = RetryPolicy::new(2)
            .with_delay(200)
            .with_exponential_backoff()
            .with_max_delay(5000);

        assert_eq!(policy.max_retries, 2);
        assert_eq!(policy.retry_delay_ms, 200);
        assert!(policy.exponential_backoff);
        assert_eq!(policy.max_retry_delay_ms, 5000);
    }

    #[test]
    fn serde_roundtrip() {
        let policy = RetryPolicy::default();
        let json = serde_json::to_string(&policy).unwrap();
        let back: RetryPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(policy, back);
    }

    #[test]
    fn serde_no_retries() {
        let policy = RetryPolicy::no_retries();
        let json = serde_json::to_string(&policy).unwrap();
        let back: RetryPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(policy, back);
    }
}
