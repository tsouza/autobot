//! The provider adapter: sends one external operation, observes it, looks it up and
//! deduplicates it, as `docs/design/AUTOBOT-KERNEL.md` §3.3 uses a provider.
//!
//! A provider is a forge, CI service or other remote party that applies effects. The broker
//! drives an adapter through the send sequence of KERNEL §3.3: it records the send attempt,
//! calls [`ProviderAdapter::send`], then [`ProviderAdapter::observe`]. After a timeout or a
//! disconnect the operation is `OUTCOME_UNKNOWN`, and reconciliation proves definitive
//! non-application only through [`ProviderAdapter::lookup`] or through deduplication by
//! [`ProviderAdapter::send`] of the same `operation_key`, and only where the operation's
//! [`ProviderCapability`] declares it.
//!
//! Choices this module makes where the design is open:
//!
//! - Deduplication is not a separate call: a send of an `operation_key` the provider already
//!   applied answers [`SendAck::Deduplicated`] with the first application's remote identity.
//! - [`ReconciliationMethod`] names the three ways KERNEL §3.3 resolves an operation in
//!   `RECONCILING`: provider lookup, provider deduplication, or, when the capability offers
//!   neither, human adjudication after `UNRESOLVED`.
//! - A remote marker is the `operation_key` written onto the remote object; a confirmed
//!   observation of an operation whose capability declares one returns it. A marker is not
//!   idempotency (forge mirroring).
//! - An adapter refuses, before any send, an operation it has no qualified capability for and a
//!   send that lacks the source and base heads its capability requires. The broker refuses both
//!   before any send (KERNEL §3.3); the adapter's refusal is a second fence, not the first.
//! - `COMPENSATED` has no contract yet (#318): this module offers no compensation call.

mod contract;

pub use contract::{ProviderFault, ProviderHarness, ProviderRule, run};

use crate::text::{Head, OperationName, ProviderName, RemoteIdentity, TargetIdentity};
use autobot_kernel::types::Digest;
use std::fmt;

/// The `operation_key` of KERNEL §3.3: the logical identity of one intended effect across
/// retries and restores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OperationKey(pub Digest);

impl fmt::Display for OperationKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// How an operation in `RECONCILING` is resolved (KERNEL §3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReconciliationMethod {
    /// A provider lookup by `operation_key` proves application or definitive non-application.
    ProviderLookup,
    /// A resend of the same `operation_key` is deduplicated by the provider.
    ProviderDeduplication,
    /// The provider offers neither: the operation becomes `UNRESOLVED` after
    /// `provider_reconcile_bound` and only a human adjudication resolves it.
    HumanAdjudication,
}

/// What a provider guarantees for one operation (FORMAL §2 `ProviderCapability`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderCapability {
    /// The provider.
    pub provider: ProviderName,
    /// The operation.
    pub operation: OperationName,
    /// A resend of an applied `operation_key` is deduplicated.
    pub supports_idempotency: bool,
    /// A lookup by `operation_key` answers applied or definitively not applied.
    pub supports_lookup: bool,
    /// The operation writes its `operation_key` onto the remote object.
    pub supports_remote_marker: bool,
    /// A send needs the source and base heads.
    pub requires_head_base: bool,
    /// The operation can be validated without being applied.
    pub supports_dry_run: bool,
    /// How an operation in `RECONCILING` is resolved.
    pub reconciliation_method: ReconciliationMethod,
    /// The adapter is qualified for this operation; an unqualified operation is never sent.
    pub qualified: bool,
}

/// Why a [`ProviderCapability`] contradicts itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityError {
    /// The reconciliation method is lookup, but lookup is not supported.
    LookupUnsupported,
    /// The reconciliation method is deduplication, but idempotency is not supported.
    IdempotencyUnsupported,
    /// The reconciliation method is human adjudication although lookup or idempotency is
    /// supported, which KERNEL §3.3 reserves for providers that offer neither.
    AdjudicationWithProof,
}

impl fmt::Display for CapabilityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::LookupUnsupported => "reconciles by lookup without supporting lookup",
            Self::IdempotencyUnsupported => {
                "reconciles by deduplication without supporting idempotency"
            }
            Self::AdjudicationWithProof => {
                "reconciles by human adjudication although lookup or idempotency is supported"
            }
        })
    }
}

impl std::error::Error for CapabilityError {}

impl ProviderCapability {
    /// Checks that the reconciliation method agrees with the declared semantics.
    ///
    /// # Errors
    ///
    /// The [`CapabilityError`] that describes the contradiction.
    pub fn check(&self) -> Result<(), CapabilityError> {
        match self.reconciliation_method {
            ReconciliationMethod::ProviderLookup if !self.supports_lookup => {
                Err(CapabilityError::LookupUnsupported)
            }
            ReconciliationMethod::ProviderDeduplication if !self.supports_idempotency => {
                Err(CapabilityError::IdempotencyUnsupported)
            }
            ReconciliationMethod::HumanAdjudication
                if self.supports_lookup || self.supports_idempotency =>
            {
                Err(CapabilityError::AdjudicationWithProof)
            }
            _ => Ok(()),
        }
    }
}

