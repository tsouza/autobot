use super::*;
use crate::error::{RingError, ValueError};
use crate::profile::{ControlRing, Profile};
use crate::types::{
    CommitSequence, ControlRevision, Digest, Lane, PassedRevision, RejectionProof, StateRevision,
};
use schemars::{JsonSchema, schema_for};
use serde_json::{Value, json};
use std::collections::BTreeSet;

const FORMAL: &str = include_str!("../../../../docs/design/AUTOBOT-FORMAL-SURFACE.md");
const M0_PROFILE: &str = include_str!("../../../../profiles/m0.toml");
const SNAPSHOT: &str = include_str!("status_envelope.schema.json");

fn digest(byte: u8) -> Digest {
    Digest::from_bytes([byte; 32])
}

fn seq(n: u64) -> CommitSequence {
    CommitSequence::new(n).expect("in range")
}

fn crev(n: u64) -> ControlRevision {
    ControlRevision::new(n).expect("in range")
}

fn srev(n: u64) -> StateRevision {
    StateRevision::new(n).expect("in range")
}

/// The M0 profile's ring limits.
fn m0_ring() -> ControlRing {
    Profile::parse(M0_PROFILE)
        .expect("profiles/m0.toml parses")
        .values()
        .control_ring
}

/// An audit envelope of a commit at `sequence` on `lane`.
fn audit(lane: Lane, sequence: u64) -> AuditEnvelope {
    AuditEnvelope {
        aggregate_uid: "agg-1".parse().expect("uid"),
        commit_sequence: seq(sequence),
        lane,
        state_revision: srev(3),
        control_revision: crev(2),
        source_uid: "src".parse().expect("uid"),
        event_type: "HoldRequested".to_owned(),
        state_digest: digest(7),
        actor: "operator".parse().expect("principal"),
        causation_id: "cause".to_owned(),
        correlation_id: "corr".to_owned(),
        schema_version: 1,
    }
}

fn slot() -> PendingCommit {
    PendingCommit {
        command_uid: "cmd-1".parse().expect("uid"),
        receipt_uid: "rcpt-1".parse().expect("uid"),
        commit_sequence: seq(5),
        before_digest: digest(1),
        after_digest: digest(2),
        expected_revision: srev(2),
        proposed_revision: srev(3),
        control_revision_at_commit: crev(2),
        audit_envelope: audit(Lane::Domain, 5),
        effect_intents: vec![SlotEffectIntent {
            effect_index: 0,
            installation_lineage: "install-a".to_owned(),
            payload_digest: digest(4),
            provider_binding: ProviderBinding {
                provider: "fake-forge".to_owned(),
                operation: "comment".to_owned(),
            },
            desired_outcome: "comment-posted".to_owned(),
            target_identity: "pr/7".to_owned(),
            contract_revision: "v1".to_owned(),
        }],
        state: PendingCommitState::Occupied,
    }
}

/// A control receipt at commit sequence `sequence` and control revision `revision`.
fn receipt(sequence: u64, revision: u64) -> ControlReceipt {
    ControlReceipt {
        control_uid: format!("ctl-{sequence}").parse().expect("uid"),
        control_revision: crev(revision),
        commit_sequence: seq(sequence),
        before_control_digest: digest(5),
        after_control_digest: digest(6),
        audit_envelope: audit(Lane::Control, sequence),
        principal: "operator".parse().expect("principal"),
        state: ControlReceiptState::Unpublished,
    }
}

/// A ring holding receipts at commit sequences and control revisions `1..=n`.
fn ring_of(n: u64) -> ControlReceiptRing {
    let limits = m0_ring();
    let mut ring = ControlReceiptRing::default();
    for i in 1..=n {
        ring.append(receipt(i, i), &limits).expect("appends");
    }
    ring
}

