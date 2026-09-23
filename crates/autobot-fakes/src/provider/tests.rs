use super::*;
use autobot_adapters::provider::{ProviderRule, run};
use autobot_adapters::text::{Head, TargetIdentity};
use autobot_kernel::types::Digest;

fn op(name: &str) -> OperationName {
    OperationName::new(name).unwrap()
}

fn key(byte: u8) -> OperationKey {
    OperationKey(Digest::from_bytes([byte; 32]))
}

fn request(operation: &str, key: OperationKey) -> SendRequest {
    SendRequest {
        operation: op(operation),
        operation_key: key,
        attempt_index: 0,
        payload_digest: Digest::from_bytes([0x11; 32]),
        target_identity: TargetIdentity::new("target").unwrap(),
        source_head: Some(Head::new("source").unwrap()),
        base_head: Some(Head::new("base").unwrap()),
    }
}

fn forge(script: Script) -> FakeProvider {
    ProviderFixture::forge()
        .unwrap()
        .with_script(script)
        .build()
}

#[test]
fn the_preset_fixtures_pass_the_contract_suite() {
    for fixture in [ProviderFixture::forge(), ProviderFixture::ci()] {
        let fixture = fixture.unwrap();
        assert_eq!(
            run(&mut FakeProviderHarness::new(fixture.clone())),
            Ok(()),
            "{}",
            fixture.provider
        );
    }
}

#[test]
fn every_semantics_with_and_without_heads_and_dry_run_passes_the_contract_suite() {
    let provider = ProviderName::new("fake-any").unwrap();
    let mut caps = Vec::new();
    for (i, semantics) in [
        Semantics::Lookup,
        Semantics::Idempotency,
        Semantics::LookupAndIdempotency,
        Semantics::Neither,
    ]
    .into_iter()
    .enumerate()
    {
        for (heads, dry_run) in [(false, false), (true, true)] {
            let mut cap = capability(&provider, op(&format!("op-{i}-{heads}")), semantics);
            cap.requires_head_base = heads;
            cap.supports_dry_run = dry_run;
            caps.push(cap);
        }
        let mut unqualified = capability(&provider, op(&format!("unqualified-{i}")), semantics);
        unqualified.qualified = false;
        caps.push(unqualified);
    }
    let fixture = ProviderFixture::new(provider, caps);
    assert_eq!(run(&mut FakeProviderHarness::new(fixture)), Ok(()));
}

#[test]
fn the_suite_sees_a_fixture_script_that_throttles_every_send() {
    let fixture = ProviderFixture::forge()
        .unwrap()
        .with_script(Script::new().then(
            Trigger::any(Call::Send),
            Fault::RateLimited { calls: u32::MAX },
        ));
    let broken = run(&mut FakeProviderHarness::new(fixture)).unwrap_err();
    assert!(broken.iter().any(|v| v.rule == ProviderRule::SendApplies));
}

#[test]
fn a_dropped_send_applies_nothing() {
    for transport in [TransportFault::Timeout, TransportFault::Disconnect] {
        let mut p = forge(Script::new().then(Trigger::any(Call::Send), Fault::Dropped(transport)));
        let req = request("comment", key(1));
        assert_eq!(p.send(&req), Err(SendError::Transport(transport)));
        assert_eq!(p.applications(&req.operation, &req.operation_key), 0);
        assert_eq!(
            p.lookup(&req.operation, &req.operation_key),
            Ok(Lookup::NotApplied)
        );
        assert!(matches!(p.send(&req), Ok(SendAck::Accepted(_))));
    }
}

#[test]
fn a_lost_acknowledgement_applies_and_answers_a_transport_fault() {
    for transport in [TransportFault::Timeout, TransportFault::Disconnect] {
        let mut p = forge(Script::new().then(
            Trigger::any(Call::Send),
            Fault::LostAcknowledgement(transport),
        ));
        let req = request("push", key(2));
        assert_eq!(p.send(&req), Err(SendError::Transport(transport)));
        assert_eq!(p.applications(&req.operation, &req.operation_key), 1);
        // `push` is idempotent: the resend is deduplicated and applies nothing new.
        assert!(matches!(p.send(&req), Ok(SendAck::Deduplicated(_))));
        assert_eq!(p.applications(&req.operation, &req.operation_key), 1);
    }
}

