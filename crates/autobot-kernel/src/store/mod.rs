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
//!   conditioned on the object's UID and resource version, `List` for relisting, and `Watch`,
//!   whose [`WatchEvent`]s name an object and carry none of its state.
//! - [`Commit`] runs one domain or control commit of one command (KERNEL §1); [`Initialize`]
//!   writes an aggregate's first status; [`ClearSlot`] is the reconciliation-only CAS that marks
//!   a pending slot `CLEARED`; [`Create`] creates by name (KERNEL §2); [`Triggers`] turns watch
//!   events and relists into the set of objects to read.
//! - A write that returns `UNCERTAIN` is resolved only by reading: every protocol follows it
//!   with a `Get` of the same object and decides from what it reads. A domain commit that
//!   finds its command in the pending slot, or a control commit that finds it in the ring, has
//!   committed; a create that finds its own origin under the name has created. A commit that
//!   finds neither its command nor a moved lane revision has not applied, and decides again
//!   from the new read: if the uncertain write still lands later, it lands only on the resource
//!   version it was conditioned on, where the new write is the same write. A commit that finds
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
//! - The domain and control digests are the SHA-256 of the encoded fields ([`fields_digest`]).
//!   The store moves a kind's fields as opaque text, split by the field partition into
//!   [`Status::domain`] and [`Status::control`]; a domain commit writes only the first, a
//!   control commit only the second, so no commit can touch both lanes.
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

mod cas;
mod commit;
pub mod conformance;
mod create;
mod object;
mod op;
mod watch;

pub use cas::Missing;
pub use commit::{
    Change, ClearOutcome, ClearSlot, Commit, CommitOutcome, CommitRequest, ControlChange,
    DomainChange, GuardRefusal, Initialize, InitializeOutcome, Pin, Transition,
};
pub use create::{Create, CreateOutcome};
pub use object::{Kind, Object, ObjectKey, Origin, ResourceVersion, Status, fields_digest};
pub use op::{OpKind, Protocol, ProtocolError, Step, StoreOp, StoreResult, WatchEvent};
pub use watch::Triggers;

#[cfg(test)]
mod tests;
