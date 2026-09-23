//! The commit-lane scenarios: C1 crash then C2 repair (F-2), the receipt barrier (F-3), lane
//! separation, the reconciliation-only clear and the full ring (F-4), and a hold on the
//! control lane beside a pending domain commit with digest-verified repair.

use super::harness::{
    AGGREGATE, Driver, Effect, Fault, MemDriver, Run, aggregate, clear, commit, control_change,
    control_pin, domain_change, list, namespace, parse, read, ring, state_pin, status,
    write_status,
};
use super::ports::{self, DomainCommand, ReceiptState, RepairOutcome};
use autobot_kernel::digest::event_digest;
use autobot_kernel::error::RingError;
use autobot_kernel::reducer::{
    self, CommandPins, Counters, Decision, Guards, Reducer, ReducerError, ReducerState,
    StateDigests, Transition, Versioned,
};
use autobot_kernel::status::{ControlReceiptState, PendingCommit, PendingCommitState};
use autobot_kernel::store::{
    ClearOutcome, CommitOutcome, ObjectKey, OpKind, Protocol, fields_digest,
};
use autobot_kernel::types::{
    CommitObservation, CommitSequence, ControlRevision, Lane, LaneRevision, StateRevision, Uid,
};

/// The kinds of the receipt store: command receipts and audit events.
const RECEIPT_STORE: [&str; 2] = ["CommandReceipt", "AutoBotEvent"];

/// A commit sequence.
fn seq(n: u64) -> CommitSequence {
    CommitSequence::new(n).unwrap_or_else(|e| panic!("{e}"))
}

/// A `state_revision`.
fn state(n: u64) -> StateRevision {
    StateRevision::new(n).unwrap_or_else(|e| panic!("{e}"))
}

/// The domain command `key` on `target` pinned at `expected`, writing `input`.
fn command(key: &str, target: &(ObjectKey, Uid), expected: u64, input: &str) -> DomainCommand {
    DomainCommand {
        idempotency_key: key.to_owned(),
        principal: parse("writer"),
        target: target.0.clone(),
        target_uid: target.1.clone(),
        expected_revision: state(expected),
        input: input.to_owned(),
        issued_day: 0,
    }
}

/// The fault that kills the process right after its next status write to an aggregate.
fn crash_after_domain_write() -> Fault {
    Fault {
        kind: parse(AGGREGATE),
        op: OpKind::UpdateStatus,
        effect: Effect::Crash,
    }
}

/// The pending slot of `target`, which must hold one.
fn slot(driver: &mut dyn Driver, target: &ObjectKey) -> PendingCommit {
    status(driver, target)
        .envelope
        .pending_commit
        .unwrap_or_else(|| panic!("{target} holds no pending slot"))
}

/// The observation of the commit `slot` records.
fn committed(slot: &PendingCommit) -> CommitObservation {
    CommitObservation::Committed {
        revision: LaneRevision::State(slot.proposed_revision),
        commit_sequence: slot.commit_sequence,
    }
}

/// Runs `protocol` to its outcome.
fn done<O: std::fmt::Debug>(
    driver: &mut dyn Driver,
    mut protocol: Box<dyn Protocol<Outcome = O>>,
) -> O {
    super::harness::run(driver, &mut *protocol).done()
}

/// Runs `protocol` and returns how it ended.
fn attempt<O>(driver: &mut dyn Driver, mut protocol: Box<dyn Protocol<Outcome = O>>) -> Run<O> {
    super::harness::run(driver, &mut *protocol)
}

