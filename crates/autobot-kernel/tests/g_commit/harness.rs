//! The store side of the group's scenarios: the [`Driver`] a scenario runs against, the
//! in-memory [`MemDriver`], and helpers that build aggregates and lane commits from the
//! kernel's public store types.
//!
//! A scenario takes `&mut dyn Driver` and never names a concrete store, so a driver for the
//! Kubernetes store replays it unchanged. [`MemDriver`] is the group's own stand-in for the
//! testkit harness and the in-memory store of `autobot-fakes`, neither of which this crate's
//! tests can depend on yet.

use autobot_kernel::digest::digest;
use autobot_kernel::profile::{ControlRing, Profile};
use autobot_kernel::status::StatusEnvelope;
use autobot_kernel::store::{
    Change, ClearOutcome, ClearSlot, Commit, CommitOutcome, CommitRequest, ControlChange, Create,
    CreateOutcome, DomainChange, EventFields, GuardRefusal, Initialize, InitializeOutcome, Kind,
    Object, ObjectKey, OpKind, Origin, Pin, Protocol, ResourceVersion, Status, Step, StoreOp,
    StoreResult,
};
use autobot_kernel::types::{
    ControlRevision, LaneRevision, Namespace, ObjectRef, Principal, StateRevision, Uid,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;
use std::str::FromStr;

/// The M0 profile, the source of the ring and buffer bounds the scenarios run under.
const M0: &str = include_str!("../../../../profiles/m0.toml");

/// The namespace of every object a scenario names.
pub(crate) const NAMESPACE: &str = "g-commit";

/// The kind of the aggregates the scenarios commit on.
pub(crate) const AGGREGATE: &str = "CommitProbe";

/// The most operations [`run`] performs for one protocol before it reports [`Run::Stalled`].
const MAX_OPS: usize = 256;

/// Parses `text` into `T`, for the fixed identifiers of a scenario.
pub(crate) fn parse<T: FromStr>(text: &str) -> T
where
    T::Err: Debug,
{
    text.parse()
        .unwrap_or_else(|e| panic!("`{text}` does not parse: {e:?}"))
}

/// The M0 profile.
pub(crate) fn profile() -> Profile {
    Profile::parse(M0).unwrap_or_else(|e| panic!("the M0 profile does not parse: {e:?}"))
}

/// The M0 control-receipt ring bounds.
pub(crate) fn ring() -> ControlRing {
    profile().values().control_ring
}

/// The key of the object `name` of `kind` in [`NAMESPACE`].
pub(crate) fn key(kind: &str, name: &str) -> ObjectKey {
    ObjectKey {
        kind: parse(kind),
        namespace: parse(NAMESPACE),
        name: parse(name),
    }
}

/// A process fault a driver injects on the next write of `op` to an object of kind `kind`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Fault {
    /// The kind of the object whose write the fault fires on.
    pub(crate) kind: Kind,
    /// The write it fires on: [`OpKind::Create`] or [`OpKind::UpdateStatus`].
    pub(crate) op: OpKind,
    /// What happens to that write.
    pub(crate) effect: Effect,
}

/// What a [`Fault`] does to the write it fires on. The write applies in both cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Effect {
    /// The process dies before it sees the write's result.
    Crash,
    /// The acknowledgement is lost: the write reports `UNCERTAIN`.
    LostAck,
}

/// What a driver did with one operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Performed {
    /// The operation's result.
    Result(StoreResult),
    /// An armed [`Effect::Crash`] fired: the write applied and the process died.
    Crashed,
}

/// A store a scenario runs against.
pub(crate) trait Driver {
    /// Performs `op`.
    fn perform(&mut self, op: StoreOp) -> Performed;

    /// Arms `fault`; it fires once, on the first write it matches.
    fn arm(&mut self, fault: Fault);

    /// Takes every object of `kind` down (`true`) or brings it back: while down, reads of it
    /// are `Unavailable` and writes to it are `UNCERTAIN` without applying.
    fn outage(&mut self, kind: &Kind, down: bool);
}

/// How [`run`] ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Run<O> {
    /// The protocol finished.
    Done(O),
    /// An armed crash fired; the protocol's process is dead.
    Crashed,
    /// The protocol performed [`MAX_OPS`] operations without finishing, as it does while a
    /// store it needs is down.
    Stalled,
}

impl<O: Debug> Run<O> {
    /// The outcome, which the scenario requires the protocol to have reached.
    pub(crate) fn done(self) -> O {
        match self {
            Self::Done(outcome) => outcome,
            other => panic!("the protocol did not finish: {other:?}"),
        }
    }
}

/// Drives `protocol` against `driver` until it finishes, crashes or stalls.
pub(crate) fn run<P: Protocol + ?Sized>(
    driver: &mut dyn Driver,
    protocol: &mut P,
) -> Run<P::Outcome> {
    for _ in 0..MAX_OPS {
        match protocol.step() {
            Step::Done(outcome) => return Run::Done(outcome),
            Step::Op(op) => match driver.perform(op) {
                Performed::Result(result) => protocol
                    .resume(result)
                    .unwrap_or_else(|e| panic!("the protocol refused a result: {e}")),
                Performed::Crashed => return Run::Crashed,
            },
        }
    }
    Run::Stalled
}

