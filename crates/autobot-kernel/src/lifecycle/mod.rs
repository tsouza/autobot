//! Every lifecycle of `docs/design/AUTOBOT-KERNEL.md` §10 as a typed transition table.
//!
//! Each §10 machine, and each field a machine labels (`hold_state`, `fence_state`,
//! `terminal_state`, ...), is one state enum implementing [`Lifecycle`]. The enum lists its
//! states in printed order, with the initial state first, and its allowed transitions as
//! [`Edges`]. Transitions §10 says more about carry a [`Requirement`]. The names of the form
//! *Verb-ed* that §10 calls event types are [`LifecycleEvent`], never a state. [`tables`] lists
//! every machine in §10 order as a [`Table`]. The `lifecycle_sync` test diffs that list against
//! the design text through the shared §10 parser.
//!
//! State enums serialize as the printed state names (`OUTCOME_UNKNOWN`, `FENCE_PENDING`), and
//! their JSON schemas enumerate those names.
//!
//! # Reading of the design text
//!
//! - `any non-terminal → X` is an edge to `X` from every state with an outgoing transition of
//!   its own, except `X` itself. No table holds an edge from a state to itself.
//! - A state is terminal when no edge leaves it ([`Lifecycle::is_terminal`]). A table whose
//!   every state has an exit, such as [`HoldState`], has no terminal state.
//! - The parser *annotates* a transition in three ways:
//!   - a note printed on it (`(same snapshot)`);
//!   - the line note of the line that prints it, when it is an arrow into that line's last group
//!     (`(human adjudication only)`);
//!   - a note of its machine that names it (`BROKER_ACCEPTED → INVALIDATED only …`, `UNKNOWN: …`).
//!
//!   Each annotated transition carries the [`Requirement`] that quotes its annotations, whether
//!   they state a condition or a cause. A transition §10 does not annotate carries a requirement
//!   only when a machine note states a condition on it and names one of its states, such as
//!   [`Requirement::FenceActive`] or [`Requirement::HoldCausesEmpty`]. A requirement
//!   names what must hold or what happened. The controller that takes the transition checks it.
//! - `ExternalOperation`'s `(human adjudication only)` closes the line `RECONCILING →
//!   UNRESOLVED → CONFIRMED | COMPENSATED | FAILED`. It annotates only the exits of
//!   `UNRESOLVED`, the line's last arrow, which FORMAL §2 records as the human adjudication of an
//!   `UNRESOLVED` operation. It does not annotate `RECONCILING → UNRESOLVED`.
//! - `TaskRun`'s "once `fence_state` leaves `ACTIVE` the phase never moves to `EXECUTING` or
//!   `SUCCEEDED`" is [`Requirement::FenceActive`], on the edges into those two states only.
//! - `AgentRun`'s "`CANCELLED` from any phase only once `fence_state` is `FENCED` or
//!   `FENCED_UNCERTAIN`" is [`Requirement::CancelFencesFirst`] on `STARTING → CANCELLED` and
//!   `RUNNING → CANCELLED`, and [`Requirement::FenceSettled`] on `HEARTBEAT_LOST → CANCELLED`,
//!   whose line note states the same condition. "Any phase" is every phase with an edge into
//!   `CANCELLED`. `COMPLETED`, `FAILED` and `CANCELLED` are terminal and have none.
//! - `AgentRun`'s `HEARTBEAT_LOST → fence_state := FENCE_PENDING` moves the sibling field and
//!   leaves the phase where it is: it is a [`SiblingSet`], not an edge.
//! - `AgentRun.fence_state` is `as TaskRun`. Both use [`FenceState`], whose table is the
//!   `TaskRun` one. [`tables`] lists it a second time under `AgentRun`, with
//!   [`Table::same_as`] set.
//! - The pending commit slot and the control receipt are the kernel's own enums,
//!   [`crate::status::PendingCommitState`] and [`crate::status::ControlReceiptState`], which
//!   this module gives their tables.

#[macro_use]
mod macros;
mod event;
mod requirement;

mod budget;
mod custody;
mod judgment;
mod plan;
mod records;
mod run;
mod verify;

#[cfg(test)]
mod tests;

