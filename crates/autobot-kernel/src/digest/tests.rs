use super::cbor::Value;
use super::name::base32;
use super::*;
use crate as autobot_kernel;
use crate::fields::FieldClasses;
use crate::status::{
    AuditEnvelope, Condition, ConditionStatus, ControlReceipt, ControlReceiptRing,
    ControlReceiptState, PendingCommit, PendingCommitState, StatusEnvelope,
};
use crate::types::{CommitSequence, ControlRevision, Lane, ObjectRef, StateRevision};
use proptest::prelude::*;
use serde::Serialize;
use serde_json::json;
use std::collections::BTreeMap;

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

fn encoded<T: Serialize + ?Sized>(value: &T) -> Vec<u8> {
    canonical_encoding(value).expect("encodes")
}

/// SHA-256 of `version` followed by `bytes`, computed without the module's helpers.
fn sha256_of(version: u8, bytes: &[u8]) -> Digest {
    use sha2::Digest as _;
    let mut input = vec![version];
    input.extend_from_slice(bytes);
    Digest::from_bytes(sha2::Sha256::digest(&input).into())
}

/// Serializes as a CBOR byte string.
struct Bytes(&'static [u8]);

impl Serialize for Bytes {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(self.0)
    }
}

// RFC 8949 Appendix A.
#[test]
fn integers_encode_in_their_shortest_form() {
    let unsigned: [(u64, &str); 9] = [
        (0, "00"),
        (1, "01"),
        (10, "0a"),
        (23, "17"),
        (24, "1818"),
        (100, "1864"),
        (1000, "1903e8"),
        (1_000_000, "1a000f4240"),
        (1_000_000_000_000, "1b000000e8d4a51000"),
    ];
    for (n, expected) in unsigned {
        assert_eq!(encoded(&n), hex(expected), "{n}");
    }
    assert_eq!(encoded(&u64::MAX), hex("1bffffffffffffffff"));
    let signed: [(i64, &str); 5] = [
        (-1, "20"),
        (-10, "29"),
        (-100, "3863"),
        (-1000, "3903e7"),
        (i64::MIN, "3b7fffffffffffffff"),
    ];
    for (n, expected) in signed {
        assert_eq!(encoded(&n), hex(expected), "{n}");
    }
    assert_eq!(
        encoded(&-18_446_744_073_709_551_616_i128),
        hex("3bffffffffffffffff")
    );
    assert_eq!(
        canonical_encoding(&-18_446_744_073_709_551_617_i128),
        Err(EncodeError::IntegerRange)
    );
    assert_eq!(
        canonical_encoding(&(u128::from(u64::MAX) + 1)),
        Err(EncodeError::IntegerRange)
    );
}

#[test]
fn simple_values_strings_and_containers_encode_as_rfc_8949_prints_them() {
    assert_eq!(encoded(&false), hex("f4"));
    assert_eq!(encoded(&true), hex("f5"));
    assert_eq!(encoded(&None::<u8>), hex("f6"));
    assert_eq!(encoded(""), hex("60"));
    assert_eq!(encoded("a"), hex("6161"));
    assert_eq!(encoded("IETF"), hex("6449455446"));
    assert_eq!(encoded("\u{00fc}"), hex("62c3bc"));
    assert_eq!(encoded(&Bytes(&[1, 2, 3, 4])), hex("4401020304"));
    assert_eq!(encoded(&Vec::<u8>::new()), hex("80"));
    assert_eq!(encoded(&[1, 2, 3]), hex("83010203"));
    assert_eq!(encoded(&(1, [2, 3], [4, 5])), hex("8301820203820405"));
    let long: Vec<u8> = (1..=25).collect();
    assert_eq!(
        encoded(&long),
        hex("98190102030405060708090a0b0c0d0e0f101112131415161718181819")
    );
    assert_eq!(encoded(&BTreeMap::<u8, u8>::new()), hex("a0"));
    assert_eq!(
        encoded(&BTreeMap::from([(1, 2), (3, 4)])),
        hex("a201020304")
    );
    assert_eq!(
        encoded(&json!({"a": 1, "b": [2, 3]})),
        hex("a26161016162820203")
    );
}

