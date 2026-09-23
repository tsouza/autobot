//! The commit lanes of `docs/design/AUTOBOT-KERNEL.md` §1 driven by a [`Reducer`]: the
//! reducer decides, and the store's [`Commit`] protocol writes the decision in one CAS.
//!
//! - [`DomainCommit`] is the domain lane, FORMAL §3 `CommitAggregateCAS`. It reads the
//!   aggregate linearizably, holds while the pending slot is `OCCUPIED` or `REPAIRING` (the
//!   receipt barrier), checks the command's expected `state_revision`, decodes the aggregate's
//!   state through [`StatusFields`], runs [`reducer::step`](crate::reducer::step) on it, and
//!   writes the reducer's domain fields with the incremented `state_revision` and
//!   `commit_sequence` and the pending slot in the same status write. A write that times out
//!   is resolved only by reading the slot; one the read cannot place ends
//!   [`DomainOutcome::Uncertain`], which only the command's receipt resolves (#83).
//!
//! Choices this module makes where the design is open:
//!
//! - The pending slot, its digests and the audit envelope are the store's
//!   ([`crate::store`]): the slot's before and after digests are
//!   [`domain_digest`](crate::digest::domain_digest) of the status, and that is the one domain
//!   digest. The [`TransitionReceipt`](crate::reducer::TransitionReceipt) a commit returns
//!   reports it: its state digests are the store's
//!   [`domain_digest`](crate::digest::domain_digest) and
//!   [`control_digest`](crate::digest::control_digest) of the status read and of the status
//!   its write makes, so repair checks the value the receipt names. The digests a state gives
//!   through [`ReducerState`](crate::reducer::ReducerState) are those
//!   [`reducer::step`](crate::reducer::step) checks the field partition on; the commit
//!   replaces them in the receipt it returns.
//! - A domain commit is requested through the store's
//!   [`DomainCommitRequest`](crate::store::DomainCommitRequest), which carries no ring limits.
//! - A domain commit always pins its `state_revision`. `AcceptDispatch`, the one command that
//!   pins register values instead (KERNEL §3.2), commits through a store domain request
//!   with no `expected_revision`, which is [`Pin::Current`](crate::store::Pin::Current); a
//!   second acceptance of one permit is open in #298.
//! - A reducer that refuses the command ends [`DomainOutcome::Refused`] with the reducer's
//!   [`Refusal`](crate::reducer::Refusal): its ground and the revision it read. A reducer
//!   defect ([`ReducerError`](crate::reducer::ReducerError)) and a status whose fields do not
//!   decode end in outcomes of their own and write nothing; neither is a refusal, so neither
//!   can be recorded as a rejection.
//! - A commit that reads its lane past the revision it pinned, with no slot of its command,
//!   ends [`DomainOutcome::Passed`] when no write of this protocol timed out, and
//!   [`DomainOutcome::Uncertain`] when one did: the timed-out write may have landed and its
//!   slot been repaired, cleared and replaced since. Neither is a rejection; both are resolved
//!   by the command's receipt (KERNEL §2), through the command receipt protocol of #83.
//!
//! [`Reducer`]: crate::reducer::Reducer
//! [`Commit`]: crate::store::Commit

mod domain;

pub use domain::{DomainCommit, DomainOutcome, DomainRequest, FieldsError, StatusFields};