/// C1 submits a command and dies right after its domain commit lands, before the receipt's
/// terminal write; C2, a new process, repairs from the slot. The command ends with exactly one
/// `COMMITTED` receipt and one event, both rebuilt from the slot; the `PREPARED` receipt C1
/// left was no evidence of commitment; a later commit does not erase the receipt, and a replay
/// returns it without a second commit.
pub(crate) fn c1_crash_then_c2_repair(driver: &mut dyn Driver) {
    let commands = ports::commands();
    let repair = ports::repair();
    let target = aggregate(driver, "durable", false);
    let first = command("k-durable", &target, 0, "after-c1");

    driver.arm(crash_after_domain_write());
    assert_eq!(attempt(driver, commands.submit(&first, 0)), Run::Crashed);
    let c1 = slot(driver, &target.0);
    assert_eq!(c1.state, PendingCommitState::Occupied);
    assert_eq!(
        done(driver, commands.receipt(&namespace(), "k-durable")),
        ReceiptState::Prepared
    );

    assert_eq!(
        done(driver, repair.repair(&target.0, &target.1)),
        RepairOutcome::Cleared
    );
    assert_eq!(
        done(driver, commands.receipt(&namespace(), "k-durable")),
        ReceiptState::Terminal(committed(&c1))
    );
    let event = done(
        driver,
        repair.event(&namespace(), &target.1, c1.commit_sequence),
    )
    .unwrap_or_else(|| panic!("no event at commit sequence {}", c1.commit_sequence));
    assert_eq!(event.envelope, c1.audit_envelope);
    assert_eq!(
        event.event_digest,
        event_digest(&c1.audit_envelope).unwrap_or_else(|e| panic!("{e}"))
    );
    assert_eq!(list(driver, "AutoBotEvent").len(), 1);

    let second = command("k-later", &target, 1, "later");
    let later = done(driver, commands.submit(&second, 0));
    assert!(
        matches!(later, CommitObservation::Committed { .. }),
        "{later:?}"
    );
    assert_eq!(
        done(driver, commands.receipt(&namespace(), "k-durable")),
        ReceiptState::Terminal(committed(&c1))
    );
    assert_eq!(done(driver, commands.submit(&first, 0)), committed(&c1));
    assert_eq!(status(driver, &target.0).envelope.state_revision, state(2));
}

/// While a slot is `OCCUPIED` no other domain commit lands; while the receipt store is down,
/// repair runs any number of rounds and never clears the slot, so the barrier keeps holding;
/// once the store is back, repair verifies and clears the slot and the next commit lands.
pub(crate) fn receipt_barrier_until_verified(driver: &mut dyn Driver) {
    let commands = ports::commands();
    let repair = ports::repair();
    let target = aggregate(driver, "barrier-repair", false);

    driver.arm(crash_after_domain_write());
    let first = command("k-first", &target, 0, "first");
    assert_eq!(attempt(driver, commands.submit(&first, 0)), Run::Crashed);
    let held = slot(driver, &target.0);

    for kind in RECEIPT_STORE {
        driver.outage(&parse(kind), true);
    }
    for _ in 0..3 {
        assert_eq!(
            attempt(driver, repair.repair(&target.0, &target.1)),
            Run::Stalled
        );
        assert_ne!(slot(driver, &target.0).state, PendingCommitState::Cleared);
        let blocked = commit(
            driver,
            &target,
            "command-next",
            state_pin(1),
            domain_change("command-next", "next"),
        );
        assert_eq!(
            blocked.done(),
            CommitOutcome::Barrier {
                slot_command: held.command_uid.clone()
            }
        );
        assert_eq!(status(driver, &target.0).domain, "first");
    }
    for kind in RECEIPT_STORE {
        driver.outage(&parse(kind), false);
    }

    assert_eq!(
        done(driver, repair.repair(&target.0, &target.1)),
        RepairOutcome::Cleared
    );
    assert_eq!(slot(driver, &target.0).state, PendingCommitState::Cleared);
    assert_eq!(
        done(driver, commands.receipt(&namespace(), "k-first")),
        ReceiptState::Terminal(committed(&held))
    );
    let next = commit(
        driver,
        &target,
        "command-next",
        state_pin(1),
        domain_change("command-next", "next"),
    );
    assert_eq!(
        next.done(),
        CommitOutcome::Committed {
            revision: LaneRevision::State(state(2)),
            commit_sequence: seq(2)
        }
    );
}