#[test]
fn map_entries_sort_by_their_encoded_keys_whatever_order_they_come_in() {
    // The RFC 8949 §4.2.1 example order.
    let keys = [
        Value::Uint(10),
        Value::Uint(100),
        Value::Nint(0),
        Value::Text("z".to_owned()),
        Value::Text("aa".to_owned()),
        Value::Array(vec![Value::Uint(100)]),
        Value::Array(vec![Value::Nint(0)]),
        Value::Bool(false),
    ];
    let sorted = Value::Map(keys.iter().cloned().map(|k| (k, Value::Null)).collect());
    let reversed = Value::Map(
        keys.iter()
            .rev()
            .cloned()
            .map(|k| (k, Value::Null))
            .collect(),
    );
    let expected = hex("a80af61864f620f6617af6626161f6811864f68120f6f4f6");
    assert_eq!(sorted.encode(), Ok(expected.clone()));
    assert_eq!(reversed.encode(), Ok(expected));
}

#[test]
fn a_map_with_two_equal_keys_has_no_encoding() {
    let map = Value::Map(vec![
        (Value::Text("a".to_owned()), Value::Uint(1)),
        (Value::Text("a".to_owned()), Value::Uint(2)),
    ]);
    assert_eq!(map.encode(), Err(EncodeError::DuplicateKey));
}

#[test]
fn floats_have_no_encoding() {
    assert_eq!(canonical_encoding(&1.5_f64), Err(EncodeError::Float));
    assert_eq!(canonical_encoding(&[0.0_f32]), Err(EncodeError::Float));
}

#[derive(Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Shape {
    Unit,
    Newtype(u8),
    Tuple(u8, u8),
    Struct { a: u8 },
}

#[test]
fn records_encode_as_their_serde_form() {
    assert_eq!(encoded(&Shape::Unit), encoded("UNIT"));
    assert_eq!(encoded(&Shape::Newtype(1)), encoded(&json!({"NEWTYPE": 1})));
    assert_eq!(
        encoded(&Shape::Tuple(1, 2)),
        encoded(&json!({"TUPLE": [1, 2]}))
    );
    assert_eq!(
        encoded(&Shape::Struct { a: 1 }),
        encoded(&json!({"STRUCT": {"a": 1}}))
    );
    let d = Digest::from_bytes([0xab; 32]);
    assert_eq!(encoded(&d), encoded(&d.to_string()));
    assert_eq!(
        encoded(&StateRevision::new(5).expect("in range")),
        encoded(&5u8)
    );
}

#[test]
fn a_digest_is_sha256_of_the_version_byte_and_the_encoding() {
    assert_eq!(ENCODING_VERSION, 1);
    let value = json!({"kind": "Task", "n": [1, 2]});
    assert_eq!(digest(&value), Ok(sha256_of(1, &encoded(&value))));
    assert_ne!(digest(&value), Ok(sha256_of(2, &encoded(&value))));
    let env = audit(Lane::Domain, 4);
    assert_eq!(event_digest(&env), Ok(sha256_of(1, &encoded(&env))));
    let mut other = env.clone();
    other.correlation_id = "another".to_owned();
    assert_ne!(event_digest(&env), event_digest(&other));
}

// A kind-shaped status carrying the KERNEL §1 `WorkContext` classes.

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Phase {
    Draining,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, FieldClasses)]
struct ManagerAuthority {
    #[field(domain)]
    lease_uid: String,
    #[field(domain)]
    epoch: u64,
    /// Absent while the entry is in its initial phase, `ACTIVE`.
    #[field(control)]
    #[serde(skip_serializing_if = "Option::is_none")]
    phase: Option<Phase>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct LedgerEntry {
    permit: String,
    state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, FieldClasses)]
struct WorkContextStatus {
    #[serde(flatten)]
    #[field(nested)]
    envelope: StatusEnvelope,
    #[field(control)]
    hold_state: String,
    #[field(control)]
    hold_generation: u64,
    #[field(control)]
    hold_causes: Vec<String>,
    #[field(domain)]
    admission_sequence: u64,
    #[field(nested)]
    manager_authority: BTreeMap<String, ManagerAuthority>,
    #[field(control)]
    dispatch_authority_generation: u64,
    #[field(reconciliation)]
    dispatch_ledger: Vec<LedgerEntry>,
}

fn seq(n: u64) -> CommitSequence {
    CommitSequence::new(n).expect("in range")
}