pub use budget::{
    BudgetReservationState, BudgetState, OutcomeRecordState, TelemetryGapState, UsageReceiptState,
};
pub use custody::{
    ArtifactCommitState, ArtifactState, CustodyCheckpointState, CustodyPolicyState,
    RestoreRequestState, WorkspaceConflictState, WorkspaceState,
};
pub use event::LifecycleEvent;
pub use judgment::{
    DecisionState, FindingState, GateState, InterventionState, ProjectionGap, ProjectionIntegrity,
};
pub use plan::{
    CharterRevisionState, CharterState, HoldState, IntakeState, IntegrationAuthorityState,
    ManagerLeaseState, ManagerPhase, PlanPhase, PlanProposalState, PlanRevisionState,
    PlanSnapshotState, ProjectState, RevisionPhase, TaskState, WorkBriefState,
};
pub use records::{
    AdmissionStampState, CommandReceiptState, EffectIntentState, EffectReceiptState,
    ExpectedRecordState, OperationState, ReservationPhase, ReservationTerminalState, SendState,
};
pub use requirement::Requirement;
pub use run::{
    AgentCheckpointState, AgentRunState, CredentialGrantState, ExecutionIdentityState,
    FenceSessionState, FenceState, ScopeCapsuleState, TaskRunState,
};
pub use verify::{EvidenceBundleState, IntegrationBasisState, VerificationRunState};

use crate::status::{ControlReceiptState, PendingCommitState};
use schemars::JsonSchema;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fmt::Debug;
use std::hash::Hash;

/// A group of transitions as a table prints them: every state of `from` may move to every state
/// of `to`, under `requires` when it is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edges<S: 'static> {
    /// The source states.
    pub from: &'static [S],
    /// The target states.
    pub to: &'static [S],
    /// The condition every transition of the group needs, beyond its source state.
    pub requires: Option<Requirement>,
}

/// One allowed transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Edge<S> {
    /// The source state.
    pub from: S,
    /// The target state.
    pub to: S,
    /// The condition the transition needs, beyond its source state.
    pub requires: Option<Requirement>,
}

/// A move of a machine that sets a sibling field instead of its own state (§10 `field := STATE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SiblingSet<S> {
    /// The state of this machine the move is taken from; it stays in that state.
    pub from: S,
    /// The label of the sibling field that moves.
    pub field: &'static str,
    /// The printed name of the state the sibling field moves to.
    pub to: &'static str,
}

/// A lifecycle of KERNEL §10: one machine, or one labelled field of a machine.
pub trait Lifecycle:
    Copy + Eq + Hash + Debug + Serialize + DeserializeOwned + JsonSchema + 'static
{
    /// The machine's name as §10 prints it (`TaskRun`, `ExternalOperation, ToolInvocation`,
    /// `pending commit slot`).
    const MACHINE: &'static str;
    /// The field label the table belongs to (`fence_state`), or `None` for the machine's own
    /// states.
    const FIELD: Option<&'static str>;
    /// Every state, in the order §10 first lists it.
    const STATES: &'static [Self];
    /// The initial state: the first one listed.
    const INITIAL: Self = Self::STATES[0];
    /// The allowed transitions, grouped. No transition appears in two groups.
    const EDGES: &'static [Edges<Self>];
    /// The moves that set a sibling field.
    const SIBLING_SETS: &'static [SiblingSet<Self>] = &[];

    /// The state's printed name, which is also its serde form.
    fn as_str(self) -> &'static str;

    /// The state printed as `name`.
    #[must_use]
    fn from_name(name: &str) -> Option<Self> {
        Self::STATES.iter().copied().find(|s| s.as_str() == name)
    }

    /// Every allowed transition, in table order.
    fn edges() -> impl Iterator<Item = Edge<Self>> {
        Self::EDGES.iter().flat_map(|g| {
            g.from.iter().flat_map(move |&from| {
                g.to.iter().map(move |&to| Edge {
                    from,
                    to,
                    requires: g.requires,
                })
            })
        })
    }

    /// The transition from `self` to `to`, if the table allows it.
    #[must_use]
    fn edge(self, to: Self) -> Option<Edge<Self>> {
        Self::edges().find(|e| e.from == self && e.to == to)
    }

    /// Whether the table allows moving from `self` to `to`.
    #[must_use]
    fn allows(self, to: Self) -> bool {
        self.edge(to).is_some()
    }

    /// Whether no transition leaves `self`.
    #[must_use]
    fn is_terminal(self) -> bool {
        !Self::edges().any(|e| e.from == self)
    }
}