/// A domain commit installs its slot in the commit write; a second domain commit is held by
/// the barrier while the slot is `OCCUPIED` and while it is `REPAIRING`, and writes nothing.
pub(crate) fn domain_commit_waits_on_the_barrier(driver: &mut dyn Driver) {
    let target = aggregate(driver, "barrier", false);
    let first = commit(
        driver,
        &target,
        "command-a",
        state_pin(0),
        domain_change("command-a", "a"),
    );
    assert_eq!(
        first.done(),
        CommitOutcome::Committed {
            revision: LaneRevision::State(state(1)),
            commit_sequence: seq(1)
        }
    );
    let installed = slot(driver, &target.0);
    assert_eq!(installed.command_uid, parse::<Uid>("command-a"));
    assert_eq!(installed.state, PendingCommitState::Occupied);
    assert_eq!(installed.expected_revision, state(0));
    assert_eq!(installed.proposed_revision, state(1));
    assert_eq!(installed.before_digest, fields_digest("initial"));
    assert_eq!(installed.after_digest, fields_digest("a"));

    for slot_state in [PendingCommitState::Occupied, PendingCommitState::Repairing] {
        let object = read(driver, &target.0).unwrap_or_else(|| panic!("{} is missing", target.0));
        let mut marked = object.status.clone().unwrap_or_else(|| panic!("no status"));
        if let Some(slot) = &mut marked.envelope.pending_commit {
            slot.state = slot_state;
        }
        write_status(driver, &object, marked.clone());
        let blocked = commit(
            driver,
            &target,
            "command-b",
            state_pin(1),
            domain_change("command-b", "b"),
        );
        assert_eq!(
            blocked.done(),
            CommitOutcome::Barrier {
                slot_command: parse("command-a")
            }
        );
        assert_eq!(status(driver, &target.0), marked);
    }
}

/// A hold, a control commit, lands beside an `OCCUPIED` slot: it moves `control_revision` and
/// `commit_sequence`, appends its receipt, and leaves the slot and every domain byte as they
/// were, so the domain digest still equals the slot's after-digest. A control transition that
/// writes the domain is refused on both paths that could carry it: the store's control commit
/// and the reducer's lane-partition check.
pub(crate) fn lane_separation(driver: &mut dyn Driver) {
    let target = aggregate(driver, "lanes", true);
    let pending = commit(
        driver,
        &target,
        "command-a",
        state_pin(0),
        domain_change("command-a", "a"),
    );
    assert!(matches!(pending.done(), CommitOutcome::Committed { .. }));
    let before = status(driver, &target.0);

    let hold = commit(
        driver,
        &target,
        "hold-1",
        control_pin(0),
        control_change("hold-1", "HOLD"),
    );
    assert_eq!(
        hold.done(),
        CommitOutcome::Committed {
            revision: LaneRevision::Control(
                ControlRevision::new(1).unwrap_or_else(|e| panic!("{e}"))
            ),
            commit_sequence: seq(2),
        }
    );
    let after = status(driver, &target.0);
    assert_eq!(after.domain, before.domain);
    assert_eq!(
        after.envelope.pending_commit,
        before.envelope.pending_commit
    );
    assert_eq!(
        after.envelope.state_revision,
        before.envelope.state_revision
    );
    assert_eq!(
        after.envelope.last_receipt_ref,
        before.envelope.last_receipt_ref
    );
    assert!(after.envelope.commit_sequence > before.envelope.commit_sequence);
    assert_eq!(after.control, "HOLD");
    let slot = slot(driver, &target.0);
    assert_eq!(slot.state, PendingCommitState::Occupied);
    assert_eq!(fields_digest(&after.domain), slot.after_digest);
    let ring_entries = after
        .envelope
        .control_receipt_ring
        .as_ref()
        .map(|r| r.entries().to_vec())
        .unwrap_or_default();
    assert_eq!(ring_entries.len(), 1);
    let receipt = &ring_entries[0];
    assert_eq!(receipt.control_uid, parse::<Uid>("hold-1"));
    assert_eq!(receipt.commit_sequence, seq(2));
    assert_eq!(receipt.before_control_digest, fields_digest("RUNNING"));
    assert_eq!(receipt.after_control_digest, fields_digest("HOLD"));
    assert_eq!(receipt.audit_envelope.lane, Lane::Control);
    assert_eq!(receipt.state, ControlReceiptState::Unpublished);

    let crossing = commit(
        driver,
        &target,
        "hold-2",
        control_pin(1),
        domain_change("hold-2", "forged"),
    );
    assert_eq!(crossing.done(), CommitOutcome::LaneMismatch);
    assert_eq!(status(driver, &target.0), after);

    let probe = Versioned {
        uid: target.1.clone(),
        counters: Counters::default(),
        state: Probe {
            domain: "a".to_owned(),
            control: "RUNNING".to_owned(),
        },
    };
    let pins = CommandPins {
        command_uid: parse("hold-3"),
        principal: parse("controller"),
        input_digest: fields_digest("hold-3"),
        expected_revision: LaneRevision::Control(ControlRevision::ZERO),
    };
    match reducer::step::<HoldReducer>(&probe, &pins, &HoldCommand::Hold, &Guards::all()) {
        Ok(reducer::Step::Committed { receipt, aggregate }) => {
            let fields = receipt.fields();
            assert_eq!(fields.before_digests.domain, fields.after_digests.domain);
            assert_ne!(fields.before_digests.control, fields.after_digests.control);
            assert_eq!(aggregate.state.domain, "a");
        }
        other => panic!("a hold did not commit: {other:?}"),
    }
    assert_eq!(
        reducer::step::<HoldReducer>(
            &probe,
            &pins,
            &HoldCommand::HoldWritingDomain,
            &Guards::all()
        ),
        Err(ReducerError::LanePartition {
            lane: Lane::Control
        })
    );
}