fn srev(n: u64) -> StateRevision {
    StateRevision::new(n).expect("in range")
}

fn crev(n: u64) -> ControlRevision {
    ControlRevision::new(n).expect("in range")
}

fn uid(s: &str) -> Uid {
    s.parse().expect("uid")
}

fn audit(lane: Lane, sequence: u64) -> AuditEnvelope {
    AuditEnvelope {
        aggregate_uid: uid("ctx"),
        commit_sequence: seq(sequence),
        lane,
        state_revision: srev(1),
        control_revision: crev(0),
        source_uid: uid("src"),
        event_type: "HoldRequested".to_owned(),
        state_digest: Digest::from_bytes([3; 32]),
        actor: "context-controller".parse().expect("principal"),
        causation_id: "cause".to_owned(),
        correlation_id: "corr".to_owned(),
        schema_version: 1,
    }
}

/// Everything outside both digests: structural fields, the slot body, the ring and the
/// reconciliation fields.
#[derive(Debug, Clone)]
struct Outside {
    observed_generation: Option<i64>,
    condition: Option<String>,
    commit_sequence: u64,
    last_receipt: Option<String>,
    slot: Option<(u64, PendingCommitState)>,
    ring: Vec<bool>,
    ledger: Vec<String>,
}

fn slot(sequence: u64, state: PendingCommitState) -> PendingCommit {
    PendingCommit {
        command_uid: uid(&format!("cmd-{sequence}")),
        receipt_uid: uid(&format!("rcpt-{sequence}")),
        commit_sequence: seq(sequence),
        before_digest: Digest::from_bytes([u8::try_from(sequence % 256).unwrap_or(0); 32]),
        after_digest: Digest::from_bytes([2; 32]),
        expected_revision: srev(sequence),
        proposed_revision: srev(sequence + 1),
        control_revision_at_commit: crev(sequence),
        audit_envelope: audit(Lane::Domain, sequence),
        effect_intents: vec![SlotEffectIntent {
            effect_index: 0,
            installation_lineage: format!("lineage-{sequence}"),
            payload_digest: Digest::from_bytes([4; 32]),
            provider_binding: "forge".to_owned(),
            desired_outcome: "merged".to_owned(),
            target_identity: "repo".to_owned(),
            contract_revision: "1".to_owned(),
        }],
        state,
    }
}

fn ring(published: &[bool]) -> ControlReceiptRing {
    let entries = published
        .iter()
        .zip(1u64..)
        .map(|(&p, i)| ControlReceipt {
            control_uid: uid(&format!("ctl-{i}")),
            control_revision: crev(i),
            commit_sequence: seq(i),
            before_control_digest: Digest::from_bytes([5; 32]),
            after_control_digest: Digest::from_bytes([6; 32]),
            audit_envelope: audit(Lane::Control, i),
            principal: "context-controller".parse().expect("principal"),
            state: if p {
                ControlReceiptState::Published
            } else {
                ControlReceiptState::Unpublished
            },
        })
        .collect::<Vec<_>>();
    ControlReceiptRing::try_from(entries).expect("ordered")
}

/// The domain and control values of a status.
#[derive(Debug, Clone)]
struct Lanes {
    state_revision: u64,
    admission_sequence: u64,
    managers: BTreeMap<String, (u64, Option<Phase>)>,
    control_revision: u64,
    hold_state: String,
    hold_generation: u64,
    hold_causes: Vec<String>,
    dispatch_authority_generation: u64,
}

