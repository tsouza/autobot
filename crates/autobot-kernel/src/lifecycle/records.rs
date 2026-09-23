//! Receipts, stamps, operations and the records of the commit and dispatch protocols.

use crate::status::{ControlReceiptState, PendingCommitState};

lifecycle! {
    /// The state of a `CommandReceipt`.
    pub enum CommandReceiptState in "CommandReceipt" {
        /// Prepared before its command is evaluated. The initial state.
        Prepared = "PREPARED",
        /// Its command committed.
        Committed = "COMMITTED",
        /// Its command was rejected; the receipt holds the rejection proof.
        Rejected = "REJECTED",
        /// A reserved Manager command that the target's owning controller cancelled.
        Cancelled = "CANCELLED",
        /// Received after its pinned replay window; never evaluated.
        ReplayExpired = "REPLAY_EXPIRED",
        /// Whether its command committed is not yet known.
        Uncertain = "UNCERTAIN",
    }
    edges {
        [Prepared] -> [Committed, Rejected];
        [Prepared, Uncertain] -> [Cancelled] if CancelledReservation;
        [Prepared] -> [ReplayExpired] if ReplayWindowPassed;
        [Prepared] -> [Uncertain];
        [Uncertain] -> [Committed, Rejected];
    }
}

lifecycle! {
    /// The state of an `AdmissionStamp`.
    pub enum AdmissionStampState in "AdmissionStamp" {
        /// Issued by admission. The initial state.
        Issued = "ISSUED",
        /// The broker accepted it for dispatch.
        BrokerAccepted = "BROKER_ACCEPTED",
        /// Its dispatch used it.
        Consumed = "CONSUMED",
        /// Invalidated before use.
        Invalidated = "INVALIDATED",
        /// Its expiry passed before use.
        Expired = "EXPIRED",
    }
    edges {
        [Issued] -> [BrokerAccepted] if OperationPermitted;
        [BrokerAccepted] -> [Consumed];
        [Issued] -> [Invalidated] if LatePermit;
        [BrokerAccepted] -> [Invalidated] if RevalidationRefused;
        [Issued] -> [Expired] if LatePermit;
    }
}

lifecycle! {
    /// The state of an `ExternalOperation` or a `ToolInvocation`.
    pub enum OperationState in "ExternalOperation, ToolInvocation" {
        /// Requested, with no current permit. The initial state.
        Requested = "REQUESTED",
        /// Holding a permit; with a ledger entry `ACCEPTED_NOT_SENT` it is accepted, not sent.
        Permitted = "PERMITTED",
        /// Being sent; it always carries a send attempt.
        Dispatching = "DISPATCHING",
        /// Its effect is known to be applied.
        Confirmed = "CONFIRMED",
        /// The provider rejected it.
        Rejected = "REJECTED",
        /// Known to have failed.
        Failed = "FAILED",
        /// Sent, with no known outcome.
        OutcomeUnknown = "OUTCOME_UNKNOWN",
        /// Its outcome is being reconciled with the provider.
        Reconciling = "RECONCILING",
        /// Reconciliation could not settle it; only a human adjudication ends it.
        Unresolved = "UNRESOLVED",
        /// Adjudicated as compensated.
        Compensated = "COMPENSATED",
        /// Proven not applied, with no attempt that will send it.
        Released = "RELEASED",
        /// Its provider or target does not support it.
        BlockedUnsupported = "BLOCKED_UNSUPPORTED",
    }
    edges {
        [Requested] -> [Permitted];
        [Permitted] -> [Dispatching];
        [Dispatching] -> [Confirmed, Rejected, Failed];
        [Permitted] -> [Requested] if PermitInvalidated;
        [Dispatching] -> [OutcomeUnknown];
        [OutcomeUnknown] -> [Reconciling];
        [Reconciling] -> [Confirmed, Failed];
        [Reconciling] -> [Requested] if NonApplicationProven;
        [Reconciling] -> [Unresolved];
        [Unresolved] -> [Confirmed, Compensated, Failed] if HumanAdjudication;
        [Unresolved] -> [Released] if AdjudicatedNotApplied;
        [Requested, Permitted] -> [BlockedUnsupported];
        [Requested] -> [Released] if UnsentNotApplied;
        [Requested] -> [Confirmed] if RestoredFoundApplied;
        [Requested] -> [Unresolved] if RestoredUnverifiable;
    }
}

