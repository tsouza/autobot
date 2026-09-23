//! The store conformance suite: data-driven scripts every store driver runs.
//!
//! A [`Script`] is a TOML document of steps; [`scripts`] parses the suite, which is embedded
//! in this crate. A [`ScriptRun`] executes one script as a sans-I/O machine: the driver loops
//! on [`ScriptRun::step`], performs each [`Action::Op`] and passes its result to
//! [`ScriptRun::resume`], arms each [`Action::Arm`] fault, calls [`ScriptRun::crash`] when an
//! armed crash fires, and stops at [`Action::Done`]. Every driver therefore runs the same
//! scripts.
//!
//! Every object a script names is of kind [`KIND`] in namespace [`NAMESPACE`]. Steps, by the
//! value of their `do` key:
//!
//! - `create`: create `object` by name with `spec` under the create receipt `receipt`
//!   (default `create-<object>`), expecting `created`, `taken` or `vanished`.
//! - `initialize`: write the first status, with `domain` and `control` fields and a control
//!   lane when `control_lane`, expecting `initialized` or `already_initialized`.
//! - `commit`: commit `command` on `lane` (`DOMAIN` or `CONTROL`), pinned at revision `pin` or,
//!   without `pin`, at the revision read, setting the lane's fields to `fields`. With
//!   `require_control`, the transition's `control` guard refuses unless the control fields
//!   read equal it. With `interleave`, that commit step runs to its end after this commit's
//!   first read and before its write. It expects one of `committed`, `passed`,
//!   `not_reached`, `barrier`, `refused`, `ring_full`, `ring_error`, `no_control_lane`,
//!   `lane_mismatch`, `overflow` or `uninitialized`.
//! - `clear`: clear the pending slot of `command`, expecting `cleared`, `not_held` or
//!   `uninitialized`.
//! - `read`: read `object` and keep what was read under `label`.
//! - `write`: write back the status read under `from`, conditioned on the resource version
//!   read then, expecting `updated` or `conflict`.
//! - `check`: read `object` and compare each field given: `state_revision`,
//!   `control_revision`, `commit_sequence`, `domain`, `control`, `slot` (`none`, `occupied`
//!   or `cleared`), `slot_command` and `ring_entries`.
//! - `inject`: arm `fault` in the driver.
//! - `relist` and `poll`: list, or take one watch batch (relisting if the watch expired and
//!   repeating an unavailable read), and compare the names the
//!   [`Triggers`](super::Triggers) then hold due with `expect`.
//!
//! `initialize`, `commit` and `clear` also end `not_found` or `replaced` when their object is
//! missing or was created again.
//!
//! A crash restarts the current step with a fresh protocol for the same request, as a new
//! process would; an `interleave` that already ran is not run again.

mod run;

pub use run::{Action, Failure, ScriptRun};

use crate::types::Lane;
use serde::Deserialize;
use std::fmt;

/// The kind of every object a script names.
pub const KIND: &str = "ConformanceProbe";

/// The namespace of every object a script names.
pub const NAMESPACE: &str = "conformance";

/// The embedded suite, one TOML document per script.
const SUITE: [&str; 12] = [
    include_str!("stale_resource_version.toml"),
    include_str!("uncertain_write.toml"),
    include_str!("pinned_revision_passed.toml"),
    include_str!("control_between_read_and_write.toml"),
    include_str!("receipt_barrier.toml"),
    include_str!("control_lane.toml"),
    include_str!("crash_between_writes.toml"),
    include_str!("lost_create_ack.toml"),
    include_str!("watch_drop.toml"),
    include_str!("watch_duplicate.toml"),
    include_str!("watch_reorder.toml"),
    include_str!("watch_expire.toml"),
];

/// Parses the embedded suite.
///
/// # Errors
///
/// [`ScriptError`] naming the first document that does not parse.
pub fn scripts() -> Result<Vec<Script>, ScriptError> {
    SUITE
        .iter()
        .enumerate()
        .map(|(index, text)| {
            toml::from_str(text).map_err(|e| ScriptError {
                index,
                message: e.to_string(),
            })
        })
        .collect()
}

/// A suite document that does not parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptError {
    /// The document's position in the suite.
    pub index: usize,
    /// The parser's message.
    pub message: String,
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "conformance script {}: {}", self.index, self.message)
    }
}

impl std::error::Error for ScriptError {}

