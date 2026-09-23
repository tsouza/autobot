use super::*;
use crate::error::ValueError;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::fmt::Debug;

/// `value` serialized, checked against `expected`, and read back to itself.
fn round_trips<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: &T, expected: Value) {
    let json = serde_json::to_value(value).expect("serializes");
    assert_eq!(json, expected);
    let back: T = serde_json::from_value(expected).expect("deserializes");
    assert_eq!(&back, value);
}

/// Whether `json` fails to deserialize as `T`.
fn refused<T: DeserializeOwned>(json: Value) -> bool {
    serde_json::from_value::<T>(json).is_err()
}

fn state(n: u64) -> LaneRevision {
    LaneRevision::State(StateRevision::new(n).expect("in range"))
}

fn control(n: u64) -> LaneRevision {
    LaneRevision::Control(ControlRevision::new(n).expect("in range"))
}

#[test]
fn counters_start_at_zero_and_advance_by_one() {
    assert_eq!(StateRevision::default(), StateRevision::ZERO);
    assert_eq!(StateRevision::ZERO.get(), 0);
    assert_eq!(StateRevision::ZERO.next().map(StateRevision::get), Ok(1));
    assert_eq!(
        CommitSequence::new(41)
            .and_then(CommitSequence::next)
            .map(u64::from),
        Ok(42)
    );
}

#[test]
fn counters_stay_within_int64() {
    let max = i64::MAX.unsigned_abs();
    let top = ControlRevision::new(max).expect("i64::MAX is in range");
    assert_eq!(top.next(), Err(ValueError::OutOfRange(max + 1)));
    assert_eq!(
        CommitSequence::new(max + 1),
        Err(ValueError::OutOfRange(max + 1))
    );
    assert!(refused::<StateRevision>(json!(max + 1)));
    assert!(refused::<StateRevision>(json!(-1)));
    round_trips(&top, json!(max));
}

#[test]
fn lane_revisions_serialize_as_lane_and_revision() {
    round_trips(&state(3), json!({"lane": "DOMAIN", "revision": 3}));
    round_trips(&control(4), json!({"lane": "CONTROL", "revision": 4}));
    assert_eq!(state(3).lane(), Lane::Domain);
    assert_eq!(control(4).lane(), Lane::Control);
    assert!(refused::<LaneRevision>(
        json!({"lane": "OTHER", "revision": 1})
    ));
    assert!(refused::<LaneRevision>(
        json!({"lane": "CONTROL", "revision": i64::MAX.unsigned_abs() + 1})
    ));
}

#[test]
fn namespaces_are_dns_labels() {
    for ok in ["a", "team-1", "0x", &"a".repeat(63)] {
        assert_eq!(ok.parse::<Namespace>().map(String::from).as_deref(), Ok(ok));
    }
    for bad in ["", "A", "-a", "a-", "a.b", "a_b", &"a".repeat(64)] {
        assert_eq!(
            bad.parse::<Namespace>(),
            Err(ValueError::Namespace(bad.to_owned())),
            "{bad:?} must be refused"
        );
    }
}

#[test]
fn object_names_are_dns_subdomains() {
    let long = [
        "a".repeat(63),
        "b".repeat(63),
        "c".repeat(63),
        "d".repeat(61),
    ]
    .join(".");
    assert_eq!(long.len(), 253);
    for ok in ["a", "receipt-0f3a", "a.b-c.d", long.as_str()] {
        assert!(ok.parse::<ObjectName>().is_ok(), "{ok:?} must be accepted");
    }
    let too_long = format!("{long}e");
    for bad in ["", "a..b", ".a", "a.", "A.b", "a.-b", too_long.as_str()] {
        assert_eq!(
            bad.parse::<ObjectName>(),
            Err(ValueError::Name(bad.to_owned())),
            "{bad:?} must be refused"
        );
    }
}

#[test]
fn uids_and_principals_are_non_empty() {
    assert_eq!("".parse::<Uid>(), Err(ValueError::Empty("a UID")));
    assert_eq!(
        "".parse::<Principal>(),
        Err(ValueError::Empty("a principal"))
    );
    assert!(refused::<Uid>(json!("")));
    round_trips(
        &"system:serviceaccount:autobot:context"
            .parse::<Principal>()
            .expect("non-empty"),
        json!("system:serviceaccount:autobot:context"),
    );
}

#[test]
fn object_refs_carry_namespace_name_and_uid() {
    let r = ObjectRef {
        namespace: "ctx".parse().expect("label"),
        name: "receipt-1".parse().expect("subdomain"),
        uid: "7c1b".parse().expect("non-empty"),
    };
    round_trips(
        &r,
        json!({"namespace": "ctx", "name": "receipt-1", "uid": "7c1b"}),
    );
    assert!(refused::<ObjectRef>(
        json!({"namespace": "Ctx", "name": "r", "uid": "u"})
    ));
    assert!(refused::<ObjectRef>(
        json!({"namespace": "ctx", "name": "r"})
    ));
}

