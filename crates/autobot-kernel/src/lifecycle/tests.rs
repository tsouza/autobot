use super::*;
use schemars::schema_for;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Runs `$check::<L>()` for every state enum.
macro_rules! each_lifecycle {
    ($check:ident) => {
        $check::<CommandReceiptState>();
        $check::<AdmissionStampState>();
        $check::<OperationState>();
        $check::<EffectIntentState>();
        $check::<EffectReceiptState>();
        $check::<PendingCommitState>();
        $check::<ControlReceiptState>();
        $check::<ReservationPhase>();
        $check::<ReservationTerminalState>();
        $check::<SendState>();
        $check::<ExpectedRecordState>();
        $check::<HoldState>();
        $check::<ManagerPhase>();
        $check::<RevisionPhase>();
        $check::<IntegrationAuthorityState>();
        $check::<PlanPhase>();
        $check::<PlanRevisionState>();
        $check::<PlanSnapshotState>();
        $check::<PlanProposalState>();
        $check::<IntakeState>();
        $check::<WorkBriefState>();
        $check::<ProjectState>();
        $check::<CharterState>();
        $check::<CharterRevisionState>();
        $check::<ManagerLeaseState>();
        $check::<TaskState>();
        $check::<TaskRunState>();
        $check::<FenceState>();
        $check::<AgentRunState>();
        $check::<AgentCheckpointState>();
        $check::<ScopeCapsuleState>();
        $check::<ExecutionIdentityState>();
        $check::<CredentialGrantState>();
        $check::<FenceSessionState>();
        $check::<WorkspaceState>();
        $check::<CustodyPolicyState>();
        $check::<CustodyCheckpointState>();
        $check::<ArtifactCommitState>();
        $check::<ArtifactState>();
        $check::<WorkspaceConflictState>();
        $check::<RestoreRequestState>();
        $check::<IntegrationBasisState>();
        $check::<VerificationRunState>();
        $check::<EvidenceBundleState>();
        $check::<BudgetState>();
        $check::<BudgetReservationState>();
        $check::<UsageReceiptState>();
        $check::<OutcomeRecordState>();
        $check::<TelemetryGapState>();
        $check::<FindingState>();
        $check::<DecisionState>();
        $check::<InterventionState>();
        $check::<GateState>();
        $check::<ProjectionGap>();
        $check::<ProjectionIntegrity>();
    };
}

fn serde_is_the_printed_name<L: Lifecycle>() {
    for &s in L::STATES {
        let json = serde_json::to_value(s).unwrap();
        assert_eq!(json, Value::String(s.as_str().to_owned()), "{}", L::MACHINE);
        assert_eq!(serde_json::from_value::<L>(json).unwrap(), s);
        assert_eq!(L::from_name(s.as_str()), Some(s));
    }
    assert!(serde_json::from_value::<L>(Value::String("NOT_A_STATE".to_owned())).is_err());
    assert_eq!(L::from_name("NOT_A_STATE"), None);
}

#[test]
fn every_state_serializes_as_its_printed_name() {
    each_lifecycle!(serde_is_the_printed_name);
}

fn schema_enumerates_the_states<L: Lifecycle>() {
    let schema = serde_json::to_value(schema_for!(L)).unwrap();
    let consts: Vec<&str> = schema["oneOf"]
        .as_array()
        .unwrap_or_else(|| panic!("{} schema has no oneOf: {schema}", L::MACHINE))
        .iter()
        .map(|v| v["const"].as_str().unwrap())
        .collect();
    let names: Vec<&str> = L::STATES.iter().map(|s| s.as_str()).collect();
    assert_eq!(consts, names, "{}", L::MACHINE);
}

#[test]
fn every_schema_enumerates_exactly_the_states() {
    each_lifecycle!(schema_enumerates_the_states);
}

fn states_are_distinct_and_initial_first<L: Lifecycle>() {
    let names: BTreeSet<&str> = L::STATES.iter().map(|s| s.as_str()).collect();
    assert_eq!(names.len(), L::STATES.len(), "{}", L::MACHINE);
    assert_eq!(Some(&L::INITIAL), L::STATES.first(), "{}", L::MACHINE);
}

