//! The audit envelope a commit records.

use crate::types::{CommitSequence, ControlRevision, Digest, Lane, Principal, StateRevision, Uid};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Every field of a commit's `AutoBotEvent` but its digest: the FORMAL §2 `AuditEnvelope`
/// record.
///
/// A pending commit and a control receipt each hold the envelope of their commit, so a new
/// process rebuilds the event from the slot or the ring entry alone; `event_digest` is computed
/// over the event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AuditEnvelope {
    /// The UID of the aggregate the commit landed on.
    pub aggregate_uid: Uid,
    /// The commit's sequence on the aggregate.
    pub commit_sequence: CommitSequence,
    /// The commit's lane.
    pub lane: Lane,
    /// The aggregate's `state_revision` after the commit.
    pub state_revision: StateRevision,
    /// The aggregate's `control_revision` after the commit.
    pub control_revision: ControlRevision,
    /// The UID of the event's source.
    pub source_uid: Uid,
    /// The event's type.
    pub event_type: String,
    /// The digest of the aggregate's state the event records: the domain digest after a domain
    /// commit, the control digest after a control commit.
    pub state_digest: Digest,
    /// The authenticated writer of the command.
    pub actor: Principal,
    /// The identifier of what caused the event.
    pub causation_id: String,
    /// The identifier that correlates the event with others.
    pub correlation_id: String,
    /// The schema version of the event.
    pub schema_version: u32,
}
