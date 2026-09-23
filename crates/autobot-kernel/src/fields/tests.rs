use super::*;
use crate as autobot_kernel;
use crate::profile::{ControlRing, Profile};
use crate::status::{
    AuditEnvelope, Condition, ConditionStatus, ControlReceipt, ControlReceiptState, PendingCommit,
    PendingCommitState, StatusEnvelope,
};
use crate::types::{CommitSequence, ControlRevision, Digest, StateRevision};
use serde::Serialize;
use std::collections::BTreeMap;

const KERNEL: &str = include_str!("../../../../docs/design/AUTOBOT-KERNEL.md");
const M0_PROFILE: &str = include_str!("../../../../profiles/m0.toml");

use FieldClass::{Control, Domain, Reconciliation, Structural};

// Kind-shaped statuses carrying the KERNEL §1 classes; `RunStatus` stands for both `TaskRun` and
// `AgentRun`, which carry the same classes. The kinds' own status types belong to
// the tasks that own the kinds; these exercise the derive and the envelope the way they will.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
enum Hold {
    Running,
    Held,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
enum Phase {
    Active,
    Draining,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, FieldClasses)]
struct ManagerAuthority {
    #[field(domain)]
    lease_uid: String,
    #[field(domain)]
    epoch: u64,
    #[field(control)]
    phase: Phase,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LedgerEntry {
    permit: String,
    state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, FieldClasses)]
struct WorkContextStatus {
    #[serde(flatten)]
    #[field(nested)]
    envelope: StatusEnvelope,
    #[field(control)]
    hold_state: Hold,
    #[field(control)]
    hold_generation: u64,
    #[field(domain)]
    admission_sequence: u64,
    #[field(nested)]
    manager_authority: BTreeMap<String, ManagerAuthority>,
    #[field(control)]
    dispatch_authority_generation: u64,
    #[field(reconciliation)]
    dispatch_ledger: Vec<LedgerEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, FieldClasses)]
struct RunStatus {
    #[serde(flatten)]
    #[field(nested)]
    envelope: StatusEnvelope,
    #[field(domain)]
    state: String,
    #[field(control)]
    fence_state: String,
    #[field(control)]
    execution_epoch: u64,
    #[field(control)]
    revocation_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, FieldClasses)]
struct TaskStatus {
    #[serde(flatten)]
    #[field(nested)]
    envelope: StatusEnvelope,
    #[field(domain)]
    state: String,
}

fn digest(byte: u8) -> Digest {
    Digest::from_bytes([byte; 32])
}

fn seq(n: u64) -> CommitSequence {
    CommitSequence::new(n).expect("in range")
}

fn crev(n: u64) -> ControlRevision {
    ControlRevision::new(n).expect("in range")
}

fn srev(n: u64) -> StateRevision {
    StateRevision::new(n).expect("in range")
}

fn m0_ring() -> ControlRing {
    Profile::parse(M0_PROFILE)
        .expect("profiles/m0.toml parses")
        .values()
        .control_ring
}

fn slot(sequence: u64) -> PendingCommit {
    PendingCommit {
        command_uid: format!("cmd-{sequence}").parse().expect("uid"),
        receipt_uid: format!("rcpt-{sequence}").parse().expect("uid"),
        commit_sequence: seq(sequence),
        before_digest: digest(1),
        after_digest: digest(2),
        expected_revision: srev(0),
        proposed_revision: srev(1),
        control_revision_at_commit: crev(0),
        audit_digest: digest(3),
        effect_intents: Vec::new(),
        state: PendingCommitState::Occupied,
    }
}

fn receipt(sequence: u64, revision: u64) -> ControlReceipt {
    ControlReceipt {
        control_uid: format!("ctl-{sequence}").parse().expect("uid"),
        control_revision: crev(revision),
        commit_sequence: seq(sequence),
        before_control_digest: digest(4),
        after_control_digest: digest(5),
        audit_envelope: AuditEnvelope {
            state_revision: srev(1),
            source_uid: "src".parse().expect("uid"),
            event_type: "HoldRequested".to_owned(),
            causation_id: "cause".to_owned(),
            correlation_id: "corr".to_owned(),
            schema_version: 1,
        },
        principal: "context-controller".parse().expect("principal"),
        state: ControlReceiptState::Unpublished,
    }
}