/// One conformance script.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Script {
    /// The script's name.
    pub name: String,
    /// What the script shows.
    pub summary: String,
    /// The steps, in order.
    pub steps: Vec<ScriptStep>,
}

/// One step of a script; the module documentation describes each.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum ScriptStep {
    /// Create an object by name.
    Create(CreateStep),
    /// Write an object's first status.
    Initialize(InitializeStep),
    /// Commit a command on one lane.
    Commit(CommitStep),
    /// Clear a command's pending slot.
    Clear(ClearStep),
    /// Read an object and keep it under a label.
    Read(ReadStep),
    /// Write back a labelled read, conditioned on its resource version.
    Write(WriteStep),
    /// Read an object and compare fields.
    Check(CheckStep),
    /// Arm a fault in the driver.
    Inject(InjectStep),
    /// Relist and compare the due names.
    Relist(WatchStep),
    /// Take one watch batch and compare the due names.
    Poll(WatchStep),
}

/// A `create` step.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateStep {
    /// The object's name.
    pub object: String,
    /// The create receipt's UID; `create-<object>` when absent.
    pub receipt: Option<String>,
    /// The encoded spec.
    #[serde(default)]
    pub spec: String,
    /// The expected outcome.
    pub expect: String,
}

/// An `initialize` step.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitializeStep {
    /// The object's name.
    pub object: String,
    /// Whether the aggregate has a control lane.
    #[serde(default)]
    pub control_lane: bool,
    /// The initial domain fields.
    #[serde(default)]
    pub domain: String,
    /// The initial control fields.
    #[serde(default)]
    pub control: String,
    /// The expected outcome.
    pub expect: String,
}

/// A `commit` step.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommitStep {
    /// The aggregate's name.
    pub object: String,
    /// The command's name, also its UID.
    pub command: String,
    /// The lane.
    pub lane: Lane,
    /// The pinned revision; the revision read when absent.
    pub pin: Option<u64>,
    /// The lane's new fields.
    #[serde(default)]
    pub fields: String,
    /// The control fields the `control` guard requires.
    pub require_control: Option<String>,
    /// A commit step run after this commit's first read and before its write.
    pub interleave: Option<Box<CommitStep>>,
    /// The expected outcome.
    pub expect: String,
}

/// A `clear` step.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClearStep {
    /// The aggregate's name.
    pub object: String,
    /// The command whose slot to clear.
    pub command: String,
    /// The expected outcome.
    pub expect: String,
}

/// A `read` step.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadStep {
    /// The object's name.
    pub object: String,
    /// The label to keep the read under.
    pub label: String,
}

/// A `write` step.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriteStep {
    /// The label of the read to write back.
    pub from: String,
    /// The expected result.
    pub expect: String,
}

/// A `check` step.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckStep {
    /// The object's name.
    pub object: String,
    /// The expected `state_revision`.
    pub state_revision: Option<u64>,
    /// The expected `control_revision`.
    pub control_revision: Option<u64>,
    /// The expected `commit_sequence`.
    pub commit_sequence: Option<u64>,
    /// The expected domain fields.
    pub domain: Option<String>,
    /// The expected control fields.
    pub control: Option<String>,
    /// The expected slot state: `none`, `occupied` or `cleared`.
    pub slot: Option<String>,
    /// The command the slot is expected to name.
    pub slot_command: Option<String>,
    /// The expected number of ring entries.
    pub ring_entries: Option<usize>,
}

/// An `inject` step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InjectStep {
    /// The fault to arm.
    pub fault: Fault,
}

/// A `relist` or `poll` step.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WatchStep {
    /// The names expected due, in any order.
    pub expect: Vec<String>,
}

/// A fault a driver injects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Fault {
    /// The process crashes once `after_writes` more writes have applied, before it sees the
    /// last one's result.
    Crash {
        /// The writes that apply before the crash.
        after_writes: u32,
    },
    /// The next write reports `UNCERTAIN`; it applies first when `applied` is true and it
    /// would have succeeded.
    WriteTimeout {
        /// Whether the write applies.
        applied: bool,
    },
    /// The next create applies, when its name is free, and its acknowledgement is lost: it
    /// reports `UNCERTAIN`.
    LostCreateAck,
    /// The next watch batch loses its first `count` events.
    DropEvents {
        /// The events lost.
        count: u32,
    },
    /// The next watch batch delivers every event twice.
    DuplicateEvents,
    /// The next watch batch delivers its events in reverse order.
    ReorderEvents,
    /// The next watch reports that it expired.
    ExpireWatch,
}
