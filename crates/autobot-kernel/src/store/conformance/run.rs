//! The sans-I/O runner of one conformance script.

use super::{CheckStep, CommitStep, Fault, KIND, NAMESPACE, Script, ScriptStep};
use crate::profile::ControlRing;
use crate::status::{AuditEnvelope, PendingCommitState, StatusEnvelope};
use crate::store::{
    Change, ClearOutcome, ClearSlot, Commit, CommitOutcome, CommitRequest, ControlChange, Create,
    CreateOutcome, DomainChange, GuardRefusal, Initialize, InitializeOutcome, Kind, Missing,
    Object, ObjectKey, Origin, Pin, Protocol, ProtocolError, Status, Step, StoreOp, StoreResult,
    Transition, Triggers, fields_digest,
};
use crate::types::{
    ControlRevision, Lane, LaneRevision, Namespace, ObjectName, ObjectRef, Principal,
    StateRevision, Uid,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::num::NonZeroU32;

/// What the runner wants from its driver next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Perform this operation and pass its result to [`ScriptRun::resume`].
    Op(StoreOp),
    /// Arm this fault; no result is expected.
    Arm(Fault),
    /// The script is finished: passed, or failed at a step.
    Done(Result<(), Failure>),
}

/// A script step that did not behave as the script expects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// The script's name.
    pub script: String,
    /// The step's position, from zero.
    pub step: usize,
    /// What differed.
    pub message: String,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} step {}: {}", self.script, self.step, self.message)
    }
}

impl std::error::Error for Failure {}

/// The runner of one script; [`super`] describes the loop a driver runs.
pub struct ScriptRun<'s> {
    script: &'s Script,
    ring: ControlRing,
    index: usize,
    frame: Option<Frame>,
    interleaved: bool,
    uids: BTreeMap<String, Uid>,
    labels: BTreeMap<String, Object>,
    triggers: Option<Triggers>,
    armed: Vec<(Fault, usize)>,
    failure: Option<Failure>,
}

/// The running part of the current step.
struct Frame {
    main: Machine,
    interleave: Option<Machine>,
    running_interleave: bool,
    held: Option<StoreResult>,
}

/// One running protocol and what its step expects of it.
enum Machine {
    Create {
        object: String,
        protocol: Create,
        expect: String,
    },
    Initialize {
        protocol: Initialize,
        expect: String,
    },
    Commit {
        protocol: Commit<ScriptTransition>,
        expect: String,
    },
    Clear {
        protocol: ClearSlot,
        expect: String,
    },
    Single {
        op: StoreOp,
        result: Option<StoreResult>,
        then: Then,
    },
    Watch {
        expect: BTreeSet<String>,
        done: bool,
    },
}

/// What a single-operation step does with its result.
enum Then {
    Label(String),
    Expect(String),
    Check(CheckStep),
}

impl<'s> ScriptRun<'s> {
    /// The runner of `script`, whose control commits append within `ring`.
    #[must_use]
    pub fn new(script: &'s Script, ring: ControlRing) -> Self {
        Self {
            script,
            ring,
            index: 0,
            frame: None,
            interleaved: false,
            uids: BTreeMap::new(),
            labels: BTreeMap::new(),
            triggers: None,
            armed: Vec::new(),
            failure: None,
        }
    }

