//! A fake CI service: in-memory CI runs behind the [`ObservationSource`] trait of
//! `autobot-adapters`, delivered through the same [`Feed`] as the fake forge.
//!
//! A [`FakeCi`] run is bound to the head it tests, the digest of its workflow definition, the
//! digest of its environment and its provider run identity (forge mirroring, Decision 7).
//! Every change of a run is one authenticated observation of the run as a remote object at its
//! remote generation, carrying that binding. A result that arrives late, for a head the pull
//! request has since left, is delivered with the head it tested: the fake never rebinds a
//! result, so a consumer that compares the result's head and generation with the current state
//! sees that it satisfies nothing current (G-EVIDENCE). [`FeedHarness`] runs the observation
//! contract suite of `autobot-adapters` against it.
//!
//! Choices this module makes where the design is open:
//!
//! - The provider name is the `fake-ci` of the preset fake provider fixture
//!   ([`ProviderFixture::ci`]).
//! - A run is started [`CiConclusion::Pending`] at generation 1 and concluded once, at
//!   generation 2; its remote identity is also its provider run identity. A run observation
//!   carries the tested head as its source head and no base head or protection digest.
//! - Delivery, authentication and rate limiting are the feed's, as the fake forge documents:
//!   a throttled poll or relist answers [`SourceError::RateLimited`] with its back-off.
//!
//! [`FeedHarness`]: crate::forge::FeedHarness
//! [`ProviderFixture::ci`]: crate::provider::ProviderFixture::ci

use crate::forge::{Feed, FeedSource};
use crate::provider::ProviderFixture;
use autobot_adapters::observation::{
    CiConclusion, CiRun, Delivery, Fact, Observation, ObservationSource, SourceError,
};
use autobot_adapters::text::{EmptyText, Head, ProviderName, RemoteIdentity, RunIdentity};
use autobot_kernel::types::Digest;
use std::collections::BTreeMap;
use std::fmt;

/// Why the fake CI refused a change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CiError {
    /// No run has this remote identity.
    UnknownRun(RemoteIdentity),
    /// The run is already concluded.
    Concluded(RemoteIdentity),
    /// A run is concluded [`CiConclusion::Pending`], which is no conclusion.
    NotAConclusion,
    /// A generated identifier was empty; never in practice.
    Empty(EmptyText),
}

impl fmt::Display for CiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownRun(run) => write!(f, "no run {run}"),
            Self::Concluded(run) => write!(f, "run {run} is already concluded"),
            Self::NotAConclusion => f.write_str("pending is no conclusion"),
            Self::Empty(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for CiError {}

impl From<EmptyText> for CiError {
    fn from(e: EmptyText) -> Self {
        Self::Empty(e)
    }
}

/// An in-memory CI service and its observation source.
#[derive(Debug, Clone)]
pub struct FakeCi {
    feed: Feed,
    runs: BTreeMap<RemoteIdentity, CiRun>,
}

impl FakeCi {
    /// A CI service with no run, whose feed delivers as `delivery` says.
    ///
    /// # Errors
    ///
    /// Never in practice: see [`FeedSource::provider`].
    pub fn new(delivery: Delivery) -> Result<Self, EmptyText> {
        Ok(Self::from_feed(Feed::new(Self::provider()?, delivery)))
    }

    /// Starts a run of the workflow `workflow_digest` in the environment `environment_digest`
    /// on `head`, and returns its remote identity.
    ///
    /// # Errors
    ///
    /// Never in practice: see [`CiError::Empty`].
    pub fn start(
        &mut self,
        head: Head,
        workflow_digest: Digest,
        environment_digest: Digest,
    ) -> Result<RemoteIdentity, CiError> {
        let n = self.runs.len() + 1;
        let object = RemoteIdentity::new(format!("{}/run/{n}", self.feed.provider()))?;
        let run = CiRun {
            head,
            workflow_digest,
            environment_digest,
            run: RunIdentity::new(object.as_str())?,
            conclusion: CiConclusion::Pending,
        };
        self.runs.insert(object.clone(), run.clone());
        self.observe(&object, 1, run)?;
        Ok(object)
    }

    /// Concludes the pending run `run` as `conclusion`, at generation 2.
    ///
    /// # Errors
    ///
    /// [`CiError::UnknownRun`] if no run has that identity, [`CiError::Concluded`] if it is
    /// already concluded and [`CiError::NotAConclusion`] if `conclusion` is pending.
    pub fn conclude(
        &mut self,
        run: &RemoteIdentity,
        conclusion: CiConclusion,
    ) -> Result<(), CiError> {
        if conclusion == CiConclusion::Pending {
            return Err(CiError::NotAConclusion);
        }
        let held = self
            .runs
            .get_mut(run)
            .ok_or_else(|| CiError::UnknownRun(run.clone()))?;
        if held.conclusion != CiConclusion::Pending {
            return Err(CiError::Concluded(run.clone()));
        }
        held.conclusion = conclusion;
        let concluded = held.clone();
        self.observe(run, 2, concluded)
    }

    fn observe(
        &mut self,
        object: &RemoteIdentity,
        generation: u64,
        run: CiRun,
    ) -> Result<(), CiError> {
        let head = run.head.clone();
        self.feed
            .observe(object, generation, Some(head), None, None, Fact::Ci(run))?;
        Ok(())
    }
}

impl FeedSource for FakeCi {
    fn provider() -> Result<ProviderName, EmptyText> {
        Ok(ProviderFixture::ci()?.provider)
    }

    fn from_feed(feed: Feed) -> Self {
        Self {
            feed,
            runs: BTreeMap::new(),
        }
    }

    fn feed(&mut self) -> &mut Feed {
        &mut self.feed
    }
}

impl ObservationSource for FakeCi {
    fn poll(&mut self) -> Result<Vec<Observation>, SourceError> {
        self.feed.poll()
    }

    fn relist(&mut self) -> Result<Vec<Observation>, SourceError> {
        self.feed.relist()
    }
}

#[cfg(test)]
mod tests;
