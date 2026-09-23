//! The kernel surfaces the group's scenarios need that the kernel does not have yet, as the
//! ports the scenarios call, and the registry that resolves them.
//!
//! Each port is the smallest surface its scenario needs, written in the kernel's own types:
//! commands and receipts (#83), slot repair and audit publication (#84), projections (#85) and
//! the create command path (#86). Each one that yields store operations is a sans-I/O
//! [`Protocol`], so the scenario drives it against whichever [`Driver`](super::harness::Driver)
//! it runs on.
//!
//! The registry is the stand-in for the testkit reducer registry (#79): it resolves no port
//! today, so a scenario that needs one fails with [`Unregistered`], naming the task it awaits.

use autobot_kernel::digest::CreateIndex;
use autobot_kernel::status::AuditEnvelope;
use autobot_kernel::store::{ObjectKey, Protocol};
use autobot_kernel::types::{
    CommitObservation, CommitSequence, Digest, Namespace, Principal, RejectionProof, StateRevision,
    Uid,
};
use std::fmt;

/// One domain command on one target: the replay identity, the pins and the new domain fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DomainCommand {
    /// The idempotency key; the receipt's deterministic name is derived from it.
    pub(crate) idempotency_key: String,
    /// The authenticated writer.
    pub(crate) principal: Principal,
    /// The target aggregate.
    pub(crate) target: ObjectKey,
    /// The target UID the command pins.
    pub(crate) target_uid: Uid,
    /// The `state_revision` the command pins.
    pub(crate) expected_revision: StateRevision,
    /// The command's input: the domain fields its transition writes. The input digest is
    /// [`digest`](autobot_kernel::digest::digest) of it.
    pub(crate) input: String,
    /// The day the command was issued, on the scenario's clock.
    pub(crate) issued_day: u32,
}

/// What a command's receipt records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReceiptState {
    /// The receipt is `PREPARED`: no evidence of commitment.
    Prepared,
    /// The receipt's terminal result.
    Terminal(CommitObservation),
}

/// The command path of KERNEL §2 (#83).
pub(crate) trait Commands {
    /// The protocol that submits `command` on day `today`: it prepares the receipt, commits
    /// on the target and records the terminal result, or answers from the receipt its replay
    /// identity already has.
    fn submit(
        &self,
        command: &DomainCommand,
        today: u32,
    ) -> Box<dyn Protocol<Outcome = CommitObservation>>;

    /// The protocol that reads the receipt of `idempotency_key` in `namespace`.
    fn receipt(
        &self,
        namespace: &Namespace,
        idempotency_key: &str,
    ) -> Box<dyn Protocol<Outcome = ReceiptState>>;
}

/// How a slot repair ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RepairOutcome {
    /// The receipt and the event are written, read back and verified, and the slot is
    /// `CLEARED`.
    Cleared,
    /// The aggregate's current domain digest is not the slot's after-digest; the slot stays.
    DigestMismatch,
}

/// Slot repair and audit publication, the owning controller's duty (#84).
pub(crate) trait Repair {
    /// The protocol that repairs the pending slot of `target`: it rebuilds the receipt and
    /// the event from the slot, writes and reads them back, verifies the domain digest and
    /// clears the slot. It takes no clock: nothing it decides depends on elapsed time.
    fn repair(&self, target: &ObjectKey, uid: &Uid) -> Box<dyn Protocol<Outcome = RepairOutcome>>;

    /// The protocol that reads the published event of `aggregate` at `commit_sequence`.
    fn event(
        &self,
        namespace: &Namespace,
        aggregate: &Uid,
        commit_sequence: CommitSequence,
    ) -> Box<dyn Protocol<Outcome = Option<Event>>>;
}

/// An `AutoBotEvent`: its audit envelope and its digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Event {
    /// Every field of the event but its digest.
    pub(crate) envelope: AuditEnvelope,
    /// The event's digest.
    pub(crate) event_digest: Digest,
}

/// What a projection did with one delivered event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Delivery {
    /// Applied, with every buffered event it made next.
    Applied,
    /// Held in the late-event buffer behind a gap.
    Buffered,
    /// Already applied or buffered with the same digest; nothing changed.
    Duplicate,
    /// A same-identity event with another digest: rejected and quarantined.
    Rejected,
}