    /// The next action.
    pub fn step(&mut self) -> Action {
        loop {
            if let Some(failure) = &self.failure {
                return Action::Done(Err(failure.clone()));
            }
            let Some(step) = self.script.steps.get(self.index) else {
                if let Some((fault, _)) = self.armed.first() {
                    let message = format!("{fault:?} was armed with no step left to fire in");
                    self.fail(message);
                    continue;
                }
                return Action::Done(Ok(()));
            };
            if self.frame.is_none() {
                if let ScriptStep::Inject(inject) = step {
                    let target = self
                        .script
                        .steps
                        .iter()
                        .skip(self.index)
                        .position(|s| !matches!(s, ScriptStep::Inject(_)))
                        .map_or(self.script.steps.len(), |offset| self.index + offset);
                    self.armed.push((inject.fault, target));
                    self.index += 1;
                    return Action::Arm(inject.fault);
                }
                match self.start(step) {
                    Ok(frame) => self.frame = Some(frame),
                    Err(message) => {
                        self.fail(message);
                        continue;
                    }
                }
            }
            match self.advance() {
                Ok(Some(op)) => return Action::Op(op),
                Ok(None) => {}
                Err(message) => self.fail(message),
            }
        }
    }

    /// Consumes the result of the operation the last [`step`](Self::step) returned. A result
    /// that does not answer that operation fails the script at the current step.
    pub fn resume(&mut self, result: StoreResult) {
        if let Err(e) = self.deliver(result) {
            self.fail(e.to_string());
        }
    }

    /// Passes `result` to the machine waiting for it.
    fn deliver(&mut self, result: StoreResult) -> Result<(), ProtocolError> {
        let Some(frame) = &mut self.frame else {
            return Err(ProtocolError::NotWaiting {
                result: result.name(),
            });
        };
        if frame.running_interleave
            && let Some(machine) = &mut frame.interleave
        {
            return machine.resume(result, &mut self.triggers);
        }
        if frame.interleave.is_some() && matches!(result, StoreResult::Object(_)) {
            frame.held = Some(result);
            frame.running_interleave = true;
            return Ok(());
        }
        frame.main.resume(result, &mut self.triggers)
    }

    /// Records that the driver injected `fault`, which an `inject` step armed. A fault that
    /// fires without being armed fails the current step.
    pub fn fired(&mut self, fault: Fault) {
        match self.armed.iter().position(|(armed, _)| *armed == fault) {
            Some(i) => {
                self.armed.remove(i);
            }
            None => self.fail(format!("{fault:?} fired without being armed")),
        }
    }

    /// Records that the process crashed: the current step starts again with a fresh protocol.
    pub fn crash(&mut self) {
        self.frame = None;
    }

    /// Records a failure of the current step.
    fn fail(&mut self, message: String) {
        self.failure = Some(Failure {
            script: self.script.name.clone(),
            step: self.index,
            message,
        });
    }

    /// Steps the current frame: the next operation, or `None` when the step finished.
    fn advance(&mut self) -> Result<Option<StoreOp>, String> {
        let Some(frame) = &mut self.frame else {
            return Ok(None);
        };
        if frame.running_interleave {
            let Some(machine) = &mut frame.interleave else {
                return Err("interleave lost".to_owned());
            };
            if let Some(op) = machine.step(&mut self.triggers) {
                return Ok(Some(op));
            }
            let machine = frame.interleave.take();
            frame.running_interleave = false;
            let held = frame.held.take();
            self.interleaved = true;
            if let Some(machine) = machine {
                self.finish(machine)
                    .map_err(|m| format!("interleave: {m}"))?;
            }
            let frame = self.frame.as_mut().ok_or("frame lost")?;
            if let Some(held) = held {
                frame
                    .main
                    .resume(held, &mut self.triggers)
                    .map_err(|e| e.to_string())?;
            }
            return Ok(None);
        }
        if let Some(op) = frame.main.step(&mut self.triggers) {
            return Ok(Some(op));
        }
        let frame = self.frame.take().ok_or("frame lost")?;
        if frame.interleave.is_some() {
            return Err(
                "the commit ended before its first read, so its interleave never ran".to_owned(),
            );
        }
        self.finish(frame.main)?;
        if let Some((fault, _)) = self.armed.iter().find(|(_, target)| *target == self.index) {
            return Err(format!("{fault:?} was armed for this step and never fired"));
        }
        self.index += 1;
        self.interleaved = false;
        Ok(None)
    }

