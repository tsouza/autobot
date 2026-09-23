//! The API request token bucket.

use autobot_kernel::profile::ApiBudget;
use std::num::NonZeroU32;
use std::time::{Duration, Instant};

/// Bucket units per request: the level is kept in billionths of a request, so one nanosecond
/// at `r` requests per second refills exactly `r` units and the arithmetic is exact.
const UNITS_PER_REQUEST: u128 = 1_000_000_000;

/// A token bucket granting API requests at a sustained rate with a burst.
#[derive(Debug, Clone)]
pub struct ApiLimiter {
    /// Requests per second, which is also the units refilled per nanosecond.
    rate: u128,
    /// Bucket size in units.
    capacity: u128,
    /// Current level in units.
    level: u128,
    /// The time the level was last brought up to date.
    refilled_at: Instant,
}

impl ApiLimiter {
    /// A full bucket of `burst` requests refilling at `requests_per_second`, starting at `now`.
    #[must_use]
    pub fn new(requests_per_second: NonZeroU32, burst: NonZeroU32, now: Instant) -> Self {
        let capacity = u128::from(burst.get()) * UNITS_PER_REQUEST;
        Self {
            rate: u128::from(requests_per_second.get()),
            capacity,
            level: capacity,
            refilled_at: now,
        }
    }

    /// The limiter for a profile's API budget, starting at `now`.
    #[must_use]
    pub fn from_budget(budget: &ApiBudget, now: Instant) -> Self {
        Self::new(budget.requests_per_second, budget.burst, now)
    }

    /// Takes one request from the bucket at `now`.
    ///
    /// A `now` earlier than a previous call refills nothing.
    ///
    /// # Errors
    ///
    /// The bucket holds less than one request; the error is the time after which it holds one.
    pub fn try_acquire(&mut self, now: Instant) -> Result<(), Duration> {
        self.refill(now);
        if self.level >= UNITS_PER_REQUEST {
            self.level -= UNITS_PER_REQUEST;
            return Ok(());
        }
        let nanos = (UNITS_PER_REQUEST - self.level).div_ceil(self.rate);
        // At most `UNITS_PER_REQUEST` nanoseconds, since `rate` is at least 1.
        Err(Duration::from_nanos(
            u64::try_from(nanos).unwrap_or(u64::MAX),
        ))
    }

    /// Adds what the bucket earned between the last refill and `now`, up to its size.
    fn refill(&mut self, now: Instant) {
        let Some(elapsed) = now.checked_duration_since(self.refilled_at) else {
            return;
        };
        let earned = elapsed.as_nanos().saturating_mul(self.rate);
        self.level = self.level.saturating_add(earned).min(self.capacity);
        self.refilled_at = now;
    }
}
