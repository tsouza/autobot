//! `ScriptedAgent`: a [`RuntimeAdapter`] that replays a fixture instead of calling a model.
//!
//! A [`Fixture`] lists what the session does, as [`Step`]s, and how it ends, as an
//! [`Ending`]. [`ScriptedAgent::run`] replays the steps in order through the
//! [`RuntimePort`], so a test drives the broker, the checkpoint path, the finding path and the
//! outbox exactly as a model-backed session would, and then:
//!
//! - ends in a semantic outcome ([`Ending::Complete`], [`Ending::Refuse`], [`Ending::Block`])
//!   after writing its `OutcomeRecord` and `UsageReceipt` to the durable outbox
//!   (KERNEL §8 step 2), or
//! - is interrupted by an injected [`RuntimeFailure`] ([`Ending::Fail`], [`Step::Disconnect`],
//!   [`Fixture::open_failure`]), reported as that category and never as an outcome
//!   (ROLES §4 Continuation, KERNEL §9).
//!
//! The agent keeps the runtime contract the `autobot_adapters::runtime` suite checks: the
//! context reaches the session with its trust classes, every call goes through the broker, a
//! refused call is not asked again, and a checkpoint follows every tool boundary.
//!
//! Choices this module makes where the design is open:
//!
//! - A fault is always the last thing a replay does, so a fault "at step k" is a fixture whose
//!   steps stop at k and whose ending is [`Ending::Fail`].
//! - Before any end but [`RuntimeFailure::Crash`] the agent records a final checkpoint: the
//!   run-end inventory for a semantic outcome, the checkpoint a continuation starts from for a
//!   failure. A crash records nothing more: the process is gone.
//! - An interrupted session writes no records; the session that ends the `AgentRun` writes
//!   them.
//! - A session whose outbox write is refused stops at the first refused record and reports
//!   [`RuntimeFailure::OutboxRefused`].
//! - Budget is one unit per call asked of the broker.
//! - A record's name is its kind and the `AgentRun`, so every retry of the same session end
//!   resolves to the same record; its create key is the digest of its name and input digest.
//! - A replacement attempt's handling of open invocations is the open question #300; the agent
//!   lists them in its checkpoint and leaves resuming them to the next fixture.

use autobot_adapters::artifact::sha256;
use autobot_adapters::runtime::{
    CheckpointClaim, FindingClaim, OutboxRecord, RecordKind, RuntimeAdapter, RuntimeFailure,
    RuntimePort, SessionContext, SessionEnd, ToolCall,
};
use autobot_adapters::trust::LabelledText;
use autobot_kernel::types::Digest;

/// One thing a scripted session does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Asks the broker to make `call`. When the broker answers, the call touched `touches`.
    Tool {
        /// The call.
        call: ToolCall,
        /// The files and concerns the call touches when the broker makes it.
        touches: Vec<String>,
    },
    /// Attempts an effect on `path`, which the capsule does not allow, through the broker. A
    /// broker that refuses it leaves no effect; one that makes it lets `path` be touched, for
    /// the next checkpoint to show.
    OutOfScope {
        /// The call.
        call: ToolCall,
        /// The path the call writes.
        path: String,
    },
    /// A subprocess of the session writes `path` in the workspace, outside the broker; the
    /// next checkpoint lists it as touched.
    SubprocessWrite {
        /// The path written.
        path: String,
    },
    /// Records a checkpoint outside a tool boundary, as the profile's cadence asks.
    Checkpoint,
    /// Reports progress.
    Progress(String),
    /// Reports a finding.
    Finding(FindingClaim),
    /// Asks the broker to make `call`, then loses the stream before the answer: the call stays
    /// open and the session ends in [`RuntimeFailure::StreamDisconnect`].
    Disconnect(ToolCall),
}

/// How a scripted session ends after its steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ending {
    /// The session completes its assignment.
    Complete {
        /// The candidate change's digest, if it made one.
        candidate: Option<Digest>,
    },
    /// The model refuses the assignment.
    Refuse {
        /// The refusal as the model gives it.
        reason: String,
    },
    /// The session reports it is blocked.
    Block {
        /// What it is blocked on.
        reason: String,
    },
    /// The session is interrupted by this failure.
    Fail(RuntimeFailure),
}

