use super::*;
use autobot_adapters::backoff::BackOff;
use autobot_adapters::observation::run;
use autobot_adapters::trust::TrustClass;

fn head(name: &str) -> Head {
    Head::new(name).unwrap()
}

fn actor(name: &str) -> Actor {
    Actor::new(name).unwrap()
}

fn digest(byte: u8) -> Digest {
    Digest::from_bytes([byte; 32])
}

const PROMPT: Delivery = Delivery {
    duplicate: false,
    reorder: false,
    delay_polls: 0,
};

const UNRULY: Delivery = Delivery {
    duplicate: true,
    reorder: true,
    delay_polls: 2,
};

fn forge(delivery: Delivery) -> FakeForge {
    FakeForge::new(delivery).unwrap()
}

/// Polls `n` times and returns everything delivered, in delivery order.
fn poll_n(forge: &mut FakeForge, n: u32) -> Vec<Observation> {
    (0..n).flat_map(|_| forge.poll().unwrap()).collect()
}

#[test]
fn passes_the_observation_contract_suite() {
    assert_eq!(run(&mut FeedHarness::<FakeForge>::new().unwrap()), Ok(()));
}

#[test]
fn every_change_advances_the_generation_of_its_pull_request_only() {
    let mut f = forge(PROMPT);
    let a = f.open(head("a1"), head("main"), digest(1)).unwrap();
    let b = f.open(head("b1"), head("main"), digest(1)).unwrap();
    f.push(&a, head("a2")).unwrap();
    f.protect(&a, digest(2)).unwrap();

    let got = f.poll().unwrap();
    let generations: Vec<_> = got
        .iter()
        .map(|o| (o.object.clone(), o.generation))
        .collect();
    assert_eq!(
        generations,
        vec![
            (a.clone(), 1),
            (b.clone(), 1),
            (a.clone(), 2),
            (a.clone(), 3)
        ]
    );
    assert!(got.iter().all(|o| o.provider.as_str() == "fake-forge"));

    let listed = f.relist().unwrap();
    let current = listed.iter().find(|o| o.object == a).unwrap();
    assert_eq!(current.generation, 3);
    assert_eq!(current.source_head, Some(head("a2")));
    assert_eq!(current.base_head, Some(head("main")));
    assert_eq!(current.protection_digest, Some(digest(2)));
    assert_eq!(listed.iter().find(|o| o.object == b).unwrap().generation, 1);
    assert_eq!(listed.len(), 2);
}

#[test]
fn a_change_to_an_unknown_pull_request_is_refused_and_emits_nothing() {
    let mut f = forge(PROMPT);
    let ghost = RemoteIdentity::new("fake-forge/pull/99").unwrap();
    assert_eq!(
        f.push(&ghost, head("x")),
        Err(ForgeError::UnknownPull(ghost.clone()))
    );
    assert_eq!(
        f.comment(&ghost, actor("someone"), "hi"),
        Err(ForgeError::UnknownPull(ghost))
    );
    assert_eq!(f.poll(), Ok(Vec::new()));
    assert_eq!(f.relist(), Ok(Vec::new()));
}

#[test]
fn deliveries_are_duplicated_delayed_and_reordered_within_the_bound() {
    let mut f = forge(UNRULY);
    let pull = f.open(head("h1"), head("main"), digest(1)).unwrap();
    for i in 2..=6 {
        f.push(&pull, head(&format!("h{i}"))).unwrap();
    }
    let first = f.poll().unwrap();
    let rest = poll_n(&mut f, UNRULY.delay_polls);
    assert_eq!(
        f.feed().undelivered(),
        0,
        "everything is delivered within the bound"
    );

    let generations =
        |batch: &[Observation]| batch.iter().map(|o| o.generation).collect::<Vec<_>>();
    // Some observations are held back past the first poll.
    assert!(
        first.len() < 6,
        "first poll delivered {:?}",
        generations(&first)
    );
    let all: Vec<_> = first.iter().chain(&rest).cloned().collect();
    for g in 1..=6 {
        assert!(all.iter().any(|o| o.generation == g), "generation {g} lost");
    }
    // Some are delivered twice, each copy identical, so it deduplicates by event id and key.
    assert!(all.len() > 6);
    for o in &all {
        let copies: Vec<_> = all.iter().filter(|c| c.event_id == o.event_id).collect();
        assert!(copies.iter().all(|c| *c == o));
    }
    // The order differs from the order of the changes.
    let order = generations(&all);
    assert!(
        order.windows(2).any(|w| w[0] > w[1]),
        "delivered in order: {order:?}"
    );
    // The relist is the current state whatever the delivery did.
    let listed = f.relist().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].generation, 6);
    assert_eq!(listed[0].source_head, Some(head("h6")));
}

