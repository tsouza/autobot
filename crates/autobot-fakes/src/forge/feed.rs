//! The delivery channel between a fake provider and the observation source that polls it.

use crate::hmac_sha256;
use autobot_adapters::backoff::BackOff;
use autobot_adapters::observation::{
    CiConclusion, Delivery, Fact, Observation, ObservationHarness, ObservationSource, SourceError,
};
use autobot_adapters::text::{EmptyText, EventId, Head, ProviderName, RemoteIdentity};
use autobot_kernel::types::Digest;
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, VecDeque};

/// The key a provider signs its authentic answers with and its source verifies them under.
pub const SIGNING_KEY: [u8; 32] = *b"autobot fake feed signing key 01";

/// The key [`Feed::inject_forged`] signs with: an answer under it never verifies.
pub const FORGER_KEY: [u8; 32] = *b"autobot fake feed forger key 001";

/// The `observed_at` of a feed's first observation; each later one is a second later.
pub const EPOCH: u64 = 1_700_000_000;

/// One provider answer: an observation and the signature it arrived with.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Signed {
    observation: Observation,
    signature: [u8; 32],
}

impl Signed {
    fn new(observation: Observation, key: &[u8; 32]) -> Self {
        let signature = sign(key, &observation);
        Self {
            observation,
            signature,
        }
    }

    fn verifies(&self) -> bool {
        sign(&SIGNING_KEY, &self.observation) == self.signature
    }
}

/// A fake provider's outgoing deliveries and the source end that receives them.
///
/// The provider side emits observations; the source side answers
/// [`ObservationSource::poll`] and [`ObservationSource::relist`]. Every delivery is signed with
/// [`SIGNING_KEY`] and verified before anything in it is delivered; a poll whose answer does
/// not verify answers [`SourceError::Unauthenticated`] and delivers nothing of it. A relist
/// answers the provider's current state, which only authentic emissions change.
///
/// Delivery follows the feed's [`Delivery`], deterministically. The `n`-th emitted
/// observation, counting from zero, is held back for `n % (delay_polls + 1)` polls; when
/// `duplicate` is set, every even-numbered one is delivered a second time, held back for the
/// rest of the bound; when `reorder` is set, each poll delivers its batch last emitted first.
/// Polls are counted as [`ObservationHarness`] counts them: every poll made while the provider
/// is reachable and not rate-limited. A poll that receives a forged answer counts but delivers
/// nothing else, and what was due at it is delivered by the next counted poll. So every
/// observation is delivered by the `delay_polls + 1`-th counted poll after it was emitted, plus
/// one more counted poll for each forged answer received in between.
#[derive(Debug, Clone)]
pub struct Feed {
    provider: ProviderName,
    delivery: Delivery,
    available: bool,
    throttled: u32,
    /// The back-off the throttled calls state; read only while `throttled` is not zero.
    back_off: BackOff,
    polls: u32,
    emitted: u64,
    /// Undelivered answers and the poll count at which each is due.
    pending: Vec<(Signed, u32)>,
    forged: VecDeque<Signed>,
    /// For every remote object, its emitted observation with the highest generation.
    current: BTreeMap<RemoteIdentity, Observation>,
}

impl Feed {
    /// An empty, reachable feed of `provider` that delivers as `delivery` says.
    #[must_use]
    pub fn new(provider: ProviderName, delivery: Delivery) -> Self {
        Self {
            provider,
            delivery,
            available: true,
            throttled: 0,
            back_off: BackOff { seconds: 0 },
            polls: 0,
            emitted: 0,
            pending: Vec::new(),
            forged: VecDeque::new(),
            current: BTreeMap::new(),
        }
    }

    /// The provider.
    #[must_use]
    pub fn provider(&self) -> &ProviderName {
        &self.provider
    }

    /// How the feed delivers.
    #[must_use]
    pub fn delivery(&self) -> Delivery {
        self.delivery
    }

    /// Emits `observation` unchanged as an authentic answer.
    pub fn emit(&mut self, observation: &Observation) {
        let n = self.emitted;
        self.emitted = self.emitted.saturating_add(1);
        let bound = self.delivery.delay_polls;
        let delay = u32::try_from(n % (u64::from(bound) + 1)).unwrap_or(bound);
        let signed = Signed::new(observation.clone(), &SIGNING_KEY);
        if self.delivery.duplicate && n.is_multiple_of(2) {
            let again = self.polls.saturating_add(bound - delay);
            self.pending.push((signed.clone(), again));
        }
        self.pending
            .push((signed, self.polls.saturating_add(delay)));
        match self.current.get(&observation.object) {
            Some(known) if known.generation >= observation.generation => {}
            _ => {
                self.current
                    .insert(observation.object.clone(), observation.clone());
            }
        }
    }

