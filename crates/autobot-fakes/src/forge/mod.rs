//! A fake forge: an in-memory forge behind the [`ObservationSource`] trait of
//! `autobot-adapters`, and the delivery [`Feed`] it shares with the fake CI.
//!
//! M0 uses a fake forge and CI and asserts no real provider capability (M0 preamble;
//! forge mirroring, Gate). A [`FakeForge`] holds pull requests and forge text, assigns each
//! remote object its remote generation, and emits one authenticated observation for every
//! change: the provider event id, the semantic key, the remote generation, the source and base
//! heads and the branch-protection digest (forge mirroring, Decision 3). Its [`Feed`] delivers
//! them duplicated, delayed and reordered as the feed's [`Delivery`] says, fails closed while
//! the forge is unreachable or rate-limited, refuses answers that fail authentication, and
//! relists the current state of every object. [`FeedHarness`] runs the observation contract
//! suite of `autobot-adapters` against it.
//!
//! The fake holds no store and writes no aggregate: what it observes reaches AutoBot only as
//! the answers of [`ObservationSource::poll`] and [`ObservationSource::relist`], which a
//! controller turns into state through its own commits (F-30). Forge text keeps its
//! authenticated actor and carries no authority; [`FakeForge::permission`] answers the actor's
//! permission, against which a controller validates a command written in it.
//!
//! Choices this module makes where the design is open:
//!
//! - The provider name is the `fake-forge` of the preset fake provider fixture
//!   ([`ProviderFixture::forge`]), so the forge's observations and its effects name one
//!   provider.
//! - A pull request is one remote object at generation 1 when opened; each push and each
//!   protection change advances its generation by one. A comment is its own remote object at
//!   generation 1 and carries the heads of its pull request when it was written.
//! - The semantic key is SHA-256 over what an observation states: the provider, the object,
//!   its generation, heads and protection, and the fact, each length-prefixed. Two deliveries
//!   of one fact share it, and observations that state different things do not.
//! - An answer is signed with SHA-256 over a key shared by the fake provider and its source,
//!   followed by every field of the observation, each length-prefixed. It detects a forged or
//!   altered answer in tests and claims no cryptographic strength.
//! - A rate-limited call answers [`SourceError::RateLimited`] with the back-off the fixture
//!   throttled it with ([`Feed::throttle`]); it fails closed like an unreachable forge.
//! - Permissions are the three ordered levels of [`Permission`]; an actor with no grant has
//!   none. A permission query reaches the forge like a poll and fails closed the same way.
//!
//! [`ProviderFixture::forge`]: crate::provider::ProviderFixture::forge

mod feed;

pub use feed::{EPOCH, FORGER_KEY, Feed, FeedHarness, FeedSource, SIGNING_KEY};

use crate::provider::ProviderFixture;
use autobot_adapters::observation::{
    Delivery, Fact, ForgeText, Observation, ObservationSource, SourceError,
};
use autobot_adapters::text::{Actor, EmptyText, Head, ProviderName, RemoteIdentity};
use autobot_kernel::types::Digest;
use std::collections::BTreeMap;
use std::fmt;

/// What an actor may do on the fake forge, lowest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Permission {
    /// Read the repository and write comments.
    Read,
    /// Also push, label and review.
    Write,
    /// Also change protection and merge.
    Admin,
}

/// Why the fake forge refused a change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForgeError {
    /// No pull request has this remote identity.
    UnknownPull(RemoteIdentity),
    /// A generated identifier was empty; never in practice.
    Empty(EmptyText),
}

impl fmt::Display for ForgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownPull(pull) => write!(f, "no pull request {pull}"),
            Self::Empty(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for ForgeError {}

impl From<EmptyText> for ForgeError {
    fn from(e: EmptyText) -> Self {
        Self::Empty(e)
    }
}

/// One pull request as the forge holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Pull {
    source: Head,
    base: Head,
    protection: Digest,
    generation: u64,
}

/// An in-memory forge and its observation source.
#[derive(Debug, Clone)]
pub struct FakeForge {
    feed: Feed,
    pulls: BTreeMap<RemoteIdentity, Pull>,
    permissions: BTreeMap<Actor, Permission>,
    /// How many remote identities the forge has issued.
    issued: u64,
}

impl FakeForge {
    /// An empty forge whose feed delivers as `delivery` says.
    ///
    /// # Errors
    ///
    /// Never in practice: see [`FeedSource::provider`].
    pub fn new(delivery: Delivery) -> Result<Self, EmptyText> {
        Ok(Self::from_feed(Feed::new(Self::provider()?, delivery)))
    }

