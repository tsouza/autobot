//! Delete: an object is deleted only once its create receipt is terminal, and only after the
//! tombstone that proves the deletion holds its name (KERNEL §2).

use super::cas::Missing;
use super::create::{Create, CreateOutcome};
use super::object::{Object, ObjectKey};
use super::op::{OpKind, Protocol, ProtocolError, Step, StoreOp, StoreResult};
use crate::types::Uid;

/// How a delete reads its target's create receipt.
///
/// The encoding of a `CommandReceipt` belongs to the command path, so the store contract
/// leaves the judgement to its caller. Once a receipt is terminal it cannot be rewritten
/// (KERNEL §2), so a judgement that it is terminal stays true.
pub trait ReceiptCheck {
    /// Whether `receipt`, read under the delete's create-receipt key, is the terminal create
    /// receipt of `target`.
    fn is_terminal_create_receipt(&self, target: &Object, receipt: &Object) -> bool;
}

impl<F> ReceiptCheck for F
where
    F: Fn(&Object, &Object) -> bool,
{
    fn is_terminal_create_receipt(&self, target: &Object, receipt: &Object) -> bool {
        self(target, receipt)
    }
}

/// One delete of one object.
#[derive(Debug, Clone)]
pub struct DeleteRequest<C> {
    /// The object to delete.
    pub target: ObjectKey,
    /// The target's UID, which the delete pins.
    pub uid: Uid,
    /// The key of the target's create receipt.
    pub create_receipt: ObjectKey,
    /// The UID of the target's create receipt, which the target's origin records.
    pub create_receipt_uid: Uid,
    /// The key of the tombstone: the delete command's receipt. A key equal to `target` is
    /// refused.
    pub tombstone: ObjectKey,
    /// The tombstone's encoded spec.
    pub tombstone_spec: String,
    /// The judgement of the create receipt.
    pub check: C,
}

/// How a delete ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteOutcome {
    /// The target is gone and this tombstone, which carries the target's origin, holds the
    /// tombstone name; in this call or an earlier one. A target found absent is deleted only
    /// when the object under the tombstone name has the request's create receipt UID in its
    /// origin and the request's tombstone spec.
    Deleted(Box<Object>),
    /// The target's create receipt is not terminal, or is absent: nothing was written.
    Refused {
        /// The create receipt as read, if one was found.
        create_receipt: Option<Box<Object>>,
    },
    /// The tombstone name holds an object with another origin, or, with the target absent,
    /// another create receipt UID or spec: the target is untouched by this delete.
    TombstoneTaken(Box<Object>),
    /// The request names the target's own key as the tombstone: nothing was read or written.
    TombstoneIsTarget,
    /// The tombstone name was taken when the tombstone was created and empty when read: the
    /// target is untouched.
    TombstoneVanished,
    /// The target is missing and no tombstone holds the tombstone name, or the target's name
    /// holds another object.
    Missing(Missing),
}

/// The next thing a delete does.
enum Next {
    /// Read the target.
    ReadTarget,
    /// Read the target's create receipt.
    ReadReceipt { target: Box<Object> },
    /// Create the tombstone.
    Tombstone {
        target: Box<Object>,
        create: Box<Create>,
    },
    /// Delete the target, conditioned on its UID and the resource version read.
    Delete {
        target: Box<Object>,
        tombstone: Box<Object>,
    },
    /// Read the target after a conflicting or `UNCERTAIN` delete.
    Reread { tombstone: Box<Object> },
    /// Read the tombstone of a target found missing on the first read.
    ReadTombstone,
    /// Finished.
    Done(DeleteOutcome),
}

/// The protocol that deletes one object.
///
/// It reads the target, then its create receipt, and refuses unless the receipt is terminal.
/// It then creates the tombstone by name, with the target's origin, as [`Create`] does, and
/// only then deletes the target conditioned on the target's UID and the resource version it
/// read. A conflicting or `UNCERTAIN` delete is followed by a read of the target: the same
/// incarnation is deleted again at the resource version read, and an absent target is
/// deleted. A fresh protocol for the same request that finds the target absent reads the
/// tombstone and ends [`DeleteOutcome::Deleted`] only when it is the request's own: its origin
/// names the request's create receipt UID and its spec is the request's tombstone spec.
pub struct Delete<C: ReceiptCheck> {
    request: DeleteRequest<C>,
    next: Next,
    waiting: Option<OpKind>,
}

impl<C: ReceiptCheck> Delete<C> {
    /// The protocol for `request`.
    #[must_use]
    pub fn new(request: DeleteRequest<C>) -> Self {
        Self {
            next: if request.tombstone == request.target {
                Next::Done(DeleteOutcome::TombstoneIsTarget)
            } else {
                Next::ReadTarget
            },
            request,
            waiting: None,
        }
    }

    /// The step after the target was read as `target`, after a delete of `tombstone`'s target
    /// failed to report its result, or on the first read when `tombstone` is `None`.
    fn target_read(&self, target: Box<Object>, tombstone: Option<Box<Object>>) -> Next {
        if target.uid != self.request.uid {
            return Next::Done(DeleteOutcome::Missing(Missing::Replaced {
                uid: target.uid,
            }));
        }
        match tombstone {
            Some(tombstone) => Next::Delete { target, tombstone },
            None => Next::ReadReceipt { target },
        }
    }