#[test]
fn event_ids_and_semantic_keys_tell_facts_apart() {
    let mut f = forge(PROMPT);
    let pull = f.open(head("h1"), head("main"), digest(1)).unwrap();
    f.push(&pull, head("h2")).unwrap();
    let got = f.poll().unwrap();
    assert_ne!(got[0].event_id, got[1].event_id);
    assert_ne!(got[0].semantic_key, got[1].semantic_key);
    assert!(got[1].observed_at > got[0].observed_at);
    // Another forge stating the same fact gives it the same key; a pull request at the same
    // identity and generation with other heads or protection does not.
    let mut same = forge(PROMPT);
    same.open(head("h1"), head("main"), digest(1)).unwrap();
    let same = same.poll().unwrap().remove(0);
    assert_eq!(same.object, got[0].object);
    assert_eq!(same.semantic_key, got[0].semantic_key);
    for other in [
        forge_with(head("other"), head("main"), digest(1)),
        forge_with(head("h1"), head("develop"), digest(1)),
        forge_with(head("h1"), head("main"), digest(9)),
    ] {
        assert_eq!((&other.object, other.generation), (&got[0].object, 1));
        assert_ne!(other.semantic_key, got[0].semantic_key, "{other:?}");
    }
}

/// The first observation of a fresh forge whose first pull request is opened as given.
fn forge_with(source: Head, base: Head, protection: Digest) -> Observation {
    let mut f = forge(PROMPT);
    f.open(source, base, protection).unwrap();
    f.poll().unwrap().remove(0)
}

#[test]
fn forge_text_keeps_its_actor_whatever_the_actor_may_do() {
    let mut f = forge(PROMPT);
    let maintainer = actor("maintainer");
    let outsider = actor("outsider");
    f.grant(maintainer.clone(), Permission::Write);
    let pull = f.open(head("h1"), head("main"), digest(1)).unwrap();
    f.comment(&pull, maintainer.clone(), "/merge").unwrap();
    f.comment(&pull, outsider.clone(), "/merge").unwrap();

    let texts: Vec<ForgeText> = f
        .poll()
        .unwrap()
        .into_iter()
        .filter_map(|o| match o.fact {
            Fact::Text(t) => Some(t),
            _ => None,
        })
        .collect();
    let actors: Vec<_> = texts.iter().map(|t| t.actor.clone()).collect();
    assert_eq!(actors, vec![maintainer.clone(), outsider.clone()]);
    for t in &texts {
        assert_eq!(t.body, "/merge");
        assert_eq!(t.untrusted("comment").class, TrustClass::UntrustedContent);
    }

    assert_eq!(f.permission(&maintainer), Ok(Some(Permission::Write)));
    assert_eq!(f.permission(&outsider), Ok(None));
    assert!(Permission::Write < Permission::Admin);
    f.revoke(&maintainer);
    assert_eq!(f.permission(&maintainer), Ok(None));
}

#[test]
fn an_unreachable_forge_fails_closed_and_keeps_what_it_emitted() {
    let mut f = forge(PROMPT);
    f.grant(actor("maintainer"), Permission::Admin);
    f.feed().set_available(false);
    let pull = f.open(head("h1"), head("main"), digest(1)).unwrap();
    assert_eq!(f.poll(), Err(SourceError::Unavailable));
    assert_eq!(f.relist(), Err(SourceError::Unavailable));
    assert_eq!(
        f.permission(&actor("maintainer")),
        Err(SourceError::Unavailable)
    );
    f.feed().set_available(true);
    let got = f.poll().unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].object, pull);
}

