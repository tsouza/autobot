//! The operator's API budget and work queue: the API budget of
//! `docs/design/AUTOBOT-M0-AND-GATES.md` §2, with the reserved control capacity that
//! `queue_fairness_bound` of `docs/design/AUTOBOT-FORMAL-SURFACE.md` §6 assumes.
//!
//! [`ApiLimiter`] is a token bucket that grants API requests at the profile's sustained rate
//! with its burst. [`WorkQueue`] is a bounded queue of keys, FIFO within [`Priority`], in which
//! [`Priority::Control`] work dequeues before any ordinary work and the control classes the
//! profile reserves have capacity that ordinary work cannot take. Neither type reads a clock or
//! blocks: the caller passes the current [`Instant`](std::time::Instant) and waits for the
//! [`Duration`](std::time::Duration) a refusal names, so both are deterministic under test.
//!
//! Choices this module makes where the design is open:
//!
//! - The bucket holds `burst` requests and refills at `requests_per_second`: the burst is the
//!   bucket size, not an allowance added to it, so over any interval of length `t` the limiter
//!   grants at most `burst + requests_per_second × t` requests. The bucket starts full.
//! - The reserve is a tenth of the queue's keys, rounded up (50 of the M0 profile's 500), shared
//!   by every reserved control class. The profile names the reserved classes but no size.
//!   Ordinary work may fill the queue only up to the keys outside the reserve; reserved
//!   control work may take any free key, so while fewer reserved control keys than the reserve
//!   are queued, reserved control work is always admitted.
//! - All control classes share one FIFO lane above every ordinary priority; a control class the
//!   profile does not reserve keeps that precedence but is admitted only into ordinary capacity.
//! - Keys are deduplicated. Enqueueing a key already queued keeps its place, unless the new
//!   priority ranks above the queued one, in which case the key moves to the back of the higher
//!   lane. Either way the queued key stays accepted and is dequeued once.
//! - A full queue refuses the key and hands it back in [`QueueFull`]; it never evicts a queued
//!   key. [`WorkQueue::admits`] lets the caller stop admission before it has a key to refuse.

mod limiter;
mod work;

pub use limiter::ApiLimiter;
pub use work::{Admission, Priority, QueueConfigError, QueueFull, WorkQueue};

#[cfg(test)]
mod tests;