fn envelope() -> StatusEnvelope {
    StatusEnvelope {
        observed_generation: Some(4),
        conditions: vec![Condition {
            type_: "Ready".to_owned(),
            status: ConditionStatus::True,
            observed_generation: Some(4),
            last_transition_time: "2026-09-23T10:00:00Z".to_owned(),
            reason: "Committed".to_owned(),
            message: "slot cleared".to_owned(),
        }],
        state_revision: srev(3),
        control_revision: crev(2),
        commit_sequence: seq(5),
        last_receipt_ref: Some(crate::types::ObjectRef {
            namespace: "ctx".parse().expect("label"),
            name: "rcpt-1".parse().expect("subdomain"),
            uid: "rcpt-1".parse().expect("uid"),
        }),
        pending_commit: Some(slot()),
        control_receipt_ring: Some(ring_of(2)),
    }
}

/// The property names of `T`'s JSON schema.
fn properties<T: JsonSchema>() -> BTreeSet<String> {
    let schema = schema_for!(T);
    schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|p| p.keys().cloned().collect())
        .unwrap_or_default()
}

/// The fields of FORMAL §2 record `name`, in order; a record written `Base ⊕ [...]` has the
/// fields of `Base` first.
fn formal_fields(name: &str) -> Vec<String> {
    let (start, base) = FORMAL
        .lines()
        .enumerate()
        .find_map(|(i, l)| {
            let rest = l
                .strip_prefix(name)?
                .trim_start()
                .strip_prefix('=')?
                .trim_start();
            if rest.starts_with('[') {
                Some((i, None))
            } else {
                let (base, tail) = rest.split_once('⊕')?;
                tail.trim_start()
                    .starts_with('[')
                    .then(|| (i, Some(base.trim().to_owned())))
            }
        })
        .unwrap_or_else(|| panic!("FORMAL §2 has no record {name}"));
    let mut body = String::new();
    for line in FORMAL.lines().skip(start) {
        let line = line.split("\\*").next().unwrap_or_default();
        body.push_str(line);
        body.push(' ');
        if line.contains(']') {
            break;
        }
    }
    let open = body.find('[').expect("record opens with [");
    let close = body.rfind(']').expect("record closes with ]");
    let mut fields = Vec::new();
    let (mut depth, mut current) = (0u32, String::new());
    for c in body[open + 1..close].chars() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => fields.push(std::mem::take(&mut current)),
            _ => {}
        }
        if c != ',' || depth > 0 {
            current.push(c);
        }
    }
    fields.push(current);
    let own = fields
        .iter()
        .map(|f| f.split('∈').next().unwrap_or_default().trim().to_owned());
    base.map(|b| formal_fields(&b))
        .unwrap_or_default()
        .into_iter()
        .chain(own)
        .collect()
}

#[test]
fn the_formal_parser_reads_every_field_of_a_record() {
    assert_eq!(
        formal_fields("ControlReceipt"),
        [
            "control_uid",
            "control_revision",
            "commit_sequence",
            "before_control_digest",
            "after_control_digest",
            "audit_envelope",
            "principal",
            "state"
        ]
    );
}

#[test]
fn pending_commit_has_exactly_the_formal_fields() {
    let formal: BTreeSet<String> = formal_fields("PendingCommit").into_iter().collect();
    assert_eq!(formal.len(), 11);
    assert_eq!(properties::<PendingCommit>(), formal);
}

#[test]
fn control_receipt_has_exactly_the_formal_fields() {
    let formal: BTreeSet<String> = formal_fields("ControlReceipt").into_iter().collect();
    assert_eq!(properties::<ControlReceipt>(), formal);
}