#[test]
fn a_rate_limit_states_its_back_off_and_does_not_eat_into_the_delay() {
    let delivery = Delivery {
        duplicate: false,
        reorder: false,
        delay_polls: 1,
    };
    let mut f = forge(delivery);
    let pull = f.open(head("h1"), head("main"), digest(1)).unwrap();
    f.push(&pull, head("h2")).unwrap();
    let short = BackOff { seconds: 5 };
    let long = BackOff { seconds: 600 };
    f.feed().throttle(2, short);
    let throttled = |back_off| SourceError::RateLimited {
        back_off: Some(back_off),
    };
    assert_eq!(f.poll(), Err(throttled(short)));
    f.feed().throttle(1, long);
    assert_eq!(f.relist(), Err(throttled(long)));
    assert_eq!(f.permission(&actor("a")), Err(throttled(long)));
    // Generation 2 is held back one counted poll; the throttled calls are not counted.
    let first = f.poll().unwrap();
    assert_eq!(
        first.iter().map(|o| o.generation).collect::<Vec<_>>(),
        vec![1]
    );
    let second = f.poll().unwrap();
    assert_eq!(
        second.iter().map(|o| o.generation).collect::<Vec<_>>(),
        vec![2]
    );
}

#[test]
fn a_forged_answer_is_refused_and_never_delivered_or_listed() {
    let mut f = forge(PROMPT);
    let pull = f.open(head("h1"), head("main"), digest(1)).unwrap();
    let genuine = f.relist().unwrap().remove(0);
    // A forged answer claims a newer head for the same pull request.
    let mut forged = genuine.clone();
    forged.generation = 7;
    forged.source_head = Some(head("attacker"));
    f.feed().inject_forged(&forged);

    assert_eq!(f.poll(), Err(SourceError::Unauthenticated));
    let got = f.poll().unwrap();
    assert_eq!(got, vec![genuine.clone()]);
    assert_eq!(f.relist(), Ok(vec![genuine]));
    f.push(&pull, head("h2")).unwrap();
    assert_eq!(f.poll().unwrap()[0].source_head, Some(head("h2")));
}

#[test]
fn relist_keeps_the_highest_generation_whatever_the_emission_order() {
    let mut f = forge(PROMPT);
    let pull = f.open(head("h1"), head("main"), digest(1)).unwrap();
    f.push(&pull, head("h2")).unwrap();
    f.push(&pull, head("h3")).unwrap();
    let emitted = f.poll().unwrap();
    let newest = emitted[2].clone();
    assert_eq!(newest.generation, 3);
    // An older generation of the same object arrives after the newer one.
    f.feed().emit(&emitted[0]);
    f.feed().emit(&emitted[1]);
    assert_eq!(f.relist(), Ok(vec![newest]));
    // It is still delivered by poll, as emitted.
    assert_eq!(f.poll(), Ok(emitted[..2].to_vec()));
}

#[test]
fn reorder_alone_reverses_each_batch() {
    for reorder in [false, true] {
        let mut f = forge(Delivery {
            duplicate: false,
            reorder,
            delay_polls: 0,
        });
        let pull = f.open(head("h1"), head("main"), digest(1)).unwrap();
        f.push(&pull, head("h2")).unwrap();
        f.push(&pull, head("h3")).unwrap();
        let order: Vec<_> = f.poll().unwrap().iter().map(|o| o.generation).collect();
        let expected = if reorder {
            vec![3, 2, 1]
        } else {
            vec![1, 2, 3]
        };
        assert_eq!(order, expected, "reorder: {reorder}");
    }
}

#[test]
fn a_poll_that_receives_a_forged_answer_counts_towards_the_delay() {
    let delivery = Delivery {
        duplicate: false,
        reorder: false,
        delay_polls: 1,
    };
    let mut f = forge(delivery);
    let pull = f.open(head("h1"), head("main"), digest(1)).unwrap();
    f.push(&pull, head("h2")).unwrap();
    let forged = f.relist().unwrap().remove(0);
    f.feed().inject_forged(&forged);
    // The forged poll counts: generation 1 was due at it and arrives at the next poll, and
    // generation 2, held back one poll, arrives with it.
    assert_eq!(f.poll(), Err(SourceError::Unauthenticated));
    let next: Vec<_> = f.poll().unwrap().iter().map(|o| o.generation).collect();
    assert_eq!(next, vec![1, 2]);
    assert_eq!(f.feed().undelivered(), 0);
}
