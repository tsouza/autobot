//! The conditions §10 puts on a transition beyond its source state.

/// A condition a transition needs beyond its source state, as KERNEL §10 states it.
///
/// Each variant names the text of §10 that states it ([`Requirement::phrases`]); the
/// `lifecycle_sync` test requires that text in the notes of every machine whose table uses the
/// variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Requirement {
    /// The stamp's operation records its permit as `PERMITTED`.
    OperationPermitted,
    /// A currency or register re-validation failed, or the target was refused as blocked at
    /// step 2a, before any send attempt.
    RevalidationRefused,
    /// Only a human adjudication takes the transition.
    HumanAdjudication,
    /// The intent's operation is terminal.
    OperationTerminal,
    /// The reservation's `terminal_state` is set, with the receipt that proves it.
    TerminalStateRecorded,
    /// The context's `hold_causes` is empty.
    HoldCausesEmpty,
    /// The plan's activation receipt is `COMMITTED`.
    ActivationCommitted,
    /// The activation was not submitted, or its receipt is `REJECTED`; never while it is
    /// `UNCERTAIN`.
    ActivationNotUncertain,
    /// No activation receipt of the plan is `UNCERTAIN`.
    NoUncertainActivation,
    /// The plan is activating a replacement revision.
    ReplacementRevision,
    /// The run's `fence_state` is `ACTIVE`.
    FenceActive,
    /// The run's `fence_state` is not `FENCE_PENDING`: it is `ACTIVE`, or the fence has settled.
    FenceNotPending,
    /// The run's `fence_state` is `FENCED` or `FENCED_UNCERTAIN`.
    FenceSettled,
    /// The run's `FenceSession` at its epoch reached the matching state: `CONFIRMED` for
    /// `FENCED`, `UNCERTAIN` for `FENCED_UNCERTAIN`.
    FenceSessionReached,
    /// The workspace was never `QUARANTINED` or in `CONFLICT`.
    NotRetireOnly,
    /// For uncertain custody, a later custody checkpoint of the workspace is `VERIFIED`; for a
    /// scope escape or unattributed work, its `WorkspaceConflict` is `ADJUDICATED`.
    QuarantineCleared,
    /// The workspace's `WorkspaceConflict` is `ADJUDICATED`.
    ConflictAdjudicated,
    /// The `CustodyCheckpoint` is `VERIFIED` and its completion marker is written.
    CustodyCompleted,
    /// An explicit conservative expiry, counted at the reservation ceiling.
    ConservativeExpiry,
    /// The transition is an append-only correction: the earlier record stays.
    AppendOnlyCorrection,
}

impl Requirement {
    /// Every requirement.
    pub const ALL: &'static [Self] = &[
        Self::OperationPermitted,
        Self::RevalidationRefused,
        Self::HumanAdjudication,
        Self::OperationTerminal,
        Self::TerminalStateRecorded,
        Self::HoldCausesEmpty,
        Self::ActivationCommitted,
        Self::ActivationNotUncertain,
        Self::NoUncertainActivation,
        Self::ReplacementRevision,
        Self::FenceActive,
        Self::FenceNotPending,
        Self::FenceSettled,
        Self::FenceSessionReached,
        Self::NotRetireOnly,
        Self::QuarantineCleared,
        Self::ConflictAdjudicated,
        Self::CustodyCompleted,
        Self::ConservativeExpiry,
        Self::AppendOnlyCorrection,
    ];

    /// The text of §10 that states the requirement, one entry per sentence part it takes.
    #[must_use]
    pub fn phrases(self) -> &'static [&'static str] {
        match self {
            Self::OperationPermitted => {
                &["ISSUED → BROKER_ACCEPTED only for a permit its operation records as PERMITTED"]
            }
            Self::RevalidationRefused => &[
                "BROKER_ACCEPTED → INVALIDATED only on a currency or register re-validation \
                 failure, or a blocked-target refusal at 2a, before any send_attempt",
            ],
            Self::HumanAdjudication => &["human adjudication only"],
            Self::OperationTerminal => {
                &["the Broker writes ACKNOWLEDGED once the intent's operation is terminal"]
            }
            Self::TerminalStateRecorded => {
                &["RESOLVED only with a non-empty terminal_state and its receipt"]
            }
            Self::HoldCausesEmpty => &["CompleteHoldRelease requires hold_causes empty"],
            Self::ActivationCommitted => &["ACTIVE follows only its COMMITTED receipt"],
            Self::ActivationNotUncertain => &[
                "reached only before that submission or on a REJECTED receipt, never while the \
                 receipt is UNCERTAIN",
            ],
            Self::NoUncertainActivation => {
                &["no plan is CANCELLED while an activation receipt is UNCERTAIN"]
            }
            Self::ReplacementRevision => &["replacement revision only"],
            Self::FenceActive => {
                &["once fence_state leaves ACTIVE the phase never moves to EXECUTING or SUCCEEDED"]
            }
            Self::FenceNotPending => &[
                "it moves to FAILED, or to CANCELLED when a cancel caused the fence, and only once \
                 fence_state is FENCED or FENCED_UNCERTAIN",
            ],
            Self::FenceSettled => &["only once fence_state is FENCED or FENCED_UNCERTAIN"],
            Self::FenceSessionReached => &[
                "FENCED and FENCED_UNCERTAIN acknowledge the FenceSession of this TaskRun at that \
                 epoch reaching CONFIRMED or UNCERTAIN, never the reverse",
            ],
            Self::NotRetireOnly => &[
                "a workspace that was ever QUARANTINED or in CONFLICT never returns to READY or IN_USE",
            ],
            Self::QuarantineCleared => &[
                "QUARANTINED for uncertain custody leaves once a later custody checkpoint of it is \
                 VERIFIED",
                "QUARANTINED for a scope escape or unattributed work, and CONFLICT, leave only once \
                 their WorkspaceConflict is ADJUDICATED",
            ],
            Self::ConflictAdjudicated => {
                &["and CONFLICT, leave only once their WorkspaceConflict is ADJUDICATED"]
            }
            Self::CustodyCompleted => &[
                "VERIFIED only for a CustodyCheckpoint VERIFIED with its completion marker written",
            ],
            Self::ConservativeExpiry => &["EXPIRED only by an explicit conservative expiry"],
            Self::AppendOnlyCorrection => &["append-only correction"],
        }
    }
}