fn status(lanes: &Lanes, outside: &Outside) -> WorkContextStatus {
    WorkContextStatus {
        envelope: StatusEnvelope {
            observed_generation: outside.observed_generation,
            conditions: outside
                .condition
                .iter()
                .map(|reason| Condition {
                    type_: "RingFull".to_owned(),
                    status: ConditionStatus::True,
                    observed_generation: None,
                    last_transition_time: "2026-01-01T00:00:00Z".to_owned(),
                    reason: reason.clone(),
                    message: String::new(),
                })
                .collect(),
            state_revision: srev(lanes.state_revision),
            control_revision: crev(lanes.control_revision),
            commit_sequence: seq(outside.commit_sequence),
            last_receipt_ref: outside.last_receipt.as_ref().map(|n| ObjectRef {
                namespace: "ns".parse().expect("namespace"),
                name: n.parse().expect("name"),
                uid: uid(n),
            }),
            pending_commit: outside.slot.map(|(s, state)| slot(s, state)),
            control_receipt_ring: Some(ring(&outside.ring)),
        },
        hold_state: lanes.hold_state.clone(),
        hold_generation: lanes.hold_generation,
        hold_causes: lanes.hold_causes.clone(),
        admission_sequence: lanes.admission_sequence,
        manager_authority: lanes
            .managers
            .iter()
            .map(|(plan, (epoch, phase))| {
                (
                    plan.clone(),
                    ManagerAuthority {
                        lease_uid: format!("lease-{plan}"),
                        epoch: *epoch,
                        phase: phase.clone(),
                    },
                )
            })
            .collect(),
        dispatch_authority_generation: lanes.dispatch_authority_generation,
        dispatch_ledger: outside
            .ledger
            .iter()
            .map(|p| LedgerEntry {
                permit: p.clone(),
                state: "ACCEPTED_NOT_SENT".to_owned(),
            })
            .collect(),
    }
}

fn some_lanes() -> Lanes {
    Lanes {
        state_revision: 3,
        admission_sequence: 9,
        managers: BTreeMap::from([
            ("plan-a".to_owned(), (1, None)),
            ("plan-b".to_owned(), (2, Some(Phase::Draining))),
        ]),
        control_revision: 4,
        hold_state: "HELD".to_owned(),
        hold_generation: 2,
        hold_causes: vec!["hold-1".to_owned()],
        dispatch_authority_generation: 5,
    }
}

fn some_outside() -> Outside {
    Outside {
        observed_generation: Some(2),
        condition: Some("Full".to_owned()),
        commit_sequence: 7,
        last_receipt: Some("receipt-6".to_owned()),
        slot: Some((6, PendingCommitState::Occupied)),
        ring: vec![true, false],
        ledger: vec!["permit-1".to_owned()],
    }
}

#[test]
fn the_domain_digest_covers_exactly_the_domain_fields() {
    let s = status(&some_lanes(), &some_outside());
    let expected = json!({
        "state_revision": 3,
        "admission_sequence": 9,
        "manager_authority": {
            "plan-a": {"lease_uid": "lease-plan-a", "epoch": 1},
            "plan-b": {"lease_uid": "lease-plan-b", "epoch": 2},
        },
    });
    assert_eq!(domain_digest(&s), digest(&expected));
}

#[test]
fn the_control_digest_covers_exactly_the_control_fields() {
    let s = status(&some_lanes(), &some_outside());
    let expected = json!({
        "control_revision": 4,
        "hold_state": "HELD",
        "hold_generation": 2,
        "hold_causes": ["hold-1"],
        "manager_authority": {"plan-b": {"phase": "DRAINING"}},
        "dispatch_authority_generation": 5,
    });
    assert_eq!(control_digest(&s), digest(&expected));
}

#[test]
fn a_status_with_nothing_of_a_class_digests_as_the_empty_map() {
    #[derive(Serialize, FieldClasses)]
    struct OnlyReconciliation {
        #[field(reconciliation)]
        progress: u8,
    }
    let empty = BTreeMap::<u8, u8>::new();
    let s = OnlyReconciliation { progress: 1 };
    assert_eq!(domain_digest(&s), digest(&empty));
    assert_eq!(control_digest(&s), digest(&empty));
}

#[test]
fn creating_a_keyed_entry_in_its_initial_control_state_leaves_the_control_digest() {
    let before = some_lanes();
    let mut after = before.clone();
    after.managers.insert("plan-c".to_owned(), (1, None));
    let outside = some_outside();
    let (b, a) = (status(&before, &outside), status(&after, &outside));
    assert_eq!(control_digest(&b), control_digest(&a));
    assert_ne!(domain_digest(&b), domain_digest(&a));
}

#[test]
fn a_serialized_field_the_status_does_not_declare_is_refused() {
    #[derive(Serialize, FieldClasses)]
    struct Renamed {
        #[field(domain)]
        #[serde(rename = "somethingElse")]
        value: u8,
    }
    assert_eq!(
        domain_digest(&Renamed { value: 1 }),
        Err(EncodeError::UnknownField("somethingElse".to_owned()))
    );
}

