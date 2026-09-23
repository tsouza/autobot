//! The commit protocols: domain and control commits, status initialization and the
//! reconciliation-only CAS that clears a pending slot.

use super::cas::{Cas, Decide, Decision, Missing};
use super::object::{Object, ObjectKey, Status, fields_digest};
use super::op::{Protocol, ProtocolError, Step, StoreResult};
use crate::error::RingError;
use crate::profile::ControlRing;
use crate::status::{
    AuditEnvelope, ControlReceipt, ControlReceiptState, PendingCommit, PendingCommitState,
    SlotEffectIntent,
};
use crate::types::{
    CommitSequence, ControlRevision, Digest, Lane, LaneRevision, ObjectRef, Principal,
    StateRevision, Uid,
};

/// What a lane commit is conditioned on, besides the resource version of the object it read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pin {
    /// The command pinned this revision: the commit applies only at exactly this revision of
    /// its lane.
    Revision(LaneRevision),
    /// The command pinned no `state_revision`: the domain commit applies at the revision it
    /// reads, when its transition's guards accept what it read. It exists only for
    /// `AcceptDispatch`, whose guards carry the register values its permit pins; every other
    /// command pins its revision, and no control commit can be unpinned. A fresh protocol for
    /// a command whose slot a later domain commit has already replaced cannot tell that the
    /// command committed and commits it again; that is safe only while the command's guards
    /// refuse a second acceptance of one permit, which is open in #298.
    Current,
}

impl Pin {
    /// The lane the commit is on.
    #[must_use]
    pub fn lane(self) -> Lane {
        match self {
            Self::Revision(r) => r.lane(),
            Self::Current => Lane::Domain,
        }
    }
}

/// A guard of a transition that refused the state it read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardRefusal {
    /// The guard's identifier.
    pub guard: String,
}

/// The audit-event fields a commit's caller supplies. The store fills the rest of the
/// [`AuditEnvelope`] from the commit it computes: the aggregate UID, the commit sequence, the
/// lane, both revisions after the commit and the state digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventFields {
    /// The UID of the event's source.
    pub source_uid: Uid,
    /// The event's type.
    pub event_type: String,
    /// The authenticated writer of the command; a control receipt's principal.
    pub actor: Principal,
    /// The identifier of what caused the event.
    pub causation_id: String,
    /// The identifier that correlates the event with others.
    pub correlation_id: String,
    /// The schema version of the event.
    pub schema_version: u32,
}

/// The commit-computed facts an [`AuditEnvelope`] records.
struct CommitFacts<'a> {
    aggregate_uid: &'a Uid,
    commit_sequence: CommitSequence,
    lane: Lane,
    state_revision: StateRevision,
    control_revision: ControlRevision,
    state_digest: Digest,
}

impl EventFields {
    /// The full audit envelope of the commit `facts` describes.
    fn envelope(self, facts: CommitFacts<'_>) -> AuditEnvelope {
        AuditEnvelope {
            aggregate_uid: facts.aggregate_uid.clone(),
            commit_sequence: facts.commit_sequence,
            lane: facts.lane,
            state_revision: facts.state_revision,
            control_revision: facts.control_revision,
            source_uid: self.source_uid,
            event_type: self.event_type,
            state_digest: facts.state_digest,
            actor: self.actor,
            causation_id: self.causation_id,
            correlation_id: self.correlation_id,
            schema_version: self.schema_version,
        }
    }
}

/// The change a domain commit makes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainChange {
    /// The new encoded domain fields.
    pub fields: String,
    /// The command's receipt, prepared before the commit.
    pub receipt: ObjectRef,
    /// The caller's fields of the commit's audit event.
    pub event: EventFields,
    /// The commit's effect intents, in effect-index order.
    pub effect_intents: Vec<SlotEffectIntent>,
}

/// The change a control commit makes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlChange {
    /// The new encoded control fields.
    pub fields: String,
    /// The caller's fields of the commit's audit event; its actor is the receipt's principal.
    pub event: EventFields,
}

/// The change of one lane commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// A domain commit.
    Domain(DomainChange),
    /// A control commit.
    Control(ControlChange),
}

impl Change {
    /// The lane of the change.
    #[must_use]
    pub fn lane(&self) -> Lane {
        match self {
            Self::Domain(_) => Lane::Domain,
            Self::Control(_) => Lane::Control,
        }
    }
}

