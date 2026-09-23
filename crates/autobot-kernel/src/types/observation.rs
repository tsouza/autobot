//! What a reader observes about the fate of one command.

use super::{CommitSequence, Digest, LaneRevision, Uid};
use crate::error::ValueError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The observed fate of one command at its target.
///
/// `PREPARED` has no variant: it is a stored receipt state that is not evidence of commitment,
/// so it is never an observed outcome. Every variant but [`Uncertain`](Self::Uncertain) is
/// final.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitObservation {
    /// The command committed.
    Committed {
        /// The revision the commit produced, in the command's lane.
        revision: LaneRevision,
        /// The commit's sequence on the target.
        commit_sequence: CommitSequence,
    },
    /// The command can never commit, with the proof of it.
    Rejected(RejectionProof),
    /// The command was cancelled by a write on the target that consumed its expected revision.
    Cancelled {
        /// The receipt of the cancellation.
        cancellation_receipt_uid: Uid,
    },
    /// The command was retried after its replay window; it is not read as new intent.
    ReplayExpired,
    /// A write timed out or its result was lost: the command may or may not have committed.
    /// It is reconciled by reading the target's pending slot and the command's receipt, never
    /// by elapsed time and never by treating it as a rejection.
    Uncertain,
}

impl CommitObservation {
    /// Whether the observation is final: every variant but [`Uncertain`](Self::Uncertain).
    #[must_use]
    pub fn is_final(&self) -> bool {
        !matches!(self, Self::Uncertain)
    }
}

/// The proof a `REJECTED` receipt records, one shape per rejection ground of KERNEL §2: the
/// FORMAL §2 `RejectionProof` record, tagged by `ground`.
///
/// Every rejection has one of these grounds, and a rejection without its proof is none.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "ground", rename_all = "snake_case")]
pub enum RejectionProof {
    /// The exact expected revision can no longer commit and no matching slot or receipt exists.
    PassedRevision(PassedRevision),
    /// The replay key is already bound to a different payload or principal.
    ReplayConflict {
        /// The receipt the key is bound to.
        existing_receipt_uid: Uid,
        /// The input digest bound to that receipt.
        bound_digest: Digest,
    },
    /// The create's name is held by another object.
    CreateConflict {
        /// The UID of the object that holds the name.
        observed_uid: Uid,
        /// The commit sequence of the read that observed it.
        observed_commit_sequence: CommitSequence,
    },
    /// The owning controller refused a guard before any compare-and-swap.
    GuardRefusal {
        /// The identifier of the refused guard.
        guard_id: String,
        /// The target revision the guard read.
        read_revision: LaneRevision,
    },
}

/// The proof of the passed-revision ground: a command can never commit at the revision it
/// pinned.
///
/// It holds the two facts that ground needs: the target's revision in the command's lane was
/// read past the revision the command pinned, so that exact revision can no longer commit; and
/// in the same read neither the target's pending slot nor any receipt matched the command. A
/// compare-and-swap conflict alone proves neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "PassedRevisionWire", into = "PassedRevisionWire")]
pub struct PassedRevision {
    expected_revision: LaneRevision,
    observed_revision: LaneRevision,
    observed_commit_sequence: CommitSequence,
}

impl PassedRevision {
    /// The proof that a command pinning `expected_revision` can never commit, from a read of
    /// its target at commit sequence `observed_commit_sequence` that showed `observed_revision`
    /// and no pending slot or receipt matching the command.
    ///
    /// # Errors
    ///
    /// [`ValueError::ProofLanes`] if the two revisions are on different lanes, and
    /// [`ValueError::ProofNotPassed`] if `observed_revision` is not past `expected_revision`.
    pub fn new(
        expected_revision: LaneRevision,
        observed_revision: LaneRevision,
        observed_commit_sequence: CommitSequence,
    ) -> Result<Self, ValueError> {
        let (expected, observed) = (expected_revision, observed_revision);
        if expected.lane() != observed.lane() {
            return Err(ValueError::ProofLanes { expected, observed });
        }
        if observed.get() <= expected.get() {
            return Err(ValueError::ProofNotPassed { expected, observed });
        }
        Ok(Self {
            expected_revision,
            observed_revision,
            observed_commit_sequence,
        })
    }

    /// The revision the command pinned.
    #[must_use]
    pub fn expected_revision(&self) -> LaneRevision {
        self.expected_revision
    }

    /// The target's revision in the same lane, read past the expected one.
    #[must_use]
    pub fn observed_revision(&self) -> LaneRevision {
        self.observed_revision
    }

    /// The target's commit sequence in the read that found no matching slot or receipt.
    #[must_use]
    pub fn observed_commit_sequence(&self) -> CommitSequence {
        self.observed_commit_sequence
    }
}

/// The serde form of a [`PassedRevision`].
#[derive(Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[schemars(rename = "PassedRevision")]
struct PassedRevisionWire {
    /// The revision the command pinned.
    expected_revision: LaneRevision,
    /// The target's revision in the same lane, read past the expected one.
    observed_revision: LaneRevision,
    /// The target's commit sequence in the read that found no matching slot or receipt.
    observed_commit_sequence: CommitSequence,
}

impl TryFrom<PassedRevisionWire> for PassedRevision {
    type Error = ValueError;

    fn try_from(w: PassedRevisionWire) -> Result<Self, Self::Error> {
        Self::new(
            w.expected_revision,
            w.observed_revision,
            w.observed_commit_sequence,
        )
    }
}

impl From<PassedRevision> for PassedRevisionWire {
    fn from(p: PassedRevision) -> Self {
        Self {
            expected_revision: p.expected_revision,
            observed_revision: p.observed_revision,
            observed_commit_sequence: p.observed_commit_sequence,
        }
    }
}