/// What a scripted session does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fixture {
    /// A failure that stops the session from starting; `None` lets it start.
    pub open_failure: Option<RuntimeFailure>,
    /// The steps, in order.
    pub steps: Vec<Step>,
    /// How the session ends when every step is replayed.
    pub ending: Ending,
}

impl Fixture {
    /// A fixture that starts, replays `steps`, and ends in `ending`.
    #[must_use]
    pub fn new(steps: Vec<Step>, ending: Ending) -> Self {
        Self {
            open_failure: None,
            steps,
            ending,
        }
    }
}

/// The framing the agent adds before the context.
pub const PREAMBLE_LABEL: &str = "scripted preamble";

/// A runtime adapter that replays a [`Fixture`].
#[derive(Debug, Clone)]
pub struct ScriptedAgent {
    fixture: Fixture,
    session: Option<Session>,
}

impl ScriptedAgent {
    /// An agent whose session follows `fixture`.
    #[must_use]
    pub fn new(fixture: Fixture) -> Self {
        Self {
            fixture,
            session: None,
        }
    }
}

/// The state of an opened session.
#[derive(Debug, Clone)]
struct Session {
    context: SessionContext,
    context_digest: Digest,
    budget_consumed: u64,
    progress: Vec<u8>,
    touched: Vec<String>,
}

impl Session {
    /// Appends `parts` to the progress log.
    fn log(&mut self, parts: &[&[u8]]) {
        frame(&mut self.progress, parts);
    }

    fn touch(&mut self, path: &str) {
        if !self.touched.iter().any(|t| t == path) {
            self.touched.push(path.to_owned());
        }
    }

    /// Asks the broker to make `call`, which touches `touches` if the broker makes it, then
    /// records the checkpoint of that tool boundary.
    fn tool_boundary(&mut self, port: &mut dyn RuntimePort, call: &ToolCall, touches: &[String]) {
        self.budget_consumed += 1;
        let digest = call.request_digest.as_bytes();
        match port.invoke_tool(call) {
            Ok(output) => {
                self.log(&[b"answered", digest, output.content.as_bytes()]);
                for path in touches {
                    self.touch(path);
                }
            }
            Err(refused) => self.log(&[b"refused", digest, refused.reason.as_bytes()]),
        }
        self.checkpoint(port, Vec::new());
    }

    /// Records a checkpoint with `open` still open and clears the touched set.
    fn checkpoint(&mut self, port: &mut dyn RuntimePort, open: Vec<Digest>) {
        port.checkpoint(&CheckpointClaim {
            session_sequence: self.context.session_sequence,
            execution_epoch: self.context.execution_epoch,
            context_digest: self.context_digest,
            scope_digest: self.context.capsule_digest,
            budget_consumed: self.budget_consumed,
            open_tool_invocations: open,
            progress_digest: sha256(&self.progress),
            touched: std::mem::take(&mut self.touched),
        });
    }

    /// The record of `kind` whose input is `input`.
    fn record(&self, kind: RecordKind, input: &[u8]) -> OutboxRecord {
        let prefix = match kind {
            RecordKind::Outcome => "outcome",
            RecordKind::Usage => "usage",
        };
        let name = format!("{prefix}-{}", self.context.agent_run);
        let input_digest = sha256(input);
        let create_key = sha256(format!("{name}\n{input_digest}").as_bytes());
        OutboxRecord {
            kind,
            name,
            input_digest,
            create_key,
        }
    }

    /// The outcome and usage records of a session that ends in `end`.
    fn records(&self, end: &SessionEnd) -> [OutboxRecord; 2] {
        let outcome = match end {
            SessionEnd::Completed { candidate } => format!(
                "completed\n{}",
                candidate.map(|d| d.to_string()).unwrap_or_default()
            ),
            SessionEnd::Refused { reason } => format!("refused\n{reason}"),
            SessionEnd::Blocked { reason } => format!("blocked\n{reason}"),
        };
        let run = format!(
            "{}\n{}\n{}",
            self.context.agent_run,
            self.context.session_sequence,
            sha256(&self.progress)
        );
        [
            self.record(RecordKind::Outcome, format!("{run}\n{outcome}").as_bytes()),
            self.record(
                RecordKind::Usage,
                format!("{run}\n{}", self.budget_consumed).as_bytes(),
            ),
        ]
    }
}