/// The transition a lane commit applies: a pure function of the object read.
///
/// It is evaluated again on every read, so after a conflict it sees the state the conflicting
/// write left, and its guards decide again.
pub trait Transition {
    /// The change to make to `object`, whose status is `status`, or the guard that refuses it.
    ///
    /// # Errors
    ///
    /// The [`GuardRefusal`] of the guard that refuses the state read.
    fn apply(&self, object: &Object, status: &Status) -> Result<Change, GuardRefusal>;
}

impl<F> Transition for F
where
    F: Fn(&Object, &Status) -> Result<Change, GuardRefusal>,
{
    fn apply(&self, object: &Object, status: &Status) -> Result<Change, GuardRefusal> {
        self(object, status)
    }
}

/// One lane commit of one command on one aggregate.
#[derive(Debug, Clone)]
pub struct CommitRequest<T> {
    /// The aggregate.
    pub target: ObjectKey,
    /// The aggregate's UID, which the command pins.
    pub uid: Uid,
    /// The UID of the command.
    pub command_uid: Uid,
    /// The command's pin.
    pub pin: Pin,
    /// The profile's ring limits, which a control commit appends within.
    pub ring: ControlRing,
    /// The transition.
    pub transition: T,
}

/// How a lane commit ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitOutcome {
    /// The command committed, in this call or an earlier one, at this revision and sequence.
    Committed {
        /// The revision the commit produced.
        revision: LaneRevision,
        /// The commit's sequence.
        commit_sequence: CommitSequence,
    },
    /// The lane's revision moved past the one the command could commit at, and the aggregate
    /// no longer shows whether the command committed there. This is not a rejection: only the
    /// command's receipt decides (KERNEL §2).
    Passed {
        /// The lane's revision read.
        observed: LaneRevision,
        /// The commit sequence read.
        at: CommitSequence,
    },
    /// The lane's revision is still below the pinned one.
    NotReached {
        /// The lane's revision read.
        observed: LaneRevision,
    },
    /// A domain commit found the pending slot of another command unresolved: the receipt
    /// barrier holds.
    Barrier {
        /// The command whose slot holds.
        slot_command: Uid,
    },
    /// A guard of the transition refused the state read.
    Refused {
        /// The guard's identifier.
        guard: String,
        /// The lane's revision read.
        read: LaneRevision,
        /// The commit sequence read.
        at: CommitSequence,
    },
    /// The control-receipt ring refused the receipt: a full ring refuses the transition.
    Ring(RingError),
    /// A control commit on an aggregate without a control lane.
    NoControlLane,
    /// The transition returned a change of the other lane; a commit writes one lane only.
    LaneMismatch,
    /// A counter would pass `i64::MAX`.
    Overflow,
    /// The aggregate's status is not initialized.
    Uninitialized,
    /// The aggregate is missing.
    Missing(Missing),
}

/// The domain or control commit protocol of one command.
pub struct Commit<T: Transition>(Cas<LaneCommit<T>>);

impl<T: Transition> Commit<T> {
    /// The protocol for `request`.
    #[must_use]
    pub fn new(request: CommitRequest<T>) -> Self {
        let key = request.target.clone();
        let uid = request.uid.clone();
        Self(Cas::new(key, uid, LaneCommit(request)))
    }
}

impl<T: Transition> Protocol for Commit<T> {
    type Outcome = CommitOutcome;

    fn step(&mut self) -> Step<CommitOutcome> {
        self.0.step()
    }

    fn resume(&mut self, result: StoreResult) -> Result<(), ProtocolError> {
        self.0.resume(result)
    }
}

/// The decider of a lane commit.
struct LaneCommit<T>(CommitRequest<T>);

impl<T: Transition> Decide for LaneCommit<T> {
    type Outcome = CommitOutcome;

    fn decide(
        &mut self,
        object: &Object,
        uncertain_base: Option<&Object>,
    ) -> Decision<CommitOutcome> {
        let request = &self.0;
        let Some(status) = &object.status else {
            return Decision::Done(CommitOutcome::Uninitialized);
        };
        let lane = request.pin.lane();
        if let Some(committed) = committed(status, lane, &request.command_uid) {
            return Decision::Done(committed);
        }
        let observed = status.revision(lane);
        let at = status.envelope.commit_sequence;
        let anchor = match request.pin {
            Pin::Revision(pinned) => Some(pinned),
            Pin::Current => uncertain_base
                .and_then(|base| base.status.as_ref())
                .map(|base| base.revision(lane)),
        };
        if let Some(anchor) = anchor {
            if observed.get() > anchor.get() {
                return Decision::Done(CommitOutcome::Passed { observed, at });
            }
            if observed.get() < anchor.get() {
                return Decision::Done(CommitOutcome::NotReached { observed });
            }
        }
        if lane == Lane::Domain
            && let Some(slot) = &status.envelope.pending_commit
            && slot.state != PendingCommitState::Cleared
        {
            return Decision::Done(CommitOutcome::Barrier {
                slot_command: slot.command_uid.clone(),
            });
        }
        let change = match request.transition.apply(object, status) {
            Ok(change) => change,
            Err(refusal) => {
                return Decision::Done(CommitOutcome::Refused {
                    guard: refusal.guard,
                    read: observed,
                    at,
                });
            }
        };
        let written = match change {
            Change::Domain(change) if lane == Lane::Domain => {
                domain_commit(status, &request.uid, &request.command_uid, change)
            }
            Change::Control(change) if lane == Lane::Control => control_commit(
                status,
                &request.uid,
                &request.command_uid,
                change,
                &request.ring,
            ),
            _ => Err(CommitOutcome::LaneMismatch),
        };
        match written {
            Ok(status) => Decision::Write(Box::new(status)),
            Err(outcome) => Decision::Done(outcome),
        }
    }

