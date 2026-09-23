//! The reducer-driven domain commit.

use crate::digest::{control_digest, domain_digest};
use crate::reducer::{
    self, CommandPins, Counters, Guards, Reducer, ReducerError, ReducerState, Refusal,
    StateDigests, TransitionReceipt, Versioned,
};
use crate::store::{
    Change, Commit, CommitOutcome, DomainChange, DomainCommitRequest, EventFields, GuardRefusal,
    Object, ObjectKey, Protocol, ProtocolError, Status, Step, StoreResult, Transition,
    domain_successor,
};
use crate::types::{CommitSequence, Digest, LaneRevision, ObjectRef, StateRevision, Uid};
use std::cell::Cell;
use std::fmt;
use std::marker::PhantomData;
use std::rc::Rc;

/// A reducer state held in an aggregate's status as its encoded fields.
pub trait StatusFields: ReducerState + Sized {
    /// The state `status` holds, decoded from its domain and control fields.
    ///
    /// # Errors
    ///
    /// A [`FieldsError`] when the fields do not decode.
    fn decode(status: &Status) -> Result<Self, FieldsError>;

    /// The encoded domain fields of the state, which a domain commit writes as
    /// [`Status::domain`].
    ///
    /// # Errors
    ///
    /// A [`FieldsError`] when the state has no encoding.
    fn domain_fields(&self) -> Result<String, FieldsError>;
}

/// Why a status's fields do not decode, or a state does not encode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldsError {
    /// What is wrong with the fields.
    pub reason: String,
}

impl fmt::Display for FieldsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the status fields do not decode: {}", self.reason)
    }
}

impl std::error::Error for FieldsError {}

/// One domain command of reducer `R` on one aggregate.
pub struct DomainRequest<R: Reducer> {
    /// The aggregate.
    pub target: ObjectKey,
    /// The aggregate's UID, which the command pins.
    pub uid: Uid,
    /// The UID of the command.
    pub command_uid: Uid,
    /// The command the reducer decides.
    pub command: R::Command,
    /// The digest of the command's input.
    pub input_digest: Digest,
    /// The `state_revision` the command pins.
    pub expected_revision: StateRevision,
    /// The command's receipt, prepared before the commit.
    pub receipt: ObjectRef,
    /// The caller's fields of the commit's audit event; its actor is the command's principal.
    pub event: EventFields,
    /// The guards the reducer decides under.
    pub guards: Guards,
}

/// How a domain commit ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainOutcome {
    /// The command committed, in this protocol or an earlier one, at this revision and
    /// sequence.
    Committed {
        /// The `state_revision` the commit produced.
        revision: StateRevision,
        /// The commit's sequence.
        commit_sequence: CommitSequence,
        /// The reducer's receipt of the commit, when this protocol wrote it; `None` when the
        /// slot read shows a commit of an earlier protocol.
        receipt: Option<TransitionReceipt>,
    },
    /// The pending slot of another command is `OCCUPIED` or `REPAIRING`: the receipt barrier
    /// holds, and nothing was written.
    Held {
        /// The command whose slot holds.
        slot_command: Uid,
    },
    /// The reducer refused the command; nothing was written.
    Refused(Refusal),
    /// A write of this protocol timed out, and the read after it found the lane past the
    /// pinned revision without the command's slot: the command may or may not have committed,
    /// and only its receipt resolves it (KERNEL §1), through the command receipt protocol of
    /// #83.
    Uncertain {
        /// The `state_revision` read.
        observed: StateRevision,
        /// The commit sequence read.
        at: CommitSequence,
    },
    /// No write of this protocol timed out, and the read found the lane past the pinned
    /// revision without the command's slot. This is not a rejection: only the command's
    /// receipt decides (KERNEL §2), through the command receipt protocol of #83.
    Passed {
        /// The `state_revision` read.
        observed: StateRevision,
        /// The commit sequence read.
        at: CommitSequence,
    },
    /// The lane's revision is still below the pinned one.
    NotReached {
        /// The `state_revision` read.
        observed: StateRevision,
    },
    /// The status's fields do not decode, or the reducer's state after the transition does
    /// not encode; nothing was written.
    Undecodable(FieldsError),
    /// The reducer's transition is defective, such as a domain transition that changes the
    /// control digest; nothing was written.
    Defect(ReducerError),
    /// The store's commit ended without deciding the command: the aggregate is missing or
    /// uninitialized, a counter would overflow, or the status has no canonical encoding.
    Failed(CommitOutcome),
}