/// Appends each of `parts` to `bytes`, length-prefixed so that no two sequences of parts
/// produce the same bytes.
fn frame(bytes: &mut Vec<u8>, parts: &[&[u8]]) {
    for part in parts {
        bytes.extend_from_slice(&(part.len() as u64).to_be_bytes());
        bytes.extend_from_slice(part);
    }
}

/// The digest of the context as the session receives it.
fn context_digest(presented: &[LabelledText]) -> Digest {
    let mut bytes = Vec::new();
    for item in presented {
        let class = format!("{:?}", item.class);
        frame(
            &mut bytes,
            &[
                class.as_bytes(),
                item.label.as_bytes(),
                item.content.as_bytes(),
            ],
        );
    }
    sha256(&bytes)
}

impl RuntimeAdapter for ScriptedAgent {
    /// Presents the preamble, as untrusted content, followed by every context item unchanged.
    fn open(&mut self, context: &SessionContext) -> Result<Vec<LabelledText>, RuntimeFailure> {
        if let Some(failure) = self.fixture.open_failure {
            return Err(failure);
        }
        let mut presented = vec![LabelledText::untrusted(
            PREAMBLE_LABEL,
            "a scripted session replaying a fixture",
        )];
        presented.extend(context.items.iter().cloned());
        self.session = Some(Session {
            context: context.clone(),
            context_digest: context_digest(&presented),
            budget_consumed: 0,
            progress: Vec::new(),
            touched: Vec::new(),
        });
        Ok(presented)
    }

    /// Replays the fixture. A session that was not opened, or was already run, fails with
    /// [`RuntimeFailure::Crash`].
    fn run(&mut self, port: &mut dyn RuntimePort) -> Result<SessionEnd, RuntimeFailure> {
        let mut s = self.session.take().ok_or(RuntimeFailure::Crash)?;
        for step in &self.fixture.steps {
            match step {
                Step::Tool { call, touches } => s.tool_boundary(port, call, touches),
                Step::OutOfScope { call, path } => {
                    s.tool_boundary(port, call, std::slice::from_ref(path));
                }
                Step::SubprocessWrite { path } => {
                    s.log(&[b"subprocess write", path.as_bytes()]);
                    s.touch(path);
                }
                Step::Checkpoint => s.checkpoint(port, Vec::new()),
                Step::Progress(summary) => {
                    s.log(&[b"progress", summary.as_bytes()]);
                    port.progress(summary);
                }
                Step::Finding(finding) => {
                    s.log(&[b"finding", finding.observed.as_bytes()]);
                    port.finding(finding);
                }
                Step::Disconnect(call) => {
                    s.budget_consumed += 1;
                    // The answer is lost with the stream, whatever the broker returned.
                    let _lost = port.invoke_tool(call);
                    s.log(&[b"disconnected", call.request_digest.as_bytes()]);
                    s.checkpoint(port, vec![call.request_digest]);
                    return Err(RuntimeFailure::StreamDisconnect);
                }
            }
        }
        if self.fixture.ending == Ending::Fail(RuntimeFailure::Crash) {
            return Err(RuntimeFailure::Crash);
        }
        s.checkpoint(port, Vec::new());
        let end = match &self.fixture.ending {
            Ending::Fail(failure) => return Err(*failure),
            Ending::Complete { candidate } => SessionEnd::Completed {
                candidate: *candidate,
            },
            Ending::Refuse { reason } => SessionEnd::Refused {
                reason: reason.clone(),
            },
            Ending::Block { reason } => SessionEnd::Blocked {
                reason: reason.clone(),
            },
        };
        for record in s.records(&end) {
            port.outbox(&record)
                .map_err(|_refused| RuntimeFailure::OutboxRefused)?;
        }
        Ok(end)
    }
}

#[cfg(test)]
mod tests;
