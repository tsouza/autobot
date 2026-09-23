//! The pending commit slot of the domain lane.

use super::AuditEnvelope;
use crate::types::{CommitSequence, ControlRevision, Digest, StateRevision, Uid};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The state of an aggregate's pending commit slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PendingCommitState {
    /// The slot's receipt and audit event are written and verified; the next domain commit may
    /// proceed. The initial state.
    Cleared,
    /// A domain commit installed the slot and its receipt barrier holds.
    Occupied,
    /// A new process is reconstructing the slot's receipt and audit event.
    Repairing,
}

/// The pending commit a domain commit installs, the FORMAL §2 `PendingCommit` record.
///
/// The slot names its command's receipt, prepared before the commit, by `receipt_uid`, holds
/// the full [`AuditEnvelope`] of the commit, so a new process rebuilds the audit event from the
/// slot alone, and holds its effect intents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PendingCommit {
    /// The UID of the committed command.
    pub command_uid: Uid,
    /// The UID of the command's receipt.
    pub receipt_uid: Uid,
    /// The commit's sequence on the aggregate.
    pub commit_sequence: CommitSequence,
    /// The aggregate's domain digest before the commit.
    pub before_digest: Digest,
    /// The aggregate's domain digest after the commit.
    pub after_digest: Digest,
    /// The `state_revision` the command pinned.
    pub expected_revision: StateRevision,
    /// The `state_revision` the commit produced.
    pub proposed_revision: StateRevision,
    /// The aggregate's `control_revision` when the commit landed.
    pub control_revision_at_commit: ControlRevision,
    /// The envelope of the commit's audit event.
    pub audit_envelope: AuditEnvelope,
    /// The effect intents of the commit, in effect-index order.
    pub effect_intents: Vec<SlotEffectIntent>,
    /// The slot's state.
    pub state: PendingCommitState,
}

/// One effect intent as a pending commit holds it.
///
/// It holds the fields of the FORMAL §2 `EffectIntent` record that the commit decides. The
/// rest follow from it: `aggregate_uid` is the UID of the aggregate whose slot holds the
/// intent, `committed_revision` is the slot's `proposed_revision`, `uid` and `operation_key`
/// are derived from those and the fields here, and `state` belongs to the `EffectIntent`
/// object materialized from it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SlotEffectIntent {
    /// The position of the intent among the commit's intents.
    pub effect_index: u32,
    /// The installation lineage the intent's operation key binds.
    pub installation_lineage: String,
    /// The digest of the intent's payload.
    pub payload_digest: Digest,
    /// The provider binding that is to carry out the effect.
    pub provider_binding: String,
    /// The outcome the effect is to produce.
    pub desired_outcome: String,
    /// The identity of the effect's target at the provider.
    pub target_identity: String,
    /// The revision of the provider contract the intent is written against.
    pub contract_revision: String,
}
