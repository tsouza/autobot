//! The store side of the scenarios: running protocols on a [`Driver`], resolving ports, the
//! store faults a scenario positions ([`OnWrite`], [`Outage`]), and helpers that build
//! aggregates and lane commits from the kernel's public store types.

use autobot_fakes::store::Execution;
use autobot_kernel::digest::digest;
use autobot_kernel::profile::{ControlRing, Profile};
use autobot_kernel::status::StatusEnvelope;
use autobot_kernel::store::conformance::Fault;
use autobot_kernel::store::{
    Change, ClearOutcome, ClearSlot, Commit, CommitOutcome, CommitRequest, ControlChange, Create,
    CreateOutcome, DomainChange, EventFields, GuardRefusal, Initialize, InitializeOutcome, Kind,
    Object, ObjectKey, OpKind, Origin, Pin, Protocol, Status, StoreOp, StoreResult,
};
use autobot_kernel::types::{
    ControlRevision, LaneRevision, Namespace, ObjectRef, Principal, StateRevision, Uid,
};
use autobot_testkit::harness::{self, Bounds, Driver, Run};
use autobot_testkit::registry::{self, Port};
use std::fmt::Debug;
use std::num::NonZeroU32;
use std::str::FromStr;

/// The M0 profile, the source of the ring and buffer bounds the scenarios run under.
const M0: &str = include_str!("../../../../../profiles/m0.toml");

/// The namespace of every object a scenario names.
pub(crate) const NAMESPACE: &str = "g-commit";

/// The kind of the aggregates the scenarios commit on.
pub(crate) const AGGREGATE: &str = "CommitProbe";

/// The group's liveness bounds: a protocol that performs this many store operations without
/// finishing counts as stalled, as a repair does while the receipt store is down. No scenario
/// polls a clock, so the tick bound is zero.
const BOUNDS: Bounds = Bounds {
    steps: 256,
    ticks: 0,
};

/// Parses `text` into `T`, for the fixed identifiers of a scenario.
pub(crate) fn parse<T: FromStr>(text: &str) -> T
where
    T::Err: Debug,
{
    text.parse()
        .unwrap_or_else(|e| panic!("`{text}` does not parse: {e:?}"))
}

/// The implementation of port `P` from the installed testkit registry; an unregistered port
/// fails the scenario with the task it awaits.
pub(crate) fn port<P: Port>() -> Box<P::Object> {
    registry::resolve::<P>().unwrap_or_else(|e| panic!("{e}"))
}

/// Drives `protocol` against `driver` within the group's bounds.
pub(crate) fn attempt<P: Protocol + ?Sized>(
    driver: &mut dyn Driver,
    protocol: &mut P,
) -> Run<P::Outcome> {
    harness::run(driver, protocol, &BOUNDS)
}

/// Drives `protocol` against `driver` to its outcome, which the scenario requires it to reach.
pub(crate) fn run<P: Protocol + ?Sized>(driver: &mut dyn Driver, protocol: &mut P) -> P::Outcome
where
    P::Outcome: Debug,
{
    match attempt(driver, protocol) {
        Run::Done(outcome) => outcome,
        other => panic!("the protocol did not finish: {other:?}"),
    }
}

/// The M0 profile.
pub(crate) fn profile() -> Profile {
    Profile::parse(M0).unwrap_or_else(|e| panic!("the M0 profile does not parse: {e:?}"))
}

/// The M0 control-receipt ring bounds.
pub(crate) fn ring() -> ControlRing {
    profile().values().control_ring
}

/// The namespace of every object a scenario names, parsed.
pub(crate) fn namespace() -> Namespace {
    parse(NAMESPACE)
}

/// The key of the object `name` of `kind` in [`NAMESPACE`].
pub(crate) fn key(kind: &str, name: &str) -> ObjectKey {
    ObjectKey {
        kind: parse(kind),
        namespace: namespace(),
        name: parse(name),
    }
}

/// A driver that arms one store [`Fault`] on its inner driver as the first write of `op` to
/// an object of `kind` is sent, so the fault lands on that write whatever writes to other
/// kinds come before it.
pub(crate) struct OnWrite<'a> {
    inner: &'a mut dyn Driver,
    kind: Kind,
    op: OpKind,
    fault: Option<Fault>,
}

impl<'a> OnWrite<'a> {
    /// `inner`, with `fault` armed as the first write of `op` to `kind` is sent.
    pub(crate) fn new(inner: &'a mut dyn Driver, kind: &str, op: OpKind, fault: Fault) -> Self {
        Self {
            inner,
            kind: parse(kind),
            op,
            fault: Some(fault),
        }
    }

