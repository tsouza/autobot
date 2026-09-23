//! The work context's registers, plans, intake, charters and tasks.

lifecycle! {
    /// The `hold_state` of a `WorkContext`.
    pub enum HoldState in "WorkContext", field "hold_state" {
        /// No hold. The initial state.
        Running = "RUNNING",
        /// A hold was requested; admission stops.
        FreezePending = "FREEZE_PENDING",
        /// The hold is propagating to in-flight work.
        Propagating = "PROPAGATING",
        /// Nothing is in flight.
        Enforced = "ENFORCED",
        /// The hold is being released.
        Releasing = "RELEASING",
    }
    edges {
        [Running] -> [FreezePending];
        [FreezePending] -> [Propagating];
        [Propagating] -> [Enforced];
        [Enforced] -> [Releasing];
        [Releasing] -> [Running] if HoldCausesEmpty;
    }
}

lifecycle! {
    /// The phase of a `WorkContext`'s `manager_authority[plan]` entry.
    pub enum ManagerPhase in "WorkContext", field "manager_authority[plan].phase" {
        /// The lease holder has authority. The initial state.
        Active = "ACTIVE",
        /// The authority is draining.
        Draining = "DRAINING",
    }
    edges {
        [Active] -> [Draining];
        [Draining] -> [Active] if NewEpoch;
    }
}

lifecycle! {
    /// The `revision_phase` of a `WorkContext`'s `plan_authority[plan]` entry.
    pub enum RevisionPhase in "WorkContext", field "plan_authority[plan].revision_phase" {
        /// The revision admits work. The initial state.
        Active = "ACTIVE",
        /// The revision is quiescing.
        Quiescing = "QUIESCING",
    }
    edges {
        [Active] -> [Quiescing];
        [Quiescing] -> [Active] if SameOrReplacementRevision;
    }
}

lifecycle! {
    /// The state of a `WorkContext`'s `integration_authority[basis]` entry.
    pub enum IntegrationAuthorityState in "WorkContext", field "integration_authority[basis].state" {
        /// The basis is reserved. The initial state.
        Reserved = "RESERVED",
        /// The basis is invalidated; final for its key.
        Invalidated = "INVALIDATED",
    }
    edges {
        [Reserved] -> [Invalidated];
    }
}

lifecycle! {
    /// The phase of a `Plan`.
    pub enum PlanPhase in "Plan.phase" {
        /// Accepted, not yet activating. The initial state.
        Accepted = "ACCEPTED",
        /// Its snapshot is being verified and its activation submitted.
        Activating = "ACTIVATING",
        /// Its revision is active.
        Active = "ACTIVE",
        /// Its activation failed.
        ActivationFailed = "ACTIVATION_FAILED",
        /// Cancelled.
        Cancelled = "CANCELLED",
        /// Its revision is quiescing.
        Quiescing = "QUIESCING",
        /// Paused.
        Paused = "PAUSED",
        /// Completed.
        Completed = "COMPLETED",
        /// Failed.
        Failed = "FAILED",
    }
    edges {
        [Accepted] -> [Activating];
        [Activating] -> [Active] if ActivationCommitted;
        [Activating] -> [ActivationFailed] if ActivationNotUncertain;
        [ActivationFailed] -> [Activating] if SameSnapshot;
        [ActivationFailed] -> [Cancelled] if NoUncertainActivation;
        [ActivationFailed] -> [Quiescing] if ReplacementRevision;
        [Active] -> [Paused];
        [Paused] -> [Active];
        [Active, Paused] -> [Quiescing];
        [Quiescing] -> [Active] if SameRevision;
        [Quiescing] -> [Activating] if ReplacementRevision;
        [Active] -> [Completed];
        [Active, Paused, Quiescing] -> [Failed];
        [Accepted, Activating, Active, Paused, Quiescing] -> [Cancelled] if NoUncertainActivation;
    }
}

lifecycle! {
    /// The state of one revision in `Plan.status.revisions`.
    pub enum PlanRevisionState in "Plan.status.revisions[rev]" {
        /// Proposed. The initial state.
        Proposed = "PROPOSED",
        /// Its snapshot is verified.
        Verified = "VERIFIED",
        /// The active revision.
        Active = "ACTIVE",
        /// Quiescing.
        Quiescing = "QUIESCING",
        /// Replaced by a later revision.
        Superseded = "SUPERSEDED",
        /// Abandoned before activation.
        Abandoned = "ABANDONED",
    }
    edges {
        [Proposed] -> [Verified];
        [Verified] -> [Active];
        [Active] -> [Quiescing];
        [Quiescing] -> [Superseded];
        [Proposed, Verified] -> [Abandoned];
        [Quiescing] -> [Active] if ExplicitResume;
    }
}