    /// Checks a finished machine against its expectation and keeps what it learned.
    fn finish(&mut self, machine: Machine) -> Result<(), String> {
        match machine {
            Machine::Create {
                object,
                mut protocol,
                expect,
            } => {
                let outcome = done(&mut protocol)?;
                if let CreateOutcome::Created(created) = &outcome {
                    self.uids.insert(object, created.uid.clone());
                }
                compare(&expect, create_name(&outcome), &outcome)
            }
            Machine::Initialize {
                mut protocol,
                expect,
            } => {
                let outcome = done(&mut protocol)?;
                compare(&expect, initialize_name(&outcome), &outcome)
            }
            Machine::Commit {
                mut protocol,
                expect,
                ..
            } => {
                let outcome = done(&mut protocol)?;
                compare(&expect, commit_name(&outcome), &outcome)
            }
            Machine::Clear {
                mut protocol,
                expect,
            } => {
                let outcome = done(&mut protocol)?;
                compare(&expect, clear_name(&outcome), &outcome)
            }
            Machine::Single { result, then, .. } => {
                let result = result.ok_or("no result")?;
                match then {
                    Then::Label(label) => match result {
                        StoreResult::Object(object) => {
                            self.labels.insert(label, *object);
                            Ok(())
                        }
                        other => Err(format!("read returned {}", other.name())),
                    },
                    Then::Expect(expect) => {
                        let name = match &result {
                            StoreResult::Object(_) => "updated",
                            StoreResult::Conflict => "conflict",
                            other => other.name(),
                        };
                        compare(&expect, name, &result)
                    }
                    Then::Check(check) => match result {
                        StoreResult::Object(object) => check_object(&check, &object),
                        other => Err(format!("check read returned {}", other.name())),
                    },
                }
            }
            Machine::Watch { expect, .. } => {
                let triggers = self.triggers.as_mut().ok_or("no watch")?;
                let due: BTreeSet<String> = triggers
                    .take_due()
                    .into_iter()
                    .map(|key| key.name.to_string())
                    .collect();
                if due == expect {
                    Ok(())
                } else {
                    Err(format!("due {due:?}, expected {expect:?}"))
                }
            }
        }
    }

    /// The frame that runs `step`.
    fn start(&mut self, step: &ScriptStep) -> Result<Frame, String> {
        let mut interleave = None;
        let main = match step {
            ScriptStep::Create(s) => {
                let receipt = s
                    .receipt
                    .clone()
                    .unwrap_or_else(|| format!("create-{}", s.object));
                let origin = Origin {
                    create_receipt_uid: parse(&receipt)?,
                    input_digest: fields_digest(&s.spec),
                    context_uid: parse("conformance-context")?,
                };
                Machine::Create {
                    object: s.object.clone(),
                    protocol: Create::new(key(&s.object)?, s.spec.clone(), origin),
                    expect: s.expect.clone(),
                }
            }
            ScriptStep::Initialize(s) => {
                let envelope = if s.control_lane {
                    StatusEnvelope::with_control_lane()
                } else {
                    StatusEnvelope::default()
                };
                let status = Status {
                    envelope,
                    domain: s.domain.clone(),
                    control: s.control.clone(),
                };
                Machine::Initialize {
                    protocol: Initialize::new(key(&s.object)?, self.uid(&s.object)?, status),
                    expect: s.expect.clone(),
                }
            }
            ScriptStep::Commit(s) => {
                if let Some(inner) = &s.interleave
                    && !self.interleaved
                {
                    interleave = Some(self.commit(inner)?);
                }
                self.commit(s)?
            }
            ScriptStep::Clear(s) => Machine::Clear {
                protocol: ClearSlot::new(key(&s.object)?, self.uid(&s.object)?, parse(&s.command)?),
                expect: s.expect.clone(),
            },
            ScriptStep::Read(s) => Machine::Single {
                op: StoreOp::Get {
                    key: key(&s.object)?,
                },
                result: None,
                then: Then::Label(s.label.clone()),
            },
            ScriptStep::Write(s) => {
                let read = self
                    .labels
                    .get(&s.from)
                    .ok_or_else(|| format!("no read labelled {}", s.from))?;
                let status = read
                    .status
                    .clone()
                    .ok_or("the labelled read has no status")?;
                Machine::Single {
                    op: StoreOp::UpdateStatus {
                        key: read.key.clone(),
                        uid: match &s.uid {
                            Some(uid) => parse(uid)?,
                            None => read.uid.clone(),
                        },
                        resource_version: read.resource_version.clone(),
                        status: Box::new(status),
                    },
                    result: None,
                    then: Then::Expect(s.expect.clone()),
                }
            }
            ScriptStep::Check(s) => Machine::Single {
                op: StoreOp::Get {
                    key: key(&s.object)?,
                },
                result: None,
                then: Then::Check(s.clone()),
            },
            ScriptStep::Relist(s) | ScriptStep::Poll(s) => {
                let triggers = match &mut self.triggers {
                    Some(triggers) => triggers,
                    None => self.triggers.insert(Triggers::new(
                        parse(KIND)?,
                        parse(NAMESPACE)?,
                        NonZeroU32::MAX,
                    )),
                };
                if matches!(step, ScriptStep::Relist(_)) {
                    triggers.relist();
                }
                Machine::Watch {
                    expect: s.expect.iter().cloned().collect(),
                    done: false,
                }
            }
            ScriptStep::Inject(_) => return Err("inject has no frame".to_owned()),
        };
        Ok(Frame {
            main,
            interleave,
            running_interleave: false,
            held: None,
        })
    }

