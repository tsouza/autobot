use super::*;
use crate::forge::{FakeForge, FeedHarness};
use autobot_adapters::observation::run;

fn head(name: &str) -> Head {
    Head::new(name).unwrap()
}

fn digest(byte: u8) -> Digest {
    Digest::from_bytes([byte; 32])
}

const PROMPT: Delivery = Delivery {
    duplicate: false,
    reorder: false,
    delay_polls: 0,
};

fn ci_run(o: &Observation) -> &CiRun {
    match &o.fact {
        Fact::Ci(run) => run,
        other => panic!("not a CI run: {other:?}"),
    }
}

#[test]
fn passes_the_observation_contract_suite() {
    assert_eq!(run(&mut FeedHarness::<FakeCi>::new().unwrap()), Ok(()));
}

#[test]
fn a_run_is_observed_bound_to_its_head_workflow_environment_and_run() {
    let mut ci = FakeCi::new(PROMPT).unwrap();
    let object = ci.start(head("h1"), digest(0x71), digest(0x72)).unwrap();
    ci.conclude(&object, CiConclusion::Failure).unwrap();
    let got = ci.poll().unwrap();
    assert_eq!(got.len(), 2);
    for (o, (generation, conclusion)) in got
        .iter()
        .zip([(1, CiConclusion::Pending), (2, CiConclusion::Failure)])
    {
        let run = ci_run(o);
        assert_eq!(o.provider.as_str(), "fake-ci");
        assert_eq!(o.object, object);
        assert_eq!(o.generation, generation);
        assert_eq!(o.source_head, Some(head("h1")));
        assert_eq!(run.head, head("h1"));
        assert_eq!(run.workflow_digest, digest(0x71));
        assert_eq!(run.environment_digest, digest(0x72));
        assert_eq!(run.run.as_str(), object.as_str());
        assert_eq!(run.conclusion, conclusion);
    }
    let listed = ci.relist().unwrap();
    assert_eq!(listed, vec![got[1].clone()]);
}

#[test]
fn a_run_concludes_once_and_only_with_a_conclusion() {
    let mut ci = FakeCi::new(PROMPT).unwrap();
    let object = ci.start(head("h1"), digest(1), digest(2)).unwrap();
    assert_eq!(
        ci.conclude(&object, CiConclusion::Pending),
        Err(CiError::NotAConclusion)
    );
    ci.conclude(&object, CiConclusion::Success).unwrap();
    assert_eq!(
        ci.conclude(&object, CiConclusion::Cancelled),
        Err(CiError::Concluded(object.clone()))
    );
    let ghost = RemoteIdentity::new("fake-ci/run/99").unwrap();
    assert_eq!(
        ci.conclude(&ghost, CiConclusion::Success),
        Err(CiError::UnknownRun(ghost))
    );
    assert_eq!(ci.poll().unwrap().len(), 2);
}

#[test]
fn a_late_result_for_an_old_head_keeps_the_head_it_tested() {
    let mut forge = FakeForge::new(PROMPT).unwrap();
    let mut ci = FakeCi::new(PROMPT).unwrap();
    let pull = forge.open(head("h1"), head("main"), digest(1)).unwrap();
    let old = ci.start(head("h1"), digest(0x71), digest(0x72)).unwrap();
    forge.push(&pull, head("h2")).unwrap();
    let new = ci.start(head("h2"), digest(0x71), digest(0x72)).unwrap();
    ci.conclude(&new, CiConclusion::Failure).unwrap();
    // The run on the old head succeeds last.
    ci.conclude(&old, CiConclusion::Success).unwrap();

    let latest = ci.poll().unwrap().pop().unwrap();
    assert_eq!(latest.object, old);
    assert_eq!(ci_run(&latest).conclusion, CiConclusion::Success);
    assert_eq!(ci_run(&latest).head, head("h1"));
    assert_eq!(latest.source_head, Some(head("h1")));

    // The pull request's current state, at its highest generation, is at another head, and
    // the only result for that head failed: the late success satisfies nothing current.
    let current = forge
        .relist()
        .unwrap()
        .into_iter()
        .find(|o| o.object == pull)
        .unwrap();
    assert_eq!(current.generation, 2);
    let current_head = current.source_head.unwrap();
    assert_ne!(ci_run(&latest).head, current_head);
    let for_current: Vec<_> = ci
        .relist()
        .unwrap()
        .into_iter()
        .filter(|o| ci_run(o).head == current_head)
        .map(|o| ci_run(&o).conclusion)
        .collect();
    assert_eq!(for_current, vec![CiConclusion::Failure]);
}

#[test]
fn a_throttled_ci_answers_rate_limited_with_its_back_off_and_loses_nothing() {
    let mut ci = FakeCi::new(PROMPT).unwrap();
    let object = ci.start(head("h1"), digest(0x71), digest(0x72)).unwrap();
    let back_off = autobot_adapters::backoff::BackOff { seconds: 90 };
    ci.feed().throttle(2, back_off);
    let throttled = Err(SourceError::RateLimited {
        back_off: Some(back_off),
    });
    assert_eq!(ci.poll(), throttled);
    assert_eq!(ci.relist(), throttled);
    let got = ci.poll().unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].object, object);
}