/// A [`Lifecycle`] with its states as their printed names, so tables of different enums can be
/// listed and compared together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    /// The machine's name as §10 prints it.
    pub machine: &'static str,
    /// The field label, or `None` for the machine's own states.
    pub field: Option<&'static str>,
    /// The machine whose table of the same field this one is a copy of (`fence_state: as
    /// TaskRun`).
    pub same_as: Option<&'static str>,
    /// Every state, in printed order; the first is the initial state.
    pub states: Vec<&'static str>,
    /// Every allowed transition, in table order.
    pub edges: Vec<Edge<&'static str>>,
    /// The moves that set a sibling field.
    pub sibling_sets: Vec<SiblingSet<&'static str>>,
}

impl Table {
    /// The table of `L`.
    #[must_use]
    pub fn of<L: Lifecycle>() -> Self {
        Self {
            machine: L::MACHINE,
            field: L::FIELD,
            same_as: None,
            states: L::STATES.iter().map(|s| s.as_str()).collect(),
            edges: L::edges()
                .map(|e| Edge {
                    from: e.from.as_str(),
                    to: e.to.as_str(),
                    requires: e.requires,
                })
                .collect(),
            sibling_sets: L::SIBLING_SETS
                .iter()
                .map(|s| SiblingSet {
                    from: s.from.as_str(),
                    field: s.field,
                    to: s.to,
                })
                .collect(),
        }
    }

    /// The table of `L`'s field on `machine`, which §10 prints as `field: as L::MACHINE`.
    #[must_use]
    pub fn copy_of<L: Lifecycle>(machine: &'static str) -> Self {
        Self {
            machine,
            same_as: Some(L::MACHINE),
            ..Self::of::<L>()
        }
    }

    /// The initial state.
    #[must_use]
    pub fn initial(&self) -> Option<&'static str> {
        self.states.first().copied()
    }
}

/// Every table, in the order §10 prints its machines and fields.
#[must_use]
pub fn tables() -> Vec<Table> {
    vec![
        Table::of::<CommandReceiptState>(),
        Table::of::<AdmissionStampState>(),
        Table::of::<OperationState>(),
        Table::of::<EffectIntentState>(),
        Table::of::<EffectReceiptState>(),
        Table::of::<PendingCommitState>(),
        Table::of::<ControlReceiptState>(),
        Table::of::<ReservationPhase>(),
        Table::of::<ReservationTerminalState>(),
        Table::of::<SendState>(),
        Table::of::<ExpectedRecordState>(),
        Table::of::<HoldState>(),
        Table::of::<ManagerPhase>(),
        Table::of::<RevisionPhase>(),
        Table::of::<IntegrationAuthorityState>(),
        Table::of::<PlanPhase>(),
        Table::of::<PlanRevisionState>(),
        Table::of::<PlanSnapshotState>(),
        Table::of::<PlanProposalState>(),
        Table::of::<IntakeState>(),
        Table::of::<WorkBriefState>(),
        Table::of::<ProjectState>(),
        Table::of::<CharterState>(),
        Table::of::<CharterRevisionState>(),
        Table::of::<ManagerLeaseState>(),
        Table::of::<TaskState>(),
        Table::of::<TaskRunState>(),
        Table::of::<FenceState>(),
        Table::of::<AgentRunState>(),
        Table::copy_of::<FenceState>("AgentRun"),
        Table::of::<AgentCheckpointState>(),
        Table::of::<ScopeCapsuleState>(),
        Table::of::<ExecutionIdentityState>(),
        Table::of::<CredentialGrantState>(),
        Table::of::<FenceSessionState>(),
        Table::of::<WorkspaceState>(),
        Table::of::<CustodyPolicyState>(),
        Table::of::<CustodyCheckpointState>(),
        Table::of::<ArtifactCommitState>(),
        Table::of::<ArtifactState>(),
        Table::of::<WorkspaceConflictState>(),
        Table::of::<RestoreRequestState>(),
        Table::of::<IntegrationBasisState>(),
        Table::of::<VerificationRunState>(),
        Table::of::<EvidenceBundleState>(),
        Table::of::<BudgetState>(),
        Table::of::<BudgetReservationState>(),
        Table::of::<UsageReceiptState>(),
        Table::of::<OutcomeRecordState>(),
        Table::of::<TelemetryGapState>(),
        Table::of::<FindingState>(),
        Table::of::<DecisionState>(),
        Table::of::<InterventionState>(),
        Table::of::<GateState>(),
        Table::of::<ProjectionGap>(),
        Table::of::<ProjectionIntegrity>(),
    ]
}
