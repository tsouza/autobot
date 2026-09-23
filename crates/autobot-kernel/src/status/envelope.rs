//! The status envelope every aggregate carries, and its conditions.

use super::{ControlReceiptRing, PendingCommit, PendingCommitState};
use crate::types::{CommitSequence, ControlRevision, ObjectRef, StateRevision};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The common part of every aggregate's status.
///
/// A kind's status embeds it with `#[serde(flatten)]` beside its own fields, so its fields
/// appear at the top level of the status.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct StatusEnvelope {
    /// The `metadata.generation` of the spec the owning controller last acted on; absent until
    /// it first has.
    #[serde(
        rename = "observedGeneration",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub observed_generation: Option<i64>,
    /// The aggregate's conditions.
    #[serde(default)]
    pub conditions: Vec<Condition>,
    /// The domain lane's revision.
    pub state_revision: StateRevision,
    /// The control lane's revision; zero on an aggregate without a control lane.
    pub control_revision: ControlRevision,
    /// The sequence of the last commit of either lane.
    pub commit_sequence: CommitSequence,
    /// The receipt of the last command committed on the aggregate; absent before the first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_receipt_ref: Option<ObjectRef>,
    /// The pending commit slot; absent until the first domain commit installs it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_commit: Option<PendingCommit>,
    /// The control-receipt ring; present exactly on an aggregate with a control lane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_receipt_ring: Option<ControlReceiptRing>,
}

impl StatusEnvelope {
    /// The initial envelope of an aggregate with a control lane: [`Default`] with an empty
    /// ring.
    #[must_use]
    pub fn with_control_lane() -> Self {
        Self {
            control_receipt_ring: Some(ControlReceiptRing::default()),
            ..Self::default()
        }
    }

    /// The state of the pending commit slot: `CLEARED` while no slot has been installed.
    #[must_use]
    pub fn pending_commit_state(&self) -> PendingCommitState {
        self.pending_commit
            .as_ref()
            .map_or(PendingCommitState::Cleared, |slot| slot.state)
    }
}

/// One condition of an aggregate, in the form of the Kubernetes `metav1.Condition`.
///
/// `last_transition_time` is carried as text: the kernel does not interpret time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Condition {
    /// The condition's type, in `CamelCase`.
    #[serde(rename = "type")]
    pub type_: String,
    /// Whether the condition holds.
    pub status: ConditionStatus,
    /// The `metadata.generation` the condition was set from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_generation: Option<i64>,
    /// When the status last changed, in RFC 3339.
    pub last_transition_time: String,
    /// Why the status last changed, in `CamelCase`.
    pub reason: String,
    /// A human-readable account of the transition.
    pub message: String,
}

/// Whether a condition holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum ConditionStatus {
    /// It holds.
    True,
    /// It does not hold.
    False,
    /// It cannot be determined.
    Unknown,
}