    fn missing(&self, missing: Missing) -> CommitOutcome {
        CommitOutcome::Missing(missing)
    }
}

/// The commit of `command` that `status` shows on `lane`: the pending slot on the domain lane,
/// a ring entry on the control lane.
fn committed(status: &Status, lane: Lane, command: &Uid) -> Option<CommitOutcome> {
    match lane {
        Lane::Domain => status
            .envelope
            .pending_commit
            .as_ref()
            .filter(|slot| &slot.command_uid == command)
            .map(|slot| CommitOutcome::Committed {
                revision: LaneRevision::State(slot.proposed_revision),
                commit_sequence: slot.commit_sequence,
            }),
        Lane::Control => status
            .envelope
            .control_receipt_ring
            .as_ref()?
            .entries()
            .iter()
            .find(|receipt| &receipt.control_uid == command)
            .map(|receipt| CommitOutcome::Committed {
                revision: LaneRevision::Control(receipt.control_revision),
                commit_sequence: receipt.commit_sequence,
            }),
    }
}

/// The status after the domain commit of `command` applies `change` to `status`.
fn domain_commit(
    status: &Status,
    aggregate: &Uid,
    command: &Uid,
    change: DomainChange,
) -> Result<Status, CommitOutcome> {
    let envelope = &status.envelope;
    let commit_sequence = envelope
        .commit_sequence
        .next()
        .map_err(|_| CommitOutcome::Overflow)?;
    let proposed_revision = envelope
        .state_revision
        .next()
        .map_err(|_| CommitOutcome::Overflow)?;
    let after_digest = fields_digest(&change.fields);
    let audit_envelope = change.event.envelope(CommitFacts {
        aggregate_uid: aggregate,
        commit_sequence,
        lane: Lane::Domain,
        state_revision: proposed_revision,
        control_revision: envelope.control_revision,
        state_digest: after_digest,
    });
    let slot = PendingCommit {
        command_uid: command.clone(),
        receipt_uid: change.receipt.uid.clone(),
        commit_sequence,
        before_digest: fields_digest(&status.domain),
        after_digest,
        expected_revision: envelope.state_revision,
        proposed_revision,
        control_revision_at_commit: envelope.control_revision,
        audit_envelope,
        effect_intents: change.effect_intents,
        state: PendingCommitState::Occupied,
    };
    let mut next = status.clone();
    next.domain = change.fields;
    next.envelope.state_revision = proposed_revision;
    next.envelope.commit_sequence = commit_sequence;
    next.envelope.pending_commit = Some(slot);
    next.envelope.last_receipt_ref = Some(change.receipt);
    Ok(next)
}

/// The status after the control commit of `command` applies `change` to `status`.
fn control_commit(
    status: &Status,
    aggregate: &Uid,
    command: &Uid,
    change: ControlChange,
    limits: &ControlRing,
) -> Result<Status, CommitOutcome> {
    let envelope = &status.envelope;
    let commit_sequence = envelope
        .commit_sequence
        .next()
        .map_err(|_| CommitOutcome::Overflow)?;
    let control_revision = envelope
        .control_revision
        .next()
        .map_err(|_| CommitOutcome::Overflow)?;
    let after_control_digest = fields_digest(&change.fields);
    let principal = change.event.actor.clone();
    let audit_envelope = change.event.envelope(CommitFacts {
        aggregate_uid: aggregate,
        commit_sequence,
        lane: Lane::Control,
        state_revision: envelope.state_revision,
        control_revision,
        state_digest: after_control_digest,
    });
    let receipt = ControlReceipt {
        control_uid: command.clone(),
        control_revision,
        commit_sequence,
        before_control_digest: fields_digest(&status.control),
        after_control_digest,
        audit_envelope,
        principal,
        state: ControlReceiptState::Unpublished,
    };
    let mut next = status.clone();
    let ring = next
        .envelope
        .control_receipt_ring
        .as_mut()
        .ok_or(CommitOutcome::NoControlLane)?;
    ring.append(receipt, limits).map_err(CommitOutcome::Ring)?;
    next.control = change.fields;
    next.envelope.control_revision = control_revision;
    next.envelope.commit_sequence = commit_sequence;
    Ok(next)
}