    /// The step after the create receipt was read as `receipt`.
    fn receipt_read(&self, target: Box<Object>, receipt: Option<Box<Object>>) -> Next {
        match receipt {
            Some(receipt)
                if self
                    .request
                    .check
                    .is_terminal_create_receipt(&target, &receipt) =>
            {
                let create = Create::new(
                    self.request.tombstone.clone(),
                    self.request.tombstone_spec.clone(),
                    target.origin.clone(),
                );
                Next::Tombstone {
                    target,
                    create: Box::new(create),
                }
            }
            create_receipt => Next::Done(DeleteOutcome::Refused { create_receipt }),
        }
    }

    /// The outcome of finding `found` under the tombstone name with the target absent: the
    /// request's own tombstone, or another object.
    fn found_absent(&self, found: Box<Object>) -> DeleteOutcome {
        if found.origin.create_receipt_uid == self.request.create_receipt_uid
            && found.spec == self.request.tombstone_spec
        {
            DeleteOutcome::Deleted(found)
        } else {
            DeleteOutcome::TombstoneTaken(found)
        }
    }

    /// The state `outcome` of the tombstone's create leads to.
    fn tombstone_created(target: Box<Object>, outcome: CreateOutcome) -> Next {
        match outcome {
            CreateOutcome::Created(tombstone) => Next::Delete { target, tombstone },
            CreateOutcome::Taken(other) => Next::Done(DeleteOutcome::TombstoneTaken(other)),
            CreateOutcome::Vanished => Next::Done(DeleteOutcome::TombstoneVanished),
        }
    }

    /// The state after `result` answers the operation of `next`; `next` and `result` back
    /// when it does not answer it.
    fn advance(&self, next: Next, result: StoreResult) -> Result<Next, (Next, StoreResult)> {
        Ok(match (next, result) {
            (Next::ReadTarget, StoreResult::Object(target)) => self.target_read(target, None),
            (Next::ReadTarget, StoreResult::NotFound) => Next::ReadTombstone,
            (Next::ReadReceipt { target }, StoreResult::Object(receipt)) => {
                self.receipt_read(target, Some(receipt))
            }
            (Next::ReadReceipt { target }, StoreResult::NotFound) => {
                self.receipt_read(target, None)
            }
            (Next::Delete { tombstone, .. }, StoreResult::Object(_) | StoreResult::NotFound)
            | (Next::Reread { tombstone }, StoreResult::NotFound) => {
                Next::Done(DeleteOutcome::Deleted(tombstone))
            }
            (Next::ReadTombstone, StoreResult::Object(found)) => {
                Next::Done(self.found_absent(found))
            }
            (Next::Delete { tombstone, .. }, StoreResult::Conflict | StoreResult::Uncertain) => {
                Next::Reread { tombstone }
            }
            (Next::Reread { tombstone }, StoreResult::Object(target)) => {
                self.target_read(target, Some(tombstone))
            }
            (Next::ReadTombstone, StoreResult::NotFound) => {
                Next::Done(DeleteOutcome::Missing(Missing::NotFound))
            }
            (
                next @ (Next::ReadTarget
                | Next::ReadReceipt { .. }
                | Next::Reread { .. }
                | Next::ReadTombstone),
                StoreResult::Unavailable,
            ) => next,
            (next, result) => return Err((next, result)),
        })
    }
}

impl<C: ReceiptCheck> Protocol for Delete<C> {
    type Outcome = DeleteOutcome;

    fn step(&mut self) -> Step<DeleteOutcome> {
        loop {
            let op = match &mut self.next {
                Next::Done(outcome) => return Step::Done(outcome.clone()),
                Next::ReadTarget | Next::Reread { .. } => StoreOp::Get {
                    key: self.request.target.clone(),
                },
                Next::ReadReceipt { .. } => StoreOp::Get {
                    key: self.request.create_receipt.clone(),
                },
                Next::ReadTombstone => StoreOp::Get {
                    key: self.request.tombstone.clone(),
                },
                Next::Tombstone { create, .. } => match create.step() {
                    Step::Op(op) => op,
                    Step::Done(outcome) => {
                        if let Next::Tombstone { target, .. } =
                            std::mem::replace(&mut self.next, Next::ReadTarget)
                        {
                            self.next = Self::tombstone_created(target, outcome);
                        }
                        continue;
                    }
                },
                Next::Delete { target, .. } => StoreOp::Delete {
                    key: target.key.clone(),
                    uid: target.uid.clone(),
                    resource_version: target.resource_version.clone(),
                },
            };
            self.waiting = Some(op.kind());
            return Step::Op(op);
        }
    }

    fn resume(&mut self, result: StoreResult) -> Result<(), ProtocolError> {
        let Some(waiting) = self.waiting else {
            return Err(ProtocolError::unexpected(None, &result));
        };
        if let Next::Tombstone { create, .. } = &mut self.next {
            create.resume(result)?;
            self.waiting = None;
            return Ok(());
        }
        let next = std::mem::replace(&mut self.next, Next::ReadTarget);
        match self.advance(next, result) {
            Ok(next) => {
                self.next = next;
                self.waiting = None;
                Ok(())
            }
            Err((next, result)) => {
                self.next = next;
                Err(ProtocolError::unexpected(Some(waiting), &result))
            }
        }
    }
}
