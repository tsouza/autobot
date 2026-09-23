//! The `RuntimeAdapter`: one agent session of one role, as
//! `docs/design/AUTOBOT-ROLES-AND-RUNTIME.md` §4 states the agent-runtime contract.
//!
//! [`RuntimeAdapter::open`] takes the session's context, whose every item carries its trust
//! class, and returns the context as the session receives it. [`RuntimeAdapter::run`] runs the
//! session to its end. Its only channel out is the [`RuntimePort`] AutoBot hands it: tool calls
//! go to the broker through [`RuntimePort::invoke_tool`], progress, checkpoints and findings
//! become resources through the other port calls, and the session's outcome and usage records
//! go to the durable outbox through [`RuntimePort::outbox`] before the session reports its end
//! (KERNEL §8 step 2).
//!
//! A session ends in a [`SessionEnd`], a semantic outcome whose quality review judges later, or
//! in a [`RuntimeFailure`], a transport or process category that KERNEL §9 answers with a
//! continuation. A refusal is a semantic outcome, [`SessionEnd::Refused`], never a failure.
//!
//! Choices this module makes where the design is open:
//!
//! - The context is a list of [`LabelledText`] items beside the identity fields a checkpoint
//!   repeats. The capsule, charter entries, task obligation, acceptance evidence and the other
//!   ROLES §4 inputs are items labelled [`TrustClass::CanonicalFact`](crate::trust::TrustClass);
//!   repository and forge text are [`TrustClass::UntrustedContent`](crate::trust::TrustClass).
//!   An adapter may add framing of its own, only as untrusted content.
//! - A tool's output reaches the session as untrusted content.
//! - An adapter whose outbox write is refused does not report a [`SessionEnd`].
//! - A checkpoint lists the request digests of the tool invocations still open. A continuation
//!   resumes them (KERNEL §9); how a replacement attempt inherits them is open (#300).

mod contract;

pub use contract::{Ending, RuntimeHarness, RuntimeRule, Script, run};

use crate::text::ToolName;
use crate::trust::LabelledText;
use autobot_kernel::types::{Digest, Uid};
use std::fmt;

/// What a session starts from (ROLES §4, Context in).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionContext {
    /// The `AgentRun`.
    pub agent_run: Uid,
    /// The session's `session_sequence` on the `AgentRun`.
    pub session_sequence: u64,
    /// The `execution_epoch` of the `TaskRun`.
    pub execution_epoch: u64,
    /// The digest of the capsule the session runs under.
    pub capsule_digest: Digest,
    /// The charter digest the capsule carries.
    pub charter_digest: Digest,
    /// The context items, each with its trust class.
    pub items: Vec<LabelledText>,
}

/// One tool call a session asks the broker to make.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    /// The tool.
    pub tool: ToolName,
    /// The digest of the request: the identity of the call across retries and continuations.
    pub request_digest: Digest,
    /// The call's input as the session wrote it.
    pub input: String,
}

/// Why the broker made no call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolRefused {
    /// The broker's reason.
    pub reason: String,
}

/// An `AgentCheckpoint` as a session claims it (KERNEL §9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointClaim {
    /// The session's `session_sequence`.
    pub session_sequence: u64,
    /// The session's `execution_epoch`.
    pub execution_epoch: u64,
    /// The digest of the context the session holds.
    pub context_digest: Digest,
    /// The digest of the capsule the session runs under.
    pub scope_digest: Digest,
    /// Budget consumed so far, in the budget's units.
    pub budget_consumed: u64,
    /// The request digests of the tool invocations still open.
    pub open_tool_invocations: Vec<Digest>,
    /// The digest of the progress made.
    pub progress_digest: Digest,
    /// The files and concerns touched since the previous checkpoint.
    pub touched: Vec<String>,
}

/// A `Finding` as a session reports it: a discovery outside the capsule (ROLES §2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindingClaim {
    /// The observed fact.
    pub observed: String,
    /// The affected scope.
    pub affected_scope: Vec<String>,
}

