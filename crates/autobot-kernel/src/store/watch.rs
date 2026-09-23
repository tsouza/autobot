//! Watch as a trigger: watch events and relists name objects to read, and carry no state.

use super::object::{Kind, ObjectKey, ResourceVersion};
use super::op::{OpKind, ProtocolError, StoreOp, StoreResult};
use crate::types::{Namespace, ObjectName};
use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU32;

/// The objects of one kind in one namespace that changed and are due to be read.
///
/// The machine lists first, then watches from where the listing ended, and lists again after
/// `relist_every` watch batches, when the watch expires, or on [`relist`](Self::relist). An
/// object is due when a listing or an event shows it at a resource version other than the one
/// last seen, or a listing no longer shows it. Events and listings only make objects due: the
/// caller reads every due object with a linearizable `Get`, so a dropped event is caught by the
/// next relist and a duplicated or reordered one costs at most an extra read.
///
/// It is stepped like a [`Protocol`](super::Protocol) but never finishes, so it does not
/// implement that trait: [`step`](Self::step) always returns an operation.
#[derive(Debug, Clone)]
pub struct Triggers {
    kind: Kind,
    namespace: Namespace,
    relist_every: NonZeroU32,
    cursor: Option<ResourceVersion>,
    batches: u32,
    seen: BTreeMap<ObjectName, ResourceVersion>,
    due: BTreeSet<ObjectName>,
    waiting: Option<OpKind>,
}

impl Triggers {
    /// The machine for `kind` in `namespace`, relisting after `relist_every` watch batches.
    #[must_use]
    pub fn new(kind: Kind, namespace: Namespace, relist_every: NonZeroU32) -> Self {
        Self {
            kind,
            namespace,
            relist_every,
            cursor: None,
            batches: 0,
            seen: BTreeMap::new(),
            due: BTreeSet::new(),
            waiting: None,
        }
    }

    /// The next operation: a `List` when a relist is due, else a `Watch` from the cursor.
    pub fn step(&mut self) -> StoreOp {
        let op = match &self.cursor {
            Some(since) if self.batches < self.relist_every.get() => StoreOp::Watch {
                kind: self.kind.clone(),
                namespace: self.namespace.clone(),
                since: since.clone(),
            },
            _ => StoreOp::List {
                kind: self.kind.clone(),
                namespace: self.namespace.clone(),
            },
        };
        self.waiting = Some(op.kind());
        op
    }

    /// Consumes the result of the operation the last [`step`](Self::step) returned.
    ///
    /// # Errors
    ///
    /// [`ProtocolError`] if no operation is outstanding or the result cannot answer it; the
    /// machine is unchanged.
    pub fn resume(&mut self, result: StoreResult) -> Result<(), ProtocolError> {
        match (self.waiting, result) {
            (
                Some(OpKind::List),
                StoreResult::Listed {
                    items,
                    resource_version,
                },
            ) => {
                let listed: BTreeMap<_, _> = items
                    .into_iter()
                    .filter(|o| o.key.kind == self.kind && o.key.namespace == self.namespace)
                    .map(|o| (o.key.name, o.resource_version))
                    .collect();
                for (name, version) in &listed {
                    self.see(name, version);
                }
                let gone: Vec<_> = self
                    .seen
                    .keys()
                    .filter(|name| !listed.contains_key(*name))
                    .cloned()
                    .collect();
                for name in gone {
                    self.seen.remove(&name);
                    self.due.insert(name);
                }
                self.cursor = Some(resource_version);
                self.batches = 0;
            }
            (Some(OpKind::Watch), StoreResult::Events { events, cursor }) => {
                for event in events {
                    if event.key.kind == self.kind && event.key.namespace == self.namespace {
                        self.see(&event.key.name, &event.resource_version);
                    }
                }
                self.cursor = Some(cursor);
                self.batches = self.batches.saturating_add(1);
            }
            (Some(OpKind::Watch), StoreResult::Expired) => self.cursor = None,
            (Some(OpKind::List | OpKind::Watch), StoreResult::Unavailable) => {}
            (waiting, result) => return Err(ProtocolError::unexpected(waiting, &result)),
        }
        self.waiting = None;
        Ok(())
    }

    /// Makes the next [`step`](Self::step) a `List`.
    pub fn relist(&mut self) {
        self.cursor = None;
    }

    /// Takes the keys of the objects due to be read.
    pub fn take_due(&mut self) -> BTreeSet<ObjectKey> {
        std::mem::take(&mut self.due)
            .into_iter()
            .map(|name| ObjectKey {
                kind: self.kind.clone(),
                namespace: self.namespace.clone(),
                name,
            })
            .collect()
    }

    /// Records that `name` was seen at `version`, making it due if that is news.
    fn see(&mut self, name: &ObjectName, version: &ResourceVersion) {
        if self.seen.get(name) != Some(version) {
            self.seen.insert(name.clone(), version.clone());
            self.due.insert(name.clone());
        }
    }
}