/// The reconciliation-only CAS that clears a slot writes the slot's state and nothing else:
/// no revision, no commit sequence, no receipt, no domain or control byte.
pub(crate) fn clearing_is_reconciliation_only(driver: &mut dyn Driver) {
    let target = aggregate(driver, "reconcile", true);
    let pending = commit(
        driver,
        &target,
        "command-a",
        state_pin(0),
        domain_change("command-a", "a"),
    );
    assert!(matches!(pending.done(), CommitOutcome::Committed { .. }));
    let before = status(driver, &target.0);

    assert_eq!(clear(driver, &target, "command-a"), ClearOutcome::Cleared);
    let after = status(driver, &target.0);
    assert_eq!(
        after.envelope.pending_commit_state(),
        PendingCommitState::Cleared
    );
    let mut unmarked = after.clone();
    if let Some(slot) = &mut unmarked.envelope.pending_commit {
        slot.state = PendingCommitState::Occupied;
    }
    assert_eq!(unmarked, before);
}

/// The control-receipt ring holds the profile's number of unpublished receipts; the next
/// control transition is refused, and nothing is written and no receipt is dropped.
pub(crate) fn full_ring_refuses(driver: &mut dyn Driver) {
    let target = aggregate(driver, "ring", true);
    let capacity = u64::from(ring().entries.get());
    let commands: Vec<String> = (0..capacity).map(|i| format!("hold-{i}")).collect();
    for (i, name) in (0..).zip(&commands) {
        let outcome = commit(
            driver,
            &target,
            name,
            control_pin(i),
            control_change(name, name),
        );
        assert!(
            matches!(outcome.done(), CommitOutcome::Committed { .. }),
            "{name}"
        );
    }
    let full = status(driver, &target.0);

    let refused = commit(
        driver,
        &target,
        "hold-over",
        control_pin(capacity),
        control_change("hold-over", "over"),
    );
    let unpublished = usize::try_from(capacity).unwrap_or(usize::MAX);
    assert_eq!(
        refused.done(),
        CommitOutcome::Ring(RingError::Full { unpublished })
    );
    let after = status(driver, &target.0);
    assert_eq!(after, full);
    let kept: Vec<Uid> = after
        .envelope
        .control_receipt_ring
        .as_ref()
        .map(|r| r.entries().iter().map(|e| e.control_uid.clone()).collect())
        .unwrap_or_default();
    let expected: Vec<Uid> = commands.iter().map(|c| parse(c)).collect();
    assert_eq!(kept, expected);
}

