//! Budgets and canonical records.

lifecycle! {
    /// The state of a `Budget`.
    pub enum BudgetState in "Budget" {
        /// Open. The initial state.
        Open = "OPEN",
        /// Its ceiling is reached.
        Exhausted = "EXHAUSTED",
        /// Closed.
        Closed = "CLOSED",
    }
    edges {
        [Open] -> [Exhausted, Closed];
    }
}

lifecycle! {
    /// The state of a `BudgetReservation`.
    pub enum BudgetReservationState in "BudgetReservation" {
        /// Reserved. The initial state.
        Reserved = "RESERVED",
        /// Settled as used.
        Committed = "COMMITTED",
        /// Released on proven zero use.
        Released = "RELEASED",
        /// Its consumer is terminal or fenced, or its operation is terminal or `UNRESOLVED`, and its
        /// usage is not settled.
        Unknown = "UNKNOWN",
        /// Expired by an explicit conservative expiry, counted at the reservation ceiling.
        Expired = "EXPIRED",
    }
    edges {
        [Reserved] -> [Committed, Released];
        [Reserved] -> [Unknown] if ConsumerUnsettled;
        [Reserved] -> [Expired] if ConservativeExpiry;
        [Unknown] -> [Committed, Released, Expired] if Settlement;
    }
}

lifecycle! {
    /// The state of a `UsageReceipt`.
    pub enum UsageReceiptState in "UsageReceipt" {
        /// Pending. The initial state.
        Pending = "PENDING",
        /// Partly recorded.
        Partial = "PARTIAL",
        /// Settled.
        Settled = "SETTLED",
        /// Disputed.
        Disputed = "DISPUTED",
        /// Its usage is unknown.
        Unknown = "UNKNOWN",
        /// Recorded with a censored bound.
        Censored = "CENSORED",
    }
    edges {
        [Pending] -> [Partial];
        [Partial] -> [Settled, Disputed];
        [Pending, Partial] -> [Unknown];
        [Unknown] -> [Censored];
        [Censored] -> [Settled] if AppendOnlyCorrection;
    }
}

lifecycle! {
    /// The state of an `OutcomeRecord`.
    pub enum OutcomeRecordState in "OutcomeRecord" {
        /// Provisional. The initial state.
        Provisional = "PROVISIONAL",
        /// Mature.
        Mature = "MATURE",
        /// A defect is confirmed.
        DefectConfirmed = "DEFECT_CONFIRMED",
        /// Revised.
        Revised = "REVISED",
    }
    edges {
        [Provisional] -> [Mature, DefectConfirmed];
        [Mature, DefectConfirmed] -> [Revised];
    }
}

lifecycle! {
    /// The state of a `TelemetryGap`; a gap is never removed.
    pub enum TelemetryGapState in "TelemetryGap" {
        /// Open. The initial state.
        Open = "OPEN",
        /// A late record of its kind is linked from it.
        Closed = "CLOSED",
        /// The outbox that held the record is lost.
        Permanent = "PERMANENT",
    }
    edges {
        [Open] -> [Closed] if LateRecordLinked;
        [Open] -> [Permanent] if OutboxLost;
    }
}