#[test]
fn every_initial_state_is_the_first_listed() {
    each_lifecycle!(states_are_distinct_and_initial_first);
    assert_eq!(OperationState::INITIAL, OperationState::Requested);
    assert_eq!(PendingCommitState::INITIAL, PendingCommitState::Cleared);
    assert_eq!(GateState::INITIAL, GateState::NotRun);
}

#[test]
fn no_table_has_a_self_loop_or_a_repeated_edge() {
    for t in tables() {
        let mut seen = BTreeSet::new();
        for e in &t.edges {
            assert_ne!(e.from, e.to, "{} {:?}: self loop", t.machine, t.field);
            assert!(
                seen.insert((e.from, e.to)),
                "{} {:?}: {} → {} in two groups",
                t.machine,
                t.field,
                e.from,
                e.to
            );
        }
    }
}

#[test]
fn every_state_is_reachable_from_the_initial_state() {
    for t in tables() {
        let mut reached: BTreeSet<&str> = t.initial().into_iter().collect();
        let mut grew = true;
        while grew {
            grew = false;
            for e in &t.edges {
                if reached.contains(e.from) && reached.insert(e.to) {
                    grew = true;
                }
            }
        }
        let unreached: Vec<_> = t.states.iter().filter(|s| !reached.contains(*s)).collect();
        assert!(
            unreached.is_empty(),
            "{} {:?}: {unreached:?}",
            t.machine,
            t.field
        );
    }
}

#[test]
fn tables_list_each_machine_field_once() {
    let all = tables();
    let keys: BTreeSet<_> = all.iter().map(|t| (t.machine, t.field)).collect();
    assert_eq!(keys.len(), all.len());
    let copies: Vec<_> = all.iter().filter(|t| t.same_as.is_some()).collect();
    assert_eq!(copies.len(), 1);
    let agent_fence = copies[0];
    assert_eq!(
        (agent_fence.machine, agent_fence.field, agent_fence.same_as),
        ("AgentRun", Some("fence_state"), Some("TaskRun"))
    );
    let task_fence = Table::of::<FenceState>();
    assert_eq!(agent_fence.states, task_fence.states);
    assert_eq!(agent_fence.edges, task_fence.edges);
}

#[test]
fn every_requirement_is_listed_and_used() {
    // A new variant fails to compile here until it is placed in `ALL`.
    for &r in Requirement::ALL {
        let i = match r {
            Requirement::OperationPermitted => 0,
            Requirement::RevalidationRefused => 1,
            Requirement::HumanAdjudication => 2,
            Requirement::OperationTerminal => 3,
            Requirement::TerminalStateRecorded => 4,
            Requirement::HoldCausesEmpty => 5,
            Requirement::ActivationCommitted => 6,
            Requirement::ActivationNotUncertain => 7,
            Requirement::NoUncertainActivation => 8,
            Requirement::ReplacementRevision => 9,
            Requirement::FenceActive => 10,
            Requirement::FenceNotPending => 11,
            Requirement::FenceSettled => 12,
            Requirement::FenceSessionReached => 13,
            Requirement::NotRetireOnly => 14,
            Requirement::QuarantineCleared => 15,
            Requirement::ConflictAdjudicated => 16,
            Requirement::CustodyCompleted => 17,
            Requirement::ConservativeExpiry => 18,
            Requirement::AppendOnlyCorrection => 19,
        };
        assert_eq!(Requirement::ALL.get(i), Some(&r));
        assert!(!r.phrases().is_empty());
    }
    let used: BTreeSet<Requirement> = tables()
        .iter()
        .flat_map(|t| t.edges.iter().filter_map(|e| e.requires))
        .collect();
    let all: BTreeSet<Requirement> = Requirement::ALL.iter().copied().collect();
    assert_eq!(used, all);
}

