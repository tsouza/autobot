//! Runs, sessions, capsules, identities, grants and fencing.

lifecycle! {
    /// The phase of a `TaskRun`.
    pub enum TaskRunState in "TaskRun" {
        /// Created, not yet admitted. The initial state.
        Pending = "PENDING",
        /// Admitted.
        Admitted = "ADMITTED",
        /// Its workspace and session are being prepared.
        Preparing = "PREPARING",
        /// Executing.
        Executing = "EXECUTING",
        /// Its candidate is being verified.
        Verifying = "VERIFYING",
        /// Succeeded.
        Succeeded = "SUCCEEDED",
        /// Failed.
        Failed = "FAILED",
        /// Recovering from an interrupted execution or verification.
        Recovering = "RECOVERING",
        /// Cancelled, after its fence.
        Cancelled = "CANCELLED",
    }
    edges {
        [Pending] -> [Admitted];
        [Admitted] -> [Preparing];
        [Preparing] -> [Executing] if FenceActive;
        [Executing] -> [Verifying];
        [Verifying] -> [Succeeded] if FenceActive;
        [Verifying] -> [Failed] if FenceNotPending;
        [Executing, Verifying] -> [Recovering];
        [Recovering] -> [Executing] if FenceActive;
        [Recovering] -> [Failed] if FenceNotPending;
        [Preparing] -> [Failed] if FenceNotPending;
        [Pending, Admitted, Executing] -> [Failed] if FenceSettled;
        [Pending, Admitted, Preparing, Executing, Verifying, Recovering] -> [Cancelled] if FenceSettled;
    }
}

lifecycle! {
    /// The `fence_state` of a `TaskRun`, on its control lane; an `AgentRun` holds an acknowledged
    /// copy of it.
    pub enum FenceState in "TaskRun", field "fence_state" {
        /// Not fenced. The initial state.
        Active = "ACTIVE",
        /// A fence was requested and the execution epoch advanced.
        FencePending = "FENCE_PENDING",
        /// The fence is confirmed.
        Fenced = "FENCED",
        /// The fence ended without confirmation.
        FencedUncertain = "FENCED_UNCERTAIN",
    }
    edges {
        [Active] -> [FencePending];
        [FencePending] -> [Fenced, FencedUncertain] if FenceSessionReached;
        [FencedUncertain] -> [Fenced] if FenceSessionReached;
    }
}

lifecycle! {
    /// The phase of an `AgentRun`.
    pub enum AgentRunState in "AgentRun" {
        /// Starting. The initial state.
        Starting = "STARTING",
        /// Running.
        Running = "RUNNING",
        /// Completed.
        Completed = "COMPLETED",
        /// Failed.
        Failed = "FAILED",
        /// Cancelled, after its TaskRun's fence.
        Cancelled = "CANCELLED",
        /// Its heartbeat was lost.
        HeartbeatLost = "HEARTBEAT_LOST",
    }
    edges {
        [Starting] -> [Running];
        [Running] -> [Completed, Failed];
        [Running] -> [Cancelled] if FenceSettled;
        [Starting] -> [Failed];
        [Starting] -> [Cancelled] if FenceSettled;
        [Running] -> [HeartbeatLost];
        [HeartbeatLost] -> [Running];
        [HeartbeatLost] -> [Failed, Cancelled] if FenceSettled;
    }
    sibling_sets {
        HeartbeatLost -> "fence_state" := FenceState::FencePending;
    }
}

lifecycle! {
    /// The state of an `AgentCheckpoint`.
    pub enum AgentCheckpointState in "AgentCheckpoint" {
        /// Created. The initial state.
        Created = "CREATED",
        /// Covered by a verified custody checkpoint.
        Verified = "VERIFIED",
        /// Its execution epoch is below its TaskRun's.
        Stale = "STALE",
        /// Out-of-scope content was detected at the checkpoint.
        Quarantined = "QUARANTINED",
    }
    edges {
        [Created] -> [Verified, Stale, Quarantined];
        [Verified] -> [Stale];
    }
}

lifecycle! {
    /// The state of a `ScopeCapsule`.
    pub enum ScopeCapsuleState in "ScopeCapsule" {
        /// Issued. The initial state.
        Issued = "ISSUED",
        /// Revoked.
        Revoked = "REVOKED",
    }
    edges {
        [Issued] -> [Revoked];
    }
}

lifecycle! {
    /// The state of an `ExecutionIdentity`.
    pub enum ExecutionIdentityState in "ExecutionIdentity" {
        /// Issued. The initial state.
        Issued = "ISSUED",
        /// Revoked.
        Revoked = "REVOKED",
    }
    edges {
        [Issued] -> [Revoked];
    }
}

lifecycle! {
    /// The state of a `CredentialGrant`.
    pub enum CredentialGrantState in "CredentialGrant" {
        /// Issued. The initial state.
        Issued = "ISSUED",
        /// Its expiry passed.
        Expired = "EXPIRED",
        /// Revoked.
        Revoked = "REVOKED",
    }
    edges {
        [Issued] -> [Expired, Revoked];
    }
}

lifecycle! {
    /// The state of a `FenceSession`: the Broker's evidence of one fence of one TaskRun at one
    /// execution epoch.
    pub enum FenceSessionState in "FenceSession" {
        /// The fence is in progress. The initial state.
        Pending = "PENDING",
        /// The fence is confirmed; it records `FenceConfirmed`.
        Confirmed = "CONFIRMED",
        /// The fence could not be confirmed.
        Uncertain = "UNCERTAIN",
    }
    edges {
        [Pending] -> [Confirmed, Uncertain];
        [Uncertain] -> [Confirmed];
    }
}