#[test]
fn digests_use_the_sha256_text_form() {
    let text = format!("sha256:{}", "0a".repeat(32));
    let d: Digest = text.parse().expect("valid digest");
    assert_eq!(d.as_bytes(), &[0x0a; 32]);
    assert_eq!(Digest::from_bytes([0x0a; 32]), d);
    round_trips(&d, json!(text));
    for bad in [
        "0a".repeat(32),
        format!("sha256:{}", "0A".repeat(32)),
        format!("sha256:{}", "0a".repeat(31)),
        format!("md5:{}", "0a".repeat(32)),
    ] {
        assert_eq!(bad.parse::<Digest>(), Err(ValueError::Digest(bad.clone())));
    }
}

#[test]
fn a_passed_revision_proof_needs_a_passed_revision_on_the_same_lane() {
    let at = CommitSequence::new(9).expect("in range");
    let proof = PassedRevision::new(state(3), state(4), at).expect("valid proof");
    assert_eq!(
        (
            proof.expected_revision(),
            proof.observed_revision(),
            proof.observed_commit_sequence()
        ),
        (state(3), state(4), at)
    );
    assert_eq!(
        PassedRevision::new(state(3), state(3), at),
        Err(ValueError::ProofNotPassed {
            expected: state(3),
            observed: state(3)
        })
    );
    assert_eq!(
        PassedRevision::new(state(3), state(2), at),
        Err(ValueError::ProofNotPassed {
            expected: state(3),
            observed: state(2)
        })
    );
    assert_eq!(
        PassedRevision::new(state(3), control(5), at),
        Err(ValueError::ProofLanes {
            expected: state(3),
            observed: control(5)
        })
    );
}

#[test]
fn a_passed_revision_proof_is_checked_when_read() {
    let proof = PassedRevision::new(
        control(1),
        control(2),
        CommitSequence::new(7).expect("in range"),
    )
    .expect("valid proof");
    round_trips(
        &proof,
        json!({
            "expected_revision": {"lane": "CONTROL", "revision": 1},
            "observed_revision": {"lane": "CONTROL", "revision": 2},
            "observed_commit_sequence": 7,
        }),
    );
    assert!(refused::<PassedRevision>(json!({
        "expected_revision": {"lane": "CONTROL", "revision": 2},
        "observed_revision": {"lane": "CONTROL", "revision": 2},
        "observed_commit_sequence": 7,
    })));
    assert!(refused::<PassedRevision>(json!({
        "expected_revision": {"lane": "DOMAIN", "revision": 1},
        "observed_revision": {"lane": "CONTROL", "revision": 2},
        "observed_commit_sequence": 7,
    })));
}

#[test]
fn a_rejection_proof_is_tagged_by_its_ground() {
    let passed = PassedRevision::new(
        state(1),
        state(2),
        CommitSequence::new(4).expect("in range"),
    )
    .expect("valid proof");
    round_trips(
        &RejectionProof::PassedRevision(passed),
        json!({
            "ground": "passed_revision",
            "expected_revision": {"lane": "DOMAIN", "revision": 1},
            "observed_revision": {"lane": "DOMAIN", "revision": 2},
            "observed_commit_sequence": 4,
        }),
    );
    let digest = format!("sha256:{}", "0b".repeat(32));
    round_trips(
        &RejectionProof::ReplayConflict {
            existing_receipt_uid: "rcpt-1".parse().expect("non-empty"),
            bound_digest: digest.parse().expect("valid digest"),
        },
        json!({
            "ground": "replay_conflict",
            "existing_receipt_uid": "rcpt-1",
            "bound_digest": digest,
        }),
    );
    round_trips(
        &RejectionProof::CreateConflict {
            observed_uid: "obj-1".parse().expect("non-empty"),
            observed_commit_sequence: CommitSequence::new(9).expect("in range"),
        },
        json!({
            "ground": "create_conflict",
            "observed_uid": "obj-1",
            "observed_commit_sequence": 9,
        }),
    );
    round_trips(
        &RejectionProof::GuardRefusal {
            guard_id: "phase-active".to_owned(),
            read_revision: control(3),
        },
        json!({
            "ground": "guard_refusal",
            "guard_id": "phase-active",
            "read_revision": {"lane": "CONTROL", "revision": 3},
        }),
    );
    assert!(refused::<RejectionProof>(json!({
        "ground": "passed_revision",
        "expected_revision": {"lane": "DOMAIN", "revision": 2},
        "observed_revision": {"lane": "DOMAIN", "revision": 2},
        "observed_commit_sequence": 4,
    })));
    assert!(refused::<RejectionProof>(json!({"ground": "timeout"})));
    assert!(refused::<RejectionProof>(
        json!({"ground": "replay_conflict", "existing_receipt_uid": "rcpt-1"})
    ));
}

#[test]
fn only_an_uncertain_observation_is_not_final() {
    let proof = PassedRevision::new(
        state(1),
        state(2),
        CommitSequence::new(2).expect("in range"),
    )
    .expect("valid proof");
    let observations = [
        CommitObservation::Committed {
            revision: state(2),
            commit_sequence: CommitSequence::new(2).expect("in range"),
        },
        CommitObservation::Rejected(RejectionProof::PassedRevision(proof)),
        CommitObservation::Cancelled {
            cancellation_receipt_uid: "c".parse().expect("non-empty"),
        },
        CommitObservation::ReplayExpired,
    ];
    assert!(observations.iter().all(CommitObservation::is_final));
    assert!(!CommitObservation::Uncertain.is_final());
}
