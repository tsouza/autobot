//! The control receipt and the bounded ring that holds them.

use crate::error::RingError;
use crate::profile::ControlRing;
use crate::types::{CommitSequence, ControlRevision, Digest, Principal, StateRevision, Uid};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The state of a control receipt in the ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ControlReceiptState {
    /// The receipt's audit event is not yet published. The initial state.
    Unpublished,
    /// The receipt's audit event is published.
    Published,
}

/// The receipt of one control commit, the FORMAL §2 `ControlReceipt` record.
///
/// The control commit appends it to the ring in the same write, so the receipt is durable
/// without a second write. With its [`AuditEnvelope`] it holds every field of its audit event;
/// the envelope's untyped text fields are open in #329.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ControlReceipt {
    /// The UID of the control command.
    pub control_uid: Uid,
    /// The `control_revision` the commit produced.
    pub control_revision: ControlRevision,
    /// The commit's sequence on the aggregate.
    pub commit_sequence: CommitSequence,
    /// The aggregate's control digest before the commit.
    pub before_control_digest: Digest,
    /// The aggregate's control digest after the commit.
    pub after_control_digest: Digest,
    /// The parts of the audit event the rest of the receipt does not hold.
    pub audit_envelope: AuditEnvelope,
    /// The authenticated writer of the control command.
    pub principal: Principal,
    /// The receipt's publication state.
    pub state: ControlReceiptState,
}

/// The parts of a control commit's `AutoBotEvent` that neither its receipt nor its aggregate
/// holds.
///
/// With the receipt they give every field of the event: `aggregate_uid` is the aggregate's
/// UID, `lane` is `CONTROL`, `commit_sequence` and `control_revision` are the receipt's, `actor`
/// is its principal, `state_digest` is its after-control digest, and `event_digest` is computed
/// over the event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct AuditEnvelope {
    /// The aggregate's `state_revision` when the commit landed.
    pub state_revision: StateRevision,
    /// The UID of the event's source.
    pub source_uid: Uid,
    /// The event's type.
    pub event_type: String,
    /// The identifier of what caused the event.
    pub causation_id: String,
    /// The identifier that correlates the event with others.
    pub correlation_id: String,
    /// The schema version of the event.
    pub schema_version: u32,
}

/// The bounded ring of control receipts of an aggregate with a control lane.
///
/// Entries are in commit order: each entry's commit sequence and control revision are past the
/// previous entry's, which deserialization checks. The bound is the profile's
/// [`ControlRing::entries`]: the ring is full when it holds that many unpublished receipts.
/// Published entries are kept while there is room and give way, oldest first, to new receipts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "Vec<ControlReceipt>", into = "Vec<ControlReceipt>")]
pub struct ControlReceiptRing(Vec<ControlReceipt>);

impl ControlReceiptRing {
    /// The entries, oldest first.
    #[must_use]
    pub fn entries(&self) -> &[ControlReceipt] {
        &self.0
    }

    /// The number of unpublished entries.
    #[must_use]
    pub fn unpublished(&self) -> usize {
        self.0
            .iter()
            .filter(|r| r.state == ControlReceiptState::Unpublished)
            .count()
    }

    /// Whether the ring refuses another receipt under `limits`.
    #[must_use]
    pub fn is_full(&self, limits: &ControlRing) -> bool {
        self.unpublished() >= capacity(limits)
    }

    /// Appends `receipt`, removing the oldest published entries needed to keep the ring within
    /// `limits`.
    ///
    /// # Errors
    ///
    /// [`RingError::Full`] if the ring [`is_full`](Self::is_full), so the control transition is
    /// refused; [`RingError::AlreadyPublished`] if `receipt` is not unpublished; and
    /// [`RingError::OutOfOrder`] if `receipt` does not follow the last entry. The ring is
    /// unchanged on error.
    pub fn append(
        &mut self,
        receipt: ControlReceipt,
        limits: &ControlRing,
    ) -> Result<(), RingError> {
        if receipt.state != ControlReceiptState::Unpublished {
            return Err(RingError::AlreadyPublished(receipt.commit_sequence));
        }
        if let Some(last) = self.0.last()
            && !follows(last, &receipt)
        {
            return Err(RingError::OutOfOrder {
                last: last.commit_sequence,
                refused: receipt.commit_sequence,
            });
        }
        if self.is_full(limits) {
            return Err(RingError::Full {
                unpublished: self.unpublished(),
            });
        }
        while self.0.len() >= capacity(limits) {
            match self
                .0
                .iter()
                .position(|r| r.state == ControlReceiptState::Published)
            {
                Some(i) => {
                    self.0.remove(i);
                }
                None => break,
            }
        }
        self.0.push(receipt);
        Ok(())
    }

    /// Marks the entry with commit sequence `commit_sequence` as published.
    ///
    /// Marking an entry that is already published changes nothing.
    ///
    /// # Errors
    ///
    /// [`RingError::NotInRing`] if no entry has that commit sequence.
    pub fn mark_published(&mut self, commit_sequence: CommitSequence) -> Result<(), RingError> {
        let entry = self
            .0
            .iter_mut()
            .find(|r| r.commit_sequence == commit_sequence)
            .ok_or(RingError::NotInRing(commit_sequence))?;
        entry.state = ControlReceiptState::Published;
        Ok(())
    }
}

/// The ring capacity `limits` sets.
fn capacity(limits: &ControlRing) -> usize {
    usize::try_from(limits.entries.get()).unwrap_or(usize::MAX)
}

/// Whether `next` may follow `last` in a ring.
fn follows(last: &ControlReceipt, next: &ControlReceipt) -> bool {
    next.commit_sequence > last.commit_sequence && next.control_revision > last.control_revision
}

impl TryFrom<Vec<ControlReceipt>> for ControlReceiptRing {
    type Error = RingError;

    fn try_from(entries: Vec<ControlReceipt>) -> Result<Self, Self::Error> {
        let disorder = entries.windows(2).find_map(|w| match w {
            [last, next] if !follows(last, next) => Some(RingError::OutOfOrder {
                last: last.commit_sequence,
                refused: next.commit_sequence,
            }),
            _ => None,
        });
        disorder.map_or(Ok(Self(entries)), Err)
    }
}

impl From<ControlReceiptRing> for Vec<ControlReceipt> {
    fn from(ring: ControlReceiptRing) -> Self {
        ring.0
    }
}