    /// The machine of a commit step.
    fn commit(&self, s: &CommitStep) -> Result<Machine, String> {
        let pin = match s.pin {
            None if s.lane == Lane::Domain => Pin::Current,
            None => return Err("a control commit pins its revision".to_owned()),
            Some(value) => Pin::Revision(match s.lane {
                Lane::Domain => {
                    LaneRevision::State(StateRevision::new(value).map_err(|e| e.to_string())?)
                }
                Lane::Control => {
                    LaneRevision::Control(ControlRevision::new(value).map_err(|e| e.to_string())?)
                }
            }),
        };
        let request = CommitRequest {
            target: key(&s.object)?,
            uid: self.uid(&s.object)?,
            command_uid: parse(&s.command)?,
            pin,
            ring: self.ring,
            transition: ScriptTransition::new(s)?,
        };
        Ok(Machine::Commit {
            protocol: Commit::new(request),
            expect: s.expect.clone(),
        })
    }

    /// The UID `object` was created with.
    fn uid(&self, object: &str) -> Result<Uid, String> {
        self.uids
            .get(object)
            .cloned()
            .ok_or_else(|| format!("{object} was not created"))
    }
}

impl Machine {
    /// The machine's next operation, or `None` when it is finished.
    fn step(&mut self, triggers: &mut Option<Triggers>) -> Option<StoreOp> {
        match self {
            Self::Create { protocol, .. } => op(protocol),
            Self::Initialize { protocol, .. } => op(protocol),
            Self::Commit { protocol, .. } => op(protocol),
            Self::Clear { protocol, .. } => op(protocol),
            Self::Single { op, result, .. } => result.is_none().then(|| op.clone()),
            Self::Watch { done, .. } => {
                if *done {
                    None
                } else {
                    triggers.as_mut().map(Triggers::step)
                }
            }
        }
    }

