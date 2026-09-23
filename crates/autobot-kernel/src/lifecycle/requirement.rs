//! What §10 states about a transition beyond its source and target.

/// Defines [`Requirement`], its `ALL` list and its phrases from one line per variant.
macro_rules! requirements {
    ($($(#[$doc:meta])* $name:ident => $phrase:literal;)+) => {
        /// One clause of a KERNEL §10 note that states a condition or a cause of a transition.
        ///
        /// Each variant quotes one `;`-separated clause of §10, verbatim
        /// ([`Requirement::phrase`]). A transition carries every variant whose clause applies
        /// to it. The `lifecycle_sync` test accounts for every clause of §10. Each clause is
        /// quoted by exactly one variant, on every transition the clause annotates or names and
        /// only on transitions of the states the clause holds, or it is listed there as
        /// descriptive.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum Requirement {
            $($(#[$doc])* $name,)+
        }

        impl Requirement {
            /// Every requirement, in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$name,)+];

            /// The §10 clause the requirement quotes.
            #[must_use]
            pub fn phrase(self) -> &'static str {
                match self {
                    $(Self::$name => $phrase,)+
                }
            }
        }
    };
}

requirements! {
    /// A reserved Manager command whose slot was `APPLYING` when its target's owning controller
    /// consumed its expected revision with the cancel CAS.
    CancelledReservation => "CANCELLED: a reserved Manager command whose slot was APPLYING when \
        its target's owning controller consumed its fixed expected revision with the §4 cancel \
        CAS, so it can never commit";
    /// The command arrived after its pinned replay window.
    ReplayWindowPassed => "REPLAY_EXPIRED: the command was received after its own pinned replay \
        window";
    /// The stamp's operation records its permit as `PERMITTED`.
    OperationPermitted => "ISSUED → BROKER_ACCEPTED only for a permit its operation records as \
        PERMITTED, the only permit the broker submits AcceptDispatch for";
    /// A currency or register re-validation failed, or the target was refused as blocked at
    /// step 2a, before any send attempt.
    RevalidationRefused => "BROKER_ACCEPTED → INVALIDATED only on a currency or register \
        re-validation failure, or a blocked-target refusal at 2a, before any send_attempt";
    /// A permit whose create landed after its operation ended.
    LatePermit => "a permit whose create lands after its operation ended is never accepted and \
        ends INVALIDATED or EXPIRED, §3.3";
    /// The permit was invalidated before acceptance, or by a failed re-validation or a
    /// blocked-target refusal at step 2a, before any send.
    PermitInvalidated => "permit invalidated: before acceptance, or on a currency or register \
        re-validation failure or a blocked-target refusal at 2a, before any send";
    /// Non-application is proven.
    NonApplicationProven => "non-application proven";
    /// Only a human adjudication takes the transition.
    HumanAdjudication => "human adjudication only";
    /// A human adjudication that finds no send attempt and the effect not applied.
    AdjudicatedNotApplied => "human adjudication only: no send_attempt, not applied, §3.3";
    /// Unsent, no attempt will send it, and proven not applied.
    UnsentNotApplied => "unsent, no attempt will send it, proven not applied, §3.3";
    /// Restored and unsent, and a lookup finds it applied.
    RestoredFoundApplied => "restored, unsent, a lookup finds it applied, §3.3";
    /// Restored and unsent, on a provider offering no lookup.
    RestoredUnverifiable => "restored, unsent, a provider offering no lookup, §7";
    /// The intent's operation is terminal.
    OperationTerminal => "the Broker writes ACKNOWLEDGED once the intent's operation is terminal";
    /// The intent's operation is not `OUTCOME_UNKNOWN`, `RECONCILING` or `UNRESOLVED`.
    OperationSettled => "an operation OUTCOME_UNKNOWN, RECONCILING or UNRESOLVED keeps it \
        MATERIALIZED";
    /// A new process reconstructed the slot's receipt and event.
    ReceiptReconstructed => "REPAIRING while a new process reconstructs the receipt and event";
    /// Audit publication drained the ring entry.
    AuditPublished => "drained by audit publication";
    /// Released as `CANCELLED` before any claim.
    ReleasedBeforeClaim => "released as CANCELLED before any claim, §4";
    /// A new reservation overwrites the `RESOLVED` slot.
    NewReservation => "a new reservation overwrites a RESOLVED slot";
    /// A new reservation clears the terminal state of the `RESOLVED` slot.
    NewReservationClears => "a new reservation overwrites the RESOLVED slot";
    /// The reservation's `terminal_state` is set, with the receipt that proves it.
    TerminalStateRecorded => "RESOLVED only with a non-empty terminal_state and its receipt";
    /// `ReleaseManagerTransaction` sets it with its proving receipt.
    ReleasedWithReceipt => "set by ReleaseManagerTransaction with its proving receipt";
    /// Written in the CAS that removes the ledger entry.
    EntryRemoved => "ACKNOWLEDGED is written in the CAS that removes the entry";
    /// A currency failure or blocked-target refusal at step 2, or a currency or register failure
    /// at recovery row 1.
    RefusedBeforeSend => "currency failure or blocked-target refusal at step 2, or currency or \
        register failure at recovery row 1";
    /// No send was attempted.
    NoSendAttempt => "no send_attempt";
    /// The same revision resumes, or the replacement activates.
    SameOrReplacementRevision => "the same revision, or the replacement";
    /// The context's `hold_causes` is empty.
    HoldCausesEmpty => "a hold requested in RELEASING keeps it there until that hold's own \
        RESUME, because CompleteHoldRelease requires hold_causes empty";
    /// The same snapshot is activated again.
    SameSnapshot => "same snapshot";
    /// Only for a replacement revision.
    ReplacementRevisionOnly => "replacement revision only";
    /// The same revision resumes.
    SameRevision => "same revision";
    /// A replacement revision activates.
    ReplacementRevision => "replacement revision";
    /// The plan's activation receipt is `COMMITTED`.
    ActivationCommitted => "ACTIVE follows only its COMMITTED receipt";
    /// The activation was not submitted, or its receipt is `REJECTED`; never while it is
    /// `UNCERTAIN`.
    ActivationNotUncertain => "ACTIVATION_FAILED acknowledges PlanSnapshot ACTIVATION_FAILED and \
        is reached only before that submission or on a REJECTED receipt, never while the receipt \
        is UNCERTAIN";
    /// A plan that holds a `plan_authority` entry was quiesced and its active attempts waited
    /// for first.
    QuiescedFirst => "FAILED and CANCELLED of a plan that holds a plan_authority entry follow \
        its QuiescePlan and the §5 wait for active attempts, during which the phase stays where \
        it was and then moves directly to FAILED or CANCELLED";
    /// No activation receipt of the plan is `UNCERTAIN`.
    NoUncertainActivation => "no plan is CANCELLED while an activation receipt is UNCERTAIN";
    /// An explicit resume of the same revision.
    ExplicitResume => "explicit resume of the same revision";
    /// The same snapshot is retried; verification starts again.
    SnapshotRetry => "retry of the same snapshot: verification starts again";
    /// The intake client revised the proposal.
    RevisedByIntakeClient => "revised by the intake client";
    /// The Intake, or a proposal it holds, was revised.
    IntakeRevised => "a revision of the Intake or of a proposal it holds";
    /// The Context controller writes `EXPIRED` when the lease's entry is drained or removed.
    LeaseDrained => "the Context controller writes EXPIRED on the DrainManager control commit \
        that drained this lease_uid, whatever caused the drain, or on the COMMITTED receipt of \
        the RetirePlanAuthority that removed its entry";
    /// The acceptance adjudication the Task controller commits.
    AcceptanceAdjudication => "VERIFYING → ACCEPTED is the acceptance adjudication the Task \
        controller commits";
    /// The blocking decision, dependency, budget, capability or evidence is resolved.
    BlockResolved => "blocking decision, dependency, budget, capability or evidence resolved";
    /// Setup failed, or the run was fenced.
    SetupFailedOrFenced => "setup failed, or fenced";
    /// The run was fenced.
    Fenced => "fenced";
    /// A cancel fences the run first: its `fence_state` is `FENCED` or `FENCED_UNCERTAIN`.
    FenceSettled => "a cancel fences first: only once fence_state is FENCED or FENCED_UNCERTAIN";
    /// The fence is requested on the control lane from a phase that is not terminal, and the
    /// phase does not move.
    FenceFromLivePhase => "control lane, from any non-terminal phase, phase unchanged";
    /// One TaskRun control CAS that also increments `execution_epoch`.
    FenceRequested => "the TaskRun holds the fence request and the epoch: ACTIVE → FENCE_PENDING \
        is one TaskRun control CAS that also increments execution_epoch";
    /// The run's `FenceSession` at its epoch reached the matching state: `CONFIRMED` for
    /// `FENCED`, `UNCERTAIN` for `FENCED_UNCERTAIN`.
    FenceSessionReached => "FENCED and FENCED_UNCERTAIN acknowledge the FenceSession of this \
        TaskRun at that epoch reaching CONFIRMED or UNCERTAIN, never the reverse";
    /// The run's `fence_state` is `ACTIVE`.
    FenceActive => "once fence_state leaves ACTIVE the phase never moves to EXECUTING or \
        SUCCEEDED";
    /// The run's `fence_state` is not `FENCE_PENDING`: it is `ACTIVE`, or the fence has settled.
    FenceNotPending => "it moves to FAILED, or to CANCELLED when a cancel caused the fence, and \
        only once fence_state is FENCED or FENCED_UNCERTAIN";
    /// The confirmation may land after the phase is terminal.
    LateFenceConfirmation => "FENCED_UNCERTAIN → FENCED may land after the phase is terminal";
    /// The session continued.
    Continuation => "continuation";
    /// The session's `continuation_deadline` passed.
    ContinuationDeadlinePassed => "continuation_deadline passed";
    /// The run's `fence_state` is `FENCED` or `FENCED_UNCERTAIN`.
    HeartbeatFenceSettled => "only once fence_state is FENCED or FENCED_UNCERTAIN";
    /// A cancel fences the TaskRun first.
    CancelFencesFirst => "CANCELLED from any phase only once fence_state is FENCED or \
        FENCED_UNCERTAIN: a cancel fences the TaskRun first";
    /// The checkpoint's `execution_epoch` is below its TaskRun's.
    EpochBelowRun => "STALE: its execution_epoch is below its TaskRun's";
    /// Out-of-scope content was detected at the checkpoint.
    OutOfScopeContent => "QUARANTINED: out-of-scope content was detected at the checkpoint, \
        ROLES §2";
    /// The write fence is lifted after a custody checkpoint.
    WriteFenceLifted => "write fence lifted after a custody checkpoint";
    /// For uncertain custody, a later custody checkpoint of the workspace is `VERIFIED`.
    QuarantineCleared => "QUARANTINED for uncertain custody leaves once a later custody \
        checkpoint of it is VERIFIED";
    /// For a scope escape or unattributed work, and for a conflict, the workspace's
    /// `WorkspaceConflict` is `ADJUDICATED`.
    ConflictAdjudicated => "QUARANTINED for a scope escape or unattributed work, and CONFLICT, \
        leave only once their WorkspaceConflict is ADJUDICATED";
    /// The workspace was never `QUARANTINED` or in `CONFLICT`.
    NotRetireOnly => "a workspace that was ever QUARANTINED or in CONFLICT never returns to \
        READY or IN_USE, and later work restores its preserved artifact into a new Workspace";
    /// Every artifact is verified by digest.
    DigestsVerified => "UPLOADED: every Artifact is VERIFIED by digest";
    /// An independent restore into a fresh location is verified and `restore_receipt` set.
    RestoreVerified => "VERIFIED: an independent restore into a fresh location is verified and \
        restore_receipt set, and the completion marker follows";
    /// The `CustodyCheckpoint` is `VERIFIED` and its completion marker is written.
    CustodyCompleted => "VERIFIED only for a CustodyCheckpoint VERIFIED with its completion \
        marker written";
    /// The register's entry for the basis is `INVALIDATED`.
    RegisterInvalidated => "STALE and RELEASED acknowledge the register's INVALIDATED and differ \
        by cause";
    /// Every merge operation of the merge order is terminal and the register is advanced.
    MergesTerminal => "RELEASED: every merge operation of its merge order is terminal and \
        InvalidateIntegrationBasis has advanced the register";
    /// Settlement, or an explicit conservative expiry.
    Settlement => "settlement or explicit conservative expiry";
    /// The consumer is terminal or fenced (`ATTEMPT`), or the operation is terminal or
    /// `UNRESOLVED` (`EFFECT`), and the usage is not settled.
    ConsumerUnsettled => "UNKNOWN: its consumer, the TaskRun for ATTEMPT, is terminal or fenced, \
        or the operation for EFFECT is terminal or UNRESOLVED, and its usage is not SETTLED";
    /// An `EFFECT` reservation's operation is not `OUTCOME_UNKNOWN` or `RECONCILING`.
    EffectStaysReserved => "an EFFECT reservation stays RESERVED while its operation is \
        OUTCOME_UNKNOWN or RECONCILING, so a re-request, or a later attempt that inherits the \
        operation, sends under it, §9";
    /// The usage is settled.
    OnSettlement => "COMMITTED on settlement";
    /// Zero use is proven.
    ProvenZeroUse => "RELEASED on proven zero use";
    /// An explicit conservative expiry, counted at the reservation ceiling.
    ConservativeExpiry => "EXPIRED only by an explicit conservative expiry, counted at the \
        reservation ceiling";
    /// An append-only correction: the earlier record stays.
    AppendOnlyCorrection => "append-only correction";
    /// A record of the gap's kind committed after the gap is linked from it.
    LateRecordLinked => "CLOSED: a record of the gap's kind for its TaskRun committed after the \
        gap, linked from it";
    /// The outbox that held the record is lost.
    OutboxLost => "PERMANENT: the outbox that held the record is lost, §8";
    /// A new run of the gate.
    NewRun => "a new run of the gate";
    /// `DeclarePermanentGap` makes the gap permanent.
    GapDeclaredPermanent => "a projection's read model of one aggregate, not a kind: \
        DetectAuditGap opens a gap, the repaired event closes it and DeclarePermanentGap makes \
        it PERMANENT";
    /// `RejectDigestConflict` records the conflict.
    DigestConflictRejected => "RejectDigestConflict records DIGEST_CONFLICT";
}