#[test]
fn two_declared_fields_that_fold_to_one_name_are_refused() {
    #[derive(Serialize, FieldClasses)]
    struct Clash {
        #[field(domain)]
        ab: u8,
        #[field(control)]
        a_b: u8,
    }
    assert_eq!(
        control_digest(&Clash { ab: 1, a_b: 2 }),
        Err(EncodeError::AmbiguousField("a_b".to_owned()))
    );
}

#[test]
fn a_value_without_its_declared_shape_is_refused() {
    #[derive(Serialize, FieldClasses)]
    struct Inner {
        #[field(domain)]
        x: u8,
    }
    #[derive(FieldClasses)]
    struct Outer {
        #[field(nested)]
        inner: Inner,
    }
    impl Serialize for Outer {
        fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            use serde::ser::SerializeMap;
            let mut m = s.serialize_map(Some(1))?;
            m.serialize_entry("inner", &self.inner.x)?;
            m.end()
        }
    }
    let outer = Outer {
        inner: Inner { x: 1 },
    };
    assert_eq!(
        domain_digest(&outer),
        Err(EncodeError::Shape("inner".to_owned()))
    );
}

fn slot_state() -> impl Strategy<Value = PendingCommitState> {
    prop_oneof![
        Just(PendingCommitState::Cleared),
        Just(PendingCommitState::Occupied),
        Just(PendingCommitState::Repairing),
    ]
}

fn outside() -> impl Strategy<Value = Outside> {
    (
        proptest::option::of(0i64..5),
        proptest::option::of("[A-Z][a-z]{0,6}"),
        0u64..100,
        proptest::option::of("[a-z]{1,8}"),
        proptest::option::of((0u64..50, slot_state())),
        proptest::collection::vec(any::<bool>(), 0..4),
        proptest::collection::vec("[a-z]{1,6}", 0..3),
    )
        .prop_map(
            |(
                observed_generation,
                condition,
                commit_sequence,
                last_receipt,
                slot,
                ring,
                ledger,
            )| {
                Outside {
                    observed_generation,
                    condition,
                    commit_sequence,
                    last_receipt,
                    slot,
                    ring,
                    ledger,
                }
            },
        )
}

fn phase() -> impl Strategy<Value = Option<Phase>> {
    prop_oneof![Just(None), Just(Some(Phase::Draining))]
}

fn lanes() -> impl Strategy<Value = Lanes> {
    (
        (
            0u64..5,
            0u64..5,
            proptest::collection::btree_map("[a-c]", (0u64..3, phase()), 0..3),
        ),
        (
            0u64..5,
            "RUNNING|HELD",
            0u64..5,
            proptest::collection::vec("[a-c]", 0..3),
            0u64..5,
        ),
    )
        .prop_map(
            |(
                (state_revision, admission_sequence, managers),
                (control_revision, hold_state, hold_generation, hold_causes, generation),
            )| Lanes {
                state_revision,
                admission_sequence,
                managers,
                control_revision,
                hold_state,
                hold_generation,
                hold_causes,
                dispatch_authority_generation: generation,
            },
        )
}

/// `lanes` with only its domain values taken from `domain`.
fn with_domain(lanes: &Lanes, domain: &Lanes) -> Lanes {
    let managers = domain
        .managers
        .iter()
        .map(|(plan, (epoch, _))| {
            let phase = lanes.managers.get(plan).and_then(|(_, p)| p.clone());
            (plan.clone(), (*epoch, phase))
        })
        .collect();
    Lanes {
        state_revision: domain.state_revision,
        admission_sequence: domain.admission_sequence,
        managers,
        ..lanes.clone()
    }
}

/// `lanes` with only its control values taken from `control`, on `lanes`' entries.
fn with_control(lanes: &Lanes, control: &Lanes) -> Lanes {
    let managers = lanes
        .managers
        .iter()
        .map(|(plan, (epoch, _))| {
            let phase = control.managers.get(plan).and_then(|(_, p)| p.clone());
            (plan.clone(), (*epoch, phase))
        })
        .collect();
    Lanes {
        control_revision: control.control_revision,
        hold_state: control.hold_state.clone(),
        hold_generation: control.hold_generation,
        hold_causes: control.hold_causes.clone(),
        dispatch_authority_generation: control.dispatch_authority_generation,
        managers,
        ..lanes.clone()
    }
}