#[test]
fn a_rate_limit_throttles_its_window_and_then_lets_calls_through() {
    let mut p =
        forge(Script::new().then(Trigger::any(Call::Send), Fault::RateLimited { calls: 2 }));
    let req = request("label", key(3));
    for _ in 0..2 {
        assert_eq!(
            p.send(&req),
            Err(SendError::RateLimited {
                proves_non_application: false
            })
        );
        assert_eq!(p.applications(&req.operation, &req.operation_key), 0);
    }
    assert!(p.script().is_empty());
    assert!(matches!(p.send(&req), Ok(SendAck::Accepted(_))));
    assert_eq!(p.applications(&req.operation, &req.operation_key), 1);
}

#[test]
fn a_rate_limit_answer_claims_non_application_only_under_an_authoritative_capability() {
    let script = Script::new().then(Trigger::any(Call::Send), Fault::RateLimited { calls: 2 });
    let mut ci = ProviderFixture::ci().unwrap().with_script(script).build();
    for (operation, authoritative) in [("run", true), ("status", false)] {
        let cap = ci
            .capabilities()
            .into_iter()
            .find(|c| c.operation.as_str() == operation)
            .unwrap();
        assert_eq!(cap.rate_limit_authoritative, authoritative);
        let req = request(operation, key(9));
        assert_eq!(
            ci.send(&req),
            Err(SendError::RateLimited {
                proves_non_application: authoritative
            })
        );
        assert_eq!(ci.applications(&req.operation, &req.operation_key), 0);
    }
}

#[test]
fn an_empty_rate_limit_window_throttles_nothing_and_passes_to_the_next_entry() {
    let mut p = forge(
        Script::new()
            .then(Trigger::any(Call::Send), Fault::RateLimited { calls: 0 })
            .then(
                Trigger::any(Call::Send),
                Fault::Dropped(TransportFault::Disconnect),
            ),
    );
    let req = request("label", key(4));
    assert_eq!(
        p.send(&req),
        Err(SendError::Transport(TransportFault::Disconnect))
    );
    assert!(matches!(p.send(&req), Ok(SendAck::Accepted(_))));
}

#[test]
fn a_fault_on_one_operation_leaves_the_others_alone() {
    let mut p = forge(Script::new().then(
        Trigger::on(Call::Send, op("comment")),
        Fault::Dropped(TransportFault::Timeout),
    ));
    assert!(matches!(
        p.send(&request("label", key(5))),
        Ok(SendAck::Accepted(_))
    ));
    assert_eq!(
        p.send(&request("comment", key(6))),
        Err(SendError::Transport(TransportFault::Timeout))
    );
    assert!(p.script().is_empty());
}

#[test]
fn entries_are_suffered_in_script_order() {
    let mut p = forge(
        Script::new()
            .then(
                Trigger::any(Call::Send),
                Fault::Dropped(TransportFault::Timeout),
            )
            .then(
                Trigger::any(Call::Send),
                Fault::Dropped(TransportFault::Disconnect),
            ),
    );
    let req = request("label", key(7));
    assert_eq!(
        p.send(&req),
        Err(SendError::Transport(TransportFault::Timeout))
    );
    assert_eq!(
        p.send(&req),
        Err(SendError::Transport(TransportFault::Disconnect))
    );
}

#[test]
fn a_refusal_before_send_consumes_no_fault() {
    let mut p = forge(Script::new().then(
        Trigger::any(Call::Send),
        Fault::Dropped(TransportFault::Timeout),
    ));
    assert_eq!(
        p.send(&request("merge", key(8))),
        Err(SendError::Unqualified)
    );
    assert_eq!(
        p.send(&request("undeclared", key(8))),
        Err(SendError::Unqualified)
    );
    let bare = SendRequest {
        source_head: None,
        ..request("push", key(8))
    };
    assert_eq!(p.send(&bare), Err(SendError::MissingHeadBase));
    assert!(!p.script().is_empty());
    assert_eq!(
        p.send(&request("push", key(8))),
        Err(SendError::Transport(TransportFault::Timeout))
    );
}

