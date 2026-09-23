//! The in-memory store driver: it performs the store operations of
//! `autobot_kernel::store` against a map in memory and injects the faults of
//! [`Fault`] on request.
//!
//! [`MemStore::execute`] performs one [`StoreOp`]. Every write that applies, a delete included,
//! takes a new resource version from one counter for the whole store and appends a watch
//! event, so a `Watch` after resource version `v` returns the events of every write after `v`.
//! `Get` and `List` read the map itself, the store's only state, so every read is
//! linearizable.
//!
//! Faults are armed with [`MemStore::arm`] and each fires once; [`MemStore::take_fired`] reports
//! each fault that fired, as it was armed:
//!
//! - [`Fault::Crash`]: once `after_writes` more writes have applied, the last one's result is
//!   replaced by [`Execution::Crashed`]; the driver treats the process as dead.
//! - [`Fault::WriteTimeout`]: the next write reports `UNCERTAIN`, having applied if `applied`
//!   and it would have succeeded.
//! - [`Fault::LateWrite`]: the next write reports `UNCERTAIN` unapplied, and is applied, if its
//!   conditions still hold, right after the store answers the next `Get`.
//! - [`Fault::LostCreateAck`]: the next create applies, when its name is free, and reports
//!   `UNCERTAIN`.
//! - [`Fault::DropEvents`], [`Fault::DuplicateEvents`], [`Fault::ReorderEvents`]: the next
//!   watch batch loses its first events, delivers each twice, or delivers them in reverse.
//! - [`Fault::ExpireWatch`]: the next watch reports `Expired`.
//!
//! [`run`] drives one protocol to its outcome and [`run_script`] one conformance script.

use autobot_kernel::profile::ControlRing;
use autobot_kernel::store::conformance::{Action, Failure, Fault, Script, ScriptRun};
use autobot_kernel::store::{
    Kind, Object, ObjectKey, OpKind, Protocol, ProtocolError, ResourceVersion, Step, StoreOp,
    StoreResult, WatchEvent,
};
use autobot_kernel::types::{Namespace, Uid};
use std::collections::BTreeMap;
use std::fmt;
use std::num::NonZeroU32;

/// What [`MemStore::execute`] did with one operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Execution {
    /// The operation's result.
    Result(StoreResult),
    /// An armed crash fired: the write applied and the process died before seeing its result.
    Crashed,
}

/// An in-memory store with fault injection.
#[derive(Debug, Clone, Default)]
pub struct MemStore {
    objects: BTreeMap<ObjectKey, Object>,
    version: u64,
    uids: u64,
    log: Vec<(u64, ObjectKey)>,
    armed: Vec<Fault>,
    fired: Vec<Fault>,
    late: Option<StoreOp>,
}

impl MemStore {
    /// An empty store with no fault armed.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Arms `fault`; it fires on the next operation it applies to.
    pub fn arm(&mut self, fault: Fault) {
        self.armed.push(fault);
    }

    /// Takes the faults that fired since the last call, in the order they fired, each as it
    /// was armed.
    pub fn take_fired(&mut self) -> Vec<Fault> {
        std::mem::take(&mut self.fired)
    }

    /// The object under `key`, read without going through a protocol.
    #[must_use]
    pub fn object(&self, key: &ObjectKey) -> Option<&Object> {
        self.objects.get(key)
    }

    /// Performs `op`.
    pub fn execute(&mut self, op: StoreOp) -> Execution {
        let kind = op.kind();
        let write_fault = if kind.is_write() {
            self.disarm(|f| {
                matches!(f, Fault::WriteTimeout { .. } | Fault::LateWrite)
                    || (kind == OpKind::Create && *f == Fault::LostCreateAck)
            })
        } else {
            None
        };
        match write_fault {
            Some(fault @ Fault::WriteTimeout { applied: false }) => {
                self.fired.push(fault);
                return Execution::Result(StoreResult::Uncertain);
            }
            Some(fault @ Fault::LateWrite) => {
                self.fired.push(fault);
                self.late = Some(op);
                return Execution::Result(StoreResult::Uncertain);
            }
            _ => {}
        }
        let result = self.perform(op);
        if kind == OpKind::Get
            && let Some(late) = self.late.take()
        {
            self.perform(late);
        }
        if let Some(fault) = write_fault {
            self.fired.push(fault);
        }
        if kind.is_write() && matches!(result, StoreResult::Object(_)) && self.count_down_crash() {
            return Execution::Crashed;
        }
        if write_fault.is_some() {
            return Execution::Result(StoreResult::Uncertain);
        }
        Execution::Result(result)
    }