/// The kinds of canonical record a session produces (KERNEL §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecordKind {
    /// The `OutcomeRecord`.
    Outcome,
    /// The `UsageReceipt`.
    Usage,
}

/// One record written to the durable outbox (KERNEL §8 step 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxRecord {
    /// The record's kind.
    pub kind: RecordKind,
    /// The record's deterministic name.
    pub name: String,
    /// The digest of the record's input.
    pub input_digest: Digest,
    /// The create key the outbox drains it under.
    pub create_key: Digest,
}

/// Why the outbox did not durably store a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxRefused {
    /// The outbox's reason.
    pub reason: String,
}

/// The only channel a session has out of its sandbox.
pub trait RuntimePort {
    /// Asks the broker to make `call`; the output is untrusted content.
    ///
    /// # Errors
    ///
    /// [`ToolRefused`] when the broker refused the call.
    fn invoke_tool(&mut self, call: &ToolCall) -> Result<LabelledText, ToolRefused>;

    /// Records a checkpoint.
    fn checkpoint(&mut self, checkpoint: &CheckpointClaim);

    /// Reports progress.
    fn progress(&mut self, summary: &str);

    /// Reports a finding.
    fn finding(&mut self, finding: &FindingClaim);

    /// Writes `record` to the durable outbox.
    ///
    /// # Errors
    ///
    /// [`OutboxRefused`] when the record was not durably stored.
    fn outbox(&mut self, record: &OutboxRecord) -> Result<(), OutboxRefused>;
}

/// How a session ended: a semantic outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEnd {
    /// The session finished its assignment, with the digest of its candidate change when it
    /// made one.
    Completed {
        /// The candidate change's digest.
        candidate: Option<Digest>,
    },
    /// The model refused the assignment.
    Refused {
        /// The refusal as the model gave it, untrusted.
        reason: String,
    },
    /// The session reports it cannot go on without a decision it may not take.
    Blocked {
        /// What it is blocked on, untrusted.
        reason: String,
    },
}

/// A failure category ROLES §4 keeps apart from model quality; each is answered by a
/// continuation (KERNEL §9), never counted as a result of the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuntimeFailure {
    /// The model provider is unavailable.
    ProviderOutage,
    /// The model provider is rate limiting the session.
    RateLimited,
    /// The session's context window is exhausted.
    ContextExhausted,
    /// The session itself expired or ran out.
    SessionExhausted,
    /// The stream disconnected mid-answer.
    StreamDisconnect,
    /// The model's output could not be parsed.
    MalformedOutput,
    /// The session's process crashed.
    Crash,
}

impl RuntimeFailure {
    /// Every category.
    pub const ALL: [Self; 7] = [
        Self::ProviderOutage,
        Self::RateLimited,
        Self::ContextExhausted,
        Self::SessionExhausted,
        Self::StreamDisconnect,
        Self::MalformedOutput,
        Self::Crash,
    ];
}

impl fmt::Display for RuntimeFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ProviderOutage => "provider outage",
            Self::RateLimited => "rate limited",
            Self::ContextExhausted => "context exhausted",
            Self::SessionExhausted => "session exhausted",
            Self::StreamDisconnect => "stream disconnect",
            Self::MalformedOutput => "malformed output",
            Self::Crash => "crash",
        })
    }
}

impl std::error::Error for RuntimeFailure {}

/// A runtime adapter: the only runtime contract (ROLES §4).
pub trait RuntimeAdapter {
    /// Starts the session from `context` and returns the context as the session receives it:
    /// every item of `context`, in order and with its trust class, and any framing of the
    /// adapter's own as untrusted content.
    ///
    /// # Errors
    ///
    /// The [`RuntimeFailure`] that stopped the session from starting.
    fn open(&mut self, context: &SessionContext) -> Result<Vec<LabelledText>, RuntimeFailure>;

    /// Runs the opened session to its end, reporting only through `port`.
    ///
    /// # Errors
    ///
    /// The [`RuntimeFailure`] that interrupted the session.
    fn run(&mut self, port: &mut dyn RuntimePort) -> Result<SessionEnd, RuntimeFailure>;
}

#[cfg(test)]
mod tests;