/// One send of an effect intent: the fields of the intent and of the `ExternalOperation` a
/// provider needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendRequest {
    /// The operation.
    pub operation: OperationName,
    /// The intent's `operation_key`.
    pub operation_key: OperationKey,
    /// The attempt: zero, then one more for every re-request after proven non-application.
    pub attempt_index: u32,
    /// The intent's `payload_digest`.
    pub payload_digest: Digest,
    /// The intent's `target_identity`.
    pub target_identity: TargetIdentity,
    /// The source head, when the operation has one.
    pub source_head: Option<Head>,
    /// The base head, when the operation has one.
    pub base_head: Option<Head>,
}

/// The provider's acknowledgement of a send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendAck {
    /// The provider accepted the operation; it applies it under this remote identity.
    Accepted(RemoteIdentity),
    /// The provider had already applied this `operation_key` and applied nothing new.
    Deduplicated(RemoteIdentity),
}

impl SendAck {
    /// The remote identity the acknowledgement names.
    #[must_use]
    pub fn remote_identity(&self) -> &RemoteIdentity {
        match self {
            Self::Accepted(r) | Self::Deduplicated(r) => r,
        }
    }
}

/// A transport failure: the call's outcome at the provider is unknown.
///
/// A send that ends in one is `OUTCOME_UNKNOWN`: stream closure never proves the operation
/// failed or did not apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportFault {
    /// No answer arrived within the call's deadline.
    Timeout,
    /// The connection closed before an answer arrived.
    Disconnect,
}

/// Why a send produced no acknowledgement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendError {
    /// Refused before any send: the adapter has no qualified capability for the operation.
    Unqualified,
    /// Refused before any send: the capability requires source and base heads the request
    /// lacks.
    MissingHeadBase,
    /// The request may or may not have reached the provider.
    Transport(TransportFault),
}

impl SendError {
    /// Whether the adapter refused before sending, so the provider applied nothing.
    #[must_use]
    pub fn before_send(&self) -> bool {
        matches!(self, Self::Unqualified | Self::MissingHeadBase)
    }
}

/// The state of an applied operation at the provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteOutcome {
    /// The provider has not finished applying it.
    Pending,
    /// Applied as desired.
    Confirmed,
    /// The provider refused it.
    Rejected,
    /// The provider tried and failed.
    Failed,
}

/// What an observation of a remote object shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    /// The operation's state at the provider.
    pub outcome: RemoteOutcome,
    /// The `operation_key` written on the remote object, when the capability declares a remote
    /// marker.
    pub marker: Option<OperationKey>,
}

/// The answer of a lookup by `operation_key`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    /// The provider applied the operation under this remote identity.
    Applied(RemoteIdentity),
    /// The provider definitively did not apply the operation.
    NotApplied,
}

/// Why an observation, lookup or dry run produced no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    /// The operation's capability does not declare the call; no answer proves anything.
    Unsupported,
    /// The call's outcome is unknown.
    Transport(TransportFault),
}

/// A provider adapter.
pub trait ProviderAdapter {
    /// The provider this adapter talks to.
    fn provider(&self) -> ProviderName;

    /// The capability the adapter declares for each operation it offers, one entry per
    /// operation.
    fn capabilities(&self) -> Vec<ProviderCapability>;

    /// Sends `request` once.
    ///
    /// # Errors
    ///
    /// [`SendError::Unqualified`] or [`SendError::MissingHeadBase`] before any send, and
    /// [`SendError::Transport`] when the outcome is unknown.
    fn send(&mut self, request: &SendRequest) -> Result<SendAck, SendError>;

    /// Observes the remote object an acknowledged send named.
    ///
    /// # Errors
    ///
    /// [`ProviderError::Transport`] when no answer arrived.
    fn observe(
        &mut self,
        operation: &OperationName,
        remote: &RemoteIdentity,
    ) -> Result<Observation, ProviderError>;

    /// Looks up `key` for `operation`.
    ///
    /// # Errors
    ///
    /// [`ProviderError::Unsupported`] when the operation's capability does not declare lookup,
    /// and [`ProviderError::Transport`] when no answer arrived.
    fn lookup(
        &mut self,
        operation: &OperationName,
        key: &OperationKey,
    ) -> Result<Lookup, ProviderError>;

    /// Validates `request` without applying it.
    ///
    /// # Errors
    ///
    /// [`ProviderError::Unsupported`] when the operation's capability does not declare a dry
    /// run, and [`ProviderError::Transport`] when no answer arrived.
    fn dry_run(&mut self, request: &SendRequest) -> Result<(), ProviderError>;
}

#[cfg(test)]
mod tests;
