//! The commit-lane scenarios: C1 crash then C2 repair (F-2), the receipt barrier (F-3), lane
//! separation, the reconciliation-only clear and the full ring (F-4), and a hold on the
//! control lane beside a pending domain commit with digest-verified repair.

use super::store::{
    AGGREGATE, OnWrite, Outage, aggregate, attempt, clear, commit, control_change, control_pin,
    domain_change, list, namespace, parse, port, read, ring, run, state_pin, status, write_status,
};
use autobot_kernel::digest::{control_digest, digest, domain_digest, event_digest};
use autobot_kernel::error::RingError;
use autobot_kernel::reducer::{
    self, CommandPins, Counters, Decision, Guards, Reducer, ReducerError, ReducerState,
    StateDigests, Transition, Versioned,
};
use autobot_kernel::status::{ControlReceiptState, PendingCommit, PendingCommitState};
use autobot_kernel::store::{ClearOutcome, CommitOutcome, ObjectKey, OpKind};
use autobot_kernel::types::{
    CommitObservation, CommitSequence, ControlRevision, Digest, Lane, LaneRevision, StateRevision,
    Uid,
};
use autobot_testkit::harness::{Driver, Run};
use autobot_testkit::registry::g_commit::{
    CommandsPort, DomainCommand, ReceiptState, RepairOutcome, RepairPort,
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

/// `driver`, with the process killed right after its next status write to an aggregate: the
/// domain commit of a submitted command.
fn crash_after_domain_write(driver: &mut dyn Driver) -> OnWrite<'_> {
    OnWrite::crash(driver, AGGREGATE, OpKind::UpdateStatus)
}