lifecycle! {
    /// The state of a `PlanSnapshot`.
    pub enum PlanSnapshotState in "PlanSnapshot" {
        /// Proposed. The initial state.
        Proposed = "PROPOSED",
        /// The snapshot itself is verified.
        SnapshotVerified = "SNAPSHOT_VERIFIED",
        /// Its members are verified.
        MembersVerified = "MEMBERS_VERIFIED",
        /// Activated.
        Activated = "ACTIVATED",
        /// Its activation failed.
        ActivationFailed = "ACTIVATION_FAILED",
    }
    edges {
        [Proposed] -> [SnapshotVerified];
        [SnapshotVerified] -> [MembersVerified];
        [MembersVerified] -> [Activated];
        [Proposed, SnapshotVerified, MembersVerified] -> [ActivationFailed];
        [ActivationFailed] -> [Proposed] if SnapshotRetry;
    }
}

lifecycle! {
    /// The state of a `PlanProposal`.
    pub enum PlanProposalState in "PlanProposal" {
        /// Being drafted. The initial state.
        Draft = "DRAFT",
        /// Under review.
        Review = "REVIEW",
        /// Accepted.
        Accepted = "ACCEPTED",
        /// Rejected.
        Rejected = "REJECTED",
    }
    edges {
        [Draft] -> [Review];
        [Review] -> [Accepted, Rejected];
        [Draft] -> [Rejected];
        [Review] -> [Draft] if RevisedByIntakeClient;
    }
}

lifecycle! {
    /// The state of an `Intake`.
    pub enum IntakeState in "Intake" {
        /// Captured. The initial state.
        Captured = "CAPTURED",
        /// Its proposals are submitted.
        Proposed = "PROPOSED",
        /// Accepted.
        Accepted = "ACCEPTED",
        /// Rejected.
        Rejected = "REJECTED",
    }
    edges {
        [Captured] -> [Proposed];
        [Proposed] -> [Accepted, Rejected];
        [Captured] -> [Rejected];
        [Proposed] -> [Captured] if IntakeRevised;
    }
}

lifecycle! {
    /// The state of a `WorkBrief`: immutable.
    pub enum WorkBriefState in "WorkBrief" {
        /// Recorded. The only state.
        Recorded = "RECORDED",
    }
    edges {}
}

lifecycle! {
    /// The state of a `Project` or a `Repository`.
    pub enum ProjectState in "Project, Repository" {
        /// Proposed. The initial state.
        Proposed = "PROPOSED",
        /// Adopted.
        Adopted = "ADOPTED",
        /// Active.
        Active = "ACTIVE",
        /// Retired.
        Retired = "RETIRED",
        /// Rejected.
        Rejected = "REJECTED",
    }
    edges {
        [Proposed] -> [Adopted];
        [Adopted] -> [Active];
        [Active] -> [Retired];
        [Proposed] -> [Rejected];
    }
}

lifecycle! {
    /// The state of a `Charter` or a `ProjectCharter`.
    pub enum CharterState in "Charter, ProjectCharter" {
        /// Active. The initial state.
        Active = "ACTIVE",
        /// Retired.
        Retired = "RETIRED",
    }
    edges {
        [Active] -> [Retired];
    }
}

lifecycle! {
    /// The state of one revision in a charter's `revisions`.
    pub enum CharterRevisionState in "Charter, ProjectCharter", field "revisions[rev]" {
        /// Proposed. The initial state.
        Proposed = "PROPOSED",
        /// Accepted: immutable and digested.
        Accepted = "ACCEPTED",
        /// Replaced by a later revision; still pinned by every plan revision that pinned it.
        Superseded = "SUPERSEDED",
        /// Rejected.
        Rejected = "REJECTED",
    }
    edges {
        [Proposed] -> [Accepted];
        [Accepted] -> [Superseded];
        [Proposed] -> [Rejected];
    }
}

lifecycle! {
    /// The state of a `ManagerLease`, an acknowledgement of `manager_authority`.
    pub enum ManagerLeaseState in "ManagerLease" {
        /// Acknowledges its `manager_authority` entry. The initial state.
        Acknowledged = "ACKNOWLEDGED",
        /// Its lease was drained or its entry removed; it never returns.
        Expired = "EXPIRED",
    }
    edges {
        [Acknowledged] -> [Expired] if LeaseDrained;
    }
}

lifecycle! {
    /// The state of a `Task` or a `Milestone`.
    pub enum TaskState in "Task, Milestone" {
        /// Proposed. The initial state.
        Proposed = "PROPOSED",
        /// Ready to run.
        Ready = "READY",
        /// Running.
        Running = "RUNNING",
        /// Its evidence is being verified.
        Verifying = "VERIFYING",
        /// Accepted by the acceptance adjudication.
        Accepted = "ACCEPTED",
        /// Blocked on a decision, dependency, budget, capability or evidence.
        Blocked = "BLOCKED",
        /// Failed.
        Failed = "FAILED",
        /// Cancelled.
        Cancelled = "CANCELLED",
        /// Replaced by a revision.
        Superseded = "SUPERSEDED",
    }
    edges {
        [Proposed] -> [Ready];
        [Ready] -> [Running];
        [Running] -> [Verifying];
        [Verifying] -> [Accepted] if AcceptanceAdjudication;
        [Verifying] -> [Failed];
        [Ready, Running, Verifying] -> [Blocked];
        [Blocked] -> [Ready] if BlockResolved;
        [Ready, Running, Verifying, Blocked] -> [Superseded, Cancelled];
    }
}
