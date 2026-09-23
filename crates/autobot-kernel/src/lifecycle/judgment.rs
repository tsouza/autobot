//! Findings, decisions, interventions, gates and projections.

lifecycle! {
    /// The state of a `Finding`.
    pub enum FindingState in "Finding" {
        /// Raised. The initial state.
        Raised = "RAISED",
        /// Classified.
        Classified = "CLASSIFIED",
        /// Linked to history.
        Linked = "LINKED",
        /// Promoted.
        Promoted = "PROMOTED",
        /// Deferred.
        Deferred = "DEFERRED",
        /// Rejected.
        Rejected = "REJECTED",
    }
    edges {
        [Raised] -> [Classified];
        [Classified] -> [Linked, Promoted, Deferred, Rejected];
    }
}

lifecycle! {
    /// The state of a `Decision`: immutable.
    pub enum DecisionState in "Decision" {
        /// Recorded. The only state.
        Recorded = "RECORDED",
    }
    edges {}
}

lifecycle! {
    /// The state of an `Intervention`.
    pub enum InterventionState in "Intervention" {
        /// Requested. The initial state.
        Requested = "REQUESTED",
        /// Acknowledged.
        Acknowledged = "ACKNOWLEDGED",
        /// Applied.
        Applied = "APPLIED",
        /// Rejected.
        Rejected = "REJECTED",
        /// Expired.
        Expired = "EXPIRED",
    }
    edges {
        [Requested] -> [Acknowledged];
        [Acknowledged] -> [Applied, Rejected, Expired];
    }
}

lifecycle! {
    /// The state of a gate of M0 §4, which is not a kind: its evidence is a signed manifest.
    pub enum GateState in "gate" {
        /// No manifest. The initial state.
        NotRun = "NOT_RUN",
        /// A run is in progress.
        Running = "RUNNING",
        /// Passed.
        Passed = "PASSED",
        /// Failed.
        Failed = "FAILED",
        /// A passed gate that no longer holds.
        Invalidated = "INVALIDATED",
    }
    edges {
        [NotRun] -> [Running];
        [Running] -> [Passed, Failed];
        [Passed] -> [Invalidated];
        [Failed, Invalidated] -> [Running] if NewRun;
    }
}

lifecycle! {
    /// The `gap` of a projection's read model of one aggregate.
    pub enum ProjectionGap in "ProjectionState", field "gap" {
        /// No gap. The initial state.
        None = "NONE",
        /// An audit gap is detected.
        Open = "OPEN",
        /// The gap is declared permanent.
        Permanent = "PERMANENT",
    }
    edges {
        [None] -> [Open];
        [Open] -> [None, Permanent];
    }
}

lifecycle! {
    /// The `integrity` of a projection's read model of one aggregate.
    pub enum ProjectionIntegrity in "ProjectionState", field "integrity" {
        /// No conflict. The initial state.
        Ok = "OK",
        /// A digest conflict is recorded.
        DigestConflict = "DIGEST_CONFLICT",
    }
    edges {
        [Ok] -> [DigestConflict];
    }
}