/// A `WorkContext` after one domain commit and one control commit, with one manager entry and
/// one ledger entry.
fn work_context() -> WorkContextStatus {
    let mut envelope = StatusEnvelope::with_control_lane();
    envelope.state_revision = srev(1);
    envelope.control_revision = crev(1);
    envelope.commit_sequence = seq(2);
    envelope.pending_commit = Some(slot(1));
    envelope
        .control_receipt_ring
        .as_mut()
        .expect("ring")
        .append(receipt(2, 1), &m0_ring())
        .expect("append");
    WorkContextStatus {
        envelope,
        hold_state: Hold::Running,
        hold_generation: 1,
        admission_sequence: 1,
        manager_authority: BTreeMap::from([(
            "plan-a".to_owned(),
            ManagerAuthority {
                lease_uid: "lease-a".to_owned(),
                epoch: 1,
                phase: Phase::Active,
            },
        )]),
        dispatch_authority_generation: 1,
        dispatch_ledger: vec![LedgerEntry {
            permit: "permit-1".to_owned(),
            state: "ACCEPTED".to_owned(),
        }],
    }
}

fn run() -> RunStatus {
    RunStatus {
        envelope: StatusEnvelope::with_control_lane(),
        state: "RUNNING".to_owned(),
        fence_state: "UNFENCED".to_owned(),
        execution_epoch: 1,
        revocation_generation: 0,
    }
}

fn set(classes: &[FieldClass]) -> ClassSet {
    classes.iter().copied().collect()
}

/// The paths `S` declares in `class`, as text.
fn paths_in<S: FieldClasses>(class: FieldClass) -> Vec<String> {
    partition::<S>()
        .into_iter()
        .filter(|(_, c)| *c == class)
        .map(|(p, _)| p.to_string())
        .collect()
}

/// The backticked names in `text`, with a trailing `[]` dropped (`dispatch_ledger[]` names the
/// whole ledger).
fn backticked(text: &str) -> Vec<String> {
    text.split('`')
        .skip(1)
        .step_by(2)
        .map(|name| name.trim_end_matches("[]").to_owned())
        .collect()
}

/// One clause of KERNEL §1 "Field classes in the core": the kinds it names, its control
/// fields and its reconciliation fields.
struct KernelClause {
    kinds: Vec<String>,
    control: Vec<String>,
    reconciliation: Vec<String>,
}

fn kernel_clauses() -> Vec<KernelClause> {
    let start = KERNEL
        .find("Field classes in the core: ")
        .expect("KERNEL §1 prints the field classes");
    let rest = &KERNEL[start + "Field classes in the core: ".len()..];
    let end = rest.find(". Everything else").expect("the list ends");
    rest[..end]
        .split("; ")
        .map(|clause| {
            let (head, reconciliation) = clause
                .split_once("reconciliation fields ")
                .expect("every clause names reconciliation fields");
            let (kinds, control) = head.split_once("control fields ").unwrap_or((head, ""));
            KernelClause {
                kinds: backticked(kinds),
                control: backticked(control),
                reconciliation: backticked(reconciliation),
            }
        })
        .collect()
}

fn clause_for(kind: &str) -> KernelClause {
    let mut clauses = kernel_clauses();
    let i = clauses
        .iter()
        .position(|c| c.kinds.iter().any(|k| k == kind))
        .or_else(|| clauses.iter().position(|c| c.kinds.is_empty()))
        .expect("a clause covers every kind");
    clauses.swap_remove(i)
}

/// KERNEL §1's control fields for `kind`, after `control_revision` (module rustdoc).
fn kernel_control(kind: &str) -> Vec<String> {
    let mut control = vec!["control_revision".to_owned()];
    control.extend(clause_for(kind).control);
    control
}

#[test]
fn kernel_clauses_are_read() {
    let clauses = kernel_clauses();
    assert_eq!(clauses.len(), 3);
    assert_eq!(clauses[0].kinds, ["WorkContext"]);
    assert_eq!(clauses[1].kinds, ["TaskRun", "AgentRun"]);
    assert!(clauses[2].kinds.is_empty());
    assert!(clauses[2].control.is_empty());
}

#[test]
fn work_context_partition_is_kernel_section_1() {
    assert_eq!(
        paths_in::<WorkContextStatus>(Control),
        kernel_control("WorkContext")
    );
    assert_eq!(
        paths_in::<WorkContextStatus>(Reconciliation),
        clause_for("WorkContext").reconciliation
    );
    assert_eq!(
        paths_in::<WorkContextStatus>(Domain),
        [
            "state_revision",
            "admission_sequence",
            "manager_authority[*]",
            "manager_authority[*].lease_uid",
            "manager_authority[*].epoch",
        ]
    );
}

