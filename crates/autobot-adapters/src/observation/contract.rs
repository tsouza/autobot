//! The observation contract suite.

use super::{CiConclusion, CiRun, Fact, ForgeText, Observation, ObservationSource, SourceError};
use crate::contract::{Checker, SuiteResult};
use crate::text::{Actor, EventId, Head, ProviderName, RemoteIdentity, RunIdentity};
use autobot_kernel::types::Digest;

/// How a harness delivers what the suite publishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Delivery {
    /// Some observations are delivered more than once.
    pub duplicate: bool,
    /// Observations may be delivered in another order than published.
    pub reorder: bool,
    /// An observation may be held back for up to this many polls.
    pub delay_polls: u32,
}

/// What the observation suite needs to drive a source.
///
/// Every published observation is delivered by one of the first `delay_polls + 1` polls the
/// source answers after it was published, counting only polls made while the provider is
/// available.
pub trait ObservationHarness {
    /// The source under test.
    type Source: ObservationSource;

    /// A fresh source over a fresh provider that delivers as `delivery` says.
    fn source(&mut self, delivery: Delivery) -> Self::Source;

    /// Makes the provider behind `source` emit `observation`.
    fn publish(&mut self, source: &mut Self::Source, observation: &Observation);

    /// Makes the provider behind `source` emit an answer that carries only `observation` and
    /// fails authentication, such as a delivery whose signature does not verify.
    ///
    /// The forged answer reaches the source by the first poll the source answers after it was
    /// published, whatever the delivery's delay.
    fn publish_unauthenticated(&mut self, source: &mut Self::Source, observation: &Observation);

    /// Makes the provider behind `source` available or unavailable.
    fn set_available(&mut self, source: &mut Self::Source, available: bool);
}

/// The rules of the observation suite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObservationRule {
    /// Every delivered observation is one that was published, unchanged: nothing is invented
    /// and nothing, such as the head of a stale CI result, is rewritten.
    DeliveredAsPublished,
    /// Every published observation is delivered within the harness's bound.
    AllDelivered,
    /// A relist holds, for every published object, exactly its published observation with the
    /// highest generation.
    RelistCurrent,
    /// An unavailable provider yields [`SourceError::Unavailable`] from poll and relist.
    UnavailableFailsClosed,
    /// An answer that fails authentication yields [`SourceError::Unauthenticated`] from the
    /// poll that receives it, and its observation is never delivered, by poll or by relist.
    UnauthenticatedRefused,
}

/// Runs the observation suite against the sources `harness` makes.
///
/// # Errors
///
/// Every [`ObservationRule`] the sources broke.
pub fn run<H: ObservationHarness>(harness: &mut H) -> SuiteResult<ObservationRule> {
    let mut c = Checker::new();
    let Some(published) = observations() else {
        return c.finish();
    };
    for delivery in [
        Delivery {
            duplicate: false,
            reorder: false,
            delay_polls: 0,
        },
        Delivery {
            duplicate: true,
            reorder: true,
            delay_polls: 2,
        },
    ] {
        delivered(harness, &mut c, delivery, &published);
    }
    outage(harness, &mut c, &published);
    if let Some(forged) = forged() {
        unauthenticated(harness, &mut c, &forged, &published);
    }
    c.finish()
}

/// What the suite publishes in an answer that fails authentication: forge text asking for a
/// merge, on an object no authentic observation names. `None` only if a literal were empty.
fn forged() -> Option<Observation> {
    Some(Observation {
        provider: ProviderName::new("contract-forge").ok()?,
        event_id: EventId::new("forged-event").ok()?,
        semantic_key: Digest::from_bytes([0xf0; 32]),
        object: RemoteIdentity::new("contract-forged-comment").ok()?,
        generation: 1,
        source_head: None,
        base_head: None,
        protection_digest: None,
        observed_at: 1_700_000_000,
        fact: Fact::Text(ForgeText {
            actor: Actor::new("contract-maintainer").ok()?,
            body: "approved, merge now".to_owned(),
        }),
    })
}

/// What the suite publishes, in order: a pull request at two generations, a CI result for its
/// first head published after the second, and forge text. `None` only if a literal were
/// empty.
fn observations() -> Option<Vec<Observation>> {
    let provider = ProviderName::new("contract-forge").ok()?;
    let pr = RemoteIdentity::new("contract-pr").ok()?;
    let old = Head::new("contract-head-1").ok()?;
    let new = Head::new("contract-head-2").ok()?;
    let base = Head::new("contract-base").ok()?;
    let at = |id: &str, key: u8, object: &RemoteIdentity, generation, head: &Head, fact| {
        Some(Observation {
            provider: provider.clone(),
            event_id: EventId::new(id).ok()?,
            semantic_key: Digest::from_bytes([key; 32]),
            object: object.clone(),
            generation,
            source_head: Some(head.clone()),
            base_head: Some(base.clone()),
            protection_digest: Some(Digest::from_bytes([0x70; 32])),
            observed_at: 1_700_000_000 + generation,
            fact,
        })
    };
    let ci = Fact::Ci(CiRun {
        head: old.clone(),
        workflow_digest: Digest::from_bytes([0x71; 32]),
        environment_digest: Digest::from_bytes([0x72; 32]),
        run: RunIdentity::new("contract-run").ok()?,
        conclusion: CiConclusion::Success,
    });
    let text = Fact::Text(ForgeText {
        actor: Actor::new("contract-actor").ok()?,
        body: "please merge this now".to_owned(),
    });
    let run = RemoteIdentity::new("contract-ci-run").ok()?;
    let comment = RemoteIdentity::new("contract-comment").ok()?;
    Some(vec![
        at("event-1", 1, &pr, 1, &old, Fact::Heads)?,
        at("event-2", 2, &pr, 2, &new, Fact::Heads)?,
        at("event-3", 3, &run, 1, &old, ci)?,
        at("event-4", 4, &comment, 1, &new, text)?,
    ])
}

