//! What §10 prints about a transition beyond its source and target.

/// Defines [`Requirement`], its `ALL` list and its phrases from one line per variant.
macro_rules! requirements {
    ($($(#[$doc:meta])* $name:ident => [$($phrase:literal),+ $(,)?];)+) => {
        /// What KERNEL §10 prints about a transition beyond its source and target: the condition
        /// it is taken under, or the cause or effect §10 annotates it with.
        ///
        /// Each variant quotes the §10 text that states it ([`Requirement::phrases`]). The
        /// `lifecycle_sync` test requires every transition §10 annotates to carry the variant
        /// whose phrases its annotations hold, and each phrase to be printed on the transition or
        /// in a note that names one of its two states.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum Requirement {
            $($(#[$doc])* $name,)+
        }

        impl Requirement {
            /// Every requirement, in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$name,)+];

            /// The §10 text that states the requirement: one entry per place it is printed.
            #[must_use]
            pub fn phrases(self) -> &'static [&'static str] {
                match self {
                    $(Self::$name => &[$($phrase),+],)+
                }
            }
        }
    };
}

requirements! {
    /// A reserved Manager command whose slot was `APPLYING` when its target's owning controller
    /// consumed its expected revision with the cancel CAS.
    CancelledReservation => ["CANCELLED: a reserved Manager command whose slot was APPLYING"];
    /// The command arrived after its pinned replay window; it is never evaluated.
    ReplayWindowPassed => [
        "REPLAY_EXPIRED: the command was received after its own pinned replay window",
    ];
    /// The stamp's operation records its permit as `PERMITTED`.
    OperationPermitted => [
        "ISSUED → BROKER_ACCEPTED only for a permit its operation records as PERMITTED",
    ];
    /// A currency or register re-validation failed, or the target was refused as blocked at
    /// step 2a, before any send attempt.
    RevalidationRefused => [
        "BROKER_ACCEPTED → INVALIDATED only on a currency or register re-validation failure, or \
         a blocked-target refusal at 2a, before any send_attempt",
    ];
    /// The permit was invalidated before acceptance, or by a failed re-validation or a
    /// blocked-target refusal at step 2a, before any send.
    PermitInvalidated => [
        "permit invalidated: before acceptance, or on a currency or register re-validation \
         failure or a blocked-target refusal at 2a, before any send",
    ];
    /// Non-application is proven; the same key is requested again with the next attempt index.
    NonApplicationProven => ["non-application proven"];
    /// Only a human adjudication takes the transition.
    HumanAdjudication => ["human adjudication only"];
    /// Unsent, no attempt will send it, and proven not applied.
    UnsentNotApplied => ["unsent, no attempt will send it, proven not applied"];
    /// Restored and unsent, and a lookup finds it applied.
    RestoredFoundApplied => ["restored, unsent, a lookup finds it applied"];
    /// Restored and unsent, on a provider offering no lookup.
    RestoredUnverifiable => ["restored, unsent, a provider offering no lookup"];
    /// The intent's operation is terminal.
    OperationTerminal => [
        "the Broker writes ACKNOWLEDGED once the intent's operation is terminal",
    ];
    /// A new process reconstructed the slot's receipt and event.
    ReceiptReconstructed => [
        "REPAIRING while a new process reconstructs the receipt and event",
    ];
    /// Audit publication drained the ring entry.
    AuditPublished => ["drained by audit publication"];
    /// The reservation's `terminal_state` is set, with the receipt that proves it.
    TerminalStateRecorded => [
        "RESOLVED only with a non-empty terminal_state and its receipt",
    ];
    /// Released as `CANCELLED` before any claim, with that terminal state and its receipt.
    ReleasedBeforeClaim => [
        "released as CANCELLED before any claim",
        "RESOLVED only with a non-empty terminal_state and its receipt",
    ];
    /// A new reservation overwrites the `RESOLVED` slot.
    NewReservation => ["new reservation overwrites"];
    /// `ReleaseManagerTransaction` sets it with its proving receipt.
    ReleasedWithReceipt => ["set by ReleaseManagerTransaction with its proving receipt"];
    /// Written in the CAS that removes the ledger entry.
    EntryRemoved => ["ACKNOWLEDGED is written in the CAS that removes the entry"];
    /// A currency failure or blocked-target refusal at step 2, or a currency or register failure
    /// at recovery row 1, with no send attempt.
    RefusedBeforeSend => [
        "currency failure or blocked-target refusal at step 2, or currency or register failure \
         at recovery row 1; no send_attempt",
    ];
    /// The record was expected, committed `PENDING` before its producer spent.
    ExpectedBeforeSpend => ["committed PENDING before its producer spends"];
    /// The context's `hold_causes` is empty.
    HoldCausesEmpty => [
        "a hold requested in RELEASING keeps it there until that hold's own RESUME, because \
         CompleteHoldRelease requires hold_causes empty",
    ];
    /// The authority returns under a new epoch.
    NewEpoch => ["new epoch"];
    /// The same revision resumes, or the replacement activates.
    SameOrReplacementRevision => ["the same revision, or the replacement"];
    /// The plan's activation receipt is `COMMITTED`.
    ActivationCommitted => ["ACTIVE follows only its COMMITTED receipt"];
    /// The activation was not submitted, or its receipt is `REJECTED`; never while it is
    /// `UNCERTAIN`.
    ActivationNotUncertain => [
        "ACTIVATION_FAILED acknowledges PlanSnapshot ACTIVATION_FAILED and is reached only before \
         that submission or on a REJECTED receipt, never while the receipt is UNCERTAIN",
    ];
    /// No activation receipt of the plan is `UNCERTAIN`.
    NoUncertainActivation => ["no plan is CANCELLED while an activation receipt is UNCERTAIN"];
    /// The same snapshot is activated again.
    SameSnapshot => ["same snapshot"];
    /// The plan is activating a replacement revision.
    ReplacementRevision => ["replacement revision"];
    /// The same revision resumes.
    SameRevision => ["same revision"];
    /// An explicit resume of the same revision.
    ExplicitResume => ["explicit resume of the same revision"];
    /// The same snapshot is retried; verification starts again.
    SnapshotRetry => ["retry of the same snapshot"];
    /// The intake client revised the proposal.
    RevisedByIntakeClient => ["revised by the intake client"];
    /// The Intake, or a proposal it holds, was revised.
    IntakeRevised => ["a revision of the Intake or of a proposal it holds"];
    /// The lease acknowledges `manager_authority` and never is authority; the Context controller
    /// writes `EXPIRED` when its entry is drained or removed.
    LeaseDrained => [
        "acknowledgement of manager_authority; never authority",
        "the Context controller writes EXPIRED on the DrainManager control commit that drained \
         this lease_uid",
    ];
    /// The acceptance adjudication the Task controller commits, listing every bundle it relies on.
    AcceptanceAdjudication => [
        "VERIFYING → ACCEPTED is the acceptance adjudication the Task controller commits",
    ];
    /// The blocking decision, dependency, budget, capability or evidence is resolved.
    BlockResolved => ["blocking decision, dependency, budget, capability or evidence resolved"];
    /// The run's `fence_state` is `ACTIVE`.
    FenceActive => [
        "once fence_state leaves ACTIVE the phase never moves to EXECUTING or SUCCEEDED",
    ];
    /// The run's `fence_state` is not `FENCE_PENDING`: it is `ACTIVE`, or the fence has settled.
    FenceNotPending => [
        "it moves to FAILED, or to CANCELLED when a cancel caused the fence, and only once \
         fence_state is FENCED or FENCED_UNCERTAIN",
    ];
    /// Setup failed, or the run was fenced and the fence has settled.
    SetupFailedOrFenced => [
        "setup failed, or fenced",
        "it moves to FAILED, or to CANCELLED when a cancel caused the fence, and only once \
         fence_state is FENCED or FENCED_UNCERTAIN",
    ];
    /// The run was fenced and the fence has settled.
    Fenced => [
        "fenced",
        "it moves to FAILED, or to CANCELLED when a cancel caused the fence, and only once \
         fence_state is FENCED or FENCED_UNCERTAIN",
    ];
    /// The run's `fence_state` is `FENCED` or `FENCED_UNCERTAIN`.
    FenceSettled => ["only once fence_state is FENCED or FENCED_UNCERTAIN"];
    /// A cancel fenced the TaskRun first: its `fence_state` is `FENCED` or `FENCED_UNCERTAIN`.
    CancelFencesFirst => [
        "CANCELLED from any phase only once fence_state is FENCED or FENCED_UNCERTAIN",
    ];
    /// One TaskRun control CAS that also increments `execution_epoch`.
    FenceRequested => [
        "ACTIVE → FENCE_PENDING is one TaskRun control CAS that also increments execution_epoch",
    ];
    /// The run's `FenceSession` at its epoch reached the matching state: `CONFIRMED` for
    /// `FENCED`, `UNCERTAIN` for `FENCED_UNCERTAIN`.
    FenceSessionReached => [
        "FENCED and FENCED_UNCERTAIN acknowledge the FenceSession of this TaskRun at that epoch \
         reaching CONFIRMED or UNCERTAIN, never the reverse",
    ];
    /// The `FenceSession` confirmed late; this may land after the phase is terminal.
    LateFenceConfirmation => [
        "FENCED_UNCERTAIN → FENCED may land after the phase is terminal",
        "FENCED and FENCED_UNCERTAIN acknowledge the FenceSession of this TaskRun at that epoch \
         reaching CONFIRMED or UNCERTAIN, never the reverse",
    ];
    /// The session continued before its deadline.
    Continuation => ["continuation"];
    /// The checkpoint's `execution_epoch` is below its TaskRun's.
    EpochBelowRun => ["STALE: its execution_epoch is below its TaskRun's"];
    /// Out-of-scope content was detected at the checkpoint.
    OutOfScopeContent => ["QUARANTINED: out-of-scope content was detected at the checkpoint"];
    /// The write fence is lifted after a custody checkpoint, for a workspace that was never
    /// `QUARANTINED` or in `CONFLICT`.
    WriteFenceLifted => [
        "write fence lifted after a custody checkpoint",
        "a workspace that was ever QUARANTINED or in CONFLICT never returns to READY or IN_USE",
    ];
    /// For uncertain custody, a later custody checkpoint of the workspace is `VERIFIED`; for a
    /// scope escape or unattributed work, its `WorkspaceConflict` is `ADJUDICATED`.
    QuarantineCleared => [
        "QUARANTINED for uncertain custody leaves once a later custody checkpoint of it is \
         VERIFIED",
        "QUARANTINED for a scope escape or unattributed work, and CONFLICT, leave only once \
         their WorkspaceConflict is ADJUDICATED",
    ];
    /// The workspace's `WorkspaceConflict` is `ADJUDICATED`.
    ConflictAdjudicated => ["and CONFLICT, leave only once their WorkspaceConflict is ADJUDICATED"];
    /// Every artifact is verified by digest.
    DigestsVerified => ["UPLOADED: every Artifact is VERIFIED by digest"];
    /// An independent restore into a fresh location is verified and `restore_receipt` set.
    RestoreVerified => [
        "VERIFIED: an independent restore into a fresh location is verified and restore_receipt set",
    ];
    /// The `CustodyCheckpoint` is `VERIFIED` and its completion marker is written.
    CustodyCompleted => [
        "VERIFIED only for a CustodyCheckpoint VERIFIED with its completion marker written",
    ];
    /// Every merge operation of the basis's merge order is terminal and the register is
    /// invalidated.
    MergesTerminal => ["RELEASED: every merge operation of its merge order is terminal"];
    /// The consumer is terminal or fenced (`ATTEMPT`), or the operation is terminal or
    /// `UNRESOLVED` (`EFFECT`), and the usage is not settled.
    ConsumerUnsettled => [
        "UNKNOWN: its consumer, the TaskRun for ATTEMPT, is terminal or fenced, or the operation \
         for EFFECT is terminal or UNRESOLVED, and its usage is not SETTLED",
    ];
    /// Settlement, or an explicit conservative expiry.
    Settlement => ["settlement or explicit conservative expiry"];
    /// An explicit conservative expiry, counted at the reservation ceiling.
    ConservativeExpiry => ["EXPIRED only by an explicit conservative expiry"];
    /// The transition is an append-only correction: the earlier record stays.
    AppendOnlyCorrection => ["append-only correction"];
    /// A record of the gap's kind committed after the gap is linked from it.
    LateRecordLinked => [
        "CLOSED: a record of the gap's kind for its TaskRun committed after the gap, linked from it",
    ];
    /// The outbox that held the record is lost.
    OutboxLost => ["PERMANENT: the outbox that held the record is lost"];
    /// A new run of the gate.
    NewRun => ["a new run of the gate"];
}