#[test]
fn run_partitions_are_kernel_section_1() {
    for kind in ["TaskRun", "AgentRun"] {
        assert_eq!(
            paths_in::<RunStatus>(Control),
            kernel_control(kind),
            "{kind}"
        );
        assert_eq!(
            paths_in::<RunStatus>(Reconciliation),
            clause_for(kind).reconciliation,
            "{kind}"
        );
    }
}

#[test]
fn other_kind_partition_is_kernel_section_1() {
    assert_eq!(paths_in::<TaskStatus>(Control), kernel_control("Task"));
    assert_eq!(
        paths_in::<TaskStatus>(Reconciliation),
        clause_for("Task").reconciliation
    );
    assert_eq!(paths_in::<TaskStatus>(Domain), ["state_revision", "state",]);
}

#[test]
/// The envelope's bookkeeping is structural as #365 decides; so are the slot body and ring entries.
fn bookkeeping_slot_body_and_ring_entries_are_structural() {
    assert_eq!(
        paths_in::<TaskStatus>(Structural),
        [
            "observed_generation",
            "conditions",
            "commit_sequence",
            "last_receipt_ref",
            "pending_commit",
            "pending_commit.command_uid",
            "pending_commit.receipt_uid",
            "pending_commit.commit_sequence",
            "pending_commit.before_digest",
            "pending_commit.after_digest",
            "pending_commit.expected_revision",
            "pending_commit.proposed_revision",
            "pending_commit.control_revision_at_commit",
            "pending_commit.audit_digest",
            "pending_commit.effect_intents",
            "control_receipt_ring",
            "control_receipt_ring[*]",
            "control_receipt_ring[*].control_uid",
            "control_receipt_ring[*].control_revision",
            "control_receipt_ring[*].commit_sequence",
            "control_receipt_ring[*].before_control_digest",
            "control_receipt_ring[*].after_control_digest",
            "control_receipt_ring[*].audit_envelope",
            "control_receipt_ring[*].principal",
        ]
    );
}

/// A change to a `WorkContextStatus`.
type Change = fn(&mut WorkContextStatus);

#[test]
fn every_single_field_change_touches_its_declared_class() {
    let before = work_context();
    let cases: [(&str, Change, FieldClass); 8] = [
        ("hold_state", |s| s.hold_state = Hold::Held, Control),
        ("hold_generation", |s| s.hold_generation += 1, Control),
        (
            "manager_authority[*].phase",
            |s| {
                if let Some(e) = s.manager_authority.get_mut("plan-a") {
                    e.phase = Phase::Draining;
                }
            },
            Control,
        ),
        (
            "dispatch_authority_generation",
            |s| s.dispatch_authority_generation += 1,
            Control,
        ),
        (
            "manager_authority[*].epoch",
            |s| {
                if let Some(e) = s.manager_authority.get_mut("plan-a") {
                    e.epoch += 1;
                }
            },
            Domain,
        ),
        ("admission_sequence", |s| s.admission_sequence += 1, Domain),
        (
            "pending_commit.state",
            |s| {
                if let Some(slot) = s.envelope.pending_commit.as_mut() {
                    slot.state = PendingCommitState::Cleared;
                }
            },
            Reconciliation,
        ),
        (
            "control_receipt_ring[*].state",
            |s| {
                if let Some(ring) = s.envelope.control_receipt_ring.as_mut() {
                    ring.mark_published(seq(2)).expect("in ring");
                }
            },
            Reconciliation,
        ),
    ];
    for (path, change, class) in cases {
        let mut after = before.clone();
        change(&mut after);
        assert_ne!(after, before, "{path} changed");
        assert_eq!(classify(&before, &after), set(&[class]), "{path}");
    }
}

#[test]
fn run_control_fields_touch_control_only() {
    let before = run();
    let cases: [fn(&mut RunStatus); 3] = [
        |s| s.fence_state = "FENCING".to_owned(),
        |s| s.execution_epoch += 1,
        |s| s.revocation_generation += 1,
    ];
    for change in cases {
        let mut after = before.clone();
        change(&mut after);
        assert_eq!(classify(&before, &after), set(&[Control]));
        assert_eq!(classify(&before, &after).write(), Ok(Write::Control));
    }
    let mut after = before.clone();
    after.state = "SUCCEEDED".to_owned();
    assert_eq!(classify(&before, &after).write(), Ok(Write::Domain));
}