/// How a status initialization ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitializeOutcome {
    /// The status is the requested one.
    Initialized,
    /// The object has another status. That status may have followed this initialization, when
    /// an `UNCERTAIN` write of it applied and a commit landed before the read-back.
    AlreadyInitialized,
    /// The object is missing.
    Missing(Missing),
}

/// The protocol that writes the first status of an object that has none.
pub struct Initialize(Cas<InitializeStatus>);

impl Initialize {
    /// The protocol writing `status` on the object `key` with UID `uid`.
    #[must_use]
    pub fn new(key: ObjectKey, uid: Uid, status: Status) -> Self {
        Self(Cas::new(key, uid, InitializeStatus(status)))
    }
}

impl Protocol for Initialize {
    type Outcome = InitializeOutcome;

    fn step(&mut self) -> Step<InitializeOutcome> {
        self.0.step()
    }

    fn resume(&mut self, result: StoreResult) -> Result<(), ProtocolError> {
        self.0.resume(result)
    }
}

/// The decider of a status initialization.
struct InitializeStatus(Status);

impl Decide for InitializeStatus {
    type Outcome = InitializeOutcome;

    fn decide(&mut self, object: &Object, _: Option<&Object>) -> Decision<InitializeOutcome> {
        match &object.status {
            None => Decision::Write(Box::new(self.0.clone())),
            Some(status) if *status == self.0 => Decision::Done(InitializeOutcome::Initialized),
            Some(_) => Decision::Done(InitializeOutcome::AlreadyInitialized),
        }
    }

    fn missing(&self, missing: Missing) -> InitializeOutcome {
        InitializeOutcome::Missing(missing)
    }
}

/// How clearing a pending slot ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClearOutcome {
    /// The command's slot is `CLEARED`.
    Cleared,
    /// The pending slot is absent or belongs to another command. An earlier `UNCERTAIN` write of
    /// this protocol may still have cleared the slot before a later domain commit replaced it.
    NotHeld,
    /// The aggregate's status is not initialized.
    Uninitialized,
    /// The aggregate is missing.
    Missing(Missing),
}

/// The reconciliation-only CAS that marks one command's pending slot `CLEARED`.
///
/// It writes the slot's state and nothing else: no revision, no commit sequence, no receipt.
/// The caller clears a slot only after its receipt and audit event are written and verified.
pub struct ClearSlot(Cas<ClearCommand>);

impl ClearSlot {
    /// The protocol clearing the slot of `command_uid` on the aggregate `key` with UID `uid`.
    #[must_use]
    pub fn new(key: ObjectKey, uid: Uid, command_uid: Uid) -> Self {
        Self(Cas::new(key, uid, ClearCommand(command_uid)))
    }
}

impl Protocol for ClearSlot {
    type Outcome = ClearOutcome;

    fn step(&mut self) -> Step<ClearOutcome> {
        self.0.step()
    }

    fn resume(&mut self, result: StoreResult) -> Result<(), ProtocolError> {
        self.0.resume(result)
    }
}

/// The decider of a slot clearing.
struct ClearCommand(Uid);

impl Decide for ClearCommand {
    type Outcome = ClearOutcome;

    fn decide(&mut self, object: &Object, _: Option<&Object>) -> Decision<ClearOutcome> {
        let Some(status) = &object.status else {
            return Decision::Done(ClearOutcome::Uninitialized);
        };
        match &status.envelope.pending_commit {
            Some(slot) if slot.command_uid == self.0 => {
                if slot.state == PendingCommitState::Cleared {
                    Decision::Done(ClearOutcome::Cleared)
                } else {
                    let mut next = status.clone();
                    if let Some(slot) = &mut next.envelope.pending_commit {
                        slot.state = PendingCommitState::Cleared;
                    }
                    Decision::Write(Box::new(next))
                }
            }
            _ => Decision::Done(ClearOutcome::NotHeld),
        }
    }

    fn missing(&self, missing: Missing) -> ClearOutcome {
        ClearOutcome::Missing(missing)
    }
}
