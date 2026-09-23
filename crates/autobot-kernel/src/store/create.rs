//! Create-by-name: an object is created under a deterministic name, and a lost
//! acknowledgement is resolved by reading that name.

use super::object::{Object, ObjectKey, Origin};
use super::op::{OpKind, Protocol, ProtocolError, Step, StoreOp, StoreResult};

/// How a create ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateOutcome {
    /// The object exists with this create's origin, created by this call or an earlier one.
    Created(Box<Object>),
    /// The name holds an object with another origin: another create receipt, or the same
    /// receipt with another input digest. Nothing was created.
    Taken(Box<Object>),
    /// The name was taken when the create ran and was empty when read. Absence after a
    /// refused create is never read as permission to create.
    Vanished,
}

/// The next thing a create does.
#[derive(Debug, Clone)]
enum Next {
    Create,
    Read { after_exists: bool },
    Done(CreateOutcome),
}

/// The protocol that creates one object by name.
///
/// It creates, and on `AlreadyExists` or `UNCERTAIN` reads the name: an object with this
/// create's origin is the create's own, whether this call or an earlier one made it; any other
/// object means the name is taken. A read that finds no object after `UNCERTAIN` means the
/// create has not applied, and it is sent again under the same name, which holds at most one
/// object whichever transmission lands.
pub struct Create {
    key: ObjectKey,
    spec: String,
    origin: Origin,
    next: Next,
    waiting: Option<OpKind>,
}

impl Create {
    /// The protocol creating `key` with `spec` and `origin`.
    #[must_use]
    pub fn new(key: ObjectKey, spec: String, origin: Origin) -> Self {
        Self {
            key,
            spec,
            origin,
            next: Next::Create,
            waiting: None,
        }
    }

    /// The outcome of finding `object` under the name.
    fn found(&self, object: Box<Object>) -> CreateOutcome {
        if object.origin == self.origin {
            CreateOutcome::Created(object)
        } else {
            CreateOutcome::Taken(object)
        }
    }
}

impl Protocol for Create {
    type Outcome = CreateOutcome;

    fn step(&mut self) -> Step<CreateOutcome> {
        let op = match &self.next {
            Next::Done(outcome) => return Step::Done(outcome.clone()),
            Next::Create => StoreOp::Create {
                key: self.key.clone(),
                spec: self.spec.clone(),
                origin: self.origin.clone(),
            },
            Next::Read { .. } => StoreOp::Get {
                key: self.key.clone(),
            },
        };
        self.waiting = Some(op.kind());
        Step::Op(op)
    }

    fn resume(&mut self, result: StoreResult) -> Result<(), ProtocolError> {
        let after_exists = matches!(self.next, Next::Read { after_exists: true });
        self.next = match (self.waiting, result) {
            (Some(OpKind::Create | OpKind::Get), StoreResult::Object(object)) => {
                Next::Done(self.found(object))
            }
            (Some(OpKind::Create), StoreResult::AlreadyExists) => Next::Read { after_exists: true },
            (Some(OpKind::Create), StoreResult::Uncertain) => Next::Read {
                after_exists: false,
            },
            (Some(OpKind::Get), StoreResult::Unavailable) => Next::Read { after_exists },
            (Some(OpKind::Get), StoreResult::NotFound) if after_exists => {
                Next::Done(CreateOutcome::Vanished)
            }
            (Some(OpKind::Get), StoreResult::NotFound) => Next::Create,
            (waiting, result) => return Err(ProtocolError::unexpected(waiting, &result)),
        };
        self.waiting = None;
        Ok(())
    }
}