/// Checks that every field of FORMAL §2 record `record` is either a property of `T` or
/// accounted for in `elsewhere`, and that `elsewhere` names only fields of the record that
/// `T` does not have.
fn covers<T: JsonSchema>(record: &str, elsewhere: &[&str]) {
    let formal: BTreeSet<String> = formal_fields(record).into_iter().collect();
    let elsewhere: BTreeSet<String> = elsewhere.iter().map(|s| (*s).to_owned()).collect();
    let props = properties::<T>();
    assert!(
        elsewhere.is_subset(&formal),
        "{record}: {elsewhere:?} not all in {formal:?}"
    );
    assert!(
        props.is_disjoint(&elsewhere),
        "{record}: a property is also mapped elsewhere"
    );
    let missing: Vec<_> = formal
        .difference(&elsewhere)
        .filter(|f| !props.contains(*f))
        .collect();
    assert!(
        missing.is_empty(),
        "{record} fields with no home: {missing:?}"
    );
}

#[test]
fn the_envelope_covers_the_formal_aggregate_record() {
    // uid and kind are object metadata, context_uid is origin metadata written at create, and
    // the two digests are computed over the status, not stored in it.
    covers::<StatusEnvelope>(
        "Aggregate",
        &[
            "uid",
            "kind",
            "context_uid",
            "domain_digest",
            "control_digest",
        ],
    );
    let m0 = ["observedGeneration", "conditions", "last_receipt_ref"];
    let formal: BTreeSet<String> = formal_fields("Aggregate").into_iter().collect();
    let extra: BTreeSet<String> = properties::<StatusEnvelope>()
        .difference(&formal)
        .cloned()
        .collect();
    assert_eq!(extra, m0.iter().map(|s| (*s).to_owned()).collect());
}

#[test]
fn a_slot_effect_intent_covers_the_formal_effect_intent_record() {
    covers::<SlotEffectIntent>(
        "EffectIntent",
        &[
            "uid",
            "operation_key",
            "aggregate_uid",
            "committed_revision",
            "state",
        ],
    );
}

#[test]
fn the_audit_envelope_has_exactly_the_formal_fields() {
    let formal: BTreeSet<String> = formal_fields("AuditEnvelope").into_iter().collect();
    assert_eq!(formal.len(), 12);
    assert_eq!(properties::<AuditEnvelope>(), formal);
}

#[test]
fn the_audit_envelope_and_its_digest_are_the_formal_event_record() {
    let mut fields = properties::<AuditEnvelope>();
    fields.insert("event_digest".to_owned());
    let formal: BTreeSet<String> = formal_fields("AutoBotEvent").into_iter().collect();
    assert_eq!(fields, formal);
}

#[test]
fn a_slot_effect_intent_is_the_formal_record_without_its_derived_key() {
    let formal: BTreeSet<String> = formal_fields("EffectIntentRecord").into_iter().collect();
    let mut props = properties::<SlotEffectIntent>();
    assert!(props.remove("installation_lineage"));
    props.insert("operation_key".to_owned());
    assert_eq!(props, formal);
}

/// The fields FORMAL §2 names for `provider_binding` in the comment on its line of
/// `EffectIntentRecord`.
fn formal_provider_binding() -> Vec<String> {
    let line = FORMAL
        .lines()
        .skip_while(|l| !l.starts_with("EffectIntentRecord "))
        .find(|l| l.trim_start().starts_with("provider_binding,"))
        .expect("EffectIntentRecord has provider_binding");
    let (_, comment) = line
        .split_once("\\*")
        .expect("provider_binding is commented");
    let open = comment.find('[').expect("the comment names a pair");
    let close = comment.find(']').expect("the pair closes");
    comment[open + 1..close]
        .split(',')
        .map(|f| f.trim().to_owned())
        .collect()
}

#[test]
fn a_slot_effect_intents_provider_binding_is_the_formal_provider_and_operation_pair() {
    let pair = formal_provider_binding();
    assert_eq!(pair, ["provider", "operation"]);
    assert_eq!(formal_fields("ProviderCapability")[..2], pair[..]);
    assert_eq!(
        properties::<ProviderBinding>(),
        pair.iter().cloned().collect::<BTreeSet<_>>()
    );
    let schema = schema_for!(SlotEffectIntent);
    assert_eq!(
        schema
            .get("properties")
            .and_then(|p| p.get("provider_binding"))
            .and_then(|b| b.get("$ref")),
        Some(&json!("#/$defs/ProviderBinding"))
    );
    let binding = ProviderBinding {
        provider: "fake-forge".to_owned(),
        operation: "comment".to_owned(),
    };
    assert_eq!(
        serde_json::to_value(&binding).expect("serializes"),
        json!({ "provider": "fake-forge", "operation": "comment" })
    );
}