/// Reads `key` with a linearizable `Get`.
pub(crate) fn read(driver: &mut dyn Driver, key: &ObjectKey) -> Option<Object> {
    match driver.perform(StoreOp::Get { key: key.clone() }) {
        Performed::Result(StoreResult::Object(object)) => Some(*object),
        Performed::Result(StoreResult::NotFound) => None,
        other => panic!("reading {key} gave {other:?}"),
    }
}

/// The status of `key`, which must exist and be initialized.
pub(crate) fn status(driver: &mut dyn Driver, key: &ObjectKey) -> Status {
    read(driver, key)
        .and_then(|o| o.status)
        .unwrap_or_else(|| panic!("{key} has no status"))
}

/// Every object of `kind` in [`NAMESPACE`], by a relist.
pub(crate) fn list(driver: &mut dyn Driver, kind: &str) -> Vec<Object> {
    let op = StoreOp::List {
        kind: parse(kind),
        namespace: parse(NAMESPACE),
    };
    match driver.perform(op) {
        Performed::Result(StoreResult::Listed { items, .. }) => items,
        other => panic!("listing {kind} gave {other:?}"),
    }
}

/// Creates the aggregate `name` and initializes its status, with a control lane when
/// `control_lane`; returns its key and UID.
pub(crate) fn aggregate(
    driver: &mut dyn Driver,
    name: &str,
    control_lane: bool,
) -> (ObjectKey, Uid) {
    let key = key(AGGREGATE, name);
    let origin = Origin {
        create_receipt_uid: parse(&format!("create-{name}")),
        input_digest: digest(name).unwrap_or_else(|e| panic!("{e}")),
        context_uid: parse("context"),
    };
    let object = match run(driver, &mut Create::new(key.clone(), String::new(), origin)).done() {
        CreateOutcome::Created(object) => object,
        other => panic!("creating {key} gave {other:?}"),
    };
    let envelope = if control_lane {
        StatusEnvelope::with_control_lane()
    } else {
        StatusEnvelope::default()
    };
    let status = Status {
        envelope,
        domain: "initial".to_owned(),
        control: if control_lane {
            "RUNNING".to_owned()
        } else {
            String::new()
        },
    };
    let initialized = run(
        driver,
        &mut Initialize::new(key.clone(), object.uid.clone(), status),
    );
    assert_eq!(initialized.done(), InitializeOutcome::Initialized);
    (key, object.uid)
}

/// The audit-event fields of `command`.
fn event(command: &str, event_type: &str) -> EventFields {
    EventFields {
        source_uid: parse(command),
        event_type: event_type.to_owned(),
        actor: parse::<Principal>("controller"),
        causation_id: command.to_owned(),
        correlation_id: "g-commit".to_owned(),
        schema_version: 1,
    }
}

/// The domain change of `command` setting the domain fields to `fields`.
pub(crate) fn domain_change(command: &str, fields: &str) -> Change {
    let receipt = format!("receipt-{command}");
    Change::Domain(DomainChange {
        fields: fields.to_owned(),
        receipt: ObjectRef {
            namespace: parse(NAMESPACE),
            name: parse(&receipt),
            uid: parse(&receipt),
        },
        event: event(command, "ProbeDomain"),
        effect_intents: Vec::new(),
    })
}

/// The control change of `command` setting the control fields to `fields`.
pub(crate) fn control_change(command: &str, fields: &str) -> Change {
    Change::Control(ControlChange {
        fields: fields.to_owned(),
        event: event(command, "ProbeControl"),
    })
}

/// Runs the lane commit of `command` on `target`, pinned at `pin`, applying `change`.
pub(crate) fn commit(
    driver: &mut dyn Driver,
    target: &(ObjectKey, Uid),
    command: &str,
    pin: Pin,
    change: Change,
) -> Run<CommitOutcome> {
    let transition =
        move |_: &Object, _: &Status| -> Result<Change, GuardRefusal> { Ok(change.clone()) };
    let request = CommitRequest {
        target: target.0.clone(),
        uid: target.1.clone(),
        command_uid: parse(command),
        pin,
        ring: ring(),
        transition,
    };
    run(driver, &mut Commit::new(request))
}

/// The pin of a domain command expecting `state_revision` `revision`.
pub(crate) fn state_pin(revision: u64) -> Pin {
    Pin::Revision(LaneRevision::State(
        StateRevision::new(revision).unwrap_or_else(|e| panic!("{e}")),
    ))
}

/// The pin of a control command expecting `control_revision` `revision`.
pub(crate) fn control_pin(revision: u64) -> Pin {
    Pin::Revision(LaneRevision::Control(
        ControlRevision::new(revision).unwrap_or_else(|e| panic!("{e}")),
    ))
}