/// The digest `result` holds; every status and input of the scenarios encodes.
fn digest_of<E: std::fmt::Display>(result: Result<Digest, E>) -> Digest {
    result.unwrap_or_else(|e| panic!("{e}"))
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

/// C1 submits a command and dies right after its domain commit lands, before the receipt's
/// terminal write; C2, a new process, repairs from the slot. The command ends with exactly one
/// `COMMITTED` receipt and one event, both rebuilt from the slot; the `PREPARED` receipt C1
/// left was no evidence of commitment; a later commit does not erase the receipt, and a replay
/// returns it without a second commit.
pub(crate) fn c1_crash_then_c2_repair(driver: &mut dyn Driver, guards: &Guards) {
    let repair = port::<RepairPort>();
    let commands = port::<CommandsPort>();
    let target = aggregate(driver, "durable", false);
    let first = command("k-durable", &target, 0, "after-c1");

    assert_eq!(
        attempt(
            &mut crash_after_domain_write(driver),
            &mut *commands.submit(&first, 0)
        ),
        Run::Crashed
    );
    let c1 = slot(driver, &target.0);
    assert_eq!(c1.state, PendingCommitState::Occupied);
    assert_eq!(
        run(driver, &mut *commands.receipt(&namespace(), "k-durable")),
        ReceiptState::Prepared
    );

    assert_eq!(
        run(driver, &mut *repair.repair(&target.0, &target.1, guards)),
        RepairOutcome::Cleared
    );
    assert_eq!(
        run(driver, &mut *commands.receipt(&namespace(), "k-durable")),
        ReceiptState::Terminal(committed(&c1))
    );
    let event = run(
        driver,
        &mut *repair.event(&namespace(), &target.1, c1.commit_sequence),
    )
    .unwrap_or_else(|| panic!("no event at commit sequence {}", c1.commit_sequence));
    assert_eq!(event.envelope, c1.audit_envelope);
    assert_eq!(
        event.event_digest,
        event_digest(&c1.audit_envelope).unwrap_or_else(|e| panic!("{e}"))
    );
    assert_eq!(list(driver, "AutoBotEvent").len(), 1);

    let second = command("k-later", &target, 1, "later");
    let later = run(driver, &mut *commands.submit(&second, 0));
    assert!(
        matches!(later, CommitObservation::Committed { .. }),
        "{later:?}"
    );
    assert_eq!(
        run(driver, &mut *commands.receipt(&namespace(), "k-durable")),
        ReceiptState::Terminal(committed(&c1))
    );
    assert_eq!(
        run(driver, &mut *commands.submit(&first, 0)),
        committed(&c1)
    );
    assert_eq!(status(driver, &target.0).envelope.state_revision, state(2));
}

/// While a slot is `OCCUPIED` no other domain commit lands; while the receipt store is down,
/// repair runs any number of rounds and never clears the slot, so the barrier keeps holding;
/// once the store is back, repair verifies and clears the slot and the next commit lands.
pub(crate) fn receipt_barrier_until_verified(driver: &mut dyn Driver, guards: &Guards) {
    let repair = port::<RepairPort>();
    let commands = port::<CommandsPort>();
    let target = aggregate(driver, "barrier-repair", false);

    let first = command("k-first", &target, 0, "first");
    assert_eq!(
        attempt(
            &mut crash_after_domain_write(driver),
            &mut *commands.submit(&first, 0)
        ),
        Run::Crashed
    );
    let held = slot(driver, &target.0);

    for _ in 0..3 {
        assert!(matches!(
            attempt(
                &mut Outage::new(driver, &RECEIPT_STORE),
                &mut *repair.repair(&target.0, &target.1, guards)
            ),
            Run::Stalled(_)
        ));
        assert_ne!(slot(driver, &target.0).state, PendingCommitState::Cleared);
        let blocked = commit(
            driver,
            &target,
            "command-next",
            state_pin(1),
            domain_change("command-next", "next"),
        );
        assert_eq!(
            blocked,
            CommitOutcome::Barrier {
                slot_command: held.command_uid.clone()
            }
        );
        assert_eq!(status(driver, &target.0).domain, "first");
    }

    assert_eq!(
        run(driver, &mut *repair.repair(&target.0, &target.1, guards)),
        RepairOutcome::Cleared
    );
    assert_eq!(slot(driver, &target.0).state, PendingCommitState::Cleared);
    assert_eq!(
        run(driver, &mut *commands.receipt(&namespace(), "k-first")),
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
        next,
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
    let initial = status(driver, &target.0);
    let first = commit(
        driver,
        &target,
        "command-a",
        state_pin(0),
        domain_change("command-a", "a"),
    );
    assert_eq!(
        first,
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
    assert_eq!(installed.before_digest, digest_of(domain_digest(&initial)));
    let committed_status = status(driver, &target.0);
    assert_eq!(committed_status.domain, "a");
    assert_eq!(
        installed.after_digest,
        digest_of(domain_digest(&committed_status))
    );

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
            blocked,
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
pub(crate) fn lane_separation(driver: &mut dyn Driver, guards: &Guards) {
    let target = aggregate(driver, "lanes", true);
    let pending = commit(
        driver,
        &target,
        "command-a",
        state_pin(0),
        domain_change("command-a", "a"),
    );
    assert!(matches!(pending, CommitOutcome::Committed { .. }));
    let before = status(driver, &target.0);

    let hold = commit(
        driver,
        &target,
        "hold-1",
        control_pin(0),
        control_change("hold-1", "HOLD"),
    );
    assert_eq!(
        hold,
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
    assert_eq!(digest_of(domain_digest(&after)), slot.after_digest);
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
    assert_eq!(
        receipt.before_control_digest,
        digest_of(control_digest(&before))
    );
    assert_eq!(
        receipt.after_control_digest,
        digest_of(control_digest(&after))
    );
    assert_eq!(receipt.audit_envelope.lane, Lane::Control);
    assert_eq!(receipt.state, ControlReceiptState::Unpublished);

    let crossing = commit(
        driver,
        &target,
        "hold-2",
        control_pin(1),
        domain_change("hold-2", "forged"),
    );
    assert_eq!(crossing, CommitOutcome::LaneMismatch);
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
        input_digest: digest_of(digest("hold-3")),
        expected_revision: LaneRevision::Control(ControlRevision::ZERO),
    };
    match reducer::step::<HoldReducer>(&probe, &pins, &HoldCommand::Hold, guards) {
        Ok(reducer::Step::Committed { receipt, aggregate }) => {
            let fields = receipt.fields();
            assert_eq!(fields.before_digests.domain, fields.after_digests.domain);
            assert_ne!(fields.before_digests.control, fields.after_digests.control);
            assert_eq!(aggregate.state.domain, "a");
        }
        other => panic!("a hold did not commit: {other:?}"),
    }
    assert_eq!(
        reducer::step::<HoldReducer>(&probe, &pins, &HoldCommand::HoldWritingDomain, guards),
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
    assert!(matches!(pending, CommitOutcome::Committed { .. }));
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
        assert!(matches!(outcome, CommitOutcome::Committed { .. }), "{name}");
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
        refused,
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
pub(crate) fn hold_beside_pending_commit_then_repair(driver: &mut dyn Driver, guards: &Guards) {
    let repair = port::<RepairPort>();
    let commands = port::<CommandsPort>();
    let target = aggregate(driver, "hold", true);

    let pending = command("k-pending", &target, 0, "pending");
    assert_eq!(
        attempt(
            &mut crash_after_domain_write(driver),
            &mut *commands.submit(&pending, 0)
        ),
        Run::Crashed
    );
    let held = slot(driver, &target.0);

    let hold = commit(
        &mut Outage::new(driver, &RECEIPT_STORE),
        &target,
        "hold-1",
        control_pin(0),
        control_change("hold-1", "HOLD"),
    );
    assert!(matches!(hold, CommitOutcome::Committed { .. }));
    assert_eq!(slot(driver, &target.0).after_digest, held.after_digest);
    assert!(matches!(
        attempt(
            &mut Outage::new(driver, &RECEIPT_STORE),
            &mut *repair.repair(&target.0, &target.1, guards)
        ),
        Run::Stalled(_)
    ));
    assert_ne!(slot(driver, &target.0).state, PendingCommitState::Cleared);

    assert_eq!(
        run(driver, &mut *repair.repair(&target.0, &target.1, guards)),
        RepairOutcome::Cleared
    );
    let repaired = status(driver, &target.0);
    assert_eq!(
        repaired.envelope.pending_commit_state(),
        PendingCommitState::Cleared
    );
    assert_eq!(repaired.control, "HOLD");
    assert_eq!(
        run(driver, &mut *commands.receipt(&namespace(), "k-pending")),
        ReceiptState::Terminal(committed(&held))
    );

    let drifted = aggregate(driver, "drifted", true);
    let lost = command("k-drifted", &drifted, 0, "committed");
    assert_eq!(
        attempt(
            &mut crash_after_domain_write(driver),
            &mut *commands.submit(&lost, 0)
        ),
        Run::Crashed
    );
    let object = read(driver, &drifted.0).unwrap_or_else(|| panic!("{} is missing", drifted.0));
    let mut foreign = object.status.clone().unwrap_or_else(|| panic!("no status"));
    foreign.domain = "foreign".to_owned();
    write_status(driver, &object, foreign);
    assert_eq!(
        run(driver, &mut *repair.repair(&drifted.0, &drifted.1, guards)),
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
            domain: digest_of(digest(&self.domain)),
            control: digest_of(digest(&self.control)),
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