lifecycle! {
    /// The state of an `EffectIntent`.
    pub enum EffectIntentState in "EffectIntent" {
        /// Materialized from its commit's slot. The initial state.
        Materialized = "MATERIALIZED",
        /// Its operation is terminal.
        Acknowledged = "ACKNOWLEDGED",
        /// Held back from dispatch.
        Quarantined = "QUARANTINED",
    }
    edges {
        [Materialized] -> [Acknowledged] if OperationTerminal, OperationSettled;
        [Materialized] -> [Quarantined] if OperationSettled;
    }
}

lifecycle! {
    /// The state of an `EffectReceipt`: immutable, one per attempt.
    pub enum EffectReceiptState in "EffectReceipt" {
        /// Recorded. The only state.
        Recorded = "RECORDED",
    }
    edges {}
}

lifecycle! {
    impl PendingCommitState in "pending commit slot" {
        Cleared = "CLEARED",
        Occupied = "OCCUPIED",
        Repairing = "REPAIRING",
    }
    edges {
        [Cleared] -> [Occupied];
        [Occupied] -> [Cleared];
        [Occupied] -> [Repairing];
        [Repairing] -> [Cleared] if ReceiptReconstructed;
    }
}

lifecycle! {
    impl ControlReceiptState in "control receipt" {
        Unpublished = "UNPUBLISHED",
        Published = "PUBLISHED",
    }
    edges {
        [Unpublished] -> [Published] if AuditPublished;
    }
}

lifecycle! {
    /// The phase of a context's `active_manager_transaction` slot.
    pub enum ReservationPhase in "reservation phase" {
        /// A Manager command holds the slot. The initial state.
        Reserved = "RESERVED",
        /// The target's owning controller claimed the command.
        Applying = "APPLYING",
        /// Released with its terminal state and proving receipt.
        Resolved = "RESOLVED",
    }
    edges {
        [Reserved] -> [Applying];
        [Applying] -> [Resolved] if TerminalStateRecorded;
        [Reserved] -> [Resolved] if ReleasedBeforeClaim, TerminalStateRecorded;
        [Resolved] -> [Reserved] if NewReservation;
    }
}

lifecycle! {
    /// The `terminal_state` of a context's `active_manager_transaction` slot.
    pub enum ReservationTerminalState in "reservation phase", field "terminal_state" {
        /// No terminal state is recorded. The initial state.
        None = "NONE",
        /// The reserved command committed.
        Committed = "COMMITTED",
        /// The reserved command was cancelled.
        Cancelled = "CANCELLED",
        /// The reserved command was rejected.
        Rejected = "REJECTED",
    }
    edges {
        [None] -> [Committed, Cancelled, Rejected] if ReleasedWithReceipt;
        [Committed, Cancelled, Rejected] -> [None] if NewReservationClears;
    }
}

lifecycle! {
    /// The `send_state` of a dispatch ledger entry.
    pub enum SendState in "ledger entry" {
        /// Accepted for dispatch, not sent. The initial state.
        AcceptedNotSent = "ACCEPTED_NOT_SENT",
        /// A send was attempted.
        SendAttempted = "SEND_ATTEMPTED",
        /// Written in the CAS that removes the entry.
        Acknowledged = "ACKNOWLEDGED",
    }
    edges {
        [AcceptedNotSent] -> [SendAttempted];
        [SendAttempted] -> [Acknowledged] if EntryRemoved;
        [AcceptedNotSent] -> [Acknowledged] if EntryRemoved, RefusedBeforeSend, NoSendAttempt;
    }
}

lifecycle! {
    /// The state of an expected record of a `TaskRun`: its outcome, or one producer's usage.
    pub enum ExpectedRecordState in "expected record" {
        /// Expected, not yet committed. The initial state.
        Pending = "PENDING",
        /// Its record is committed.
        Recorded = "RECORDED",
        /// A telemetry gap was opened for it; final.
        Gap = "GAP",
    }
    edges {
        [Pending] -> [Recorded, Gap];
    }
}