/// A store held in memory: every read is of the map itself, so every read is linearizable.
/// It serves no watch, since no scenario of the group takes one: every `Watch` is `Expired`.
#[derive(Debug, Default)]
pub(crate) struct MemDriver {
    objects: BTreeMap<ObjectKey, Object>,
    writes: u64,
    armed: Vec<Fault>,
    down: BTreeSet<Kind>,
}

impl MemDriver {
    /// An empty store with no fault armed and nothing down.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Takes the armed fault matching a write of `op` to `key`, if one is armed.
    fn fault(&mut self, key: &ObjectKey, op: OpKind) -> Option<Effect> {
        let at = self
            .armed
            .iter()
            .position(|f| f.kind == key.kind && f.op == op)?;
        Some(self.armed.remove(at).effect)
    }

    /// The next resource version.
    fn version(&mut self) -> ResourceVersion {
        self.writes += 1;
        ResourceVersion::from(self.writes)
    }

    /// The result of a write that applied, after `fault`.
    fn applied(fault: Option<Effect>, object: Object) -> Performed {
        match fault {
            Some(Effect::Crash) => Performed::Crashed,
            Some(Effect::LostAck) => Performed::Result(StoreResult::Uncertain),
            None => Performed::Result(StoreResult::Object(Box::new(object))),
        }
    }
}

impl Driver for MemDriver {
    fn perform(&mut self, op: StoreOp) -> Performed {
        let result = match op {
            StoreOp::Get { key } if self.down.contains(&key.kind) => StoreResult::Unavailable,
            StoreOp::Get { key } => self.objects.get(&key).map_or(StoreResult::NotFound, |o| {
                StoreResult::Object(Box::new(o.clone()))
            }),
            StoreOp::Create { key, .. } | StoreOp::UpdateStatus { key, .. }
                if self.down.contains(&key.kind) =>
            {
                StoreResult::Uncertain
            }
            StoreOp::Create { key, .. } if self.objects.contains_key(&key) => {
                StoreResult::AlreadyExists
            }
            StoreOp::Create { key, spec, origin } => {
                let fault = self.fault(&key, OpKind::Create);
                let resource_version = self.version();
                let object = Object {
                    uid: parse(&format!("uid-{}", self.writes)),
                    key: key.clone(),
                    resource_version,
                    origin,
                    spec,
                    status: None,
                };
                self.objects.insert(key, object.clone());
                return Self::applied(fault, object);
            }
            StoreOp::UpdateStatus {
                key,
                uid,
                resource_version,
                status,
            } => match self.objects.get(&key) {
                None => StoreResult::NotFound,
                Some(o) if o.uid != uid || o.resource_version != resource_version => {
                    StoreResult::Conflict
                }
                Some(_) => {
                    let fault = self.fault(&key, OpKind::UpdateStatus);
                    let version = self.version();
                    let Some(object) = self.objects.get_mut(&key) else {
                        return Performed::Result(StoreResult::NotFound);
                    };
                    object.status = Some(*status);
                    object.resource_version = version;
                    return Self::applied(fault, object.clone());
                }
            },
            StoreOp::List { kind, .. } if self.down.contains(&kind) => StoreResult::Unavailable,
            StoreOp::List { kind, namespace } => StoreResult::Listed {
                items: self
                    .objects
                    .values()
                    .filter(|o| o.key.kind == kind && o.key.namespace == namespace)
                    .cloned()
                    .collect(),
                resource_version: ResourceVersion::from(self.writes),
            },
            StoreOp::Watch { .. } => StoreResult::Expired,
        };
        Performed::Result(result)
    }

    fn arm(&mut self, fault: Fault) {
        self.armed.push(fault);
    }

    fn outage(&mut self, kind: &Kind, down: bool) {
        if down {
            self.down.insert(kind.clone());
        } else {
            self.down.remove(kind);
        }
    }
}

/// The namespace of every object a scenario names, parsed.
pub(crate) fn namespace() -> Namespace {
    parse(NAMESPACE)
}

/// Writes `status` on `object` as read, conditioned on its UID and resource version: a raw
/// status write that bypasses every protocol.
pub(crate) fn write_status(driver: &mut dyn Driver, object: &Object, status: Status) {
    let op = StoreOp::UpdateStatus {
        key: object.key.clone(),
        uid: object.uid.clone(),
        resource_version: object.resource_version.clone(),
        status: Box::new(status),
    };
    match driver.perform(op) {
        Performed::Result(StoreResult::Object(_)) => {}
        other => panic!("writing {} gave {other:?}", object.key),
    }
}

/// Clears the pending slot of `command` on `target` with the reconciliation-only CAS.
pub(crate) fn clear(
    driver: &mut dyn Driver,
    target: &(ObjectKey, Uid),
    command: &str,
) -> ClearOutcome {
    let mut protocol = ClearSlot::new(target.0.clone(), target.1.clone(), parse(command));
    run(driver, &mut protocol).done()
}