    /// Counts one applied write against an armed crash; whether the crash fires.
    fn count_down_crash(&mut self) -> bool {
        let Some(i) = self
            .armed
            .iter()
            .position(|f| matches!(f, Fault::Crash { .. }))
        else {
            return false;
        };
        let Fault::Crash { after_writes } = self.armed.remove(i) else {
            return false;
        };
        match NonZeroU32::new(after_writes.get() - 1) {
            Some(left) => {
                self.armed.insert(i, Fault::Crash { after_writes: left });
                false
            }
            None => {
                self.fired.push(Fault::Crash { after_writes });
                true
            }
        }
    }

    /// Removes and returns the first armed fault `applies` accepts.
    fn disarm(&mut self, applies: impl Fn(&Fault) -> bool) -> Option<Fault> {
        let i = self.armed.iter().position(applies)?;
        Some(self.armed.remove(i))
    }

    /// Performs `op` with no write fault.
    fn perform(&mut self, op: StoreOp) -> StoreResult {
        match op {
            StoreOp::Get { key } => self
                .objects
                .get(&key)
                .cloned()
                .map_or(StoreResult::NotFound, |o| StoreResult::Object(Box::new(o))),
            StoreOp::Create { key, spec, origin } => self.create(key, spec, origin),
            StoreOp::UpdateStatus {
                key,
                uid,
                resource_version,
                status,
            } => self.update(&key, &uid, &resource_version, status),
            StoreOp::Delete {
                key,
                uid,
                resource_version,
            } => self.delete(&key, &uid, &resource_version),
            StoreOp::List { kind, namespace } => self.list(&kind, &namespace),
            StoreOp::Watch {
                kind,
                namespace,
                since,
            } => self.watch(&kind, &namespace, &since),
        }
    }

    /// The next resource version, recorded as a write of `key`.
    fn write(&mut self, key: &ObjectKey) -> ResourceVersion {
        self.version += 1;
        self.log.push((self.version, key.clone()));
        version(self.version)
    }

    /// Creates `key` unless the name is taken.
    fn create(
        &mut self,
        key: ObjectKey,
        spec: String,
        origin: autobot_kernel::store::Origin,
    ) -> StoreResult {
        if self.objects.contains_key(&key) {
            return StoreResult::AlreadyExists;
        }
        self.uids += 1;
        let uid: Uid = match format!("mem-uid-{}", self.uids).parse() {
            Ok(uid) => uid,
            Err(_) => return StoreResult::Uncertain,
        };
        let object = Object {
            key: key.clone(),
            uid,
            resource_version: self.write(&key),
            origin,
            spec,
            status: None,
        };
        self.objects.insert(key, object.clone());
        StoreResult::Object(Box::new(object))
    }

    /// Replaces the status of `key` if its UID and resource version match.
    fn update(
        &mut self,
        key: &ObjectKey,
        uid: &Uid,
        resource_version: &ResourceVersion,
        status: Box<autobot_kernel::store::Status>,
    ) -> StoreResult {
        match self.objects.get(key) {
            None => StoreResult::NotFound,
            Some(o) if o.uid != *uid || o.resource_version != *resource_version => {
                StoreResult::Conflict
            }
            Some(_) => {
                let version = self.write(key);
                match self.objects.get_mut(key) {
                    Some(object) => {
                        object.resource_version = version;
                        object.status = Some(*status);
                        StoreResult::Object(Box::new(object.clone()))
                    }
                    None => StoreResult::NotFound,
                }
            }
        }
    }