/// What one run of the reducer decided on one read.
enum Decided {
    Receipt(TransitionReceipt),
    Refusal(Refusal),
    Undecodable(FieldsError),
    Defect(ReducerError),
    Failed(CommitOutcome),
}

/// The decisions of every read, in read order, shared between a [`DomainCommit`] and the
/// transition its store commit owns.
type Log = Rc<Cell<Vec<Decided>>>;

/// The store transition that runs reducer `R`.
struct ReducerTransition<R: Reducer> {
    command: R::Command,
    command_uid: Uid,
    input_digest: Digest,
    expected_revision: StateRevision,
    receipt: ObjectRef,
    event: EventFields,
    guards: Guards,
    log: Log,
    reducer: PhantomData<fn() -> R>,
}

impl<R: Reducer> ReducerTransition<R>
where
    R::State: StatusFields,
{
    /// The reducer's decision on `object`, whose status is `status`, and the change it makes.
    fn decide(&self, object: &Object, status: &Status) -> (Decided, Option<Change>) {
        let state = match R::State::decode(status) {
            Ok(state) => state,
            Err(e) => return (Decided::Undecodable(e), None),
        };
        let envelope = &status.envelope;
        let aggregate = Versioned {
            uid: object.uid.clone(),
            counters: Counters {
                state_revision: envelope.state_revision,
                control_revision: envelope.control_revision,
                commit_sequence: envelope.commit_sequence,
            },
            state,
        };
        let pins = CommandPins {
            command_uid: self.command_uid.clone(),
            principal: self.event.actor.clone(),
            input_digest: self.input_digest,
            expected_revision: LaneRevision::State(self.expected_revision),
        };
        match reducer::step::<R>(&aggregate, &pins, &self.command, &self.guards) {
            Ok(reducer::Step::Committed { aggregate, receipt }) => {
                let fields = match aggregate.state.domain_fields() {
                    Ok(fields) => fields,
                    Err(e) => return (Decided::Undecodable(e), None),
                };
                let receipt = match with_store_digests(receipt, status, &fields) {
                    Ok(receipt) => receipt,
                    Err(decided) => return (decided, None),
                };
                let change = Change::Domain(DomainChange {
                    fields,
                    receipt: self.receipt.clone(),
                    event: self.event.clone(),
                    effect_intents: receipt.fields().effect_intents.clone(),
                });
                (Decided::Receipt(receipt), Some(change))
            }
            Ok(reducer::Step::Refused(refusal)) => (Decided::Refusal(refusal), None),
            Err(e) => (Decided::Defect(e), None),
        }
    }
}

/// The store's digests of `status`, which a pending slot records.
fn store_digests(status: &Status) -> Result<StateDigests, CommitOutcome> {
    Ok(StateDigests {
        domain: domain_digest(status).map_err(CommitOutcome::Unencodable)?,
        control: control_digest(status).map_err(CommitOutcome::Unencodable)?,
    })
}

/// `receipt` reporting the store's digests of `status` and of the status that the domain
/// commit writing `fields` makes of it: the digests the commit's pending slot records.
fn with_store_digests(
    receipt: TransitionReceipt,
    status: &Status,
    fields: &str,
) -> Result<TransitionReceipt, Decided> {
    let after = domain_successor(status, fields.to_owned()).map_err(Decided::Failed)?;
    let before = store_digests(status).map_err(Decided::Failed)?;
    let after = store_digests(&after).map_err(Decided::Failed)?;
    receipt
        .with_digests(before, after)
        .map_err(|e| Decided::Defect(ReducerError::Receipt(e)))
}

impl<R: Reducer> Transition for ReducerTransition<R>
where
    R::State: StatusFields,
{
    fn apply(&self, object: &Object, status: &Status) -> Result<Change, GuardRefusal> {
        let (decided, change) = self.decide(object, status);
        let mut log = self.log.take();
        log.push(decided);
        self.log.set(log);
        // A decision without a change ends the store's commit as refused; the outcome is read
        // from the log, never from this text.
        change.ok_or_else(|| GuardRefusal {
            guard: "reducer".to_owned(),
        })
    }
}

