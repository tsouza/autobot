//! The faults a [`FakeProvider`](super::FakeProvider) is scripted with.

use autobot_adapters::backoff::BackOff;
use autobot_adapters::provider::TransportFault;
use autobot_adapters::text::OperationName;

/// A call of the provider adapter trait.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Call {
    /// [`ProviderAdapter::send`](autobot_adapters::provider::ProviderAdapter::send).
    Send,
    /// [`ProviderAdapter::observe`](autobot_adapters::provider::ProviderAdapter::observe).
    Observe,
    /// [`ProviderAdapter::lookup`](autobot_adapters::provider::ProviderAdapter::lookup).
    Lookup,
    /// [`ProviderAdapter::dry_run`](autobot_adapters::provider::ProviderAdapter::dry_run).
    DryRun,
}

/// What a scripted fault does to the call it hits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// The request never reaches the provider: nothing is applied and the call ends in the
    /// transport fault, a timeout or a disconnect.
    Dropped(TransportFault),
    /// The provider applies a send and its acknowledgement is lost: the call ends in the
    /// transport fault. On a call other than a send the provider applies nothing, so this is
    /// [`Fault::Dropped`].
    LostAcknowledgement(TransportFault),
    /// The provider throttles this call and the next `calls - 1` calls the trigger matches:
    /// none of them is applied. A send ends in [`SendError::RateLimited`] stating `back_off`;
    /// any other call ends in [`TransportFault::Timeout`], since [`ProviderError`] has no
    /// throttled answer and an unknown outcome is the one answer that claims nothing (KERNEL
    /// §3.3). A window of zero calls throttles nothing.
    ///
    /// [`SendError::RateLimited`]: autobot_adapters::provider::SendError::RateLimited
    /// [`ProviderError`]: autobot_adapters::provider::ProviderError
    RateLimited {
        /// How many matching calls the window throttles.
        calls: u32,
        /// The back-off the provider states in each throttled answer.
        back_off: BackOff,
    },
}

/// Which calls a scripted fault hits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trigger {
    /// The call.
    pub call: Call,
    /// The operation the call is about, or `None` for any operation.
    pub operation: Option<OperationName>,
}

impl Trigger {
    /// The next `call` of any operation.
    #[must_use]
    pub fn any(call: Call) -> Self {
        Self {
            call,
            operation: None,
        }
    }

    /// The next `call` about `operation`.
    #[must_use]
    pub fn on(call: Call, operation: OperationName) -> Self {
        Self {
            call,
            operation: Some(operation),
        }
    }

    fn matches(&self, call: Call, operation: &OperationName) -> bool {
        self.call == call && self.operation.as_ref().is_none_or(|o| o == operation)
    }
}

/// An ordered list of scripted faults.
///
/// A call a refusal before send ends ([`SendError::Unqualified`] or
/// [`SendError::MissingHeadBase`]) never reaches the provider and consumes no entry. Every other
/// call consumes the first entry whose trigger matches it, if any, and suffers its fault; a
/// call no entry matches is answered faithfully.
///
/// [`SendError::Unqualified`]: autobot_adapters::provider::SendError::Unqualified
/// [`SendError::MissingHeadBase`]: autobot_adapters::provider::SendError::MissingHeadBase
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Script {
    entries: Vec<(Trigger, Fault)>,
}

impl Script {
    /// An empty script: every call is answered faithfully.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends `fault` for the next call `trigger` matches after the earlier entries.
    #[must_use]
    pub fn then(mut self, trigger: Trigger, fault: Fault) -> Self {
        self.push(trigger, fault);
        self
    }

    /// Appends `fault` for the next call `trigger` matches after the earlier entries.
    pub fn push(&mut self, trigger: Trigger, fault: Fault) {
        self.entries.push((trigger, fault));
    }

    /// Puts `fault` ahead of every entry, so the next call `trigger` matches suffers it.
    pub fn push_front(&mut self, trigger: Trigger, fault: Fault) {
        self.entries.insert(0, (trigger, fault));
    }

    /// Whether no entry is left.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Consumes the fault for one `call` about `operation`: the first matching entry's, or
    /// `None`.
    pub(super) fn take(&mut self, call: Call, operation: &OperationName) -> Option<Fault> {
        let index = self
            .entries
            .iter()
            .position(|(trigger, _)| trigger.matches(call, operation))?;
        match self.entries.get_mut(index) {
            Some((_, Fault::RateLimited { calls, back_off })) if *calls > 1 => {
                *calls -= 1;
                Some(Fault::RateLimited {
                    calls: 1,
                    back_off: *back_off,
                })
            }
            Some((_, Fault::RateLimited { calls: 0, .. })) => {
                self.entries.remove(index);
                self.take(call, operation)
            }
            _ => Some(self.entries.remove(index).1),
        }
    }
}
