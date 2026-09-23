//! The store operations a protocol yields, the results it consumes, and the protocol shape.

use super::object::{Kind, Object, ObjectKey, Origin, ResourceVersion, Status};
use crate::types::{Namespace, Uid};
use std::fmt;

/// One operation a protocol asks its driver to perform.
///
/// Every read is linearizable: the driver answers `Get` and `List` from the store's quorum,
/// never from a cache. Every write is on one object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreOp {
    /// Read one object. Answered by [`StoreResult::Object`], [`StoreResult::NotFound`] or
    /// [`StoreResult::Unavailable`].
    Get {
        /// The object to read.
        key: ObjectKey,
    },
    /// Create an object under exactly this name, with no status. Answered by
    /// [`StoreResult::Object`], [`StoreResult::AlreadyExists`] or [`StoreResult::Uncertain`].
    Create {
        /// The name to create.
        key: ObjectKey,
        /// The encoded spec.
        spec: String,
        /// The origin metadata written with the spec.
        origin: Origin,
    },
    /// Replace an object's status, on the condition that its UID is `uid` and its resource
    /// version is still `resource_version`. Answered by [`StoreResult::Object`],
    /// [`StoreResult::Conflict`], [`StoreResult::NotFound`] or [`StoreResult::Uncertain`].
    UpdateStatus {
        /// The object to write.
        key: ObjectKey,
        /// The UID the object must have.
        uid: Uid,
        /// The resource version the object must still have.
        resource_version: ResourceVersion,
        /// The new status.
        status: Box<Status>,
    },
    /// Delete an object, on the condition that its UID is `uid` and its resource version is
    /// still `resource_version`. Answered by [`StoreResult::Object`], the object as it was when
    /// deleted, [`StoreResult::Conflict`], [`StoreResult::NotFound`] or
    /// [`StoreResult::Uncertain`].
    Delete {
        /// The object to delete.
        key: ObjectKey,
        /// The UID the object must have.
        uid: Uid,
        /// The resource version the object must still have.
        resource_version: ResourceVersion,
    },
    /// List every object of one kind in one namespace: a relist. Answered by
    /// [`StoreResult::Listed`] or [`StoreResult::Unavailable`].
    List {
        /// The kind to list.
        kind: Kind,
        /// The namespace to list.
        namespace: Namespace,
    },
    /// Take the next watch events of one kind in one namespace after `since`, a resource
    /// version from an earlier `List` or `Watch`. Answered by [`StoreResult::Events`],
    /// [`StoreResult::Expired`] or [`StoreResult::Unavailable`].
    Watch {
        /// The kind to watch.
        kind: Kind,
        /// The namespace to watch.
        namespace: Namespace,
        /// Where the previous listing or batch ended.
        since: ResourceVersion,
    },
}

impl StoreOp {
    /// The operation's kind.
    #[must_use]
    pub fn kind(&self) -> OpKind {
        match self {
            Self::Get { .. } => OpKind::Get,
            Self::Create { .. } => OpKind::Create,
            Self::UpdateStatus { .. } => OpKind::UpdateStatus,
            Self::Delete { .. } => OpKind::Delete,
            Self::List { .. } => OpKind::List,
            Self::Watch { .. } => OpKind::Watch,
        }
    }
}

/// The kind of a [`StoreOp`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OpKind {
    /// [`StoreOp::Get`].
    Get,
    /// [`StoreOp::Create`].
    Create,
    /// [`StoreOp::UpdateStatus`].
    UpdateStatus,
    /// [`StoreOp::Delete`].
    Delete,
    /// [`StoreOp::List`].
    List,
    /// [`StoreOp::Watch`].
    Watch,
}

impl OpKind {
    /// Whether the operation writes.
    #[must_use]
    pub fn is_write(self) -> bool {
        matches!(self, Self::Create | Self::UpdateStatus | Self::Delete)
    }
}

