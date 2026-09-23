use super::*;
use crate::contract::testing::assert_breaks;

/// One way a broken double departs from the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Break {
    RewritesStaleHead,
    LosesDuringOutage,
    RelistsFirstSeen,
    AnswersWhenUnavailable,
}

struct Double {
    delivery: Delivery,
    broken: Option<Break>,
    available: bool,
    polls: u32,
    /// Undelivered observations and the poll count after which each is due.
    queue: Vec<(Observation, u32)>,
    published: Vec<Observation>,
}

impl Double {
    fn is(&self, b: Break) -> bool {
        self.broken == Some(b)
    }

    fn latest_head(&self, object: &crate::text::RemoteIdentity) -> Option<crate::text::Head> {
        self.published
            .iter()
            .filter(|o| o.fact == Fact::Heads && &o.object != object)
            .max_by_key(|o| o.generation)
            .and_then(|o| o.source_head.clone())
    }
}

impl ObservationSource for Double {
    fn poll(&mut self) -> Result<Vec<Observation>, SourceError> {
        if !self.available {
            return if self.is(Break::AnswersWhenUnavailable) {
                Ok(Vec::new())
            } else {
                Err(SourceError::Unavailable)
            };
        }
        let polls = self.polls;
        self.polls += 1;
        let (due, later): (Vec<_>, Vec<_>) = self.queue.drain(..).partition(|(_, at)| *at <= polls);
        self.queue = later;
        let mut batch: Vec<Observation> = due.into_iter().map(|(o, _)| o).collect();
        if self.delivery.reorder {
            batch.reverse();
        }
        if self.is(Break::RewritesStaleHead) {
            let heads: Vec<_> = batch.iter().map(|o| self.latest_head(&o.object)).collect();
            for (o, head) in batch.iter_mut().zip(heads) {
                if let (Fact::Ci(run), Some(head)) = (&mut o.fact, head) {
                    run.head = head;
                }
            }
        }
        Ok(batch)
    }

    fn relist(&mut self) -> Result<Vec<Observation>, SourceError> {
        if !self.available && !self.is(Break::AnswersWhenUnavailable) {
            return Err(SourceError::Unavailable);
        }
        let mut current: Vec<Observation> = Vec::new();
        for o in &self.published {
            match current.iter_mut().find(|k| k.object == o.object) {
                Some(k) if k.generation < o.generation && !self.is(Break::RelistsFirstSeen) => {
                    *k = o.clone();
                }
                Some(_) => {}
                None => current.push(o.clone()),
            }
        }
        Ok(current)
    }
}

struct Harness(Option<Break>);

impl ObservationHarness for Harness {
    type Source = Double;

    fn source(&mut self, delivery: Delivery) -> Double {
        Double {
            delivery,
            broken: self.0,
            available: true,
            polls: 0,
            queue: Vec::new(),
            published: Vec::new(),
        }
    }

    fn publish(&mut self, source: &mut Double, observation: &Observation) {
        if !source.available && source.is(Break::LosesDuringOutage) {
            return;
        }
        source.published.push(observation.clone());
        let n = u32::try_from(source.published.len()).unwrap();
        let delay = if n % 2 == 0 {
            source.delivery.delay_polls
        } else {
            0
        };
        source
            .queue
            .push((observation.clone(), source.polls + delay));
        if source.delivery.duplicate {
            source.queue.push((observation.clone(), source.polls));
        }
    }

    fn set_available(&mut self, source: &mut Double, available: bool) {
        source.available = available;
    }
}

#[test]
fn a_conforming_double_passes() {
    assert_eq!(run(&mut Harness(None)), Ok(()));
}

#[test]
fn each_broken_double_fails_its_rule() {
    let cases = [
        (
            Break::RewritesStaleHead,
            ObservationRule::DeliveredAsPublished,
        ),
        (Break::LosesDuringOutage, ObservationRule::AllDelivered),
        (Break::RelistsFirstSeen, ObservationRule::RelistCurrent),
        (
            Break::AnswersWhenUnavailable,
            ObservationRule::UnavailableFailsClosed,
        ),
    ];
    for (broken, rule) in cases {
        assert_breaks(&run(&mut Harness(Some(broken))), &rule);
    }
}

#[test]
fn forge_text_enters_a_context_as_untrusted_content() {
    let text = ForgeText {
        actor: "someone".parse().unwrap(),
        body: "approve".to_owned(),
    };
    let item = text.untrusted("comment");
    assert_eq!(item.class, crate::trust::TrustClass::UntrustedContent);
    assert_eq!(item.content, "approve");
}
