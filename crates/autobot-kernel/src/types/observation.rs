//! What a reader observes about the fate of one command.

use super::{CommitSequence, LaneRevision, Uid};
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

/// The proof that a command can never commit.
///
/// It holds the two facts a rejection needs: the target's revision in the command's lane was
/// read past the revision the command pinned, so that exact revision can no longer commit; and
/// in the same read neither the target's pending slot nor any receipt matched the command. A
/// compare-and-swap conflict alone proves neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "RejectionProofWire", into = "RejectionProofWire")]
pub struct RejectionProof {
    expected_revision: LaneRevision,
    observed_revision: LaneRevision,
    unmatched_at: CommitSequence,
}

impl RejectionProof {
    /// The proof that a command pinning `expected_revision` can never commit, from a read of
    /// its target at commit sequence `unmatched_at` that showed `observed_revision` and no
    /// pending slot or receipt matching the command.
    ///
    /// # Errors
    ///
    /// [`ValueError::ProofLanes`] if the two revisions are on different lanes, and
    /// [`ValueError::ProofNotPassed`] if `observed_revision` is not past `expected_revision`.
    pub fn new(
        expected_revision: LaneRevision,
        observed_revision: LaneRevision,
        unmatched_at: CommitSequence,
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
            unmatched_at,
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
    pub fn unmatched_at(&self) -> CommitSequence {
        self.unmatched_at
    }
}

/// The serde form of a [`RejectionProof`].
#[derive(Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[schemars(rename = "RejectionProof")]
struct RejectionProofWire {
    /// The revision the command pinned.
    expected_revision: LaneRevision,
    /// The target's revision in the same lane, read past the expected one.
    observed_revision: LaneRevision,
    /// The target's commit sequence in the read that found no matching slot or receipt.
    unmatched_at: CommitSequence,
}

impl TryFrom<RejectionProofWire> for RejectionProof {
    type Error = ValueError;

    fn try_from(w: RejectionProofWire) -> Result<Self, Self::Error> {
        Self::new(w.expected_revision, w.observed_revision, w.unmatched_at)
    }
}

impl From<RejectionProof> for RejectionProofWire {
    fn from(p: RejectionProof) -> Self {
        Self {
            expected_revision: p.expected_revision,
            observed_revision: p.observed_revision,
            unmatched_at: p.unmatched_at,
        }
    }
}
