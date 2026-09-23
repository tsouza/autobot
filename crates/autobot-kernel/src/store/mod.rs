//! The store contract: the commit protocol of `docs/design/AUTOBOT-KERNEL.md` §1 as sans-I/O
//! state machines over the one thing `docs/design/AUTOBOT-TRUST-MODEL.md` trusts the
//! Kubernetes API server for, single-resource atomic writes with optimistic concurrency.
//!
//! A protocol yields a [`StoreOp`] from [`Protocol::step`] and consumes its [`StoreResult`] in
//! [`Protocol::resume`]; the kernel performs no I/O and declares no store trait, so a
//! synchronous driver, such as the in-memory one in `autobot-fakes`, and an asynchronous one run
//! the same machines, and the [`conformance`] suite, itself such a machine, checks every driver.
//!
//! - [`StoreOp`] is the whole vocabulary: a linearizable `Get`, create-by-name, a status update
//!   and a delete, each conditioned on the object's UID and resource version, `List` for
//!   relisting, and `Watch`, whose [`WatchEvent`]s name an object and carry none of its state.
//! - [`Commit`] runs one domain or control commit of one command (KERNEL §1); [`Initialize`]
//!   writes an aggregate's first status; [`ClearSlot`] is the reconciliation-only CAS that marks
//!   a pending slot `CLEARED`; [`Create`] creates by name and [`Delete`] deletes behind a
//!   tombstone (KERNEL §2); [`Triggers`] turns watch events and relists into the set of objects
//!   to read.
//! - A write that returns `UNCERTAIN` is resolved only by reading: every protocol follows it
//!   with a `Get` of the same object and decides from what it reads. A domain commit that
//!   finds its command in the pending slot, or a control commit that finds it in the ring, has
//!   committed; a create that finds its own origin under the name has created; a delete that
//!   finds its target absent once its tombstone holds has deleted. A commit that finds neither
//!   its command nor a moved lane revision has not applied, and decides again from the new
//!   read: if the uncertain write still lands later, it lands only on the resource version it
//!   was conditioned on, where the new write is the same write. A commit that finds
//!   the lane revision moved without its command ends [`CommitOutcome::Passed`], which only
//!   the command's receipt can resolve.
//!
//! Choices this module makes where the design is open:
//!
//! - A domain or control commit is a status update conditioned on `metadata.resourceVersion`
//!   plus the logical `state_revision` or `control_revision` check the protocol makes on the
//!   object it read ([`Pin`]). The resource-version condition fails the write when anything
//!   in the status changed since the read, including the other lane's fields; a check of the
//!   lane's own revision alone would let a hold, a control commit, land between the read and an
//!   `AcceptDispatch` domain write that read `RUNNING`. After a conflict the protocol reads
//!   again and evaluates its [`Transition`] against the new state, whose guards decide again.
//! - Authority reads are linearizable: every `Get` and `List` is an uncached quorum read, and
//!   no protocol reads an informer cache. A watch is a trigger and a relist is the truth
//!   (TRUST): [`Triggers`] only marks objects due, and their state comes from a `Get`.
//! - `AcceptDispatch` pins register values through the permit rather than
//!   `WorkContext.state_revision`: it commits with [`Pin::Current`], at the revision it reads,
//!   and its transition checks the permit's pinned register values against the registers
//!   read. Because the transition runs again after every conflict, an acceptance never
//!   commits against registers other than those it checked. The permit's own state and a
//!   second acceptance of one permit are open in #298; nothing here reads the `AdmissionStamp`.
//! - The store moves a kind's fields as opaque text, split by the field partition into
//!   [`Status::domain`] and [`Status::control`]; a domain commit writes only the first, a
//!   control commit only the second, so no commit can touch both lanes. [`Status`] declares
//!   that partition, and a slot's or control receipt's before and after digests are
//!   [`domain_digest`](crate::digest::domain_digest) and
//!   [`control_digest`](crate::digest::control_digest) of the status: the domain digest covers
//!   `state_revision` and the domain text, the control digest `control_revision` and the
//!   control text. A commit whose status has no canonical encoding ends
//!   [`CommitOutcome::Unencodable`].
//! - A domain commit sets `last_receipt_ref` to its command's receipt; a control commit leaves
//!   it, since its receipt is the ring entry it appends.
//! - A lane commit refused by a guard of its transition ends [`CommitOutcome::Refused`] with
//!   the guard's identifier and the revision and commit sequence it read, the proof the owner
//!   ruled for a guard refusal in #328, which the design text does not yet state.
//! - [`CommitOutcome`] is not a [`CommitObservation`](crate::types::CommitObservation): the
//!   protocol sees only the target, and a rejection needs the command's receipt as well
//!   (KERNEL §2), so the protocol reports what the target proves and leaves rejection to the
//!   receipt protocol.
//! - Every aggregate's envelope carries `control_revision`, zero without a control lane, as
//!   [`crate::status`] records for #327.
//! - A delete reads its target's create receipt and refuses unless [`ReceiptCheck`] judges it
//!   terminal; an absent create receipt refuses too. How a `CommandReceipt` encodes its state
//!   belongs to the command path (#86), so the caller judges it. How long a create receipt is
//!   kept beside a live target is outside KERNEL §2, which states only the replay-window and
//!   pending-effect retention.
//! - The tombstone is written before the target is deleted, by name as [`Create`] writes, and
//!   its origin is the target's: the create receipt UID, input digest and context the target
//!   was created with, which KERNEL §2 retains with the tombstone. At every point the target's
//!   name is held by the target, or the tombstone proves it was deleted, so a crash between the
//!   writes never leaves a name absent without its tombstone. A tombstone whose target still
//!   exists is a delete in progress, which a fresh protocol for the same request completes.
//! - A delete is conditioned on the target's UID and the resource version it read, like a
//!   status write; after a conflict it reads again and deletes the same incarnation at the
//!   resource version read. The create receipt is not read again: a terminal receipt cannot
//!   be rewritten.
//! - The store contract writes the tombstone's spec as the caller encodes it; what a replayed
//!   create does on finding a tombstone is the command path's (#86): [`Create`] itself still
//!   creates on a free name.

mod cas;
mod commit;
pub mod conformance;
mod create;
mod delete;
mod object;
mod op;
mod watch;

pub use cas::Missing;
pub use commit::{
    Change, ClearOutcome, ClearSlot, Commit, CommitOutcome, CommitRequest, ControlChange,
    DomainChange, EventFields, GuardRefusal, Initialize, InitializeOutcome, Pin, Transition,
};
pub use create::{Create, CreateOutcome};
pub use delete::{Delete, DeleteOutcome, DeleteRequest, ReceiptCheck};
pub use object::{Kind, Object, ObjectKey, Origin, ResourceVersion, Status};
pub use op::{OpKind, Protocol, ProtocolError, Step, StoreOp, StoreResult, WatchEvent};
pub use watch::Triggers;

#[cfg(test)]
mod tests;