    /// Consumes the result of the machine's last operation.
    fn resume(
        &mut self,
        result: StoreResult,
        triggers: &mut Option<Triggers>,
    ) -> Result<(), ProtocolError> {
        match self {
            Self::Create { protocol, .. } => protocol.resume(result),
            Self::Initialize { protocol, .. } => protocol.resume(result),
            Self::Commit { protocol, .. } => protocol.resume(result),
            Self::Clear { protocol, .. } => protocol.resume(result),
            Self::Single { result: slot, .. } => {
                *slot = Some(result);
                Ok(())
            }
            Self::Watch { done, .. } => {
                let again = matches!(result, StoreResult::Expired | StoreResult::Unavailable);
                let Some(triggers) = triggers else {
                    return Err(ProtocolError::NotWaiting {
                        result: result.name(),
                    });
                };
                triggers.resume(result)?;
                *done = !again;
                Ok(())
            }
        }
    }
}

/// The operation `protocol` wants, or `None` when it is finished.
fn op<P: Protocol>(protocol: &mut P) -> Option<StoreOp> {
    match protocol.step() {
        Step::Op(op) => Some(op),
        Step::Done(_) => None,
    }
}

/// The outcome of a finished `protocol`.
fn done<P: Protocol>(protocol: &mut P) -> Result<P::Outcome, String> {
    match protocol.step() {
        Step::Done(outcome) => Ok(outcome),
        Step::Op(_) => Err("protocol not finished".to_owned()),
    }
}

/// Compares an outcome's name with the expected one.
fn compare(expect: &str, name: &str, outcome: &impl fmt::Debug) -> Result<(), String> {
    if expect == name {
        Ok(())
    } else {
        Err(format!("expected {expect}, got {outcome:?}"))
    }
}

/// The script name of a create outcome.
fn create_name(outcome: &CreateOutcome) -> &'static str {
    match outcome {
        CreateOutcome::Created(_) => "created",
        CreateOutcome::Taken(_) => "taken",
        CreateOutcome::Vanished => "vanished",
    }
}

/// The script name of an initialization outcome.
fn initialize_name(outcome: &InitializeOutcome) -> &'static str {
    match outcome {
        InitializeOutcome::Initialized => "initialized",
        InitializeOutcome::AlreadyInitialized => "already_initialized",
        InitializeOutcome::Missing(missing) => missing_name(missing),
    }
}

/// The script name of a commit outcome.
fn commit_name(outcome: &CommitOutcome) -> &'static str {
    match outcome {
        CommitOutcome::Committed { .. } => "committed",
        CommitOutcome::Passed { .. } => "passed",
        CommitOutcome::NotReached { .. } => "not_reached",
        CommitOutcome::Barrier { .. } => "barrier",
        CommitOutcome::Refused { .. } => "refused",
        CommitOutcome::Ring(crate::error::RingError::Full { .. }) => "ring_full",
        CommitOutcome::Ring(_) => "ring_error",
        CommitOutcome::NoControlLane => "no_control_lane",
        CommitOutcome::LaneMismatch => "lane_mismatch",
        CommitOutcome::Overflow => "overflow",
        CommitOutcome::Uninitialized => "uninitialized",
        CommitOutcome::Missing(missing) => missing_name(missing),
    }
}

/// The script name of a clear outcome.
fn clear_name(outcome: &ClearOutcome) -> &'static str {
    match outcome {
        ClearOutcome::Cleared => "cleared",
        ClearOutcome::NotHeld => "not_held",
        ClearOutcome::Uninitialized => "uninitialized",
        ClearOutcome::Missing(missing) => missing_name(missing),
    }
}

/// The script name of a missing target.
fn missing_name(missing: &Missing) -> &'static str {
    match missing {
        Missing::NotFound => "not_found",
        Missing::Replaced { .. } => "replaced",
    }
}