#[test]
fn transitions_report_their_requirement() {
    use OperationState as Op;
    assert_eq!(
        Op::Unresolved
            .edge(Op::Compensated)
            .and_then(|e| e.requires),
        Some(Requirement::HumanAdjudication)
    );
    let to_unresolved = Op::Reconciling.edge(Op::Unresolved).unwrap();
    assert_eq!(to_unresolved.requires, None);
    assert!(!Op::Released.allows(Op::Requested));
    assert!(!Op::Dispatching.allows(Op::Requested));
    assert!(Op::Reconciling.allows(Op::Requested));

    assert_eq!(
        UsageReceiptState::Censored
            .edge(UsageReceiptState::Settled)
            .and_then(|e| e.requires),
        Some(Requirement::AppendOnlyCorrection)
    );
    assert_eq!(
        UsageReceiptState::Partial
            .edge(UsageReceiptState::Settled)
            .and_then(|e| e.requires),
        None
    );
    assert_eq!(
        TaskRunState::Pending
            .edge(TaskRunState::Cancelled)
            .and_then(|e| e.requires),
        Some(Requirement::FenceSettled)
    );
    assert!(!TaskRunState::Succeeded.allows(TaskRunState::Cancelled));
}

#[test]
fn terminal_states_have_no_exit() {
    assert!(TaskRunState::Succeeded.is_terminal());
    assert!(TaskRunState::Cancelled.is_terminal());
    assert!(!TaskRunState::Recovering.is_terminal());
    assert!(OperationState::Released.is_terminal());
    assert!(!OperationState::Unresolved.is_terminal());
    assert!(EffectReceiptState::Recorded.is_terminal());
    assert!(HoldState::STATES.iter().all(|s| !s.is_terminal()));
    assert!(!ReservationPhase::Resolved.is_terminal());
    assert!(FenceState::Fenced.is_terminal());
    assert!(!FenceState::FencedUncertain.is_terminal());
}

#[test]
fn any_non_terminal_expands_to_every_state_with_an_exit() {
    let into = |to: WorkspaceState| -> BTreeSet<WorkspaceState> {
        WorkspaceState::edges()
            .filter(|e| e.to == to)
            .map(|e| e.from)
            .collect()
    };
    use WorkspaceState as W;
    assert_eq!(
        into(W::Quarantined),
        [
            W::Requested,
            W::Provisioning,
            W::Ready,
            W::InUse,
            W::Preserving,
            W::Preserved,
            W::Conflict
        ]
        .into()
    );
    assert!(!W::Retired.allows(W::Quarantined));
    let into_cancelled: BTreeSet<TaskRunState> = TaskRunState::edges()
        .filter(|e| e.to == TaskRunState::Cancelled)
        .map(|e| e.from)
        .collect();
    let non_terminal: BTreeSet<TaskRunState> = TaskRunState::STATES
        .iter()
        .copied()
        .filter(|s| !s.is_terminal() && *s != TaskRunState::Cancelled)
        .collect();
    assert_eq!(into_cancelled, non_terminal);
}

#[test]
fn heartbeat_loss_requests_the_fence_without_moving_the_phase() {
    assert_eq!(
        AgentRunState::SIBLING_SETS,
        [SiblingSet {
            from: AgentRunState::HeartbeatLost,
            field: "fence_state",
            to: "FENCE_PENDING",
        }]
    );
    assert_eq!(
        FenceState::from_name(AgentRunState::SIBLING_SETS[0].to),
        Some(FenceState::FencePending)
    );
    let sets: usize = tables().iter().map(|t| t.sibling_sets.len()).sum();
    assert_eq!(sets, 1);
}

#[test]
fn events_are_not_states() {
    let mut by_name = BTreeMap::new();
    for &e in LifecycleEvent::ALL {
        assert_eq!(
            serde_json::to_value(e).unwrap(),
            Value::String(e.as_str().to_owned())
        );
        assert!(by_name.insert(e.as_str(), e).is_none());
    }
    let states: BTreeSet<&str> = tables()
        .iter()
        .flat_map(|t| t.states.iter().copied())
        .collect();
    assert!(by_name.keys().all(|n| !states.contains(n)));
}