proptest! {
    #[test]
    fn reconciliation_fields_the_slot_body_and_the_ring_change_neither_digest(
        l in lanes(),
        a in outside(),
        b in outside(),
    ) {
        let (x, y) = (status(&l, &a), status(&l, &b));
        prop_assert_eq!(domain_digest(&x)?, domain_digest(&y)?);
        prop_assert_eq!(control_digest(&x)?, control_digest(&y)?);
    }

    #[test]
    fn the_domain_digest_changes_exactly_when_a_domain_value_does(
        l in lanes(),
        other in lanes(),
        o in outside(),
    ) {
        let changed = with_domain(&l, &other);
        let (x, y) = (status(&l, &o), status(&changed, &o));
        let same_domain = l.state_revision == changed.state_revision
            && l.admission_sequence == changed.admission_sequence
            && l.managers.iter().map(|(k, (e, _))| (k, e)).eq(
                changed.managers.iter().map(|(k, (e, _))| (k, e)),
            );
        prop_assert_eq!(domain_digest(&x)? == domain_digest(&y)?, same_domain);
    }

    #[test]
    fn the_control_digest_changes_exactly_when_a_control_value_does(
        l in lanes(),
        other in lanes(),
        o in outside(),
    ) {
        let changed = with_control(&l, &other);
        let (x, y) = (status(&l, &o), status(&changed, &o));
        prop_assert_eq!(domain_digest(&x)?, domain_digest(&y)?);
        let same_control = l.control_revision == changed.control_revision
            && l.hold_state == changed.hold_state
            && l.hold_generation == changed.hold_generation
            && l.hold_causes == changed.hold_causes
            && l.dispatch_authority_generation == changed.dispatch_authority_generation
            && l.managers.values().map(|(_, p)| p).eq(changed.managers.values().map(|(_, p)| p));
        prop_assert_eq!(control_digest(&x)? == control_digest(&y)?, same_control);
    }
}

#[test]
fn the_operation_key_is_the_digest_of_its_five_inputs_as_an_array() {
    let payload = Digest::from_bytes([7; 32]);
    let key = operation_key("lineage", &uid("agg"), srev(3), 2, &payload);
    let array = ("lineage", "agg", 3u64, 2u32, payload);
    assert_eq!(key, sha256_of(1, &encoded(&array)));
}

#[test]
fn operation_key_inputs_do_not_run_into_each_other() {
    let payload = Digest::from_bytes([7; 32]);
    assert_ne!(
        operation_key("ab", &uid("c"), srev(1), 0, &payload),
        operation_key("a", &uid("bc"), srev(1), 0, &payload)
    );
    assert_ne!(
        operation_key("a", &uid("b"), srev(1), 2, &payload),
        operation_key("a", &uid("b"), srev(2), 1, &payload)
    );
}

#[test]
fn an_intent_s_operation_key_takes_its_lineage_index_and_payload_and_the_slot_s_commit() {
    let s = slot(4, PendingCommitState::Occupied);
    let intent = &s.effect_intents[0];
    assert_eq!(
        intent_operation_key(intent, &uid("agg"), s.proposed_revision),
        operation_key(
            "lineage-4",
            &uid("agg"),
            srev(5),
            0,
            &Digest::from_bytes([4; 32])
        )
    );
}

type KeyInputs = (String, String, u64, u32, u8);

fn key_inputs() -> impl Strategy<Value = KeyInputs> {
    ("[ab]{1,2}", "[ab]{1,2}", 0u64..2, 0u32..2, 0u8..2)
}

fn key_of((lineage, aggregate, revision, index, payload): &KeyInputs) -> Digest {
    operation_key(
        lineage,
        &uid(aggregate),
        srev(*revision),
        *index,
        &Digest::from_bytes([*payload; 32]),
    )
}

/// Whether `name` is a DNS-1123 label, checked here independently of the types module.
fn is_dns_label(name: &str) -> bool {
    let ok = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit();
    !name.is_empty()
        && name.len() <= 63
        && name.chars().all(|c| ok(c) || c == '-')
        && name.starts_with(ok)
        && name.ends_with(ok)
}

