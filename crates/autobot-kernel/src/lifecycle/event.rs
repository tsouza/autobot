//! The event types §10 names, which are never states.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// An event type named in KERNEL §10: a name of the form *Verb-ed*, never a state.
///
/// Serde and the JSON schema use the name as printed (`ReceiptObserved`).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum LifecycleEvent {
    /// An operation's request was sent.
    Dispatched,
    /// A provider's receipt for an operation was observed.
    ReceiptObserved,
    /// An operation's outcome could not be told from what was observed.
    Ambiguous,
    /// Reconciliation settled an operation's outcome.
    Reconciled,
    /// Reconciliation could not settle an operation's outcome.
    Unresolved,
    /// A `FenceSession` confirmed its fence (KERNEL §6).
    FenceConfirmed,
}

impl LifecycleEvent {
    /// Every event type §10 names.
    pub const ALL: &'static [Self] = &[
        Self::Dispatched,
        Self::ReceiptObserved,
        Self::Ambiguous,
        Self::Reconciled,
        Self::Unresolved,
        Self::FenceConfirmed,
    ];

    /// The name as printed.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dispatched => "Dispatched",
            Self::ReceiptObserved => "ReceiptObserved",
            Self::Ambiguous => "Ambiguous",
            Self::Reconciled => "Reconciled",
            Self::Unresolved => "Unresolved",
            Self::FenceConfirmed => "FenceConfirmed",
        }
    }
}
