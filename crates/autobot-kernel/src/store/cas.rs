//! The read–decide–write loop every status write of the store contract runs.

use super::object::{Object, ObjectKey, Status};
use super::op::{OpKind, Protocol, ProtocolError, Step, StoreOp, StoreResult};
use crate::types::Uid;

/// Why a status write found no object to decide on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Missing {
    /// No object has the key.
    NotFound,
    /// The object under the key has another UID: the name was created again.
    Replaced {
        /// The UID found.
        uid: Uid,
    },
}

/// What a decider concludes from one linearizable read of its target.
pub(crate) enum Decision<O> {
    /// Write this status, conditioned on the read's UID and resource version.
    Write(Box<Status>),
    /// Finished.
    Done(O),
}

/// The part of a status-write protocol that decides from what it read.
pub(crate) trait Decide {
    /// The outcome.
    type Outcome: Clone;

    /// Decides from `object`, the target as last read or written. `uncertain_base` is the
    /// object the protocol's first `UNCERTAIN` write was computed from, if one was.
    fn decide(
        &mut self,
        object: &Object,
        uncertain_base: Option<&Object>,
    ) -> Decision<Self::Outcome>;

    /// The outcome when the target is missing.
    fn missing(&self, missing: Missing) -> Self::Outcome;
}

/// The next thing the loop does.
enum Next<O> {
    Read,
    Write {
        base: Box<Object>,
        status: Box<Status>,
    },
    Done(O),
}

/// A status write as a loop: read the target, decide, write conditioned on the read's
/// resource version, and read again after a conflict or an `UNCERTAIN` result.
///
/// An `UNCERTAIN` or conflicting write is always followed by a read, and the object a write
/// returns is decided on like a read one.
pub(crate) struct Cas<D: Decide> {
    key: ObjectKey,
    uid: Uid,
    decider: D,
    next: Next<D::Outcome>,
    waiting: Option<OpKind>,
    uncertain_base: Option<Box<Object>>,
}

impl<D: Decide> Cas<D> {
    /// The loop writing the object `key` with UID `uid`, deciding with `decider`.
    pub(crate) fn new(key: ObjectKey, uid: Uid, decider: D) -> Self {
        Self {
            key,
            uid,
            decider,
            next: Next::Read,
            waiting: None,
            uncertain_base: None,
        }
    }

    /// Decides from a read or written object.
    fn decide(&mut self, object: Box<Object>) {
        if object.uid != self.uid {
            self.next = Next::Done(self.decider.missing(Missing::Replaced { uid: object.uid }));
            return;
        }
        self.next = match self.decider.decide(&object, self.uncertain_base.as_deref()) {
            Decision::Write(status) => Next::Write {
                base: object,
                status,
            },
            Decision::Done(outcome) => Next::Done(outcome),
        };
    }
}

impl<D: Decide> Protocol for Cas<D> {
    type Outcome = D::Outcome;

    fn step(&mut self) -> Step<D::Outcome> {
        let op = match &self.next {
            Next::Done(outcome) => return Step::Done(outcome.clone()),
            Next::Read => StoreOp::Get {
                key: self.key.clone(),
            },
            Next::Write { base, status } => StoreOp::UpdateStatus {
                key: self.key.clone(),
                uid: base.uid.clone(),
                resource_version: base.resource_version.clone(),
                status: status.clone(),
            },
        };
        self.waiting = Some(op.kind());
        Step::Op(op)
    }

    fn resume(&mut self, result: StoreResult) -> Result<(), ProtocolError> {
        match (self.waiting, result) {
            (Some(OpKind::Get | OpKind::UpdateStatus), StoreResult::Object(object)) => {
                self.decide(object);
            }
            (Some(OpKind::Get), StoreResult::NotFound) => {
                self.next = Next::Done(self.decider.missing(Missing::NotFound));
            }
            (Some(OpKind::Get), StoreResult::Unavailable)
            | (Some(OpKind::UpdateStatus), StoreResult::Conflict | StoreResult::NotFound) => {
                self.next = Next::Read;
            }
            (Some(OpKind::UpdateStatus), StoreResult::Uncertain) => {
                if let Next::Write { base, .. } = std::mem::replace(&mut self.next, Next::Read)
                    && self.uncertain_base.is_none()
                {
                    self.uncertain_base = Some(base);
                }
            }
            (waiting, result) => return Err(ProtocolError::unexpected(waiting, &result)),
        }
        self.waiting = None;
        Ok(())
    }
}