/// A watch event: a trigger naming an object that changed, never the object's state.
///
/// A watch may drop, duplicate or reorder events; whoever receives one reads the object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchEvent {
    /// The object that changed.
    pub key: ObjectKey,
    /// The object's resource version after the change.
    pub resource_version: ResourceVersion,
}

/// The result of one [`StoreOp`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreResult {
    /// The object read, created, written or deleted.
    Object(Box<Object>),
    /// No object has the key.
    NotFound,
    /// A create found the name taken.
    AlreadyExists,
    /// A status update or delete found another UID or resource version, and did not apply.
    Conflict,
    /// A write timed out or its acknowledgement was lost: it may or may not have applied.
    Uncertain,
    /// A read could not be answered; it had no effect and may be repeated.
    Unavailable,
    /// The objects a `List` found, and the resource version the listing is at.
    Listed {
        /// The objects.
        items: Vec<Object>,
        /// The listing's resource version, where a following `Watch` starts.
        resource_version: ResourceVersion,
    },
    /// The events a `Watch` took, and where the batch ended.
    Events {
        /// The events.
        events: Vec<WatchEvent>,
        /// The resource version the next `Watch` starts from.
        cursor: ResourceVersion,
    },
    /// The watch cannot continue from `since`; only a relist can.
    Expired,
}

impl StoreResult {
    /// The result's name, for errors.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Object(_) => "Object",
            Self::NotFound => "NotFound",
            Self::AlreadyExists => "AlreadyExists",
            Self::Conflict => "Conflict",
            Self::Uncertain => "Uncertain",
            Self::Unavailable => "Unavailable",
            Self::Listed { .. } => "Listed",
            Self::Events { .. } => "Events",
            Self::Expired => "Expired",
        }
    }
}

/// What a protocol wants next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step<O> {
    /// Perform this operation and pass its result to `resume`.
    Op(StoreOp),
    /// The protocol is finished with this outcome.
    Done(O),
}

/// A sans-I/O store protocol.
///
/// A driver calls [`step`](Self::step); on [`Step::Op`] it performs the operation, passes the
/// result to [`resume`](Self::resume) and steps again, until [`Step::Done`]. The protocol
/// performs no I/O, so a synchronous and an asynchronous driver run it alike. A driver that
/// stops early, such as a process that crashes, loses nothing the store holds: a new protocol
/// for the same request reads what landed.
pub trait Protocol {
    /// What the protocol ends with.
    type Outcome;

    /// The next operation, or the outcome. Calling it again before `resume` returns the same
    /// operation.
    fn step(&mut self) -> Step<Self::Outcome>;

    /// Consumes the result of the operation the last `step` returned.
    ///
    /// # Errors
    ///
    /// [`ProtocolError`] if no operation is outstanding or the result cannot answer it; the
    /// protocol is unchanged.
    fn resume(&mut self, result: StoreResult) -> Result<(), ProtocolError>;
}

/// A result a protocol cannot consume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    /// A result arrived while no operation was outstanding.
    NotWaiting {
        /// The result's name.
        result: &'static str,
    },
    /// A result that does not answer the outstanding operation.
    Unexpected {
        /// The outstanding operation.
        op: OpKind,
        /// The result's name.
        result: &'static str,
    },
}

impl ProtocolError {
    /// The error for `result` arriving while `waiting` is outstanding.
    pub(crate) fn unexpected(waiting: Option<OpKind>, result: &StoreResult) -> Self {
        match waiting {
            None => Self::NotWaiting {
                result: result.name(),
            },
            Some(op) => Self::Unexpected {
                op,
                result: result.name(),
            },
        }
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotWaiting { result } => {
                write!(f, "result {result} arrived with no operation outstanding")
            }
            Self::Unexpected { op, result } => {
                write!(f, "result {result} does not answer {op:?}")
            }
        }
    }
}

impl std::error::Error for ProtocolError {}
