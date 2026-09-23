//! The kernel's value types: identities, revisions, lanes, digests and commit observations,
//! as `docs/design/AUTOBOT-KERNEL.md` §1–§2 use them.
//!
//! Every type here is a checked value: construction and deserialization refuse what the type
//! cannot hold, so a value that exists is valid. The types that custom-resource status carries
//! derive serde and [`schemars::JsonSchema`]; the status records built from them are in
//! [`crate::status`].
//!
//! - [`Uid`], [`Namespace`], [`ObjectName`] and [`ObjectRef`] identify objects. A reference
//!   carries namespace, name and UID (`docs/design/AUTOBOT-M0-AND-GATES.md` §1). [`Principal`]
//!   is the authenticated writer of a command.
//! - [`StateRevision`], [`ControlRevision`] and [`CommitSequence`] are the per-aggregate
//!   counters of KERNEL §1, each its own type so one cannot stand in for another.
//!   [`LaneRevision`] is a revision tagged with its [`Lane`], what a command pins.
//! - [`Digest`] is a SHA-256 value that records carry.
//! - [`CommitObservation`] is what a reader learns about one command: `COMMITTED`,
//!   `REJECTED` with a [`RejectionProof`], `CANCELLED`, `REPLAY_EXPIRED`, or `UNCERTAIN`, an
//!   observation to be reconciled (KERNEL §2).
//!
//! Choices this module makes where the design is open:
//!
//! - A counter starts at zero, which means no commit of its lane has been made, and stays in
//!   `0..=i64::MAX`, the range of the `int64` integers Kubernetes stores; its JSON schema says
//!   so. Incrementing past `i64::MAX` is an error, never a wrap.
//! - A namespace is a DNS-1123 label and an object name a DNS-1123 subdomain, the Kubernetes
//!   rules for both. A UID and a principal are opaque, non-empty text: the API server assigns
//!   the one and the authenticator the other, and the kernel only compares them.
//! - An [`ObjectRef`] names no kind: the field holding it does.
//! - [`Digest`] owns the `sha256:` text form, its parser and its JSON schema, which
//!   [`ProfileDigest`](crate::profile::ProfileDigest) wraps. It carries digests and computes
//!   none.
//! - A [`RejectionProof`] has one shape per rejection ground of KERNEL §2, tagged by `ground`.
//!   Its passed-revision shape, [`PassedRevision`], also keeps the expected revision, which
//!   FORMAL §2 holds on the receipt, so that the proof checks on its own that the observed
//!   revision is past it on the same lane. A guard identifier is text. It is serializable so
//!   that a receipt can record it.
//! - [`CommitObservation`] has no `PREPARED` variant and does not derive serde: it is the
//!   outcome of reading, not a stored record.
//!
//! The `CANCELLED` and `REPLAY_EXPIRED` observations follow the KERNEL §10 `CommandReceipt`
//! machine and the meaning its notes give each state.

mod digest;
mod identity;
mod observation;
mod revision;

pub use digest::Digest;
pub use identity::{Namespace, ObjectName, ObjectRef, Principal, Uid};
pub use observation::{CommitObservation, PassedRevision, RejectionProof};
pub use revision::{CommitSequence, ControlRevision, Lane, LaneRevision, StateRevision};

#[cfg(test)]
mod tests;
