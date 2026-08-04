//! Pure retry policy, accounting, and loop control.

use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::{RetryStats, Usage, UsageEvidence};

/// Scale for jitter samples. Values consumed by [`next_delay`] are in
/// `[0, JITTER_SCALE)`, matching [`Duration::subsec_nanos`].
pub(super) const JITTER_SCALE: u32 = 1_000_000_000;

/// Return a production jitter sample in the exact domain consumed by
/// [`next_delay`].
pub(super) fn system_jitter() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RetryPolicy {
    pub(super) max_attempts: u32,
    pub(super) base: Duration,
    pub(super) max_delay: Duration,
    pub(super) total_budget: Duration,
}

impl RetryPolicy {
    pub(super) const DEFAULT: Self = Self {
        max_attempts: 4,
        base: Duration::from_secs(2),
        max_delay: Duration::from_secs(20),
        total_budget: Duration::from_secs(45),
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AttemptOutcome {
    TransportFailure { timed_out: bool },
    HttpStatus(u16),
}

/// Ratified transient statuses may be retried. Their bodies can carry billed
/// usage, and missing counters are not evidence that no billing occurred. A
/// request timeout proves even less: the provider may have generated and
/// billed a full response that never reached us, so timeouts are never retried.
pub(super) fn is_retryable(outcome: AttemptOutcome) -> bool {
    match outcome {
        AttemptOutcome::TransportFailure { timed_out } => !timed_out,
        AttemptOutcome::HttpStatus(status) => matches!(status, 429 | 500 | 502 | 503 | 504),
    }
}

pub(super) enum AttemptResult<T> {
    Success(T),
    Failure {
        outcome: AttemptOutcome,
        retry_after: Option<Duration>,
        detail: String,
        usage: Option<Usage>,
        elapsed: Duration,
    },
}

#[derive(Debug)]
pub(super) struct RetryOutput<T> {
    pub(super) value: T,
    pub(super) stats: RetryStats,
    pub(super) salvaged_usage: Option<Usage>,
    usage_overflowed: bool,
}

impl<T> RetryOutput<T> {
    /// Combine retry-attempt evidence with the successful response. Overflow
    /// refuses a numeric result instead of relabelling the last attempt exact.
    pub(super) fn combine_success_usage(&mut self, success: Option<Usage>) -> Option<Usage> {
        if self.usage_overflowed {
            self.salvaged_usage = None;
            return None;
        }
        combine_usage(self.salvaged_usage.take(), success)
    }
}

#[derive(Debug)]
pub struct RetryExhausted {
    pub stats: RetryStats,
    pub usage: Option<Usage>,
    pub last_failure: String,
    pub attempts: u32,
    pub elapsed: Duration,
}

impl fmt::Display for RetryExhausted {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.attempts == 1 {
            return formatter.write_str(&self.last_failure);
        }
        write!(
            formatter,
            "provider call failed after {} attempts over {}; last provider failure: {}",
            self.attempts,
            elapsed_label(self.elapsed),
            self.last_failure
        )
    }
}

impl std::error::Error for RetryExhausted {}

fn elapsed_label(elapsed: Duration) -> String {
    if elapsed.subsec_nanos() == 0 {
        format!("{}s", elapsed.as_secs())
    } else {
        format!("{:.3}s", elapsed.as_secs_f64())
    }
}

/// Parse only the delta-seconds form from Retry-After. HTTP dates and
/// malformed values are deliberately ignored.
pub(super) fn parse_retry_after(value: &str) -> Option<Duration> {
    value.trim().parse::<u64>().ok().map(Duration::from_secs)
}

/// Compute one jittered delay. `retry_index` is zero for the first retry.
/// `jitter_nanos` must be in `[0, JITTER_SCALE)` and maps linearly across the
/// ratified `[0.5, 1.0]` jitter span.
pub(super) fn next_delay(
    policy: RetryPolicy,
    retry_index: u32,
    retry_after: Option<Duration>,
    elapsed: Duration,
    jitter_nanos: u32,
) -> Option<Duration> {
    debug_assert!(jitter_nanos < JITTER_SCALE);
    if retry_index >= policy.max_attempts.saturating_sub(1) {
        return None;
    }

    let computed = retry_after.unwrap_or_else(|| {
        let multiplier = 1_u32.checked_shl(retry_index).unwrap_or(u32::MAX);
        policy.base.checked_mul(multiplier).unwrap_or(Duration::MAX)
    });
    let computed = computed.min(policy.max_delay);
    let nanos = computed.as_nanos();
    let minimum = nanos / 2;
    let span = nanos - minimum;
    let jittered_nanos =
        minimum + span.saturating_mul(u128::from(jitter_nanos)) / u128::from(JITTER_SCALE);
    let jittered = duration_from_nanos(jittered_nanos);

    elapsed
        .checked_add(jittered)
        .filter(|total| *total <= policy.total_budget)
        .map(|_| jittered)
}

fn duration_from_nanos(nanos: u128) -> Duration {
    let seconds = nanos / 1_000_000_000;
    if seconds > u128::from(u64::MAX) {
        return Duration::MAX;
    }
    Duration::new(seconds as u64, (nanos % 1_000_000_000) as u32)
}

pub(super) fn run_with_retry<T>(
    policy: RetryPolicy,
    sleep: &dyn Fn(Duration),
    jitter_nanos: &dyn Fn() -> u32,
    mut attempt: impl FnMut(u32) -> AttemptResult<T>,
) -> Result<RetryOutput<T>, RetryExhausted> {
    let mut attempts = 0_u32;
    let mut elapsed = Duration::ZERO;
    let mut stats = RetryStats::default();
    let mut salvaged_usage = None;
    let mut usage_overflowed = false;
    let mut saw_unreported_usage = false;

    loop {
        attempts = attempts.saturating_add(1);
        match attempt(attempts) {
            AttemptResult::Success(value) => {
                if !usage_overflowed && saw_unreported_usage && salvaged_usage.is_none() {
                    salvaged_usage = Some(Usage {
                        prompt_tokens: 0,
                        completion_tokens: 0,
                        cached_tokens: None,
                        evidence: UsageEvidence::Unknown,
                    });
                }
                return Ok(RetryOutput {
                    value,
                    stats,
                    salvaged_usage: if usage_overflowed {
                        None
                    } else {
                        salvaged_usage
                    },
                    usage_overflowed,
                });
            }
            AttemptResult::Failure {
                outcome,
                retry_after,
                detail,
                usage,
                elapsed: attempt_elapsed,
            } => {
                elapsed = elapsed.saturating_add(attempt_elapsed);
                if outcome == AttemptOutcome::HttpStatus(429) {
                    stats.rate_limited = stats.rate_limited.saturating_add(1);
                }
                if !usage_overflowed {
                    if let Some(mut usage) = usage {
                        if saw_unreported_usage {
                            usage.mark_unreported_component();
                        }
                        usage_overflowed = !merge_usage(&mut salvaged_usage, usage);
                    } else if let Some(salvaged) = &mut salvaged_usage {
                        saw_unreported_usage = true;
                        salvaged.mark_unreported_component();
                    } else {
                        saw_unreported_usage = true;
                    }
                }

                if !is_retryable(outcome) {
                    return Err(RetryExhausted {
                        stats,
                        usage: if usage_overflowed {
                            None
                        } else {
                            salvaged_usage
                        },
                        last_failure: detail,
                        attempts,
                        elapsed,
                    });
                }

                let retry_index = attempts.saturating_sub(1);
                let Some(delay) =
                    next_delay(policy, retry_index, retry_after, elapsed, jitter_nanos())
                else {
                    return Err(RetryExhausted {
                        stats,
                        usage: if usage_overflowed {
                            None
                        } else {
                            salvaged_usage
                        },
                        last_failure: detail,
                        attempts,
                        elapsed,
                    });
                };
                sleep(delay);
                elapsed = elapsed.saturating_add(delay);
                stats.retries = stats.retries.saturating_add(1);
            }
        }
    }
}

fn merge_usage(total: &mut Option<Usage>, next: Usage) -> bool {
    let Some(existing) = total else {
        *total = Some(next);
        return true;
    };
    existing.checked_add_assign(&next)
}

pub(super) fn combine_usage(first: Option<Usage>, second: Option<Usage>) -> Option<Usage> {
    let mut total = first;
    match second {
        Some(usage) => {
            if !merge_usage(&mut total, usage) {
                return None;
            }
        }
        None => {
            if let Some(usage) = &mut total {
                usage.mark_unreported_component();
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::UsageEvidence;
    use std::cell::RefCell;

    fn zero_policy(max_attempts: u32) -> RetryPolicy {
        RetryPolicy {
            max_attempts,
            base: Duration::ZERO,
            max_delay: Duration::ZERO,
            total_budget: Duration::ZERO,
        }
    }

    fn failure(outcome: AttemptOutcome, usage: Option<Usage>) -> AttemptResult<&'static str> {
        AttemptResult::Failure {
            outcome,
            retry_after: None,
            detail: format!("{outcome:?}"),
            usage,
            elapsed: Duration::ZERO,
        }
    }

    #[test]
    fn classification_retries_only_ratified_failures() {
        let cases = [
            (AttemptOutcome::TransportFailure { timed_out: false }, true),
            (AttemptOutcome::TransportFailure { timed_out: true }, false),
            (AttemptOutcome::HttpStatus(429), true),
            (AttemptOutcome::HttpStatus(500), true),
            (AttemptOutcome::HttpStatus(502), true),
            (AttemptOutcome::HttpStatus(503), true),
            (AttemptOutcome::HttpStatus(504), true),
            (AttemptOutcome::HttpStatus(400), false),
            (AttemptOutcome::HttpStatus(401), false),
            (AttemptOutcome::HttpStatus(403), false),
            (AttemptOutcome::HttpStatus(404), false),
            (AttemptOutcome::HttpStatus(408), false),
        ];
        for (outcome, expected) in cases {
            assert_eq!(is_retryable(outcome), expected, "{outcome:?}");
        }
    }

    #[test]
    fn exponential_delay_grows_and_caps_at_twenty_seconds() {
        let policy = RetryPolicy::DEFAULT;
        assert_eq!(
            next_delay(policy, 0, None, Duration::ZERO, JITTER_SCALE / 2),
            Some(Duration::from_millis(1_500))
        );
        assert_eq!(
            next_delay(policy, 1, None, Duration::ZERO, JITTER_SCALE / 2),
            Some(Duration::from_secs(3))
        );
        assert_eq!(
            next_delay(policy, 2, None, Duration::ZERO, JITTER_SCALE / 2),
            Some(Duration::from_secs(6))
        );
        let extended = RetryPolicy {
            max_attempts: 8,
            ..policy
        };
        assert_eq!(
            next_delay(extended, 4, None, Duration::ZERO, JITTER_SCALE / 2),
            Some(Duration::from_secs(15))
        );
        assert_eq!(
            next_delay(extended, 5, None, Duration::ZERO, JITTER_SCALE / 2),
            Some(Duration::from_secs(15))
        );
    }

    #[test]
    fn retry_after_is_honored_and_clamped() {
        let policy = RetryPolicy::DEFAULT;
        assert_eq!(
            next_delay(
                policy,
                0,
                Some(Duration::from_secs(7)),
                Duration::ZERO,
                JITTER_SCALE / 2
            ),
            Some(Duration::from_millis(5_250))
        );
        assert_eq!(
            next_delay(
                policy,
                0,
                Some(Duration::from_secs(90)),
                Duration::ZERO,
                JITTER_SCALE / 2
            ),
            Some(Duration::from_secs(15))
        );
    }

    #[test]
    fn malformed_and_http_date_retry_after_are_ignored() {
        assert_eq!(parse_retry_after("17"), Some(Duration::from_secs(17)));
        assert_eq!(parse_retry_after(" 3 "), Some(Duration::from_secs(3)));
        assert_eq!(parse_retry_after("later"), None);
        assert_eq!(parse_retry_after("Wed, 21 Oct 2015 07:28:00 GMT"), None);
    }

    #[test]
    fn budget_or_attempt_exhaustion_returns_none() {
        let policy = RetryPolicy::DEFAULT;
        assert_eq!(
            next_delay(policy, 0, None, Duration::from_secs(44), JITTER_SCALE - 1),
            None
        );
        assert_eq!(next_delay(policy, 3, None, Duration::ZERO, 0), None);
    }

    #[test]
    fn jitter_stays_within_half_and_full_computed_delay() {
        let policy = RetryPolicy::DEFAULT;
        assert_eq!(
            next_delay(policy, 1, None, Duration::ZERO, 0),
            Some(Duration::from_secs(2))
        );
        assert_eq!(
            next_delay(policy, 1, None, Duration::ZERO, JITTER_SCALE - 1),
            Some(Duration::from_nanos(3_999_999_998))
        );
        let middle = next_delay(policy, 1, None, Duration::ZERO, JITTER_SCALE / 2).unwrap();
        assert!(middle >= Duration::from_secs(2));
        assert!(middle <= Duration::from_secs(4));
    }

    #[test]
    fn top_jitter_sample_reaches_within_one_nanosecond_of_full_delay() {
        let delay = next_delay(
            RetryPolicy::DEFAULT,
            0,
            None,
            Duration::ZERO,
            JITTER_SCALE - 1,
        )
        .unwrap();
        assert_eq!(Duration::from_secs(2) - delay, Duration::from_nanos(1));
    }

    #[test]
    fn production_jitter_stays_in_the_documented_domain() {
        for _ in 0..64 {
            assert!(system_jitter() < JITTER_SCALE);
        }
    }

    #[test]
    fn exhausted_display_omits_attempt_narration_until_a_retry_occurs() {
        let provider_error = "provider returned HTTP 401 - key rejected";
        let one_attempt = RetryExhausted {
            stats: RetryStats::default(),
            usage: None,
            last_failure: provider_error.into(),
            attempts: 1,
            elapsed: Duration::from_millis(412),
        };
        assert_eq!(one_attempt.to_string(), provider_error);

        let multiple_attempts = RetryExhausted {
            stats: RetryStats {
                retries: 1,
                rate_limited: 0,
            },
            usage: None,
            last_failure: provider_error.into(),
            attempts: 2,
            elapsed: Duration::from_millis(1_250),
        };
        assert_eq!(
            multiple_attempts.to_string(),
            "provider call failed after 2 attempts over 1.250s; last provider failure: provider returned HTTP 401 - key rejected"
        );
    }

    #[test]
    fn full_loop_succeeds_later_and_reports_retry_stats_and_usage() {
        let script = RefCell::new(vec![
            failure(
                AttemptOutcome::HttpStatus(429),
                Some(Usage {
                    prompt_tokens: 10,
                    completion_tokens: 2,
                    cached_tokens: Some(3),
                    evidence: UsageEvidence::Measured,
                }),
            ),
            failure(
                AttemptOutcome::HttpStatus(503),
                Some(Usage {
                    prompt_tokens: 4,
                    completion_tokens: 1,
                    cached_tokens: Some(1),
                    evidence: UsageEvidence::Measured,
                }),
            ),
            AttemptResult::Success("done"),
        ]);
        let slept = RefCell::new(Vec::new());
        let output = run_with_retry(
            zero_policy(4),
            &|delay| slept.borrow_mut().push(delay),
            &|| 0,
            |_| script.borrow_mut().remove(0),
        )
        .unwrap();

        assert_eq!(output.value, "done");
        assert_eq!(
            output.stats,
            RetryStats {
                retries: 2,
                rate_limited: 1
            }
        );
        let usage = output.salvaged_usage.unwrap();
        assert_eq!(usage.prompt_tokens, 14);
        assert_eq!(usage.completion_tokens, 3);
        assert_eq!(usage.cached_tokens, Some(4));
        assert_eq!(*slept.borrow(), vec![Duration::ZERO, Duration::ZERO]);
    }

    #[test]
    fn unmetered_failed_attempt_degrades_success_in_retry_caller() {
        let missing = crate::wire::openai::extract_usage(
            r#"{"error":{"message":"temporarily unavailable"}}"#,
        );
        let success = crate::wire::openai::parse_response(
            r#"{"choices":[{"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":7,"completion_tokens":4}}"#,
        )
        .unwrap()
        .usage;
        let script = RefCell::new(vec![
            failure(AttemptOutcome::HttpStatus(503), missing),
            AttemptResult::Success("done"),
        ]);
        let mut output = run_with_retry(zero_policy(2), &|_| {}, &|| 0, |_| {
            script.borrow_mut().remove(0)
        })
        .unwrap();

        let usage = output.combine_success_usage(success).unwrap();

        assert_eq!(usage.evidence, UsageEvidence::Partial);
        assert_eq!((usage.prompt_tokens, usage.completion_tokens), (7, 4));
    }

    #[test]
    fn retry_usage_overflow_refuses_success_number() {
        let first = crate::wire::openai::extract_usage(
            r#"{"error":{"message":"busy"},"usage":{"prompt_tokens":18446744073709551615,"completion_tokens":0}}"#,
        );
        let second = crate::wire::openai::extract_usage(
            r#"{"error":{"message":"still busy"},"usage":{"prompt_tokens":1,"completion_tokens":0}}"#,
        );
        let success = crate::wire::openai::parse_response(
            r#"{"choices":[{"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":2,"completion_tokens":1}}"#,
        )
        .unwrap()
        .usage;
        let script = RefCell::new(vec![
            failure(AttemptOutcome::HttpStatus(429), first),
            failure(AttemptOutcome::HttpStatus(503), second),
            AttemptResult::Success("done"),
        ]);
        let mut output = run_with_retry(zero_policy(3), &|_| {}, &|| 0, |_| {
            script.borrow_mut().remove(0)
        })
        .unwrap();

        assert!(output.combine_success_usage(success).is_none());
    }

    #[test]
    fn full_loop_exhausts_into_typed_error_without_inventing_usage() {
        let attempts = RefCell::new(0_u32);
        let error = run_with_retry(zero_policy(4), &|_| {}, &|| 0, |_| {
            *attempts.borrow_mut() += 1;
            failure(AttemptOutcome::HttpStatus(500), None)
        })
        .unwrap_err();

        assert_eq!(*attempts.borrow(), 4);
        assert_eq!(error.attempts, 4);
        assert_eq!(
            error.stats,
            RetryStats {
                retries: 3,
                rate_limited: 0
            }
        );
        assert!(error.usage.is_none());
        assert!(error.to_string().contains("4 attempts over 0s"));
    }

    #[test]
    fn timeout_stops_after_one_attempt() {
        let attempts = RefCell::new(0_u32);
        let error = run_with_retry(
            zero_policy(4),
            &|_| panic!("timeout must not sleep"),
            &|| 0,
            |_| {
                *attempts.borrow_mut() += 1;
                failure(AttemptOutcome::TransportFailure { timed_out: true }, None)
            },
        )
        .unwrap_err();
        assert_eq!(*attempts.borrow(), 1);
        assert_eq!(error.stats.retries, 0);
    }
}
