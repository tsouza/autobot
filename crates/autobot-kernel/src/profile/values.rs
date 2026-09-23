//! The typed values of a profile, one struct per profile area.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::num::NonZeroU32;

/// Every value of a profile, as its TOML document holds them.
///
/// Field names are the TOML keys; a key's suffix names its unit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct ProfileValues {
    /// Schema version of the document; [`super::SCHEMA_VERSION`] is the only one accepted.
    pub version: u32,
    /// Replay window of command receipts.
    pub replay: Replay,
    /// Control-receipt ring of an aggregate with a control lane.
    pub control_ring: ControlRing,
    /// Dispatch ledger of a context.
    pub dispatch_ledger: DispatchLedger,
    /// Registers of a context.
    pub registers: Registers,
    /// Deployment scale M0 is exercised at.
    pub scale: Scale,
    /// Object size and count limits.
    pub objects: Objects,
    /// API request budget and work queue of one operator process.
    pub api: ApiBudget,
    /// Checkpoint cadence of active work.
    pub checkpoint: Checkpoint,
    /// Recovery point and recovery time objectives.
    pub recovery: Recovery,
    /// Artifact store limits.
    pub artifacts: Artifacts,
    /// Evidence lifetimes.
    pub evidence: Evidence,
    /// Sandbox constraints of a worker runtime.
    pub sandbox: Sandbox,
}

/// Replay window of command receipts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Replay {
    /// Days a receipt answers a replayed command.
    pub window_days: NonZeroU32,
    /// Days added to the window for clock and transport skew.
    pub margin_days: u32,
}

/// Control-receipt ring of an aggregate with a control lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct ControlRing {
    /// Unpublished entries the ring holds.
    pub entries: NonZeroU32,
    /// Largest entry, in KiB.
    pub entry_max_kib: NonZeroU32,
}

/// Dispatch ledger of a context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct DispatchLedger {
    /// Entries the ledger holds.
    pub entries: NonZeroU32,
}

/// Registers of a context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Registers {
    /// Plans the plan register holds.
    pub plans: NonZeroU32,
    /// Integration bases per context.
    pub integration_bases: NonZeroU32,
}

/// Deployment scale M0 is exercised at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Scale {
    /// Work contexts.
    pub contexts: NonZeroU32,
    /// Repositories.
    pub repositories: NonZeroU32,
    /// Tasks.
    pub tasks: NonZeroU32,
    /// Task runs active at once.
    pub active_task_runs: NonZeroU32,
    /// Simulated Managers that race each other.
    pub simulated_managers: NonZeroU32,
    /// Simulated installation identities that race each other.
    pub simulated_installation_identities: NonZeroU32,
}

/// Object size and count limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Objects {
    /// Largest status of an object, in KiB.
    pub status_max_kib: NonZeroU32,
    /// Largest pending slot, in KiB; at most `status_max_kib`.
    pub pending_slot_max_kib: NonZeroU32,
    /// Most effect intents one command may carry.
    pub effect_intents_per_command: NonZeroU32,
    /// Late events buffered per projected aggregate.
    pub late_event_buffer: NonZeroU32,
}

/// API request budget and work queue of one operator process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct ApiBudget {
    /// Sustained API requests per second.
    pub requests_per_second: NonZeroU32,
    /// Burst of API requests above the sustained rate.
    pub burst: NonZeroU32,
    /// Keys the work queue holds.
    pub queue_keys: NonZeroU32,
    /// Work classes with capacity reserved in the queue.
    pub reserved_control: BTreeSet<ControlWork>,
}

/// A class of control work that has reserved queue capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ControlWork {
    /// Holds.
    Hold,
    /// Fence confirmation.
    Fence,
    /// Receipt repair.
    ReceiptRepair,
}

/// Checkpoint cadence of active work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Checkpoint {
    /// Most seconds of active work between checkpoints; also the uncheckpointed part of the
    /// recovery point objective.
    pub max_active_work_secs: NonZeroU32,
    /// Longest indivisible write, in seconds.
    pub indivisible_write_max_secs: NonZeroU32,
}

/// Recovery point and recovery time objectives.
///
/// The recovery point is [`Checkpoint::max_active_work_secs`] plus `in_flight_operations`
/// bounded operations; it is not repeated here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Recovery {
    /// Bounded in-flight operations the recovery point may lose beyond the last checkpoint.
    pub in_flight_operations: NonZeroU32,
    /// Size of the restore fixture, in MiB.
    pub restore_fixture_mib: NonZeroU32,
    /// Minutes within which the restore fixture is restored with healthy dependencies.
    pub restore_within_minutes: NonZeroU32,
}

/// Artifact store limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Artifacts {
    /// Largest artifact footprint of one workspace, in GiB.
    pub workspace_max_gib: NonZeroU32,
}

/// Evidence lifetimes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Evidence {
    /// Evidence lifetime in hours, which repository policy may shorten and never lengthen.
    pub ttl_hours: NonZeroU32,
    /// Days until a no-test exception expires.
    pub no_test_expiry_days: NonZeroU32,
    /// Days until a defect outcome matures, by consequence class.
    pub defect_maturity_days: DefectMaturity,
}

/// Days until a defect outcome matures, by consequence class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct DefectMaturity {
    /// `REVERSIBLE`.
    pub reversible: NonZeroU32,
    /// `COMPATIBILITY_RISK`.
    pub compatibility_risk: NonZeroU32,
    /// `SECURITY_OR_DATA_INTEGRITY`.
    pub security_or_data_integrity: NonZeroU32,
}

/// Sandbox constraints of a worker runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Sandbox {
    /// Operating system of the runtime.
    pub os: SandboxOs,
    /// How the runtime image is chosen.
    pub image: SandboxImage,
    /// Network default.
    pub network: SandboxNetwork,
    /// Egress route.
    pub egress: SandboxEgress,
    /// Writable mounts.
    pub writable_mounts: u32,
    /// Whether host mounts are allowed.
    pub host_mounts: bool,
    /// Whether privileged containers are allowed.
    pub privileged: bool,
    /// Whether devices are allowed.
    pub devices: bool,
    /// Access to a live model API.
    pub model_api: SandboxModelApi,
}

/// Operating system of a sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum SandboxOs {
    /// Linux.
    Linux,
}

/// How a sandbox image is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum SandboxImage {
    /// An OCI image pinned by digest.
    PinnedOci,
}

/// Network default of a sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum SandboxNetwork {
    /// Every connection is denied unless a rule allows it.
    DefaultDeny,
}

/// Egress route of a sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum SandboxEgress {
    /// Egress only through the broker.
    BrokerOnly,
}

/// Access of a sandbox to a live model API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum SandboxModelApi {
    /// No live model API.
    Disabled,
    /// Optional, only through a metered proxy that cannot reach production endpoints.
    MeteredProxy,
}