    /// `inner`, with the process killed right after the first write of `op` to `kind` applies.
    pub(crate) fn crash(inner: &'a mut dyn Driver, kind: &str, op: OpKind) -> Self {
        Self::new(
            inner,
            kind,
            op,
            Fault::Crash {
                after_writes: NonZeroU32::MIN,
            },
        )
    }
}

impl Driver for OnWrite<'_> {
    fn perform(&mut self, op: StoreOp) -> Execution {
        if op.kind() == self.op
            && target_kind(&op) == self.kind
            && let Some(fault) = self.fault.take()
        {
            self.inner.arm(fault);
        }
        self.inner.perform(op)
    }

    fn arm(&mut self, fault: Fault) {
        self.inner.arm(fault);
    }
}

/// A driver on which every object of the kinds `down` is unreachable: reads of them are
/// `Unavailable`, and writes to them are `UNCERTAIN` and never apply. Other kinds pass through
/// to the inner driver.
pub(crate) struct Outage<'a> {
    inner: &'a mut dyn Driver,
    down: Vec<Kind>,
}

impl<'a> Outage<'a> {
    /// `inner`, with the kinds `down` unreachable.
    pub(crate) fn new(inner: &'a mut dyn Driver, down: &[&str]) -> Self {
        Self {
            inner,
            down: down.iter().map(|k| parse(k)).collect(),
        }
    }
}

impl Driver for Outage<'_> {
    fn perform(&mut self, op: StoreOp) -> Execution {
        if !self.down.contains(&target_kind(&op)) {
            return self.inner.perform(op);
        }
        if op.kind().is_write() {
            Execution::Result(StoreResult::Uncertain)
        } else {
            Execution::Result(StoreResult::Unavailable)
        }
    }

    fn arm(&mut self, fault: Fault) {
        self.inner.arm(fault);
    }
}

/// The kind of the objects `op` reads or writes.
fn target_kind(op: &StoreOp) -> Kind {
    match op {
        StoreOp::Get { key } | StoreOp::Create { key, .. } | StoreOp::UpdateStatus { key, .. } => {
            key.kind.clone()
        }
        StoreOp::List { kind, .. } | StoreOp::Watch { kind, .. } => kind.clone(),
        // No public accessor names a StoreOp target: an unnamed operation reads its Debug form.
        #[allow(unreachable_patterns)]
        other => debug_kind(other),
    }
}

/// The first `kind` field of `op`'s `Debug` form.
fn debug_kind(op: &StoreOp) -> Kind {
    let text = format!("{op:?}");
    let kind = text
        .split_once("kind: Kind(\"")
        .and_then(|(_, rest)| rest.split_once('"'))
        .map_or_else(|| panic!("{text} names no kind"), |(kind, _)| kind);
    parse(kind)
}

/// Reads `key` with a linearizable `Get`.
pub(crate) fn read(driver: &mut dyn Driver, key: &ObjectKey) -> Option<Object> {
    match driver.perform(StoreOp::Get { key: key.clone() }) {
        Execution::Result(StoreResult::Object(object)) => Some(*object),
        Execution::Result(StoreResult::NotFound) => None,
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
        namespace: namespace(),
    };
    match driver.perform(op) {
        Execution::Result(StoreResult::Listed { items, .. }) => items,
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
    let object = match run(driver, &mut Create::new(key.clone(), String::new(), origin)) {
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
    assert_eq!(initialized, InitializeOutcome::Initialized);
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
            namespace: namespace(),
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

/// Runs the lane commit of `command` on `target`, pinned at `pin`, applying `change`, to its
/// outcome.
pub(crate) fn commit(
    driver: &mut dyn Driver,
    target: &(ObjectKey, Uid),
    command: &str,
    pin: Pin,
    change: Change,
) -> CommitOutcome {
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
        Execution::Result(StoreResult::Object(_)) => {}
        other => panic!("writing {} gave {other:?}", object.key),
    }
}

/// Clears the pending slot of `command` on `target` with the reconciliation-only CAS.
pub(crate) fn clear(
    driver: &mut dyn Driver,
    target: &(ObjectKey, Uid),
    command: &str,
) -> ClearOutcome {
    run(
        driver,
        &mut ClearSlot::new(target.0.clone(), target.1.clone(), parse(command)),
    )
}
