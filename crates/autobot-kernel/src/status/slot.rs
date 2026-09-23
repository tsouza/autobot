//! The pending commit slot of the domain lane.

use super::AuditEnvelope;
use crate::error::ValueError;
use crate::types::{CommitSequence, ControlRevision, Digest, StateRevision, Uid};
use schemars::JsonSchema;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

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
    /// The provider and operation that are to carry out the effect.
    pub provider_binding: ProviderBinding,
    /// The outcome the effect is to produce.
    pub desired_outcome: String,
    /// The identity of the effect's target at the provider.
    pub target_identity: String,
    /// The revision of the provider contract the intent is written against.
    pub contract_revision: String,
}

/// The `[provider, operation]` pair of an effect intent: the key of the provider capability
/// that carries out the effect (FORMAL §2 `EffectIntentRecord`, KERNEL §3.3).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct ProviderBinding {
    /// The provider that applies the effect.
    #[serde(deserialize_with = "provider_name")]
    pub provider: String,
    /// The provider operation that applies it.
    #[serde(deserialize_with = "operation_name")]
    pub operation: String,
}

/// What [`ValueError::Empty`] names for an empty provider.
const PROVIDER: &str = "a provider binding's provider";
/// What [`ValueError::Empty`] names for an empty operation.
const OPERATION: &str = "a provider binding's operation";

impl ProviderBinding {
    /// The binding of `operation` at `provider`.
    ///
    /// Neither name may be empty, as the adapters' provider and operation names may not be;
    /// deserialization refuses an empty one too.
    ///
    /// # Errors
    ///
    /// [`ValueError::Empty`] if either name is empty.
    pub fn new(
        provider: impl Into<String>,
        operation: impl Into<String>,
    ) -> Result<Self, ValueError> {
        Ok(Self {
            provider: non_empty(provider.into(), PROVIDER)?,
            operation: non_empty(operation.into(), OPERATION)?,
        })
    }
}

/// `name`, or [`ValueError::Empty`] naming `what` if it is empty.
fn non_empty(name: String, what: &'static str) -> Result<String, ValueError> {
    if name.is_empty() {
        Err(ValueError::Empty(what))
    } else {
        Ok(name)
    }
}

/// Deserializes a name, refusing an empty one as `what`.
fn named<'de, D: Deserializer<'de>>(d: D, what: &'static str) -> Result<String, D::Error> {
    non_empty(String::deserialize(d)?, what).map_err(D::Error::custom)
}

fn provider_name<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    named(d, PROVIDER)
}

fn operation_name<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    named(d, OPERATION)
}