proptest! {
    // F-21: the key is a function of its five inputs, and of nothing else.
    #[test]
    fn operation_keys_are_equal_exactly_when_their_inputs_are(
        a in key_inputs(),
        b in key_inputs(),
    ) {
        prop_assert_eq!(key_of(&a) == key_of(&b), a == b);
    }

    #[test]
    fn every_generated_name_is_a_dns_1123_label(
        key in "\\PC{0,40}",
        bytes in any::<[u8; 32]>(),
        kind in "[A-Za-z][A-Za-z0-9]{0,29}",
        request in "\\PC{0,20}",
        parent in proptest::option::of("[a-z0-9-]{1,12}"),
    ) {
        let op = Digest::from_bytes(bytes);
        let create = CreateIndex {
            context_uid: uid("ctx"),
            principal: "writer".parse().expect("principal"),
            kind,
            parent_uid: parent.map(|p| uid(&p)),
            client_request_key: request,
        };
        for name in [
            command_receipt_name(&key)?,
            effect_intent_name(&op)?,
            external_operation_name(&op)?,
            create.receipt_name()?,
            create.target_name()?,
        ] {
            prop_assert!(is_dns_label(name.as_str()), "{name}");
        }
    }
}

#[test]
fn base32_is_rfc_4648_lowercase_without_padding() {
    let vectors = [
        ("", ""),
        ("f", "my"),
        ("fo", "mzxq"),
        ("foo", "mzxw6"),
        ("foob", "mzxw6yq"),
        ("fooba", "mzxw6ytb"),
        ("foobar", "mzxw6ytboi"),
    ];
    for (input, expected) in vectors {
        assert_eq!(base32(input.as_bytes()), expected, "{input}");
    }
}

#[test]
fn a_name_is_the_lowercased_kind_and_the_base32_of_twenty_digest_bytes() {
    let d = Digest::from_bytes(std::array::from_fn(|i| u8::try_from(i).unwrap_or(0)));
    let suffix = base32(&d.as_bytes()[..20]);
    assert_eq!(suffix.len(), 32);
    assert_eq!(
        object_name("TaskRun", &d).map(String::from),
        Ok(format!("taskrun-{suffix}"))
    );
    assert_eq!(
        effect_intent_name(&d).map(String::from),
        Ok(format!("effectintent-{suffix}"))
    );
    assert_eq!(
        external_operation_name(&d).map(String::from),
        Ok(format!("externaloperation-{suffix}"))
    );
}

#[test]
fn a_kind_that_cannot_head_a_dns_label_is_refused() {
    let d = Digest::from_bytes([1; 32]);
    let longest = "k".repeat(MAX_NAME_KIND_LEN);
    assert!(object_name(&longest, &d).is_ok());
    for kind in [
        "",
        "-kind",
        "Task.Run",
        "Task_Run",
        &"k".repeat(MAX_NAME_KIND_LEN + 1),
    ] {
        assert!(
            matches!(
                object_name(kind, &d),
                Err(crate::error::ValueError::Namespace(_))
            ),
            "{kind:?}"
        );
    }
}

fn create() -> CreateIndex {
    CreateIndex {
        context_uid: uid("ctx"),
        principal: "writer".parse().expect("principal"),
        kind: "TaskRun".to_owned(),
        parent_uid: Some(uid("task")),
        client_request_key: "req".to_owned(),
    }
}

#[test]
fn every_part_of_a_create_index_enters_its_names() {
    let base = create();
    let changes: [fn(&mut CreateIndex); 5] = [
        |c| c.context_uid = uid("ctx-2"),
        |c| c.principal = "other".parse().expect("principal"),
        |c| c.kind = "AgentRun".to_owned(),
        |c| c.parent_uid = None,
        |c| c.client_request_key = "req-2".to_owned(),
    ];
    for change in changes {
        let mut other = base.clone();
        change(&mut other);
        assert_ne!(other.receipt_name(), base.receipt_name(), "{other:?}");
    }
    let (receipt, target) = (
        base.receipt_name().map(String::from).expect("name"),
        base.target_name().map(String::from).expect("name"),
    );
    assert_eq!(
        receipt.strip_prefix("commandreceipt-"),
        target.strip_prefix("taskrun-")
    );
}

#[test]
fn a_receipt_name_depends_on_the_idempotency_key_alone() {
    assert_eq!(command_receipt_name("k-1"), command_receipt_name("k-1"));
    assert_ne!(command_receipt_name("k-1"), command_receipt_name("k-2"));
}