/// The fields of FORMAL §2 `RejectionProof` by ground, as the comment beside each line of the
/// record names the grounds its fields belong to (`passed revision` is `passed_revision`).
fn formal_ground_fields() -> std::collections::BTreeMap<String, BTreeSet<String>> {
    let mut grounds = std::collections::BTreeMap::<String, BTreeSet<String>>::new();
    let start = FORMAL
        .lines()
        .position(|l| l.starts_with("RejectionProof "))
        .expect("FORMAL §2 has RejectionProof");
    for line in FORMAL.lines().skip(start + 1) {
        let Some((fields, comment)) = line.split_once("\\*") else {
            break;
        };
        let fields = fields.trim().trim_end_matches(']');
        for ground in comment.split(',').map(|g| g.trim().replace(' ', "_")) {
            grounds.entry(ground).or_default().extend(
                fields
                    .split(',')
                    .map(str::trim)
                    .filter(|f| !f.is_empty())
                    .map(str::to_owned),
            );
        }
        if line.contains(']') {
            break;
        }
    }
    grounds
}

#[test]
fn the_rejection_proof_grounds_cover_the_formal_record() {
    let passed = PassedRevision::new(
        crate::types::LaneRevision::State(srev(1)),
        crate::types::LaneRevision::State(srev(2)),
        seq(4),
    )
    .expect("valid proof");
    let proofs = [
        RejectionProof::PassedRevision(passed),
        RejectionProof::ReplayConflict {
            existing_receipt_uid: "rcpt-1".parse().expect("uid"),
            bound_digest: digest(8),
        },
        RejectionProof::CreateConflict {
            observed_uid: "obj-1".parse().expect("uid"),
            observed_commit_sequence: seq(4),
        },
        RejectionProof::GuardRefusal {
            guard_id: "phase-active".to_owned(),
            read_revision: crate::types::LaneRevision::Control(crev(1)),
        },
    ];
    let grounds = formal_ground_fields();
    assert_eq!(grounds.len(), proofs.len());
    let mut fields = BTreeSet::new();
    for proof in &proofs {
        let json = serde_json::to_value(proof).expect("serializes");
        let mut keys: BTreeSet<String> = json
            .as_object()
            .expect("an object")
            .keys()
            .cloned()
            .collect();
        let ground = json["ground"].as_str().expect("a ground").to_owned();
        keys.remove("ground");
        keys.remove("expected_revision");
        assert_eq!(Some(&keys), grounds.get(&ground), "{ground}");
        fields.extend(json.as_object().expect("an object").keys().cloned());
    }
    // FORMAL §2 holds the expected revision on the receipt; the passed-revision proof keeps it
    // to check itself.
    assert!(fields.remove("expected_revision"));
    let formal: BTreeSet<String> = formal_fields("RejectionProof").into_iter().collect();
    assert_eq!(fields, formal);
    assert!(formal_fields("CommandReceipt").contains(&"rejection_proof".to_owned()));
}

#[test]
fn the_envelope_round_trips_under_the_design_field_names() {
    let env = envelope();
    let json = serde_json::to_value(&env).expect("serializes");
    let keys: BTreeSet<&str> = json
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        BTreeSet::from([
            "observedGeneration",
            "conditions",
            "state_revision",
            "control_revision",
            "commit_sequence",
            "last_receipt_ref",
            "pending_commit",
            "control_receipt_ring",
        ])
    );
    assert_eq!(json["pending_commit"]["state"], json!("OCCUPIED"));
    assert_eq!(
        json["control_receipt_ring"][1]["state"],
        json!("UNPUBLISHED")
    );
    assert_eq!(
        json["conditions"][0]["lastTransitionTime"],
        json!("2026-09-23T10:00:00Z")
    );
    assert_eq!(json["conditions"][0]["type"], json!("Ready"));
    let back: StatusEnvelope = serde_json::from_value(json).expect("deserializes");
    assert_eq!(back, env);
}