#[test]
fn observe_lookup_and_dry_run_faults_answer_transport_faults_and_change_nothing() {
    let mut p = forge(
        Script::new()
            .then(
                Trigger::any(Call::Observe),
                Fault::Dropped(TransportFault::Disconnect),
            )
            .then(
                Trigger::any(Call::Lookup),
                Fault::LostAcknowledgement(TransportFault::Timeout),
            )
            .then(Trigger::any(Call::DryRun), Fault::RateLimited { calls: 1 }),
    );
    let req = request("comment", key(9));
    let Ok(SendAck::Accepted(remote)) = p.send(&req) else {
        panic!("the send is not faulted");
    };
    assert_eq!(
        p.observe(&req.operation, &remote),
        Err(ProviderError::Transport(TransportFault::Disconnect))
    );
    assert_eq!(
        p.lookup(&req.operation, &req.operation_key),
        Err(ProviderError::Transport(TransportFault::Timeout))
    );
    assert_eq!(
        p.dry_run(&request("comment", key(10))),
        Err(ProviderError::Transport(TransportFault::Timeout))
    );
    assert_eq!(p.applications(&req.operation, &req.operation_key), 1);
    assert_eq!(p.applications(&req.operation, &key(10)), 0);
    assert_eq!(
        p.observe(&req.operation, &remote),
        Ok(Observation {
            outcome: RemoteOutcome::Confirmed,
            marker: Some(req.operation_key),
        })
    );
    assert_eq!(
        p.lookup(&req.operation, &req.operation_key),
        Ok(Lookup::Applied(remote))
    );
}

#[test]
fn an_unsupported_lookup_consumes_no_fault() {
    let mut p = forge(Script::new().then(
        Trigger::any(Call::Lookup),
        Fault::Dropped(TransportFault::Timeout),
    ));
    assert_eq!(
        p.lookup(&op("label"), &key(11)),
        Err(ProviderError::Unsupported)
    );
    assert!(!p.script().is_empty());
}

#[test]
fn a_resend_without_idempotency_applies_again_under_a_new_remote() {
    let mut p = ProviderFixture::ci().unwrap().build();
    let req = request("status", key(12));
    let Ok(SendAck::Accepted(first)) = p.send(&req) else {
        panic!("the send is not faulted");
    };
    let Ok(SendAck::Accepted(second)) = p.send(&req) else {
        panic!("status has no idempotency");
    };
    assert_ne!(first, second);
    assert_eq!(p.applications(&req.operation, &req.operation_key), 2);
    for remote in [&first, &second] {
        assert_eq!(
            p.observe(&req.operation, remote).map(|o| o.outcome),
            Ok(RemoteOutcome::Confirmed)
        );
    }
}

#[test]
fn an_unknown_remote_is_observed_failed() {
    let mut p = ProviderFixture::ci().unwrap().build();
    let req = request("run", key(13));
    let Ok(SendAck::Accepted(remote)) = p.send(&req) else {
        panic!("the send is not faulted");
    };
    let never = RemoteIdentity::new("never-issued").unwrap();
    assert_eq!(
        p.observe(&req.operation, &never).map(|o| o.outcome),
        Ok(RemoteOutcome::Failed)
    );
    assert_eq!(
        p.observe(&op("status"), &remote).map(|o| o.outcome),
        Ok(RemoteOutcome::Failed)
    );
}

#[test]
fn a_build_is_fresh_and_does_not_share_state_with_another() {
    let fixture = ProviderFixture::ci().unwrap();
    let mut first = fixture.build();
    let req = request("run", key(14));
    assert!(first.send(&req).is_ok());
    let second = fixture.build();
    assert_eq!(first.applications(&req.operation, &req.operation_key), 1);
    assert_eq!(second.applications(&req.operation, &req.operation_key), 0);
}