#[test]
fn an_unchanged_status_is_unchanged() {
    let status = work_context();
    assert!(classify(&status, &status.clone()).is_empty());
    assert_eq!(classify(&status, &status).write(), Ok(Write::Unchanged));
}

#[test]
fn a_domain_commit_is_a_domain_write() {
    let before = work_context();
    let mut after = before.clone();
    after.admission_sequence += 1;
    after.envelope.state_revision = srev(2);
    after.envelope.commit_sequence = seq(3);
    after.envelope.pending_commit = Some(slot(3));
    after.dispatch_ledger.push(LedgerEntry {
        permit: "permit-2".to_owned(),
        state: "ACCEPTED".to_owned(),
    });
    let touched = classify(&before, &after);
    assert_eq!(touched, set(&[Domain, Reconciliation, Structural]));
    assert_eq!(touched.write(), Ok(Write::Domain));
}

#[test]
fn a_control_commit_is_a_control_write() {
    let before = work_context();
    let mut after = before.clone();
    after.hold_state = Hold::Held;
    after.hold_generation += 1;
    after.envelope.control_revision = crev(2);
    after.envelope.commit_sequence = seq(3);
    after
        .envelope
        .control_receipt_ring
        .as_mut()
        .expect("ring")
        .append(receipt(3, 2), &m0_ring())
        .expect("append");
    let touched = classify(&before, &after);
    assert_eq!(touched, set(&[Control, Reconciliation, Structural]));
    assert_eq!(touched.write(), Ok(Write::Control));
}

/// FORMAL §5: "control commit touching a domain field".
#[test]
fn a_control_commit_touching_a_domain_field_is_rejected() {
    let before = work_context();
    let mut after = before.clone();
    after.hold_state = Hold::Held;
    after.envelope.control_revision = crev(2);
    after.admission_sequence += 1;
    let touched = classify(&before, &after);
    assert_eq!(touched, set(&[Domain, Control]));
    assert_eq!(touched.write(), Err(RejectedWrite::DomainAndControl));

    let mut after = before.clone();
    after.hold_state = Hold::Held;
    after.envelope.state_revision = srev(2);
    assert_eq!(
        classify(&before, &after).write(),
        Err(RejectedWrite::DomainAndControl)
    );
}

#[test]
fn a_new_manager_authority_entry_touches_domain_and_control() {
    let before = work_context();
    let mut after = before.clone();
    after.manager_authority.insert(
        "plan-b".to_owned(),
        ManagerAuthority {
            lease_uid: "lease-b".to_owned(),
            epoch: 1,
            phase: Phase::Active,
        },
    );
    assert_eq!(classify(&before, &after), set(&[Domain, Control]));
    assert_eq!(classify(&after, &before), set(&[Domain, Control]));
}

#[test]
fn reconciliation_only_writes_are_recognised() {
    let before = work_context();

    let mut cleared = before.clone();
    if let Some(slot) = cleared.envelope.pending_commit.as_mut() {
        slot.state = PendingCommitState::Cleared;
    }
    let mut published = before.clone();
    if let Some(ring) = published.envelope.control_receipt_ring.as_mut() {
        ring.mark_published(seq(2)).expect("in ring");
    }
    let mut advanced = before.clone();
    advanced.dispatch_ledger[0].state = "SETTLED".to_owned();
    let mut removed = before.clone();
    removed.dispatch_ledger.clear();

    for after in [cleared, published, advanced, removed] {
        assert_eq!(
            classify(&before, &after).write(),
            Ok(Write::ReconciliationOnly)
        );
    }
}

#[test]
fn structural_changes_without_a_lane_are_rejected() {
    let before = work_context();

    let mut body = before.clone();
    if let Some(slot) = body.envelope.pending_commit.as_mut() {
        slot.audit_digest = digest(9);
    }
    let mut sequence = before.clone();
    if let Some(slot) = sequence.envelope.pending_commit.as_mut() {
        slot.state = PendingCommitState::Cleared;
    }
    sequence.envelope.commit_sequence = seq(3);
    let mut ring_body = before.clone();
    ring_body.envelope.control_receipt_ring = Some(
        vec![receipt(5, 1)]
            .try_into()
            .expect("a one-entry ring is ordered"),
    );

    for after in [body, sequence, ring_body] {
        assert_eq!(
            classify(&before, &after).write(),
            Err(RejectedWrite::StructuralOnly)
        );
    }
}