/// A projection's gap state (FORMAL §2 `ProjectionState.gap`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gap {
    /// No gap.
    None,
    /// A gap is visible.
    Open,
    /// The gap is declared permanent.
    Permanent,
}

/// A projection's integrity (FORMAL §2 `ProjectionState.integrity`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Integrity {
    /// No conflict seen.
    Ok,
    /// A same-identity event with another digest was delivered.
    DigestConflict,
}

/// What a projection holds for one aggregate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectionView {
    /// The commit sequences applied, in the order they were applied.
    pub(crate) applied: Vec<CommitSequence>,
    /// The number of events in the late-event buffer.
    pub(crate) buffered: usize,
    /// The gap state.
    pub(crate) gap: Gap,
    /// The integrity state.
    pub(crate) integrity: Integrity,
}

/// One projection's read model (#85).
pub(crate) trait Projection {
    /// Delivers `event`.
    fn deliver(&mut self, event: &Event) -> Delivery;

    /// Declares the open gap of `aggregate` permanent.
    fn declare_permanent_gap(&mut self, aggregate: &Uid);

    /// What the projection holds for `aggregate`.
    fn view(&self, aggregate: &Uid) -> ProjectionView;
}

/// The maker of projections (#85).
pub(crate) trait Projections {
    /// An empty projection whose late-event buffer holds `late_event_buffer` events.
    fn projection(&self, late_event_buffer: u32) -> Box<dyn Projection>;
}

/// One create command: its index, the namespace it creates in and its spec, whose digest is
/// the input digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CreateCommand {
    /// The create index.
    pub(crate) index: CreateIndex,
    /// The namespace the target is created in.
    pub(crate) namespace: Namespace,
    /// The encoded spec.
    pub(crate) spec: String,
}

/// How a create command ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CreateResult {
    /// The target exists with this create's origin.
    Created {
        /// The target's UID.
        uid: Uid,
    },
    /// The create was rejected with this proof.
    Rejected(RejectionProof),
}

/// How a delete command ended. The scenarios only reach a refused delete, since the store has
/// no delete operation; the implementation behind the port constructs the other variant.
#[allow(
    dead_code,
    reason = "constructed by the implementation behind the port"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DeleteResult {
    /// The target was deleted and its tombstone written.
    Deleted,
    /// The delete was refused; the target is untouched.
    Refused,
}

/// The create command path of KERNEL §2 (#86).
pub(crate) trait Creates {
    /// The protocol that runs `command`: it reserves the receipt name, creates by name with
    /// origin metadata and records the terminal result.
    fn create(&self, command: &CreateCommand) -> Box<dyn Protocol<Outcome = CreateResult>>;

    /// The protocol that deletes `target`, whose UID is `uid`, on behalf of `principal`.
    fn delete(
        &self,
        target: &ObjectKey,
        uid: &Uid,
        principal: &Principal,
    ) -> Box<dyn Protocol<Outcome = DeleteResult>>;
}

/// A port no implementation is registered for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Unregistered {
    /// The port.
    pub(crate) port: &'static str,
    /// The task whose implementation fills it.
    pub(crate) awaiting: u32,
}

impl fmt::Display for Unregistered {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "no {} implementation is registered (awaiting #{})",
            self.port, self.awaiting
        )
    }
}

/// Resolves a port, failing the scenario with the task it awaits.
fn resolve<T: ?Sized>(port: &'static str, awaiting: u32) -> Box<T> {
    panic!("{}", Unregistered { port, awaiting })
}

/// The command path.
pub(crate) fn commands() -> Box<dyn Commands> {
    resolve("Commands", 83)
}

/// Slot repair and audit publication.
pub(crate) fn repair() -> Box<dyn Repair> {
    resolve("Repair", 84)
}

/// The projection maker.
pub(crate) fn projections() -> Box<dyn Projections> {
    resolve("Projections", 85)
}

/// The create command path.
pub(crate) fn creates() -> Box<dyn Creates> {
    resolve("Creates", 86)
}
