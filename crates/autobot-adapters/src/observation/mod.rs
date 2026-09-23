//! The forge and CI observation source: how facts from a forge or a CI service reach AutoBot as
//! authenticated observations (`docs/design/AUTOBOT-TRUST-MODEL.md` §Not trusted).
//!
//! A source delivers [`Observation`]s through [`ObservationSource::poll`] and reports the
//! current state of every remote object through [`ObservationSource::relist`], which recovers
//! what a lost watch missed. Delivery may duplicate, delay and reorder; the controller that
//! consumes observations deduplicates them by event id and semantic key, persists them before
//! acknowledging them, and turns them into state only through its own commits. Freshness is
//! the remote generation: the observation of an object with the highest generation is its
//! current state.
//!
//! Choices this module makes where the design is open:
//!
//! - One observation describes one remote object at one remote generation: a pull request's
//!   heads and protection, one CI run, or one piece of forge text.
//! - A CI result names the head, workflow definition, environment and provider run it is bound
//!   to; a result for an old head is delivered with that head and satisfies nothing current.
//! - Forge text keeps its authenticated actor and stays untrusted content: it carries no
//!   authority until a controller has validated the actor's permissions.
//! - A provider answer that fails authentication is dropped whole: the poll that receives it
//!   answers [`SourceError::Unauthenticated`], nothing it carries is delivered or listed, and
//!   authentic answers before and after it are delivered as usual.
//! - A provider that rate limits a poll or relist answers [`SourceError::RateLimited`] with the
//!   back-off it stated, in the [`BackOff`] a rate-limited send carries; it delivers nothing
//!   and loses nothing, like an unavailable one.

mod contract;

pub use contract::{Delivery, ObservationHarness, ObservationRule, run};

use crate::backoff::BackOff;
use crate::text::{Actor, EventId, Head, ProviderName, RemoteIdentity, RunIdentity};
use crate::trust::LabelledText;
use autobot_kernel::types::Digest;

/// The conclusion of a CI run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CiConclusion {
    /// Not finished.
    Pending,
    /// Every job passed.
    Success,
    /// A job failed.
    Failure,
    /// The run was cancelled.
    Cancelled,
}

/// A CI run and what it is bound to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CiRun {
    /// The head the run tested.
    pub head: Head,
    /// The digest of the workflow definition.
    pub workflow_digest: Digest,
    /// The digest of the environment.
    pub environment_digest: Digest,
    /// The provider run.
    pub run: RunIdentity,
    /// The run's conclusion.
    pub conclusion: CiConclusion,
}

/// Text written on a forge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgeText {
    /// The authenticated author.
    pub actor: Actor,
    /// The text, which is untrusted content whoever wrote it.
    pub body: String,
}

impl ForgeText {
    /// The body as untrusted content labelled `label`, ready for a session's context.
    #[must_use]
    pub fn untrusted(&self, label: &str) -> LabelledText {
        LabelledText::untrusted(label, self.body.clone())
    }
}

/// What an observation is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fact {
    /// The object's heads or protection, which the observation's head and protection fields
    /// carry.
    Heads,
    /// A CI run.
    Ci(CiRun),
    /// Forge text.
    Text(ForgeText),
}

/// One authenticated observation of one remote object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    /// The provider.
    pub provider: ProviderName,
    /// The provider's id for this delivery's event.
    pub event_id: EventId,
    /// The semantic key: the same for two deliveries of the same fact, for providers without a
    /// stable event id.
    pub semantic_key: Digest,
    /// The remote object.
    pub object: RemoteIdentity,
    /// The object's remote generation.
    pub generation: u64,
    /// The source head, when the object has one.
    pub source_head: Option<Head>,
    /// The base head, when the object has one.
    pub base_head: Option<Head>,
    /// The digest of the base branch's protection, when the object has one.
    pub protection_digest: Option<Digest>,
    /// When the provider observed it, in seconds since the Unix epoch.
    pub observed_at: u64,
    /// What it is about.
    pub fact: Fact,
}

/// Why a source delivered nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceError {
    /// The provider is unavailable.
    Unavailable,
    /// The provider answered that it is rate limiting the caller; nothing is delivered and the
    /// undelivered observations remain.
    RateLimited {
        /// The back-off the provider stated, as a rate-limited send reports it
        /// ([`SendError::RateLimited`](crate::provider::SendError::RateLimited)).
        back_off: Option<BackOff>,
    },
    /// The provider's answer failed authentication; nothing from it is delivered.
    Unauthenticated,
}

/// A forge or CI observation source.
pub trait ObservationSource {
    /// The observations delivered since the previous poll.
    ///
    /// # Errors
    ///
    /// [`SourceError`] when nothing could be delivered; the undelivered observations remain
    /// for a later poll or relist.
    fn poll(&mut self) -> Result<Vec<Observation>, SourceError>;

    /// The current state: for every remote object the source knows, its observation with the
    /// highest remote generation.
    ///
    /// # Errors
    ///
    /// [`SourceError`] when the state could not be read.
    fn relist(&mut self) -> Result<Vec<Observation>, SourceError>;
}

#[cfg(test)]
mod tests;
