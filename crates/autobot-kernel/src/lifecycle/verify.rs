//! Integration and verification.

lifecycle! {
    /// The state of an `IntegrationBasis`.
    pub enum IntegrationBasisState in "IntegrationBasis" {
        /// Reserved in the register. The initial state.
        Reserved = "RESERVED",
        /// Being integrated.
        Integrating = "INTEGRATING",
        /// Its integrated candidate is verified.
        Verified = "VERIFIED",
        /// Acknowledges the register's invalidation for staleness.
        Stale = "STALE",
        /// Blocked.
        Blocked = "BLOCKED",
        /// Every merge of its merge order is terminal and the register is invalidated.
        Released = "RELEASED",
    }
    edges {
        [Reserved] -> [Integrating];
        [Integrating] -> [Verified];
        [Reserved, Integrating, Verified] -> [Stale] if RegisterInvalidated;
        [Reserved, Integrating, Verified] -> [Blocked];
        [Verified] -> [Released] if RegisterInvalidated, MergesTerminal;
    }
}

lifecycle! {
    /// The state of a `VerificationRun`.
    pub enum VerificationRunState in "VerificationRun" {
        /// Pending. The initial state.
        Pending = "PENDING",
        /// Running.
        Running = "RUNNING",
        /// Passed.
        Passed = "PASSED",
        /// Failed.
        Failed = "FAILED",
        /// Neither passed nor failed; never a generic failure.
        Inconclusive = "INCONCLUSIVE",
    }
    edges {
        [Pending] -> [Running];
        [Running] -> [Passed, Failed, Inconclusive];
    }
}

lifecycle! {
    /// The state of an `EvidenceBundle`; an accepted bundle is one a committed acceptance
    /// adjudication references, not a state.
    pub enum EvidenceBundleState in "EvidenceBundle" {
        /// Recorded. The initial state.
        Recorded = "RECORDED",
        /// Invalidated.
        Invalidated = "INVALIDATED",
        /// Expired.
        Expired = "EXPIRED",
    }
    edges {
        [Recorded] -> [Invalidated, Expired];
    }
}
