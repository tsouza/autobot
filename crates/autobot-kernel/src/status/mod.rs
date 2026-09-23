//! The status records every aggregate carries: the common status envelope of
//! `docs/design/AUTOBOT-M0-AND-GATES.md` §1, the pending commit slot and the control-receipt
//! ring of `docs/design/AUTOBOT-KERNEL.md` §1, with the `PendingCommit` and `ControlReceipt`
//! records of `docs/design/AUTOBOT-FORMAL-SURFACE.md` §2 field for field.
//!
//! Every record derives serde and [`schemars::JsonSchema`] so that the custom-resource types
//! embed them. [`StatusEnvelope`] is flattened into each kind's status beside the kind's own
//! fields. The records hold data and check only what their own shape requires; the commit
//! lanes that write them are elsewhere.
//!
//! Choices this module makes where the design is open:
//!
//! - Field names are serialized as the design prints them: `observedGeneration` and the
//!   [`Condition`] fields in the Kubernetes camel case, every kernel field in snake case
//!   (`state_revision`, `last_receipt_ref`, `pending_commit`).
//! - Every envelope carries `control_revision`, as KERNEL §1 states for every aggregate; it
//!   stays zero on an aggregate without a control lane. `control_receipt_ring` is present
//!   exactly on an aggregate with a control lane, as M0 §1 states.
//! - `pending_commit` is absent until the first domain commit, which reads as a `CLEARED` slot
//!   ([`StatusEnvelope::pending_commit_state`]), the initial state of the KERNEL §10 slot
//!   machine. A cleared slot keeps its body until the next domain commit replaces it.
//! - [`PendingCommit`] holds the FORMAL §2 fields and no others: the command's receipt, which
//!   is prepared before the commit, is named by `receipt_uid`, and the full [`AuditEnvelope`]
//!   of the commit rebuilds its audit event.
//! - Each effect intent is a [`SlotEffectIntent`]: the FORMAL §2 `EffectIntentRecord` without
//!   its `operation_key`, which is derived, and with the `installation_lineage` it is derived
//!   from. `provider_binding`, `desired_outcome`, `target_identity`, `contract_revision` and
//!   `installation_lineage` are opaque text here; `provider_binding` is text rather than the
//!   provider and operation pair FORMAL §2 names, and the slot holds `installation_lineage`
//!   where FORMAL §2 holds `operation_key` (#384).
//! - A [`ControlReceipt`]'s `control_revision` is the revision its commit produced, and its
//!   `audit_envelope` is the same [`AuditEnvelope`] a pending commit holds; the envelope repeats
//!   the receipt's commit sequence and control revision, as FORMAL §2 does.
//! - The ring's bound is the profile's [`ControlRing::entries`](crate::profile::ControlRing):
//!   the ring is full when that many receipts are unpublished, and a published receipt gives
//!   way, oldest first, when a new one needs its place. The per-entry size limit,
//!   [`ControlRing::entry_max_kib`](crate::profile::ControlRing), applies to an entry's encoded
//!   form and is not checked by these records, which have no encoding.
//! - [`PendingCommitState`] and [`ControlReceiptState`] are the state values of the KERNEL §10
//!   slot and control-receipt machines; their transitions are not defined here.
//! - No record denies unknown fields: the API server prunes fields its schema does not
//!   declare, and a Kubernetes structural schema may not declare `additionalProperties`
//!   beside `properties`.
//!
//! Open design points these records depend on: KERNEL §1 gives every aggregate a
//! `control_revision` while M0 §1 lists it only for aggregates with a control lane, and these
//! records follow KERNEL §1 (#327).
//!
//! The envelope's schema is snapshotted in `status_envelope.schema.json` beside this module;
//! the snapshot test rewrites it when `AUTOBOT_UPDATE_SNAPSHOTS` is set.

mod audit;
mod envelope;
mod ring;
mod slot;

pub use audit::AuditEnvelope;
pub use envelope::{Condition, ConditionStatus, StatusEnvelope};
pub use ring::{ControlReceipt, ControlReceiptRing, ControlReceiptState};
pub use slot::{PendingCommit, PendingCommitState, SlotEffectIntent};

#[cfg(test)]
mod tests;