/// A hold lands on the control lane while the receipt store is down and a domain slot is
/// unresolved; repair cannot finish until the store is back, then verifies the domain digest
/// against the slot and clears it. On an aggregate whose domain fields no longer match the
/// slot's after-digest, repair keeps the slot.
pub(crate) fn hold_beside_pending_commit_then_repair(driver: &mut dyn Driver) {
    let commands = ports::commands();
    let repair = ports::repair();
    let target = aggregate(driver, "hold", true);

    driver.arm(crash_after_domain_write());
    let pending = command("k-pending", &target, 0, "pending");
    assert_eq!(attempt(driver, commands.submit(&pending, 0)), Run::Crashed);
    let held = slot(driver, &target.0);

    for kind in RECEIPT_STORE {
        driver.outage(&parse(kind), true);
    }
    let hold = commit(
        driver,
        &target,
        "hold-1",
        control_pin(0),
        control_change("hold-1", "HOLD"),
    );
    assert!(matches!(hold.done(), CommitOutcome::Committed { .. }));
    assert_eq!(slot(driver, &target.0).after_digest, held.after_digest);
    assert_eq!(
        attempt(driver, repair.repair(&target.0, &target.1)),
        Run::Stalled
    );
    assert_ne!(slot(driver, &target.0).state, PendingCommitState::Cleared);
    for kind in RECEIPT_STORE {
        driver.outage(&parse(kind), false);
    }

    assert_eq!(
        done(driver, repair.repair(&target.0, &target.1)),
        RepairOutcome::Cleared
    );
    let repaired = status(driver, &target.0);
    assert_eq!(
        repaired.envelope.pending_commit_state(),
        PendingCommitState::Cleared
    );
    assert_eq!(repaired.control, "HOLD");
    assert_eq!(
        done(driver, commands.receipt(&namespace(), "k-pending")),
        ReceiptState::Terminal(committed(&held))
    );

    let drifted = aggregate(driver, "drifted", true);
    driver.arm(crash_after_domain_write());
    let lost = command("k-drifted", &drifted, 0, "committed");
    assert_eq!(attempt(driver, commands.submit(&lost, 0)), Run::Crashed);
    let object = read(driver, &drifted.0).unwrap_or_else(|| panic!("{} is missing", drifted.0));
    let mut foreign = object.status.clone().unwrap_or_else(|| panic!("no status"));
    foreign.domain = "foreign".to_owned();
    write_status(driver, &object, foreign);
    assert_eq!(
        done(driver, repair.repair(&drifted.0, &drifted.1)),
        RepairOutcome::DigestMismatch
    );
    assert_ne!(slot(driver, &drifted.0).state, PendingCommitState::Cleared);
}

/// The state [`HoldReducer`] decides over: domain and control fields as text.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Probe {
    domain: String,
    control: String,
}

impl ReducerState for Probe {
    fn digests(&self) -> StateDigests {
        StateDigests {
            domain: fields_digest(&self.domain),
            control: fields_digest(&self.control),
        }
    }
}

/// The commands [`HoldReducer`] decides.
#[derive(Debug)]
enum HoldCommand {
    /// A hold: control fields only.
    Hold,
    /// A hold whose transition also writes a domain field.
    HoldWritingDomain,
}

/// A control-lane reducer applying holds.
struct HoldReducer;

impl Reducer for HoldReducer {
    type State = Probe;
    type Command = HoldCommand;
    const VERSION: &'static str = "g-commit-hold/1";

    fn reduce(state: &Probe, command: &HoldCommand, _: &Guards) -> Decision<Probe> {
        let domain = match command {
            HoldCommand::Hold => state.domain.clone(),
            HoldCommand::HoldWritingDomain => "forged".to_owned(),
        };
        let after = Probe {
            domain,
            control: "HOLD".to_owned(),
        };
        Decision::Commit(Transition::control("CommitControlCAS", after))
    }
}

#[test]
#[ignore = "awaiting #84"]
fn f2_receipt_durability() {
    c1_crash_then_c2_repair(&mut MemDriver::new());
}

#[test]
#[ignore = "awaiting #84"]
fn f3_receipt_barrier() {
    receipt_barrier_until_verified(&mut MemDriver::new());
}

#[test]
#[ignore = "awaiting #80"]
fn domain_commit_is_held_by_an_unresolved_slot() {
    domain_commit_waits_on_the_barrier(&mut MemDriver::new());
}

#[test]
#[ignore = "awaiting #81"]
fn f4_lane_separation() {
    lane_separation(&mut MemDriver::new());
}

#[test]
#[ignore = "awaiting #82"]
fn slot_clearing_changes_only_reconciliation_fields() {
    clearing_is_reconciliation_only(&mut MemDriver::new());
}

#[test]
#[ignore = "awaiting #81"]
fn full_ring_refuses_the_next_control_transition() {
    full_ring_refuses(&mut MemDriver::new());
}

#[test]
#[ignore = "awaiting #84"]
fn hold_beside_a_pending_domain_commit_with_verified_repair() {
    hold_beside_pending_commit_then_repair(&mut MemDriver::new());
}