/// Compares every field `check` gives with `object`.
fn check_object(check: &CheckStep, object: &Object) -> Result<(), String> {
    let status = object.status.as_ref();
    let envelope = status.map(|s| &s.envelope);
    let slot = envelope.and_then(|e| e.pending_commit.as_ref());
    let mut differences = Vec::new();
    let mut expect = |field: &str, expected: Option<String>, actual: Option<String>| {
        if let Some(expected) = expected
            && Some(&expected) != actual.as_ref()
        {
            differences.push(format!("{field} is {actual:?}, expected {expected}"));
        }
    };
    expect(
        "state_revision",
        check.state_revision.map(|v| v.to_string()),
        envelope.map(|e| e.state_revision.to_string()),
    );
    expect(
        "control_revision",
        check.control_revision.map(|v| v.to_string()),
        envelope.map(|e| e.control_revision.to_string()),
    );
    expect(
        "commit_sequence",
        check.commit_sequence.map(|v| v.to_string()),
        envelope.map(|e| e.commit_sequence.to_string()),
    );
    expect(
        "domain",
        check.domain.clone(),
        status.map(|s| s.domain.clone()),
    );
    expect(
        "control",
        check.control.clone(),
        status.map(|s| s.control.clone()),
    );
    expect(
        "slot",
        check.slot.clone(),
        Some(
            match slot.map(|s| s.state) {
                None => "none",
                Some(PendingCommitState::Occupied) => "occupied",
                Some(PendingCommitState::Cleared) => "cleared",
                Some(PendingCommitState::Repairing) => "repairing",
            }
            .to_owned(),
        ),
    );
    expect(
        "slot_command",
        check.slot_command.clone(),
        slot.map(|s| s.command_uid.to_string()),
    );
    expect(
        "ring_entries",
        check.ring_entries.map(|v| v.to_string()),
        envelope
            .and_then(|e| e.control_receipt_ring.as_ref())
            .map(|r| r.entries().len().to_string()),
    );
    if differences.is_empty() {
        Ok(())
    } else {
        Err(differences.join("; "))
    }
}

/// The key of the script object `name`.
fn key(name: &str) -> Result<ObjectKey, String> {
    Ok(ObjectKey {
        kind: parse::<Kind>(KIND)?,
        namespace: parse::<Namespace>(NAMESPACE)?,
        name: parse::<ObjectName>(name)?,
    })
}

/// Parses script text into a checked value.
fn parse<T>(text: &str) -> Result<T, String>
where
    T: std::str::FromStr<Err: fmt::Display>,
{
    text.parse().map_err(|e: T::Err| format!("`{text}`: {e}"))
}

/// The transition of a script commit: set the lane's fields, guarded by `require_control`.
struct ScriptTransition {
    change: Change,
    require_control: Option<String>,
}

impl ScriptTransition {
    /// The transition of the commit step `s`.
    fn new(s: &CommitStep) -> Result<Self, String> {
        let command: Uid = parse(&s.command)?;
        let change = match s.lane {
            Lane::Domain => {
                let receipt = format!("receipt-{}", s.command);
                Change::Domain(DomainChange {
                    fields: s.fields.clone(),
                    receipt: ObjectRef {
                        namespace: parse(NAMESPACE)?,
                        name: parse(&receipt)?,
                        uid: parse(&receipt)?,
                    },
                    audit_digest: fields_digest(&s.command),
                    effect_intents: Vec::new(),
                })
            }
            Lane::Control => Change::Control(ControlChange {
                fields: s.fields.clone(),
                principal: parse::<Principal>("conformance")?,
                audit: AuditEnvelope {
                    state_revision: StateRevision::ZERO,
                    source_uid: command.clone(),
                    event_type: "ConformanceControl".to_owned(),
                    causation_id: s.command.clone(),
                    correlation_id: s.command.clone(),
                    schema_version: 1,
                },
            }),
        };
        Ok(Self {
            change,
            require_control: s.require_control.clone(),
        })
    }
}

impl Transition for ScriptTransition {
    fn apply(&self, _: &Object, status: &Status) -> Result<Change, GuardRefusal> {
        match &self.require_control {
            Some(required) if *required != status.control => Err(GuardRefusal {
                guard: "control".to_owned(),
            }),
            _ => Ok(self.change.clone()),
        }
    }
}