    /// Deletes `key` if its UID and resource version match; the result holds the object as it
    /// was.
    fn delete(
        &mut self,
        key: &ObjectKey,
        uid: &Uid,
        resource_version: &ResourceVersion,
    ) -> StoreResult {
        match self.objects.get(key) {
            None => StoreResult::NotFound,
            Some(o) if o.uid != *uid || o.resource_version != *resource_version => {
                StoreResult::Conflict
            }
            Some(_) => {
                self.write(key);
                self.objects
                    .remove(key)
                    .map_or(StoreResult::NotFound, |o| StoreResult::Object(Box::new(o)))
            }
        }
    }

    /// Every object of `kind` in `namespace`.
    fn list(&self, kind: &Kind, namespace: &Namespace) -> StoreResult {
        StoreResult::Listed {
            items: self
                .objects
                .values()
                .filter(|o| o.key.kind == *kind && o.key.namespace == *namespace)
                .cloned()
                .collect(),
            resource_version: version(self.version),
        }
    }

    /// The events of `kind` in `namespace` after `since`, with the armed watch faults applied.
    fn watch(
        &mut self,
        kind: &Kind,
        namespace: &Namespace,
        since: &ResourceVersion,
    ) -> StoreResult {
        let Ok(since) = since.as_str().parse::<u64>() else {
            return StoreResult::Expired;
        };
        if let Some(fault) = self.disarm(|f| *f == Fault::ExpireWatch) {
            self.fired.push(fault);
            return StoreResult::Expired;
        }
        let mut events: Vec<WatchEvent> = self
            .log
            .iter()
            .filter(|(v, key)| *v > since && key.kind == *kind && key.namespace == *namespace)
            .map(|(v, key)| WatchEvent {
                key: key.clone(),
                resource_version: version(*v),
            })
            .collect();
        if let Some(fault) = self.disarm(|f| matches!(f, Fault::DropEvents { .. })) {
            if let Fault::DropEvents { count } = fault {
                let dropped = usize::try_from(count).unwrap_or(usize::MAX);
                events.drain(..dropped.min(events.len()));
            }
            self.fired.push(fault);
        }
        if let Some(fault) = self.disarm(|f| *f == Fault::DuplicateEvents) {
            events = events.into_iter().flat_map(|e| [e.clone(), e]).collect();
            self.fired.push(fault);
        }
        if let Some(fault) = self.disarm(|f| *f == Fault::ReorderEvents) {
            events.reverse();
            self.fired.push(fault);
        }
        StoreResult::Events {
            events,
            cursor: version(self.version),
        }
    }
}

/// The resource version with value `v`.
fn version(v: u64) -> ResourceVersion {
    ResourceVersion::from(v)
}

/// Why [`run`] stopped without an outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunError {
    /// An armed crash fired.
    Crashed,
    /// The protocol refused a result.
    Protocol(ProtocolError),
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Crashed => f.write_str("the process crashed"),
            Self::Protocol(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for RunError {}

/// Drives `protocol` against `store` to its outcome.
///
/// # Errors
///
/// [`RunError::Crashed`] if an armed crash fired, and [`RunError::Protocol`] if the protocol
/// refused a result.
pub fn run<P: Protocol>(store: &mut MemStore, protocol: &mut P) -> Result<P::Outcome, RunError> {
    loop {
        match protocol.step() {
            Step::Done(outcome) => return Ok(outcome),
            Step::Op(op) => match store.execute(op) {
                Execution::Result(result) => protocol.resume(result).map_err(RunError::Protocol)?,
                Execution::Crashed => return Err(RunError::Crashed),
            },
        }
    }
}

/// Runs `script` against `store`, whose control commits append within `ring`.
///
/// # Errors
///
/// The [`Failure`] of the first step that did not behave as the script expects.
pub fn run_script(store: &mut MemStore, script: &Script, ring: ControlRing) -> Result<(), Failure> {
    let mut script_run = ScriptRun::new(script, ring);
    loop {
        match script_run.step() {
            Action::Done(result) => return result,
            Action::Arm(fault) => store.arm(fault),
            Action::Op(op) => {
                let execution = store.execute(op);
                for fault in store.take_fired() {
                    script_run.fired(fault);
                }
                match execution {
                    Execution::Result(result) => script_run.resume(result),
                    Execution::Crashed => script_run.crash(),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