#[test]
fn a_ring_that_evicts_as_it_appends_is_still_a_control_write() {
    let limits = ControlRing {
        entries: std::num::NonZeroU32::new(1).expect("non-zero"),
        ..m0_ring()
    };
    let mut before = run();
    let ring = before.envelope.control_receipt_ring.as_mut().expect("ring");
    ring.append(receipt(1, 1), &limits).expect("append");
    ring.mark_published(seq(1)).expect("in ring");
    before.envelope.control_revision = crev(1);
    before.envelope.commit_sequence = seq(1);

    let mut after = before.clone();
    after.fence_state = "FENCING".to_owned();
    after.envelope.control_revision = crev(2);
    after.envelope.commit_sequence = seq(2);
    let ring = after.envelope.control_receipt_ring.as_mut().expect("ring");
    ring.append(receipt(2, 2), &limits).expect("append");
    assert_eq!(ring.entries().len(), 1);

    assert_eq!(classify(&before, &after).write(), Ok(Write::Control));
}

#[test]
fn installing_the_first_slot_and_ring_touches_their_classes() {
    let before = TaskStatus {
        envelope: StatusEnvelope::default(),
        state: "PENDING".to_owned(),
    };
    let mut after = before.clone();
    after.envelope.pending_commit = Some(slot(1));
    assert_eq!(
        classify(&before, &after),
        set(&[Structural, Reconciliation])
    );

    let mut after = before.clone();
    after.envelope.control_receipt_ring = Some(Default::default());
    assert_eq!(classify(&before, &after), set(&[Structural]));
}

#[test]
fn a_control_commit_setting_a_condition_is_a_control_write() {
    let before = run();
    let mut after = before.clone();
    after.envelope.control_revision = crev(1);
    after.envelope.commit_sequence = seq(1);
    after.envelope.conditions.push(Condition {
        type_: "ControlRingFull".to_owned(),
        status: ConditionStatus::True,
        observed_generation: None,
        last_transition_time: "2026-01-01T00:00:00Z".to_owned(),
        reason: "UnpublishedReceipts".to_owned(),
        message: "the control-receipt ring is full".to_owned(),
    });
    let touched = classify(&before, &after);
    assert_eq!(touched, set(&[Control, Structural]));
    assert_eq!(touched.write(), Ok(Write::Control));

    let mut condition_only = before.clone();
    condition_only.envelope.conditions = after.envelope.conditions.clone();
    assert_eq!(
        classify(&before, &condition_only).write(),
        Err(RejectedWrite::StructuralOnly)
    );
}

#[test]
fn removing_the_slot_touches_its_classes() {
    let before = work_context();
    let mut after = before.clone();
    after.envelope.pending_commit = None;
    assert_eq!(
        classify(&before, &after),
        set(&[Structural, Reconciliation])
    );
}

#[test]
fn write_names_the_lane_of_a_class_set() {
    assert_eq!(set(&[]).write(), Ok(Write::Unchanged));
    assert_eq!(
        set(&[Reconciliation]).write(),
        Ok(Write::ReconciliationOnly)
    );
    assert_eq!(set(&[Domain]).write(), Ok(Write::Domain));
    assert_eq!(set(&[Control, Structural]).write(), Ok(Write::Control));
    assert_eq!(
        set(&[Structural, Reconciliation]).write(),
        Err(RejectedWrite::StructuralOnly)
    );
    assert_eq!(
        set(&FieldClass::ALL).write(),
        Err(RejectedWrite::DomainAndControl)
    );
}

#[test]
fn paths_print_as_kernel_section_1_does() {
    let path = FieldPath::root()
        .field("manager_authority")
        .each()
        .field("phase");
    assert_eq!(path.to_string(), "manager_authority[*].phase");
    assert_eq!(
        path.segments(),
        [
            Segment::Field("manager_authority"),
            Segment::Each,
            Segment::Field("phase")
        ]
    );
}

/// A field without `#[field(...)]`, and malformed attributes, do not compile.
#[test]
fn unannotated_and_malformed_fields_do_not_compile() {
    trybuild::TestCases::new().compile_fail("src/fields/ui/*.rs");
}