/// The domain commit of one command, decided by reducer `R` (FORMAL §3 `CommitAggregateCAS`).
pub struct DomainCommit<R: Reducer>
where
    R::State: StatusFields,
{
    commit: Commit<ReducerTransition<R>>,
    log: Log,
    timed_out: bool,
    done: Option<DomainOutcome>,
}

impl<R: Reducer> DomainCommit<R>
where
    R::State: StatusFields,
{
    /// The protocol for `request`.
    #[must_use]
    pub fn new(request: DomainRequest<R>) -> Self {
        let log = Log::default();
        let transition = ReducerTransition {
            command: request.command,
            command_uid: request.command_uid.clone(),
            input_digest: request.input_digest,
            expected_revision: request.expected_revision,
            receipt: request.receipt,
            event: request.event,
            guards: request.guards,
            log: Rc::clone(&log),
            reducer: PhantomData,
        };
        let commit = Commit::domain(DomainCommitRequest {
            target: request.target,
            uid: request.uid,
            command_uid: request.command_uid,
            expected_revision: Some(request.expected_revision),
            transition,
        });
        Self {
            commit,
            log,
            timed_out: false,
            done: None,
        }
    }

    /// The outcome of the store's commit ending `outcome`.
    fn outcome(&self, outcome: CommitOutcome) -> DomainOutcome {
        let mut log = self.log.take();
        match outcome {
            CommitOutcome::Committed {
                revision: LaneRevision::State(revision),
                commit_sequence,
            } => DomainOutcome::Committed {
                revision,
                commit_sequence,
                receipt: log.into_iter().rev().find_map(|decided| match decided {
                    Decided::Receipt(r) if r.fields().after.commit_sequence == commit_sequence => {
                        Some(r)
                    }
                    _ => None,
                }),
            },
            CommitOutcome::Barrier { slot_command } => DomainOutcome::Held { slot_command },
            CommitOutcome::Passed {
                observed: LaneRevision::State(observed),
                at,
            } if self.timed_out => DomainOutcome::Uncertain { observed, at },
            CommitOutcome::Passed {
                observed: LaneRevision::State(observed),
                at,
            } => DomainOutcome::Passed { observed, at },
            CommitOutcome::NotReached {
                observed: LaneRevision::State(observed),
            } => DomainOutcome::NotReached { observed },
            CommitOutcome::Refused { .. } => match log.pop() {
                Some(Decided::Refusal(refusal)) => DomainOutcome::Refused(refusal),
                Some(Decided::Undecodable(e)) => DomainOutcome::Undecodable(e),
                Some(Decided::Defect(e)) => DomainOutcome::Defect(e),
                Some(Decided::Failed(failed)) => DomainOutcome::Failed(failed),
                Some(Decided::Receipt(_)) | None => DomainOutcome::Failed(outcome),
            },
            other => DomainOutcome::Failed(other),
        }
    }
}