    /// Opens a pull request from `source` onto `base` under the base's `protection` digest and
    /// returns its remote identity.
    ///
    /// # Errors
    ///
    /// Never in practice: see [`ForgeError::Empty`].
    pub fn open(
        &mut self,
        source: Head,
        base: Head,
        protection: Digest,
    ) -> Result<RemoteIdentity, ForgeError> {
        let pull = self.issue("pull")?;
        self.pulls.insert(
            pull.clone(),
            Pull {
                source,
                base,
                protection,
                generation: 1,
            },
        );
        self.observe_pull(&pull)?;
        Ok(pull)
    }

    /// Moves the source head of `pull` to `head`, at the next generation.
    ///
    /// # Errors
    ///
    /// [`ForgeError::UnknownPull`] if the forge holds no such pull request.
    pub fn push(&mut self, pull: &RemoteIdentity, head: Head) -> Result<(), ForgeError> {
        self.change(pull, |p| p.source = head)
    }

    /// Changes the protection digest of the base of `pull`, at the next generation.
    ///
    /// # Errors
    ///
    /// [`ForgeError::UnknownPull`] if the forge holds no such pull request.
    pub fn protect(&mut self, pull: &RemoteIdentity, protection: Digest) -> Result<(), ForgeError> {
        self.change(pull, |p| p.protection = protection)
    }

    /// Writes `body` on `pull` as the authenticated `actor`, whatever its permission, and
    /// returns the comment's remote identity.
    ///
    /// # Errors
    ///
    /// [`ForgeError::UnknownPull`] if the forge holds no such pull request.
    pub fn comment(
        &mut self,
        pull: &RemoteIdentity,
        actor: Actor,
        body: impl Into<String>,
    ) -> Result<RemoteIdentity, ForgeError> {
        let p = self.get(pull)?.clone();
        let comment = self.issue("comment")?;
        let fact = Fact::Text(ForgeText {
            actor,
            body: body.into(),
        });
        self.feed
            .observe(&comment, 1, Some(p.source), Some(p.base), None, fact)?;
        Ok(comment)
    }

    /// Gives `actor` the permission `permission`, replacing any earlier grant.
    pub fn grant(&mut self, actor: Actor, permission: Permission) {
        self.permissions.insert(actor, permission);
    }

    /// Takes every permission from `actor`.
    pub fn revoke(&mut self, actor: &Actor) {
        self.permissions.remove(actor);
    }

    /// The permission `actor` holds now, or `None` if it holds none.
    ///
    /// # Errors
    ///
    /// [`SourceError::Unavailable`] while the forge is unreachable and
    /// [`SourceError::RateLimited`] while it is rate-limited.
    pub fn permission(&mut self, actor: &Actor) -> Result<Option<Permission>, SourceError> {
        self.feed.reach()?;
        Ok(self.permissions.get(actor).copied())
    }

    fn get(&self, pull: &RemoteIdentity) -> Result<&Pull, ForgeError> {
        self.pulls
            .get(pull)
            .ok_or_else(|| ForgeError::UnknownPull(pull.clone()))
    }

    fn issue(&mut self, kind: &str) -> Result<RemoteIdentity, EmptyText> {
        self.issued += 1;
        RemoteIdentity::new(format!("{}/{kind}/{}", self.feed.provider(), self.issued))
    }

    fn change(
        &mut self,
        pull: &RemoteIdentity,
        edit: impl FnOnce(&mut Pull),
    ) -> Result<(), ForgeError> {
        let p = self
            .pulls
            .get_mut(pull)
            .ok_or_else(|| ForgeError::UnknownPull(pull.clone()))?;
        edit(p);
        p.generation += 1;
        self.observe_pull(pull)
    }

    /// Emits the current state of `pull`.
    fn observe_pull(&mut self, pull: &RemoteIdentity) -> Result<(), ForgeError> {
        let p = self.get(pull)?.clone();
        self.feed.observe(
            pull,
            p.generation,
            Some(p.source),
            Some(p.base),
            Some(p.protection),
            Fact::Heads,
        )?;
        Ok(())
    }
}

impl FeedSource for FakeForge {
    fn provider() -> Result<ProviderName, EmptyText> {
        Ok(ProviderFixture::forge()?.provider)
    }

    fn from_feed(feed: Feed) -> Self {
        Self {
            feed,
            pulls: BTreeMap::new(),
            permissions: BTreeMap::new(),
            issued: 0,
        }
    }

    fn feed(&mut self) -> &mut Feed {
        &mut self.feed
    }
}

impl ObservationSource for FakeForge {
    fn poll(&mut self) -> Result<Vec<Observation>, SourceError> {
        self.feed.poll()
    }

    fn relist(&mut self) -> Result<Vec<Observation>, SourceError> {
        self.feed.relist()
    }
}

#[cfg(test)]
mod tests;
