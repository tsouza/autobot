//! The ports of the G-COMMIT fixture group: the kernel surfaces its scenarios resolve, each
//! the smallest surface a scenario needs, written in the kernel's own types.
//!
//! | Port | Surface | Awaiting |
//! |---|---|---|
//! | [`CommandsPort`] | the command path and its receipts | #83 |
//! | [`RepairPort`] | slot repair and audit publication | #84 |
//! | [`ProjectionsPort`] | projections of the audit stream | #85 |
//! | [`CreatesPort`] | the create command path | #86 |
//!
//! Each surface that yields store operations is a sans-I/O [`Protocol`], so a scenario drives
//! it against whichever [`Driver`](crate::harness::Driver) it runs on. A surface that a FORMAL
//! §5 guard of the group constrains takes the [`Guards`] it runs under, so a guard-removal run
//! reaches it: [`Repair::repair`] (`SlotClearedOnVerification`) and
//! [`Projections::projection`] (`ProjectionInOrder`).

use super::Port;
use autobot_kernel::digest::CreateIndex;
use autobot_kernel::reducer::Guards;
use autobot_kernel::status::AuditEnvelope;
use autobot_kernel::store::{ObjectKey, Protocol};
use autobot_kernel::types::{
    CommitObservation, CommitSequence, Digest, Namespace, Principal, RejectionProof, StateRevision,
    Uid,
};

/// One domain command on one target: the replay identity, the pins and the new domain fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainCommand {
    /// The idempotency key; the receipt's deterministic name is derived from it.
    pub idempotency_key: String,
    /// The authenticated writer.
    pub principal: Principal,
    /// The target aggregate.
    pub target: ObjectKey,
    /// The target UID the command pins.
    pub target_uid: Uid,
    /// The `state_revision` the command pins.
    pub expected_revision: StateRevision,
    /// The command's input: the domain fields its transition writes. The input digest is
    /// [`digest`](autobot_kernel::digest::digest) of it.
    pub input: String,
    /// The day the command was issued, on the scenario's clock.
    pub issued_day: u32,
}

/// What a command's receipt records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiptState {
    /// The receipt is `PREPARED`: no evidence of commitment.
    Prepared,
    /// The receipt's terminal result.
    Terminal(CommitObservation),
}

/// The command path of KERNEL §2.
pub trait Commands {
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

/// The port of [`Commands`].
#[derive(Debug)]
pub struct CommandsPort;

impl Port for CommandsPort {
    type Object = dyn Commands;
    const NAME: &'static str = "Commands";
    const AWAITING: u32 = 83;
}

/// How a slot repair ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepairOutcome {
    /// The receipt and the event are written, read back and verified, and the slot is
    /// `CLEARED`.
    Cleared,
    /// The aggregate's current domain digest is not the slot's after-digest; the slot stays.
    DigestMismatch,
}

/// An `AutoBotEvent`: its audit envelope and its digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// Every field of the event but its digest.
    pub envelope: AuditEnvelope,
    /// The event's digest.
    pub event_digest: Digest,
}

/// Slot repair and audit publication, the owning controller's duty.
pub trait Repair {
    /// The protocol that repairs the pending slot of `target` under `guards`: it rebuilds the
    /// receipt and the event from the slot, writes and reads them back, verifies the domain
    /// digest and clears the slot. It takes no clock: with `SlotClearedOnVerification`
    /// enabled, nothing it decides depends on elapsed time.
    fn repair(
        &self,
        target: &ObjectKey,
        uid: &Uid,
        guards: &Guards,
    ) -> Box<dyn Protocol<Outcome = RepairOutcome>>;

    /// The protocol that reads the published event of `aggregate` at `commit_sequence`.
    fn event(
        &self,
        namespace: &Namespace,
        aggregate: &Uid,
        commit_sequence: CommitSequence,
    ) -> Box<dyn Protocol<Outcome = Option<Event>>>;
}

/// The port of [`Repair`].
#[derive(Debug)]
pub struct RepairPort;

impl Port for RepairPort {
    type Object = dyn Repair;
    const NAME: &'static str = "Repair";
    const AWAITING: u32 = 84;
}

/// What a projection did with one delivered event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
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
pub enum Gap {
    /// No gap.
    None,
    /// A gap is visible.
    Open,
    /// The gap is declared permanent.
    Permanent,
}

/// A projection's integrity (FORMAL §2 `ProjectionState.integrity`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Integrity {
    /// No conflict seen.
    Ok,
    /// A same-identity event with another digest was delivered.
    DigestConflict,
}

/// What a projection holds for one aggregate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionView {
    /// The commit sequences applied, in the order they were applied.
    pub applied: Vec<CommitSequence>,
    /// The number of events in the late-event buffer.
    pub buffered: usize,
    /// The gap state.
    pub gap: Gap,
    /// The integrity state.
    pub integrity: Integrity,
}

/// One projection's read model.
pub trait Projection {
    /// Delivers `event`.
    fn deliver(&mut self, event: &Event) -> Delivery;

    /// Declares the open gap of `aggregate` permanent.
    fn declare_permanent_gap(&mut self, aggregate: &Uid);

    /// What the projection holds for `aggregate`.
    fn view(&self, aggregate: &Uid) -> ProjectionView;
}

/// The maker of projections.
pub trait Projections {
    /// An empty projection whose late-event buffer holds `late_event_buffer` events and which
    /// runs under `guards`: with `ProjectionInOrder` enabled it applies events in commit order.
    fn projection(&self, late_event_buffer: u32, guards: &Guards) -> Box<dyn Projection>;
}

/// The port of [`Projections`].
#[derive(Debug)]
pub struct ProjectionsPort;

impl Port for ProjectionsPort {
    type Object = dyn Projections;
    const NAME: &'static str = "Projections";
    const AWAITING: u32 = 85;
}

/// One create command: its index, the namespace it creates in and its spec, whose digest is
/// the input digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateCommand {
    /// The create index.
    pub index: CreateIndex,
    /// The namespace the target is created in.
    pub namespace: Namespace,
    /// The encoded spec.
    pub spec: String,
}

/// How a create command ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateResult {
    /// The target exists with this create's origin.
    Created {
        /// The target's UID.
        uid: Uid,
    },
    /// The create was rejected with this proof.
    Rejected(RejectionProof),
}

/// How a delete command ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteResult {
    /// The target was deleted and its tombstone written.
    Deleted,
    /// The delete was refused; the target is untouched.
    Refused,
}

/// The create command path of KERNEL §2.
pub trait Creates {
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

/// The port of [`Creates`].
#[derive(Debug)]
pub struct CreatesPort;

impl Port for CreatesPort {
    type Object = dyn Creates;
    const NAME: &'static str = "Creates";
    const AWAITING: u32 = 86;
}