#[test]
fn a_fresh_envelope_is_all_zero_with_a_cleared_slot() {
    let env = StatusEnvelope::default();
    assert_eq!(
        serde_json::to_value(&env).expect("serializes"),
        json!({"conditions": [], "state_revision": 0, "control_revision": 0, "commit_sequence": 0})
    );
    assert_eq!(env.pending_commit_state(), PendingCommitState::Cleared);
    let read: StatusEnvelope = serde_json::from_value(
        json!({"state_revision": 0, "control_revision": 0, "commit_sequence": 0}),
    )
    .expect("deserializes");
    assert_eq!(read, env);
    let with_lane = StatusEnvelope::with_control_lane();
    assert_eq!(
        with_lane.control_receipt_ring,
        Some(ControlReceiptRing::default())
    );
    assert_eq!(
        serde_json::to_value(&with_lane).expect("serializes")["control_receipt_ring"],
        json!([])
    );
}

#[test]
fn the_slot_state_is_the_installed_slot_state() {
    let mut env = envelope();
    assert_eq!(env.pending_commit_state(), PendingCommitState::Occupied);
    if let Some(slot) = env.pending_commit.as_mut() {
        slot.state = PendingCommitState::Repairing;
    }
    assert_eq!(env.pending_commit_state(), PendingCommitState::Repairing);
}

#[test]
fn records_refuse_invalid_values_when_read() {
    let mut json = serde_json::to_value(envelope()).expect("serializes");
    json["pending_commit"]["state"] = json!("PREPARED");
    assert!(serde_json::from_value::<StatusEnvelope>(json).is_err());
    let mut json = serde_json::to_value(envelope()).expect("serializes");
    json["pending_commit"]["after_digest"] = json!("sha256:xyz");
    assert!(serde_json::from_value::<StatusEnvelope>(json).is_err());
    let mut json = serde_json::to_value(envelope()).expect("serializes");
    json["commit_sequence"] = json!(-1);
    assert!(serde_json::from_value::<StatusEnvelope>(json).is_err());
}

#[test]
fn a_full_ring_refuses_and_keeps_every_receipt() {
    let limits = m0_ring();
    let mut ring = ring_of(8);
    assert!(ring.is_full(&limits));
    let before = ring.clone();
    assert_eq!(
        ring.append(receipt(9, 9), &limits),
        Err(RingError::Full { unpublished: 8 })
    );
    assert_eq!(ring, before);
}

#[test]
fn a_published_receipt_gives_way_oldest_first() {
    let limits = m0_ring();
    let mut ring = ring_of(8);
    ring.mark_published(seq(3)).expect("in ring");
    ring.mark_published(seq(5)).expect("in ring");
    assert_eq!(ring.unpublished(), 6);
    assert!(!ring.is_full(&limits));
    ring.append(receipt(9, 9), &limits)
        .expect("room after publication");
    let sequences: Vec<u64> = ring
        .entries()
        .iter()
        .map(|r| r.commit_sequence.get())
        .collect();
    assert_eq!(sequences, [1, 2, 4, 5, 6, 7, 8, 9]);
    ring.append(receipt(10, 10), &limits)
        .expect("room after publication");
    let sequences: Vec<u64> = ring
        .entries()
        .iter()
        .map(|r| r.commit_sequence.get())
        .collect();
    assert_eq!(sequences, [1, 2, 4, 6, 7, 8, 9, 10]);
    assert_eq!(
        ring.append(receipt(11, 11), &limits),
        Err(RingError::Full { unpublished: 8 })
    );
}

