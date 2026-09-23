//! The back-off a provider states when it rate limits a call.

/// How long a provider asks its caller to wait before calling again: whole seconds after the
/// answer that states it.
///
/// A re-request after a rate-limit answer is sent no earlier than the provider's back-off
/// allows (`docs/design/AUTOBOT-KERNEL.md` §3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BackOff {
    /// The seconds to wait, counted from the answer.
    pub seconds: u64,
}