    /// Emits the observation of `object` at `generation` with the next event id, the semantic
    /// key of what it states and the next observation time, and returns it.
    ///
    /// # Errors
    ///
    /// Never in practice: an event id is never empty.
    pub(crate) fn observe(
        &mut self,
        object: &RemoteIdentity,
        generation: u64,
        source_head: Option<Head>,
        base_head: Option<Head>,
        protection_digest: Option<Digest>,
        fact: Fact,
    ) -> Result<Observation, EmptyText> {
        let mut observation = Observation {
            provider: self.provider.clone(),
            event_id: EventId::new(format!(
                "{}-event-{}",
                self.provider,
                self.emitted.saturating_add(1)
            ))?,
            semantic_key: Digest::from_bytes([0; 32]),
            object: object.clone(),
            generation,
            source_head,
            base_head,
            protection_digest,
            observed_at: EPOCH.saturating_add(self.emitted),
            fact,
        };
        observation.semantic_key = semantic_key(&observation);
        self.emit(&observation);
        Ok(observation)
    }

    /// Queues an answer that carries only `observation`, signed with [`FORGER_KEY`]. The next
    /// poll made while the provider is reachable receives it, whatever the delivery's delay.
    pub fn inject_forged(&mut self, observation: &Observation) {
        self.forged
            .push_back(Signed::new(observation.clone(), &FORGER_KEY));
    }

    /// Makes the provider reachable or unreachable. Emitted observations stay queued while it
    /// is unreachable.
    pub fn set_available(&mut self, available: bool) {
        self.available = available;
    }

    /// Whether the provider is reachable.
    #[must_use]
    pub fn is_available(&self) -> bool {
        self.available
    }

    /// Rate-limits the next `calls` calls that reach the provider, polls, relists and the
    /// provider's own queries alike: each answers [`SourceError::RateLimited`] stating
    /// `back_off`, delivers nothing and is not a counted poll. A throttle while calls are still
    /// throttled extends the window, and every call left in it states the new `back_off`.
    pub fn throttle(&mut self, calls: u32, back_off: BackOff) {
        self.throttled = self.throttled.saturating_add(calls);
        self.back_off = back_off;
    }

    /// How many emitted deliveries, duplicates included, no poll has delivered yet.
    #[must_use]
    pub fn undelivered(&self) -> usize {
        self.pending.len()
    }

    /// Reaches the provider for one call.
    ///
    /// # Errors
    ///
    /// [`SourceError::Unavailable`] while the provider is unreachable and
    /// [`SourceError::RateLimited`] while it is rate-limited.
    pub fn reach(&mut self) -> Result<(), SourceError> {
        if !self.available {
            return Err(SourceError::Unavailable);
        }
        if self.throttled > 0 {
            self.throttled -= 1;
            return Err(SourceError::RateLimited {
                back_off: Some(self.back_off),
            });
        }
        Ok(())
    }
}

/// Every observation of `answer`, if every signature in it verifies.
fn verified(answer: Vec<Signed>) -> Result<Vec<Observation>, SourceError> {
    if answer.iter().all(Signed::verifies) {
        Ok(answer.into_iter().map(|s| s.observation).collect())
    } else {
        Err(SourceError::Unauthenticated)
    }
}

impl ObservationSource for Feed {
    fn poll(&mut self) -> Result<Vec<Observation>, SourceError> {
        self.reach()?;
        let now = self.polls;
        self.polls = self.polls.saturating_add(1);
        if let Some(forged) = self.forged.pop_front() {
            return verified(vec![forged]);
        }
        let (due, later): (Vec<_>, Vec<_>) = self.pending.drain(..).partition(|(_, at)| *at <= now);
        self.pending = later;
        let mut answer: Vec<Signed> = due.into_iter().map(|(s, _)| s).collect();
        if self.delivery.reorder {
            answer.reverse();
        }
        verified(answer)
    }

    fn relist(&mut self) -> Result<Vec<Observation>, SourceError> {
        self.reach()?;
        Ok(self.current.values().cloned().collect())
    }
}

/// A fake observation source built on a [`Feed`].
pub trait FeedSource: ObservationSource + Sized {
    /// The provider the source observes.
    ///
    /// # Errors
    ///
    /// Never in practice: the name is a non-empty literal.
    fn provider() -> Result<ProviderName, EmptyText>;