impl<R: Reducer> Protocol for DomainCommit<R>
where
    R::State: StatusFields,
{
    type Outcome = DomainOutcome;

    fn step(&mut self) -> Step<DomainOutcome> {
        if let Some(done) = &self.done {
            return Step::Done(done.clone());
        }
        match self.commit.step() {
            Step::Done(outcome) => {
                let done = self.outcome(outcome);
                self.done = Some(done.clone());
                Step::Done(done)
            }
            Step::Op(op) => Step::Op(op),
        }
    }

    fn resume(&mut self, result: StoreResult) -> Result<(), ProtocolError> {
        // The store's commit accepts `UNCERTAIN` only as the answer to a status write.
        let timed_out = matches!(result, StoreResult::Uncertain);
        self.commit.resume(result)?;
        self.timed_out |= timed_out;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::digest::digest;
    use crate::profile::ControlRing;
    use crate::reducer::{Decision, RefusalGround, StateDigests};
    use crate::status::{PendingCommitState, StatusEnvelope};
    use crate::store::{
        ClearOutcome, ClearSlot, CommitRequest, ControlChange, OpKind, Origin, Pin,
        ResourceVersion, StoreOp,
    };
    use crate::types::{ControlRevision, Lane};
    use std::num::NonZeroU32;

    /// A counter with a hold: the domain fields are the count, the control fields the hold.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Counter {
        count: u64,
        held: bool,
    }

    impl ReducerState for Counter {
        fn digests(&self) -> StateDigests {
            StateDigests {
                domain: digest(&self.count).expect("a count encodes"),
                control: digest(&self.held).expect("a hold encodes"),
            }
        }
    }

    impl StatusFields for Counter {
        fn decode(status: &Status) -> Result<Self, FieldsError> {
            let count = status.domain.parse().map_err(|_| FieldsError {
                reason: format!("count {:?}", status.domain),
            })?;
            Ok(Self {
                count,
                held: status.control == "HOLD",
            })
        }

        fn domain_fields(&self) -> Result<String, FieldsError> {
            Ok(self.count.to_string())
        }
    }

    enum CounterCommand {
        /// Adds to the count, refused while held, with one effect intent.
        Add(u64),
        /// Adds one and flips the hold: a domain transition touching a control field.
        AddAndFlip,
    }

    struct CounterReducer;

    impl Reducer for CounterReducer {
        type State = Counter;
        type Command = CounterCommand;
        const VERSION: &'static str = "counter-1";

        fn reduce(state: &Counter, command: &CounterCommand, _: &Guards) -> Decision<Counter> {
            match *command {
                CounterCommand::Add(_) if state.held => {
                    Decision::Refuse(RefusalGround::Precondition("not held"))
                }
                CounterCommand::Add(n) => Decision::Commit(reducer::Transition::domain(
                    "AddCount",
                    Counter {
                        count: state.count + n,
                        held: false,
                    },
                    vec![intent()],
                )),
                CounterCommand::AddAndFlip => Decision::Commit(reducer::Transition::domain(
                    "AddCount",
                    Counter {
                        count: state.count + 1,
                        held: !state.held,
                    },
                    Vec::new(),
                )),
            }
        }
    }

    fn intent() -> crate::status::SlotEffectIntent {
        crate::status::SlotEffectIntent {
            effect_index: 0,
            installation_lineage: "install-a".to_owned(),
            payload_digest: digest("payload").expect("digest"),
            provider_binding: crate::status::ProviderBinding {
                provider: "fake-forge".to_owned(),
                operation: "comment".to_owned(),
            },
            desired_outcome: "comment-posted".to_owned(),
            target_identity: "pr/7".to_owned(),
            contract_revision: "v1".to_owned(),
        }
    }

    fn uid(s: &str) -> Uid {
        s.parse().expect("uid")
    }

    fn key() -> ObjectKey {
        ObjectKey {
            kind: "Task".parse().expect("kind"),
            namespace: "ns".parse().expect("namespace"),
            name: "a".parse().expect("name"),
        }
    }

    fn state(n: u64) -> StateRevision {
        StateRevision::new(n).expect("revision")
    }

    fn seq(n: u64) -> CommitSequence {
        CommitSequence::new(n).expect("sequence")
    }

    fn event() -> EventFields {
        EventFields {
            source_uid: uid("src"),
            event_type: "AddCount".to_owned(),
            actor: "writer".parse().expect("principal"),
            causation_id: "cause".to_owned(),
            correlation_id: "corr".to_owned(),
            schema_version: 1,
        }
    }

    fn receipt(command: &str) -> ObjectRef {
        ObjectRef {
            namespace: "ns".parse().expect("namespace"),
            name: format!("receipt-{command}").parse().expect("name"),
            uid: uid(&format!("receipt-{command}")),
        }
    }

    /// The domain commit of `command`, pinned at `expected`.
    fn commit(
        command_uid: &str,
        command: CounterCommand,
        expected: u64,
    ) -> DomainCommit<CounterReducer> {
        DomainCommit::new(DomainRequest {
            target: key(),
            uid: uid("uid-a"),
            command_uid: uid(command_uid),
            command,
            input_digest: digest(command_uid).expect("digest"),
            expected_revision: state(expected),
            receipt: receipt(command_uid),
            event: event(),
            guards: Guards::all(),
        })
    }

    /// An aggregate with a control lane at count 0, not held.
    fn initial() -> Object {
        Object {
            key: key(),
            uid: uid("uid-a"),
            resource_version: ResourceVersion::from(1),
            origin: Origin {
                create_receipt_uid: uid("create-a"),
                input_digest: digest("spec").expect("digest"),
                context_uid: uid("ctx"),
            },
            spec: "spec".to_owned(),
            status: Some(Status {
                envelope: StatusEnvelope::with_control_lane(),
                domain: "0".to_owned(),
                control: String::new(),
            }),
        }
    }

    /// A store of one object that applies every write conditioned on its current UID and
    /// resource version, and numbers its writes.
    struct OneObject {
        object: Object,
        writes: u64,
    }

    impl OneObject {
        fn new() -> Self {
            Self {
                object: initial(),
                writes: 1,
            }
        }

        fn status(&self) -> Status {
            self.object.status.clone().expect("initialized")
        }

        /// Replaces the status outside any protocol, as a new resource version.
        fn set_status(&mut self, status: Status) {
            self.writes += 1;
            self.object.resource_version = ResourceVersion::from(self.writes);
            self.object.status = Some(status);
        }

        fn answer(&mut self, op: StoreOp) -> StoreResult {
            match op {
                StoreOp::Get { .. } => StoreResult::Object(Box::new(self.object.clone())),
                StoreOp::UpdateStatus {
                    uid,
                    resource_version,
                    status,
                    ..
                } => {
                    if uid != self.object.uid || resource_version != self.object.resource_version {
                        return StoreResult::Conflict;
                    }
                    self.set_status(*status);
                    StoreResult::Object(Box::new(self.object.clone()))
                }
                other => panic!("unexpected operation {other:?}"),
            }
        }

        /// Commits the control command `command` pinned at control revision `revision`,
        /// writing empty control fields, and returns its commit sequence.
        fn control(&mut self, command: &str, revision: u64) -> CommitSequence {
            let clear = |_: &Object, _: &Status| {
                Ok::<_, GuardRefusal>(Change::Control(ControlChange {
                    fields: String::new(),
                    event: event(),
                }))
            };
            let (control, _) = self.run(&mut Commit::new(CommitRequest {
                target: key(),
                uid: uid("uid-a"),
                command_uid: uid(command),
                pin: Pin::Revision(LaneRevision::Control(
                    ControlRevision::new(revision).expect("revision"),
                )),
                ring: ControlRing {
                    entries: NonZeroU32::new(8).expect("non-zero"),
                    entry_max_kib: NonZeroU32::new(8).expect("non-zero"),
                },
                transition: clear,
            }));
            match control {
                CommitOutcome::Committed {
                    commit_sequence, ..
                } => commit_sequence,
                other => panic!("expected a control commit, got {other:?}"),
            }
        }

        /// Runs `protocol` to its outcome, counting the status writes it sends.
        fn run<P: Protocol>(&mut self, protocol: &mut P) -> (P::Outcome, usize) {
            let mut sent = 0;
            loop {
                match protocol.step() {
                    Step::Done(outcome) => return (outcome, sent),
                    Step::Op(op) => {
                        sent += usize::from(op.kind() == OpKind::UpdateStatus);
                        let result = self.answer(op);
                        protocol.resume(result).expect("the result answers");
                    }
                }
            }
        }
    }

    /// Steps `p`, expecting an operation.
    fn expect_op<P: Protocol>(p: &mut P) -> StoreOp
    where
        P::Outcome: fmt::Debug,
    {
        match p.step() {
            Step::Op(op) => op,
            Step::Done(outcome) => panic!("expected an operation, got {outcome:?}"),
        }
    }

    /// Steps `p`, expecting the outcome.
    fn expect_done<P: Protocol>(p: &mut P) -> P::Outcome {
        match p.step() {
            Step::Done(outcome) => outcome,
            Step::Op(op) => panic!("expected an outcome, got {op:?}"),
        }
    }

    /// The status a status update writes.
    fn written(op: StoreOp) -> Status {
        match op {
            StoreOp::UpdateStatus { status, .. } => *status,
            other => panic!("expected a status update, got {other:?}"),
        }
    }

    #[test]
    fn a_domain_commit_writes_the_reducer_s_transition_and_its_slot_in_one_write() {
        let mut store = OneObject::new();
        let before = store.status();
        let (outcome, writes) = store.run(&mut commit("c1", CounterCommand::Add(5), 0));
        let DomainOutcome::Committed {
            revision,
            commit_sequence,
            receipt: Some(transition),
        } = outcome
        else {
            panic!("expected a commit with its receipt, got {outcome:?}");
        };
        assert_eq!(writes, 1);
        assert_eq!((revision, commit_sequence), (state(1), seq(1)));
        let after = store.status();
        assert_eq!(after.domain, "5");
        assert_eq!(after.control, before.control);
        assert_eq!(after.envelope.state_revision, state(1));
        assert_eq!(after.envelope.commit_sequence, seq(1));
        assert_eq!(after.envelope.control_revision, ControlRevision::ZERO);
        assert_eq!(after.envelope.last_receipt_ref, Some(receipt("c1")));

        let fields = transition.fields();
        assert_eq!(fields.reducer_version, "counter-1");
        assert_eq!(fields.command_uid, uid("c1"));
        assert_eq!(fields.aggregate_uid, uid("uid-a"));
        assert_eq!(fields.lane, Lane::Domain);
        assert_eq!(fields.before.state_revision, state(0));
        assert_eq!(fields.after.state_revision, state(1));
        assert_eq!(fields.after.commit_sequence, seq(1));

        let slot = after.envelope.pending_commit.clone().expect("a slot");
        assert_eq!(slot.state, PendingCommitState::Occupied);
        assert_eq!(slot.command_uid, uid("c1"));
        assert_eq!(slot.receipt_uid, uid("receipt-c1"));
        assert_eq!(slot.expected_revision, state(0));
        assert_eq!(slot.proposed_revision, state(1));
        assert_eq!(slot.commit_sequence, seq(1));
        assert_eq!(slot.effect_intents, vec![intent()]);
        assert_eq!(slot.effect_intents, fields.effect_intents);
        assert_eq!(slot.before_digest, domain_digest(&before).expect("digest"));
        assert_eq!(slot.after_digest, domain_digest(&after).expect("digest"));
        assert_eq!(slot.audit_envelope.commit_sequence, seq(1));
        assert_eq!(slot.audit_envelope.state_revision, state(1));
        assert_eq!(slot.audit_envelope.lane, Lane::Domain);
        assert_eq!(slot.audit_envelope.actor, fields.principal);
    }

    #[test]
    fn a_second_domain_commit_is_held_until_the_slot_clears() {
        let mut store = OneObject::new();
        let (first, _) = store.run(&mut commit("c1", CounterCommand::Add(1), 0));
        assert!(matches!(first, DomainOutcome::Committed { .. }));

        for slot_state in [PendingCommitState::Occupied, PendingCommitState::Repairing] {
            let mut marked = store.status();
            if let Some(slot) = &mut marked.envelope.pending_commit {
                slot.state = slot_state;
            }
            store.set_status(marked.clone());
            let (held, writes) = store.run(&mut commit("c2", CounterCommand::Add(2), 1));
            assert_eq!(
                held,
                DomainOutcome::Held {
                    slot_command: uid("c1")
                }
            );
            assert_eq!(writes, 0);
            assert_eq!(store.status(), marked);
        }

        let (cleared, _) = store.run(&mut ClearSlot::new(key(), uid("uid-a"), uid("c1")));
        assert_eq!(cleared, ClearOutcome::Cleared);
        let (second, _) = store.run(&mut commit("c2", CounterCommand::Add(2), 1));
        assert!(matches!(
            second,
            DomainOutcome::Committed { revision, commit_sequence, receipt: Some(_) }
                if revision == state(2) && commit_sequence == seq(2)
        ));
        assert_eq!(store.status().domain, "3");
    }

    #[test]
    fn commit_sequence_strictly_increases_over_the_commits_of_both_lanes() {
        let mut store = OneObject::new();
        let mut last = store.status().envelope.commit_sequence;
        for revision in 0..4 {
            let command = format!("c{revision}");
            let (outcome, _) = store.run(&mut commit(&command, CounterCommand::Add(1), revision));
            let DomainOutcome::Committed {
                commit_sequence,
                receipt: Some(transition),
                ..
            } = outcome
            else {
                panic!("expected a commit, got {outcome:?}");
            };
            assert!(commit_sequence > last, "{commit_sequence:?} after {last:?}");
            assert_eq!(store.status().envelope.commit_sequence, commit_sequence);
            assert_eq!(transition.fields().after.commit_sequence, commit_sequence);
            last = commit_sequence;

            let commit_sequence = store.control(&format!("hold-{revision}"), revision);
            assert!(commit_sequence > last, "{commit_sequence:?} after {last:?}");
            last = commit_sequence;

            let (cleared, _) = store.run(&mut ClearSlot::new(key(), uid("uid-a"), uid(&command)));
            assert_eq!(cleared, ClearOutcome::Cleared);
            assert_eq!(store.status().envelope.commit_sequence, last);
        }
        assert_eq!(last, seq(8));
    }

    /// A read-back of the aggregate after another command's commit moved it to revision 1.
    fn moved_on() -> Box<Object> {
        let mut object = initial();
        object.resource_version = ResourceVersion::from(3);
        if let Some(status) = &mut object.status {
            status.envelope.state_revision = state(1);
            status.envelope.commit_sequence = seq(1);
        }
        Box::new(object)
    }

    #[test]
    fn a_timed_out_write_that_the_read_back_cannot_place_is_uncertain() {
        let mut p = commit("c1", CounterCommand::Add(1), 0);
        expect_op(&mut p);
        p.resume(StoreResult::Object(Box::new(initial())))
            .expect("read");
        written(expect_op(&mut p));
        p.resume(StoreResult::Uncertain).expect("timeout");
        assert!(matches!(expect_op(&mut p), StoreOp::Get { .. }));
        p.resume(StoreResult::Object(moved_on()))
            .expect("read back");
        let uncertain = DomainOutcome::Uncertain {
            observed: state(1),
            at: seq(1),
        };
        assert_eq!(expect_done(&mut p), uncertain);
        assert_eq!(expect_done(&mut p), uncertain);

        let mut fresh = commit("c1", CounterCommand::Add(1), 0);
        expect_op(&mut fresh);
        fresh.resume(StoreResult::Object(moved_on())).expect("read");
        assert_eq!(
            expect_done(&mut fresh),
            DomainOutcome::Passed {
                observed: state(1),
                at: seq(1)
            }
        );
    }

    #[test]
    fn a_timed_out_write_read_back_with_its_slot_is_committed_with_its_receipt() {
        let mut p = commit("c1", CounterCommand::Add(1), 0);
        expect_op(&mut p);
        p.resume(StoreResult::Object(Box::new(initial())))
            .expect("read");
        let landed = written(expect_op(&mut p));
        p.resume(StoreResult::Uncertain).expect("timeout");
        expect_op(&mut p);
        let mut object = initial();
        object.resource_version = ResourceVersion::from(2);
        object.status = Some(landed);
        p.resume(StoreResult::Object(Box::new(object)))
            .expect("read back");
        let outcome = expect_done(&mut p);
        assert!(
            matches!(
                &outcome,
                DomainOutcome::Committed { revision, commit_sequence, receipt: Some(r) }
                    if *revision == state(1)
                        && *commit_sequence == seq(1)
                        && r.fields().after.commit_sequence == seq(1)
            ),
            "{outcome:?}"
        );
    }

    #[test]
    fn a_reducer_refusal_writes_nothing_and_records_the_revision_read() {
        let mut store = OneObject::new();
        let mut held = store.status();
        held.control = "HOLD".to_owned();
        store.set_status(held.clone());
        let (outcome, writes) = store.run(&mut commit("c1", CounterCommand::Add(1), 0));
        assert_eq!(
            outcome,
            DomainOutcome::Refused(Refusal {
                ground: RefusalGround::Precondition("not held"),
                read_revision: LaneRevision::State(state(0)),
            })
        );
        assert_eq!(writes, 0);
        assert_eq!(store.status(), held);
    }

    #[test]
    fn a_reducer_defect_or_undecodable_fields_are_not_refusals_and_write_nothing() {
        let mut store = OneObject::new();
        let before = store.status();
        let (defect, writes) = store.run(&mut commit("c1", CounterCommand::AddAndFlip, 0));
        assert_eq!(
            defect,
            DomainOutcome::Defect(ReducerError::LanePartition { lane: Lane::Domain })
        );
        assert_eq!(writes, 0);
        assert_eq!(store.status(), before);

        let mut garbled = before;
        garbled.domain = "not a count".to_owned();
        store.set_status(garbled.clone());
        let (undecodable, writes) = store.run(&mut commit("c1", CounterCommand::Add(1), 0));
        assert!(matches!(undecodable, DomainOutcome::Undecodable(_)));
        assert_eq!(writes, 0);
        assert_eq!(store.status(), garbled);
    }
    /// The store's digests of `status`, which its pending slot records.
    fn slot_digests(status: &Status) -> StateDigests {
        StateDigests {
            domain: domain_digest(status).expect("domain digest"),
            control: control_digest(status).expect("control digest"),
        }
    }

    #[test]
    fn a_committed_receipt_reports_the_digests_its_slot_records() {
        let mut store = OneObject::new();
        let before = store.status();
        let (outcome, _) = store.run(&mut commit("c1", CounterCommand::Add(5), 0));
        let DomainOutcome::Committed {
            receipt: Some(transition),
            ..
        } = outcome
        else {
            panic!("expected a commit with its receipt, got {outcome:?}");
        };
        let after = store.status();
        let slot = after.envelope.pending_commit.clone().expect("a slot");
        let fields = transition.fields();
        assert_eq!(fields.before_digests.domain, slot.before_digest);
        assert_eq!(fields.after_digests.domain, slot.after_digest);
        assert_eq!(fields.before_digests, slot_digests(&before));
        assert_eq!(fields.after_digests, slot_digests(&after));
        assert_eq!(
            fields.after_digests.domain,
            slot.audit_envelope.state_digest
        );
    }

    #[test]
    fn a_conflict_decides_again_and_returns_the_receipt_of_the_write_that_landed() {
        let mut store = OneObject::new();
        let mut p = commit("c1", CounterCommand::Add(2), 0);
        let read = expect_op(&mut p);
        let result = store.answer(read);
        p.resume(result).expect("read");
        let first = expect_op(&mut p);
        // A control commit lands between the read and the write, so the write conflicts.
        assert_eq!(store.control("hold-0", 0), seq(1));
        let result = store.answer(first);
        assert_eq!(result, StoreResult::Conflict);
        p.resume(result).expect("conflict");
        let (outcome, writes) = store.run(&mut p);
        assert_eq!(writes, 1);
        let DomainOutcome::Committed {
            revision,
            commit_sequence,
            receipt: Some(transition),
        } = outcome
        else {
            panic!("expected a commit with its receipt, got {outcome:?}");
        };
        assert_eq!((revision, commit_sequence), (state(1), seq(2)));
        let fields = transition.fields();
        assert_eq!(fields.before.commit_sequence, seq(1));
        assert_eq!(
            fields.before.control_revision,
            ControlRevision::new(1).expect("revision")
        );
        assert_eq!(fields.after.commit_sequence, seq(2));
        let after = store.status();
        let slot = after.envelope.pending_commit.clone().expect("a slot");
        assert_eq!(fields.before_digests.domain, slot.before_digest);
        assert_eq!(fields.after_digests, slot_digests(&after));
        assert_eq!(after.domain, "2");
    }

    #[test]
    fn a_fresh_protocol_that_finds_its_own_slot_is_committed_without_a_receipt() {
        let mut store = OneObject::new();
        let (first, _) = store.run(&mut commit("c1", CounterCommand::Add(5), 0));
        assert!(matches!(
            first,
            DomainOutcome::Committed {
                receipt: Some(_),
                ..
            }
        ));
        let landed = store.status();
        let (again, writes) = store.run(&mut commit("c1", CounterCommand::Add(5), 0));
        assert_eq!(
            again,
            DomainOutcome::Committed {
                revision: state(1),
                commit_sequence: seq(1),
                receipt: None,
            }
        );
        assert_eq!(writes, 0);
        assert_eq!(store.status(), landed);
    }

    #[test]
    fn an_uncertain_answer_to_a_read_is_refused_and_leaves_the_commit_certain() {
        let mut p = commit("c1", CounterCommand::Add(1), 0);
        expect_op(&mut p);
        assert!(p.resume(StoreResult::Uncertain).is_err());
        p.resume(StoreResult::Object(moved_on())).expect("read");
        assert_eq!(
            expect_done(&mut p),
            DomainOutcome::Passed {
                observed: state(1),
                at: seq(1)
            }
        );
    }
}