#[test]
fn a_ring_with_room_keeps_published_receipts() {
    let limits = m0_ring();
    let mut ring = ring_of(2);
    ring.mark_published(seq(1)).expect("in ring");
    ring.append(receipt(3, 3), &limits).expect("room");
    assert_eq!(ring.entries().len(), 3);
    assert_eq!(ring.entries()[0].state, ControlReceiptState::Published);
}

#[test]
fn a_ring_takes_receipts_only_in_commit_order_and_unpublished() {
    let limits = m0_ring();
    let mut ring = ring_of(2);
    assert_eq!(
        ring.append(receipt(2, 3), &limits),
        Err(RingError::OutOfOrder {
            last: seq(2),
            refused: seq(2)
        })
    );
    assert_eq!(
        ring.append(receipt(3, 2), &limits),
        Err(RingError::OutOfOrder {
            last: seq(2),
            refused: seq(3)
        })
    );
    let mut published = receipt(3, 3);
    published.state = ControlReceiptState::Published;
    assert_eq!(
        ring.append(published, &limits),
        Err(RingError::AlreadyPublished(seq(3)))
    );
    assert_eq!(ring.entries().len(), 2);
    assert_eq!(
        ring.mark_published(seq(7)),
        Err(RingError::NotInRing(seq(7)))
    );
}

#[test]
fn a_ring_out_of_commit_order_is_refused_when_read() {
    let entries = serde_json::to_value(vec![receipt(2, 2), receipt(1, 1)]).expect("serializes");
    assert!(serde_json::from_value::<ControlReceiptRing>(entries).is_err());
    let entries = serde_json::to_value(vec![receipt(1, 1), receipt(2, 2)]).expect("serializes");
    let ring: ControlReceiptRing = serde_json::from_value(entries).expect("deserializes");
    assert_eq!(ring, ring_of(2));
}

#[test]
fn the_envelope_schema_matches_its_snapshot() {
    let schema = schema_for!(StatusEnvelope);
    let actual = format!(
        "{}\n",
        serde_json::to_string_pretty(&schema).expect("serializes")
    );
    if std::env::var_os("AUTOBOT_UPDATE_SNAPSHOTS").is_some() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/status/status_envelope.schema.json"
        );
        std::fs::write(path, &actual).expect("writes the snapshot");
        return;
    }
    assert!(
        actual == SNAPSHOT,
        "the StatusEnvelope schema differs from src/status/status_envelope.schema.json; \
         rerun with AUTOBOT_UPDATE_SNAPSHOTS=1 to accept it. Actual schema:\n{actual}"
    );
}

#[test]
fn a_provider_binding_refuses_an_empty_name() {
    assert_eq!(
        ProviderBinding::new("fake-forge", "comment"),
        Ok(ProviderBinding {
            provider: "fake-forge".to_owned(),
            operation: "comment".to_owned(),
        })
    );
    assert_eq!(
        ProviderBinding::new("", "comment"),
        Err(ValueError::Empty("a provider binding's provider"))
    );
    assert_eq!(
        ProviderBinding::new("fake-forge", ""),
        Err(ValueError::Empty("a provider binding's operation"))
    );

    let parsed: ProviderBinding =
        serde_json::from_value(json!({ "provider": "fake-forge", "operation": "comment" }))
            .expect("non-empty names deserialize");
    assert_eq!(
        parsed,
        ProviderBinding::new("fake-forge", "comment").unwrap()
    );
    for (provider, operation, what) in
        [("", "comment", "provider"), ("fake-forge", "", "operation")]
    {
        let refused = serde_json::from_value::<ProviderBinding>(
            json!({ "provider": provider, "operation": operation }),
        )
        .expect_err("an empty name is refused");
        assert!(
            refused
                .to_string()
                .contains(&format!("a provider binding's {what} is empty")),
            "{refused}"
        );
    }
}