    /// A fresh source over `feed`, which knows no remote object.
    fn from_feed(feed: Feed) -> Self;

    /// The source's feed, to emit on and inject faults into.
    fn feed(&mut self) -> &mut Feed;
}

/// Runs [`autobot_adapters::observation::run`] against fresh sources of type `S`.
#[derive(Debug, Clone)]
pub struct FeedHarness<S> {
    provider: ProviderName,
    source: std::marker::PhantomData<S>,
}

impl<S: FeedSource> FeedHarness<S> {
    /// A harness over sources of type `S`.
    ///
    /// # Errors
    ///
    /// Never in practice: see [`FeedSource::provider`].
    pub fn new() -> Result<Self, EmptyText> {
        Ok(Self {
            provider: S::provider()?,
            source: std::marker::PhantomData,
        })
    }
}

impl<S: FeedSource> ObservationHarness for FeedHarness<S> {
    type Source = S;

    fn source(&mut self, delivery: Delivery) -> S {
        S::from_feed(Feed::new(self.provider.clone(), delivery))
    }

    fn publish(&mut self, source: &mut S, observation: &Observation) {
        source.feed().emit(observation);
    }

    fn publish_unauthenticated(&mut self, source: &mut S, observation: &Observation) {
        source.feed().inject_forged(observation);
    }

    fn set_available(&mut self, source: &mut S, available: bool) {
        source.feed().set_available(available);
    }
}

/// The semantic key of what `observation` states: SHA-256 of its [`fact_bytes`].
fn semantic_key(observation: &Observation) -> Digest {
    Digest::from_bytes(Sha256::digest(fact_bytes(observation)).into())
}

/// The signature of `observation` under `key`: HMAC-SHA256 of [`signed_bytes`].
fn sign(key: &[u8; 32], observation: &Observation) -> [u8; 32] {
    hmac_sha256(key, &signed_bytes(observation))
}

/// Appends `bytes` with its length in front, so no two field sequences encode alike.
fn field(out: &mut Vec<u8>, bytes: &[u8]) {
    let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(bytes);
}

/// Appends an absent field as the tag 0 and a present one as the tag 1 and the field.
fn optional(out: &mut Vec<u8>, bytes: Option<&[u8]>) {
    match bytes {
        None => out.push(0),
        Some(bytes) => {
            out.push(1);
            field(out, bytes);
        }
    }
}

/// Every field of `observation`, each length-prefixed, as a signature covers them: the
/// delivery's event id, semantic key and observation time, then its [`fact_bytes`].
fn signed_bytes(o: &Observation) -> Vec<u8> {
    let mut out = Vec::new();
    field(&mut out, o.event_id.as_str().as_bytes());
    field(&mut out, o.semantic_key.as_bytes());
    field(&mut out, &o.observed_at.to_be_bytes());
    out.extend(fact_bytes(o));
    out
}

/// What `observation` states, each field length-prefixed: the provider, the object, its
/// generation, heads and protection, and the fact. Two deliveries of one fact encode alike
/// whatever their event ids and observation times.
fn fact_bytes(o: &Observation) -> Vec<u8> {
    let mut out = Vec::new();
    field(&mut out, o.provider.as_str().as_bytes());
    field(&mut out, o.object.as_str().as_bytes());
    field(&mut out, &o.generation.to_be_bytes());
    optional(
        &mut out,
        o.source_head.as_ref().map(|h| h.as_str().as_bytes()),
    );
    optional(
        &mut out,
        o.base_head.as_ref().map(|h| h.as_str().as_bytes()),
    );
    optional(
        &mut out,
        o.protection_digest
            .as_ref()
            .map(|d| d.as_bytes().as_slice()),
    );
    match &o.fact {
        Fact::Heads => out.push(0),
        Fact::Ci(run) => {
            out.push(1);
            field(&mut out, run.head.as_str().as_bytes());
            field(&mut out, run.workflow_digest.as_bytes());
            field(&mut out, run.environment_digest.as_bytes());
            field(&mut out, run.run.as_str().as_bytes());
            out.push(match run.conclusion {
                CiConclusion::Pending => 0,
                CiConclusion::Success => 1,
                CiConclusion::Failure => 2,
                CiConclusion::Cancelled => 3,
            });
        }
        Fact::Text(text) => {
            out.push(2);
            field(&mut out, text.actor.as_str().as_bytes());
            field(&mut out, text.body.as_bytes());
        }
    }
    out
}

#[cfg(test)]
mod tests;
