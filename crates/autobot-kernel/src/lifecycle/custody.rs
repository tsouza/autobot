//! Workspaces, custody and restore.

lifecycle! {
    /// The state of a `Workspace`.
    pub enum WorkspaceState in "Workspace" {
        /// Requested. The initial state.
        Requested = "REQUESTED",
        /// Being provisioned.
        Provisioning = "PROVISIONING",
        /// Ready for a run.
        Ready = "READY",
        /// A run uses it.
        InUse = "IN_USE",
        /// A custody checkpoint is being taken under the write fence.
        Preserving = "PRESERVING",
        /// Its last verified custody checkpoint holds everything in it; the write fence holds.
        Preserved = "PRESERVED",
        /// Retired.
        Retired = "RETIRED",
        /// Quarantined for uncertain custody, a scope escape or unattributed work.
        Quarantined = "QUARANTINED",
        /// Held by a `WorkspaceConflict`.
        Conflict = "CONFLICT",
    }
    edges {
        [Requested] -> [Provisioning];
        [Provisioning] -> [Ready];
        [Ready] -> [InUse];
        [InUse] -> [Preserving];
        [Preserving] -> [Preserved];
        [Preserved] -> [Retired];
        [Preserved] -> [InUse] if NotRetireOnly;
        [Requested, Provisioning, Ready, InUse, Preserving, Preserved] -> [Quarantined, Conflict];
        [Quarantined] -> [Conflict, Preserving] if QuarantineCleared;
        [Conflict] -> [Quarantined, Preserving] if ConflictAdjudicated;
    }
}

lifecycle! {
    /// The state of a `CustodyPolicy`.
    pub enum CustodyPolicyState in "CustodyPolicy" {
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
    /// The state of a `CustodyCheckpoint`.
    pub enum CustodyCheckpointState in "CustodyCheckpoint" {
        /// Its workspace's contents are inventoried. The initial state.
        Inventoried = "INVENTORIED",
        /// Its artifacts are uploading.
        Uploading = "UPLOADING",
        /// Every artifact is verified by digest.
        Uploaded = "UPLOADED",
        /// An independent restore into a fresh location is verified.
        Verified = "VERIFIED",
        /// Failed.
        Failed = "FAILED",
    }
    edges {
        [Inventoried] -> [Uploading];
        [Uploading] -> [Uploaded];
        [Uploaded] -> [Verified];
        [Inventoried, Uploading, Uploaded] -> [Failed];
    }
}

lifecycle! {
    /// The state of an `ArtifactCommit`.
    pub enum ArtifactCommitState in "ArtifactCommit" {
        /// Pending its custody checkpoint. The initial state.
        Pending = "PENDING",
        /// Its custody checkpoint is verified and complete.
        Verified = "VERIFIED",
        /// Failed.
        Failed = "FAILED",
    }
    edges {
        [Pending] -> [Verified] if CustodyCompleted;
        [Pending] -> [Failed];
    }
}

lifecycle! {
    /// The state of an `Artifact`.
    pub enum ArtifactState in "Artifact" {
        /// Pending verification. The initial state.
        Pending = "PENDING",
        /// Verified by digest.
        Verified = "VERIFIED",
        /// Expired.
        Expired = "EXPIRED",
    }
    edges {
        [Pending] -> [Verified, Expired];
    }
}

lifecycle! {
    /// The state of a `WorkspaceConflict`.
    pub enum WorkspaceConflictState in "WorkspaceConflict" {
        /// Detected. The initial state.
        Detected = "DETECTED",
        /// Its workspace is being preserved.
        Preserving = "PRESERVING",
        /// Its workspace is quarantined.
        Quarantined = "QUARANTINED",
        /// Adjudicated.
        Adjudicated = "ADJUDICATED",
        /// Its workspace is preserved.
        Preserved = "PRESERVED",
    }
    edges {
        [Detected] -> [Preserving];
        [Preserving] -> [Quarantined];
        [Quarantined] -> [Adjudicated];
        [Preserving] -> [Preserved];
        [Preserved] -> [Adjudicated];
    }
}

lifecycle! {
    /// The state of a `RestoreRequest`.
    pub enum RestoreRequestState in "RestoreRequest" {
        /// Requested. The initial state.
        Requested = "REQUESTED",
        /// Restoring.
        Restoring = "RESTORING",
        /// Restored and read-only.
        ReadOnly = "READ_ONLY",
        /// The old installation is fenced.
        Fenced = "FENCED",
        /// Outstanding operations are reconciled.
        Reconciled = "RECONCILED",
        /// Identities are mapped.
        Mapped = "MAPPED",
        /// Dispatch is enabled.
        DispatchEnabled = "DISPATCH_ENABLED",
        /// Failed.
        Failed = "FAILED",
    }
    edges {
        [Requested] -> [Restoring];
        [Restoring] -> [ReadOnly];
        [ReadOnly] -> [Fenced];
        [Fenced] -> [Reconciled];
        [Reconciled] -> [Mapped];
        [Mapped] -> [DispatchEnabled];
        [Requested, Restoring, ReadOnly, Fenced, Reconciled, Mapped] -> [Failed];
    }
}