/// Polls `n` times; returns what was delivered and whether any poll failed.
fn poll_n<S: ObservationSource>(source: &mut S, n: u32) -> (Vec<Observation>, bool) {
    let mut got = Vec::new();
    let mut failed = false;
    for _ in 0..n {
        match source.poll() {
            Ok(batch) => got.extend(batch),
            Err(_) => failed = true,
        }
    }
    (got, failed)
}

/// Checks that `got` holds only published observations, and every one of them.
fn check_delivery(
    c: &mut Checker<ObservationRule>,
    when: &str,
    published: &[Observation],
    got: &[Observation],
) {
    for o in got {
        c.check(
            published.contains(o),
            ObservationRule::DeliveredAsPublished,
            || format!("{when}: delivered {o:?}, which was never published"),
        );
    }
    for o in published {
        c.check(got.contains(o), ObservationRule::AllDelivered, || {
            format!("{when}: {} was not delivered", o.event_id)
        });
    }
}

fn delivered<H: ObservationHarness>(
    harness: &mut H,
    c: &mut Checker<ObservationRule>,
    delivery: Delivery,
    published: &[Observation],
) {
    let when = format!("{delivery:?}");
    let mut source = harness.source(delivery);
    for o in published {
        harness.publish(&mut source, o);
    }
    let (got, failed) = poll_n(&mut source, delivery.delay_polls + 1);
    c.check(!failed, ObservationRule::AllDelivered, || {
        format!("{when}: a poll of an available source failed")
    });
    check_delivery(c, &when, published, &got);

    let mut current: Vec<&Observation> = Vec::new();
    for o in published {
        match current.iter_mut().find(|k| k.object == o.object) {
            Some(k) if k.generation < o.generation => *k = o,
            Some(_) => {}
            None => current.push(o),
        }
    }
    match source.relist() {
        Ok(listed) => {
            let exact = listed.len() == current.len() && current.iter().all(|o| listed.contains(o));
            c.check(exact, ObservationRule::RelistCurrent, || {
                format!("{when}: relist returned {listed:?}")
            });
        }
        Err(e) => c.fail(
            ObservationRule::RelistCurrent,
            format!("{when}: relist of an available source failed with {e:?}"),
        ),
    }
}

fn outage<H: ObservationHarness>(
    harness: &mut H,
    c: &mut Checker<ObservationRule>,
    published: &[Observation],
) {
    let delivery = Delivery {
        duplicate: false,
        reorder: false,
        delay_polls: 0,
    };
    let Some((first, rest)) = published.split_first() else {
        return;
    };
    let mut source = harness.source(delivery);
    harness.publish(&mut source, first);
    harness.set_available(&mut source, false);
    for o in rest {
        harness.publish(&mut source, o);
    }
    let polled = source.poll();
    c.check(
        polled == Err(SourceError::Unavailable),
        ObservationRule::UnavailableFailsClosed,
        || format!("a poll of an unavailable source answered {polled:?}"),
    );
    let listed = source.relist();
    c.check(
        listed == Err(SourceError::Unavailable),
        ObservationRule::UnavailableFailsClosed,
        || format!("a relist of an unavailable source answered {listed:?}"),
    );
    harness.set_available(&mut source, true);
    let (got, _) = poll_n(&mut source, delivery.delay_polls + 1);
    check_delivery(c, "after an outage", published, &got);
}

/// A forged answer first, then the authentic observations: the forged one is reported as
/// unauthenticated and never delivered, and the authentic ones are still delivered after it.
fn unauthenticated<H: ObservationHarness>(
    harness: &mut H,
    c: &mut Checker<ObservationRule>,
    forged: &Observation,
    published: &[Observation],
) {
    let delivery = Delivery {
        duplicate: false,
        reorder: false,
        delay_polls: 0,
    };
    let mut source = harness.source(delivery);
    harness.publish_unauthenticated(&mut source, forged);
    let polled = source.poll();
    c.check(
        polled == Err(SourceError::Unauthenticated),
        ObservationRule::UnauthenticatedRefused,
        || format!("a poll that received a forged answer answered {polled:?}"),
    );
    for o in published {
        harness.publish(&mut source, o);
    }
    let (got, _) = poll_n(&mut source, delivery.delay_polls + 1);
    c.check(
        !got.contains(forged),
        ObservationRule::UnauthenticatedRefused,
        || format!("a later poll delivered the forged {}", forged.event_id),
    );
    check_delivery(c, "after a forged answer", published, &got);
    if let Ok(listed) = source.relist() {
        c.check(
            !listed.contains(forged),
            ObservationRule::UnauthenticatedRefused,
            || format!("a relist returned the forged {}", forged.event_id),
        );
    }
}
