use super::*;
use crate::digest::{control_digest, digest, domain_digest};
use crate::profile::{ControlRing, Profile};
use crate::status::StatusEnvelope;
use crate::types::{
    CommitSequence, ControlRevision, Lane, LaneRevision, ObjectRef, StateRevision, Uid,
};
use std::collections::BTreeSet;
use std::num::NonZeroU32;

const M0_PROFILE: &str = include_str!("../../../../profiles/m0.toml");

fn ring() -> ControlRing {
    Profile::parse(M0_PROFILE)
        .expect("profiles/m0.toml parses")
        .values()
        .control_ring
}

fn key(name: &str) -> ObjectKey {
    ObjectKey {
        kind: "WorkContext".parse().expect("kind"),
        namespace: "ns".parse().expect("namespace"),
        name: name.parse().expect("name"),
    }
}

fn uid(s: &str) -> Uid {
    s.parse().expect("uid")
}

fn origin() -> Origin {
    Origin {
        create_receipt_uid: uid("create-a"),
        input_digest: digest("spec").expect("digest"),
        context_uid: uid("ctx"),
    }
}

/// The object `a` at resource version `rv` with `status`.
fn object(rv: u64, status: Option<Status>) -> Object {
    Object {
        key: key("a"),
        uid: uid("uid-a"),
        resource_version: ResourceVersion::from(rv),
        origin: origin(),
        spec: "spec".to_owned(),
        status,
    }
}

/// [`object`], boxed as a store result holds it.
fn boxed(rv: u64, status: Option<Status>) -> Box<Object> {
    Box::new(object(rv, status))
}

fn status(control_lane: bool) -> Status {
    Status {
        envelope: if control_lane {
            StatusEnvelope::with_control_lane()
        } else {
            StatusEnvelope::default()
        },
        domain: "d0".to_owned(),
        control: "c0".to_owned(),
    }
}

/// The caller's audit-event fields of a command written by `actor`.
fn event(actor: &str) -> EventFields {
    EventFields {
        source_uid: uid("src"),
        event_type: "Hold".to_owned(),
        actor: actor.parse().expect("principal"),
        causation_id: "cause".to_owned(),
        correlation_id: "corr".to_owned(),
        schema_version: 1,
    }
}

fn domain_change(fields: &str) -> Change {
    Change::Domain(DomainChange {
        fields: fields.to_owned(),
        receipt: ObjectRef {
            namespace: "ns".parse().expect("namespace"),
            name: "receipt-c1".parse().expect("name"),
            uid: uid("receipt-c1"),
        },
        event: event("controller"),
        effect_intents: Vec::new(),
    })
}

fn control_change(fields: &str) -> Change {
    Change::Control(ControlChange {
        fields: fields.to_owned(),
        event: event("controller"),
    })
}

type Apply = fn(&Object, &Status) -> Result<Change, GuardRefusal>;

fn commit(pin: Pin, transition: Apply) -> Commit<Apply> {
    Commit::new(CommitRequest {
        target: key("a"),
        uid: uid("uid-a"),
        command_uid: uid("c1"),
        pin,
        ring: ring(),
        transition,
    })
}

fn domain_pin(r: u64) -> Pin {
    Pin::Revision(LaneRevision::State(StateRevision::new(r).expect("rev")))
}

fn set_domain(_: &Object, _: &Status) -> Result<Change, GuardRefusal> {
    Ok(domain_change("d1"))
}

fn set_control(_: &Object, _: &Status) -> Result<Change, GuardRefusal> {
    Ok(control_change("c1"))
}

/// Requires the control fields to read `c0`, as an acceptance requires `RUNNING`.
fn guarded(_: &Object, status: &Status) -> Result<Change, GuardRefusal> {
    if status.control == "c0" {
        Ok(domain_change("d1"))
    } else {
        Err(GuardRefusal {
            guard: "hold".to_owned(),
        })
    }
}

/// Steps `p`, expecting an operation.
fn expect_op<P: Protocol>(p: &mut P) -> StoreOp
where
    P::Outcome: std::fmt::Debug,
{
    match p.step() {
        Step::Op(op) => op,
        Step::Done(outcome) => panic!("expected an operation, got {outcome:?}"),
    }
}

/// Steps `p`, expecting the outcome.
fn expect_done<P: Protocol>(p: &mut P) -> P::Outcome {
    match p.step() {
        Step::Done(outcome) => outcome,
        Step::Op(op) => panic!("expected an outcome, got {op:?}"),
    }
}

/// The status `op` writes, and the resource version it is conditioned on.
fn written(op: StoreOp) -> (Status, ResourceVersion) {
    match op {
        StoreOp::UpdateStatus {
            status,
            resource_version,
            ..
        } => (*status, resource_version),
        other => panic!("expected a status update, got {other:?}"),
    }
}

fn is_get(op: &StoreOp) -> bool {
    matches!(op, StoreOp::Get { key: k } if *k == key("a"))
}

#[test]
fn domain_commit_writes_the_slot_and_only_the_domain_fields() {
    let mut p = commit(domain_pin(0), set_domain);
    assert!(is_get(&expect_op(&mut p)));
    let mut before = status(true);
    before.envelope.control_revision = ControlRevision::new(3).expect("rev");
    before.envelope.commit_sequence = CommitSequence::new(3).expect("seq");
    p.resume(StoreResult::Object(boxed(7, Some(before.clone()))))
        .expect("read");
    let (after, rv) = written(expect_op(&mut p));
    assert_eq!(rv, ResourceVersion::from(7));
    assert_eq!(after.domain, "d1");
    assert_eq!(after.control, before.control);
    assert_eq!(
        after.envelope.control_receipt_ring,
        before.envelope.control_receipt_ring
    );
    assert_eq!(
        after.envelope.control_revision,
        before.envelope.control_revision
    );
    assert_eq!(after.envelope.state_revision.get(), 1);
    assert_eq!(after.envelope.commit_sequence.get(), 4);
    let slot = after.envelope.pending_commit.clone().expect("slot");
    assert_eq!(slot.command_uid, uid("c1"));
    assert_eq!(slot.receipt_uid, uid("receipt-c1"));
    assert_eq!(slot.commit_sequence.get(), 4);
    assert_eq!(slot.expected_revision.get(), 0);
    assert_eq!(slot.proposed_revision.get(), 1);
    assert_eq!(slot.control_revision_at_commit.get(), 3);
    assert_eq!(slot.before_digest, domain_digest(&before).expect("digest"));
    assert_eq!(slot.after_digest, domain_digest(&after).expect("digest"));
    assert_eq!(slot.state, crate::status::PendingCommitState::Occupied);
    let audit = &slot.audit_envelope;
    assert_eq!(audit.aggregate_uid, uid("uid-a"));
    assert_eq!(audit.commit_sequence, slot.commit_sequence);
    assert_eq!(audit.lane, Lane::Domain);
    assert_eq!(audit.state_revision, slot.proposed_revision);
    assert_eq!(audit.control_revision.get(), 3);
    assert_eq!(audit.state_digest, slot.after_digest);
    assert_eq!(audit.actor, "controller".parse().expect("principal"));
    assert_eq!(audit.event_type, "Hold");
    assert_eq!(
        after
            .envelope
            .last_receipt_ref
            .as_ref()
            .map(|r| r.uid.clone()),
        Some(uid("receipt-c1"))
    );
    p.resume(StoreResult::Object(Box::new(object(8, Some(after)))))
        .expect("write");
    assert_eq!(
        expect_done(&mut p),
        CommitOutcome::Committed {
            revision: LaneRevision::State(StateRevision::new(1).expect("rev")),
            commit_sequence: CommitSequence::new(4).expect("seq"),
        }
    );
}

#[test]
fn control_commit_appends_a_receipt_and_preserves_the_domain_side() {
    let control_pin = Pin::Revision(LaneRevision::Control(ControlRevision::ZERO));
    let mut p = commit(control_pin, set_control);
    expect_op(&mut p);
    let mut before = status(true);
    before.envelope.state_revision = StateRevision::new(2).expect("rev");
    before.envelope.commit_sequence = CommitSequence::new(2).expect("seq");
    p.resume(StoreResult::Object(boxed(1, Some(before.clone()))))
        .expect("read");
    let (after, _) = written(expect_op(&mut p));
    assert_eq!(after.domain, before.domain);
    assert_eq!(
        after.envelope.pending_commit,
        before.envelope.pending_commit
    );
    assert_eq!(
        after.envelope.state_revision,
        before.envelope.state_revision
    );
    assert_eq!(
        after.envelope.last_receipt_ref,
        before.envelope.last_receipt_ref
    );
    assert_eq!(after.control, "c1");
    assert_eq!(after.envelope.control_revision.get(), 1);
    assert_eq!(after.envelope.commit_sequence.get(), 3);
    let ring = after.envelope.control_receipt_ring.clone().expect("ring");
    let receipt = ring.entries().last().expect("receipt");
    assert_eq!(receipt.control_uid, uid("c1"));
    assert_eq!(receipt.control_revision.get(), 1);
    assert_eq!(receipt.commit_sequence.get(), 3);
    assert_eq!(
        receipt.before_control_digest,
        control_digest(&before).expect("digest")
    );
    assert_eq!(
        receipt.after_control_digest,
        control_digest(&after).expect("digest")
    );
    assert_eq!(receipt.audit_envelope.state_revision.get(), 2);
    let audit = &receipt.audit_envelope;
    assert_eq!(audit.aggregate_uid, uid("uid-a"));
    assert_eq!(audit.commit_sequence, receipt.commit_sequence);
    assert_eq!(audit.lane, Lane::Control);
    assert_eq!(audit.control_revision, receipt.control_revision);
    assert_eq!(audit.state_digest, receipt.after_control_digest);
    assert_eq!(audit.actor, receipt.principal);
}

/// The status `transition` writes when it commits at `pin` on `before`.
fn commit_once(pin: Pin, transition: Apply, before: &Status) -> Status {
    let mut p = commit(pin, transition);
    expect_op(&mut p);
    p.resume(StoreResult::Object(boxed(1, Some(before.clone()))))
        .expect("read");
    written(expect_op(&mut p)).0
}

#[test]
fn a_slot_digest_is_the_canonical_digest_of_the_domain_text_and_the_state_revision() {
    let after = commit_once(domain_pin(0), set_domain, &status(true));
    let slot = after.envelope.pending_commit.expect("slot");
    let at = |revision: u64| {
        digest(&serde_json::json!({"domain": "d1", "state_revision": revision})).expect("digest")
    };
    assert_eq!(slot.after_digest, at(1));
    assert_ne!(slot.after_digest, at(2));
    assert_eq!(
        slot.before_digest,
        digest(&serde_json::json!({"domain": "d0", "state_revision": 0})).expect("digest")
    );
}

#[test]
fn the_same_domain_text_at_another_state_revision_has_another_digest() {
    let first = commit_once(domain_pin(0), set_domain, &status(true));
    let mut later = status(true);
    later.envelope.state_revision = StateRevision::new(1).expect("rev");
    let second = commit_once(domain_pin(1), set_domain, &later);
    let digest_of = |s: Status| s.envelope.pending_commit.expect("slot").after_digest;
    assert_eq!(first.domain, second.domain);
    assert_ne!(digest_of(first), digest_of(second));
}

#[test]
fn a_control_receipt_digest_is_the_canonical_digest_of_the_control_text_and_revision() {
    let after = commit_once(
        Pin::Revision(LaneRevision::Control(ControlRevision::ZERO)),
        set_control,
        &status(true),
    );
    let ring = after.envelope.control_receipt_ring.expect("ring");
    let receipt = ring.entries().last().expect("receipt");
    let at = |control: &str, revision: u64| {
        digest(&serde_json::json!({"control": control, "control_revision": revision}))
            .expect("digest")
    };
    assert_eq!(receipt.before_control_digest, at("c0", 0));
    assert_eq!(receipt.after_control_digest, at("c1", 1));
}

#[test]
fn each_lane_s_commit_leaves_the_other_lane_s_digest() {
    let before = status(true);
    let control_pin = Pin::Revision(LaneRevision::Control(ControlRevision::ZERO));
    let after_control = commit_once(control_pin, set_control, &before);
    assert_eq!(domain_digest(&after_control), domain_digest(&before));
    assert_ne!(control_digest(&after_control), control_digest(&before));
    let after_domain = commit_once(domain_pin(0), set_domain, &before);
    assert_eq!(control_digest(&after_domain), control_digest(&before));
    assert_ne!(domain_digest(&after_domain), domain_digest(&before));
}

#[test]
fn an_uncertain_write_is_followed_by_a_read_and_never_by_an_outcome() {
    let mut p = commit(domain_pin(0), set_domain);
    expect_op(&mut p);
    p.resume(StoreResult::Object(boxed(1, Some(status(false)))))
        .expect("read");
    expect_op(&mut p);
    p.resume(StoreResult::Uncertain).expect("uncertain");
    assert!(is_get(&expect_op(&mut p)));
}

#[test]
fn an_uncertain_write_read_back_with_its_slot_is_committed() {
    let mut p = commit(domain_pin(0), set_domain);
    expect_op(&mut p);
    p.resume(StoreResult::Object(boxed(1, Some(status(false)))))
        .expect("read");
    let (after, _) = written(expect_op(&mut p));
    p.resume(StoreResult::Uncertain).expect("uncertain");
    expect_op(&mut p);
    p.resume(StoreResult::Object(Box::new(object(2, Some(after)))))
        .expect("read back");
    assert!(matches!(
        expect_done(&mut p),
        CommitOutcome::Committed { .. }
    ));
}

#[test]
fn an_uncertain_write_read_back_unapplied_is_sent_again_at_the_new_resource_version() {
    let mut p = commit(Pin::Current, set_domain);
    expect_op(&mut p);
    p.resume(StoreResult::Object(boxed(1, Some(status(false)))))
        .expect("read");
    expect_op(&mut p);
    p.resume(StoreResult::Uncertain).expect("uncertain");
    expect_op(&mut p);
    p.resume(StoreResult::Object(boxed(5, Some(status(false)))))
        .expect("read back");
    let (_, rv) = written(expect_op(&mut p));
    assert_eq!(rv, ResourceVersion::from(5));
}

#[test]
fn an_uncertain_write_whose_lane_moved_on_without_its_slot_is_passed() {
    let mut p = commit(Pin::Current, set_domain);
    expect_op(&mut p);
    p.resume(StoreResult::Object(boxed(1, Some(status(false)))))
        .expect("read");
    expect_op(&mut p);
    p.resume(StoreResult::Uncertain).expect("uncertain");
    expect_op(&mut p);
    let mut moved = status(false);
    moved.envelope.state_revision = StateRevision::new(1).expect("rev");
    moved.envelope.commit_sequence = CommitSequence::new(1).expect("seq");
    p.resume(StoreResult::Object(Box::new(object(2, Some(moved)))))
        .expect("read back");
    assert_eq!(
        expect_done(&mut p),
        CommitOutcome::Passed {
            observed: LaneRevision::State(StateRevision::new(1).expect("rev")),
            at: CommitSequence::new(1).expect("seq"),
        }
    );
}

#[test]
fn a_conflict_reads_again_and_the_guard_decides_on_the_new_state() {
    let mut p = commit(Pin::Current, guarded);
    expect_op(&mut p);
    p.resume(StoreResult::Object(boxed(1, Some(status(true)))))
        .expect("read");
    expect_op(&mut p);
    p.resume(StoreResult::Conflict).expect("conflict");
    assert!(is_get(&expect_op(&mut p)));
    let mut held = status(true);
    held.control = "held".to_owned();
    held.envelope.control_revision = ControlRevision::new(1).expect("rev");
    held.envelope.commit_sequence = CommitSequence::new(1).expect("seq");
    p.resume(StoreResult::Object(Box::new(object(2, Some(held)))))
        .expect("read");
    assert_eq!(
        expect_done(&mut p),
        CommitOutcome::Refused {
            guard: "hold".to_owned(),
            read: LaneRevision::State(StateRevision::ZERO),
            at: CommitSequence::new(1).expect("seq"),
        }
    );
}

#[test]
fn an_unavailable_read_is_repeated() {
    let mut p = commit(domain_pin(0), set_domain);
    expect_op(&mut p);
    p.resume(StoreResult::Unavailable).expect("unavailable");
    assert!(is_get(&expect_op(&mut p)));
}

#[test]
fn a_result_that_does_not_answer_the_operation_is_refused_and_changes_nothing() {
    let mut p = commit(domain_pin(0), set_domain);
    assert_eq!(
        p.resume(StoreResult::Conflict),
        Err(ProtocolError::NotWaiting { result: "Conflict" })
    );
    expect_op(&mut p);
    assert_eq!(
        p.resume(StoreResult::AlreadyExists),
        Err(ProtocolError::Unexpected {
            op: OpKind::Get,
            result: "AlreadyExists"
        })
    );
    p.resume(StoreResult::Object(boxed(1, Some(status(false)))))
        .expect("the read is still outstanding");
}

#[test]
fn a_recreated_object_is_reported_replaced() {
    let mut p = commit(domain_pin(0), set_domain);
    expect_op(&mut p);
    let mut other = object(1, Some(status(false)));
    other.uid = uid("uid-other");
    p.resume(StoreResult::Object(Box::new(other)))
        .expect("read");
    assert_eq!(
        expect_done(&mut p),
        CommitOutcome::Missing(Missing::Replaced {
            uid: uid("uid-other")
        })
    );
}

#[test]
fn a_change_of_the_other_lane_is_not_written() {
    let mut p = commit(domain_pin(0), set_control);
    expect_op(&mut p);
    p.resume(StoreResult::Object(boxed(1, Some(status(true)))))
        .expect("read");
    assert_eq!(expect_done(&mut p), CommitOutcome::LaneMismatch);
}

#[test]
fn create_resolves_a_lost_acknowledgement_by_reading_the_name() {
    let mut p = Create::new(key("a"), "spec".to_owned(), origin());
    assert!(matches!(expect_op(&mut p), StoreOp::Create { .. }));
    p.resume(StoreResult::Uncertain).expect("uncertain");
    assert!(is_get(&expect_op(&mut p)));
    p.resume(StoreResult::NotFound).expect("not found");
    assert!(matches!(expect_op(&mut p), StoreOp::Create { key: k, .. } if k == key("a")));
    p.resume(StoreResult::AlreadyExists).expect("exists");
    assert!(is_get(&expect_op(&mut p)));
    p.resume(StoreResult::Object(Box::new(object(1, None))))
        .expect("read");
    assert_eq!(
        expect_done(&mut p),
        CreateOutcome::Created(Box::new(object(1, None)))
    );
}

#[test]
fn create_finding_another_origin_or_an_empty_name_after_exists_creates_nothing() {
    let mut p = Create::new(key("a"), "spec".to_owned(), origin());
    expect_op(&mut p);
    p.resume(StoreResult::AlreadyExists).expect("exists");
    expect_op(&mut p);
    let mut other = object(1, None);
    other.origin.input_digest = digest("other").expect("digest");
    p.resume(StoreResult::Object(Box::new(other.clone())))
        .expect("read");
    assert_eq!(expect_done(&mut p), CreateOutcome::Taken(Box::new(other)));

    let mut p = Create::new(key("a"), "spec".to_owned(), origin());
    expect_op(&mut p);
    p.resume(StoreResult::AlreadyExists).expect("exists");
    expect_op(&mut p);
    p.resume(StoreResult::NotFound).expect("not found");
    assert_eq!(expect_done(&mut p), CreateOutcome::Vanished);
}

#[test]
fn clearing_writes_only_the_slot_state() {
    let mut slotted = status(false);
    let mut p = commit(domain_pin(0), set_domain);
    expect_op(&mut p);
    p.resume(StoreResult::Object(boxed(1, Some(slotted.clone()))))
        .expect("read");
    slotted = written(expect_op(&mut p)).0;

    let mut clear = ClearSlot::new(key("a"), uid("uid-a"), uid("c1"));
    expect_op(&mut clear);
    clear
        .resume(StoreResult::Object(boxed(2, Some(slotted.clone()))))
        .expect("read");
    let (cleared, _) = written(expect_op(&mut clear));
    let mut expected = slotted;
    if let Some(slot) = &mut expected.envelope.pending_commit {
        slot.state = crate::status::PendingCommitState::Cleared;
    }
    assert_eq!(cleared, expected);
}

#[test]
fn triggers_list_first_and_relist_after_expiry_or_the_bound() {
    let mut t = Triggers::new(
        "WorkContext".parse().expect("kind"),
        "ns".parse().expect("namespace"),
        NonZeroU32::new(2).expect("non-zero"),
    );
    assert!(matches!(t.step(), StoreOp::List { .. }));
    t.resume(StoreResult::Unavailable).expect("unavailable");
    assert!(matches!(t.step(), StoreOp::List { .. }));
    t.resume(StoreResult::Listed {
        items: vec![object(3, None)],
        resource_version: ResourceVersion::from(3),
    })
    .expect("listed");
    assert_eq!(t.take_due(), BTreeSet::from([key("a")]));
    let empty_batch = || StoreResult::Events {
        events: Vec::new(),
        cursor: ResourceVersion::from(3),
    };
    assert!(matches!(t.step(), StoreOp::Watch { since, .. } if since == ResourceVersion::from(3)));
    t.resume(empty_batch()).expect("batch");
    assert!(matches!(t.step(), StoreOp::Watch { .. }));
    t.resume(empty_batch()).expect("batch");
    assert!(
        matches!(t.step(), StoreOp::List { .. }),
        "relist after two batches"
    );
    t.resume(StoreResult::Listed {
        items: vec![object(3, None)],
        resource_version: ResourceVersion::from(3),
    })
    .expect("listed");
    assert!(
        t.take_due().is_empty(),
        "an unchanged object is not due again"
    );
    assert!(matches!(t.step(), StoreOp::Watch { .. }));
    t.resume(StoreResult::Expired).expect("expired");
    assert!(matches!(t.step(), StoreOp::List { .. }));
    t.resume(StoreResult::Listed {
        items: Vec::new(),
        resource_version: ResourceVersion::from(4),
    })
    .expect("listed");
    assert_eq!(
        t.take_due(),
        BTreeSet::from([key("a")]),
        "a vanished object is due"
    );
}

#[test]
fn the_conformance_suite_parses_with_distinct_names() {
    let scripts = conformance::scripts().expect("the suite parses");
    let names: BTreeSet<_> = scripts.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names.len(), scripts.len());
    assert!(scripts.iter().all(|s| !s.steps.is_empty()));
}

#[test]
fn a_script_step_with_an_unknown_key_is_refused() {
    let text = "name = \"x\"\nsummary = \"y\"\n\n[[steps]]\ndo = \"create\"\nobject = \"a\"\nexpect = \"created\"\n";
    toml::from_str::<conformance::Script>(text).expect("the script parses");
    let misspelled = text.replace("expect =", "expected =");
    assert!(toml::from_str::<conformance::Script>(&misspelled).is_err());
    let extra = format!("{text}pinn = 0\n");
    assert!(toml::from_str::<conformance::Script>(&extra).is_err());
}

#[test]
fn a_crash_after_zero_writes_is_refused() {
    let script = |n: u32| {
        format!(
            "name = \"x\"\nsummary = \"y\"\n\n[[steps]]\ndo = \"inject\"\nfault = {{ kind = \"crash\", after_writes = {n} }}\n"
        )
    };
    toml::from_str::<conformance::Script>(&script(1)).expect("one write parses");
    assert!(toml::from_str::<conformance::Script>(&script(0)).is_err());
}

#[test]
fn a_fault_the_script_did_not_arm_fails_the_step_it_fires_in() {
    let text = "name = \"x\"\nsummary = \"y\"\n\n[[steps]]\ndo = \"create\"\nobject = \"a\"\nexpect = \"created\"\n";
    let script: conformance::Script = toml::from_str(text).expect("the script parses");
    let mut run = conformance::ScriptRun::new(&script, ring());
    let conformance::Action::Op(op) = run.step() else {
        panic!("the create step starts with an operation");
    };
    run.fired(conformance::Fault::LostCreateAck);
    run.resume(created(op));
    let conformance::Action::Done(Err(failure)) = run.step() else {
        panic!("the script must fail");
    };
    assert_eq!(failure.step, 0);
    assert!(failure.message.contains("without being armed"), "{failure}");
}

#[test]
fn an_armed_fault_that_never_fires_fails_its_step() {
    let text = "name = \"x\"\nsummary = \"y\"\n\n[[steps]]\ndo = \"inject\"\nfault = { kind = \"lost_create_ack\" }\n\n[[steps]]\ndo = \"create\"\nobject = \"a\"\nexpect = \"created\"\n";
    let script: conformance::Script = toml::from_str(text).expect("the script parses");
    let mut run = conformance::ScriptRun::new(&script, ring());
    assert_eq!(
        run.step(),
        conformance::Action::Arm(conformance::Fault::LostCreateAck)
    );
    let conformance::Action::Op(op) = run.step() else {
        panic!("the create step starts with an operation");
    };
    run.resume(created(op));
    let conformance::Action::Done(Err(failure)) = run.step() else {
        panic!("the script must fail");
    };
    assert_eq!(failure.step, 1);
    assert!(failure.message.contains("never fired"), "{failure}");
}

/// The result of a store that performs the create `op`.
fn created(op: StoreOp) -> StoreResult {
    let StoreOp::Create { key, spec, origin } = op else {
        panic!("expected a create, got {op:?}");
    };
    StoreResult::Object(Box::new(Object {
        key,
        uid: uid("uid-a"),
        resource_version: ResourceVersion::from(1),
        origin,
        spec,
        status: None,
    }))
}

/// A receipt check that accepts a receipt whose domain fields are `TERMINAL`.
fn terminal(_: &Object, receipt: &Object) -> bool {
    receipt
        .status
        .as_ref()
        .is_some_and(|s| s.domain == "TERMINAL")
}

/// The delete of `a`, reading the receipt `create-a` and writing the tombstone `tombstone-a`.
fn delete_a() -> Delete<fn(&Object, &Object) -> bool> {
    Delete::new(DeleteRequest {
        target: key("a"),
        uid: uid("uid-a"),
        create_receipt: key("create-a"),
        tombstone: key("tombstone-a"),
        tombstone_spec: "delete a".to_owned(),
        check: terminal as fn(&Object, &Object) -> bool,
    })
}

/// The create receipt of `a` with `domain` as its domain fields.
fn receipt(domain: &str) -> Box<Object> {
    let mut receipt = object(1, Some(status(false)));
    receipt.key = key("create-a");
    receipt.uid = uid("uid-create-a");
    if let Some(s) = &mut receipt.status {
        s.domain = domain.to_owned();
    }
    Box::new(receipt)
}

/// The tombstone of `a` that `op`, a create, writes, as the store would return it.
fn tombstone_from(op: StoreOp) -> Box<Object> {
    let StoreOp::Create {
        key: k,
        spec,
        origin,
    } = op
    else {
        panic!("expected the tombstone create, got {op:?}");
    };
    Box::new(Object {
        key: k,
        uid: uid("uid-tombstone-a"),
        resource_version: ResourceVersion::from(9),
        origin,
        spec,
        status: None,
    })
}

/// Steps `p` through reading `a` at resource version `rv` and its terminal receipt, to the
/// tombstone create, which it answers; returns the tombstone.
fn delete_to_tombstone(p: &mut Delete<fn(&Object, &Object) -> bool>, rv: u64) -> Box<Object> {
    assert!(is_get(&expect_op(p)));
    p.resume(StoreResult::Object(boxed(rv, None)))
        .expect("target");
    assert_eq!(
        expect_op(p),
        StoreOp::Get {
            key: key("create-a")
        }
    );
    p.resume(StoreResult::Object(receipt("TERMINAL")))
        .expect("receipt");
    let tombstone = tombstone_from(expect_op(p));
    p.resume(StoreResult::Object(tombstone.clone()))
        .expect("tombstone");
    tombstone
}

/// The delete `op` of `a`, expected at resource version `rv`.
fn assert_delete_at(op: &StoreOp, rv: u64) {
    assert_eq!(
        *op,
        StoreOp::Delete {
            key: key("a"),
            uid: uid("uid-a"),
            resource_version: ResourceVersion::from(rv),
        }
    );
}

#[test]
fn a_delete_writes_nothing_while_the_create_receipt_is_absent_or_not_terminal() {
    let mut p = delete_a();
    expect_op(&mut p);
    p.resume(StoreResult::Object(boxed(1, None)))
        .expect("target");
    expect_op(&mut p);
    p.resume(StoreResult::NotFound).expect("no receipt");
    assert_eq!(
        expect_done(&mut p),
        DeleteOutcome::Refused {
            create_receipt: None
        }
    );

    let mut p = delete_a();
    expect_op(&mut p);
    p.resume(StoreResult::Object(boxed(1, None)))
        .expect("target");
    expect_op(&mut p);
    p.resume(StoreResult::Object(receipt("PREPARED")))
        .expect("receipt");
    assert_eq!(
        expect_done(&mut p),
        DeleteOutcome::Refused {
            create_receipt: Some(receipt("PREPARED"))
        }
    );
}

#[test]
fn a_delete_writes_the_tombstone_with_the_target_s_origin_before_deleting_the_target() {
    let mut p = delete_a();
    assert!(is_get(&expect_op(&mut p)));
    p.resume(StoreResult::Object(boxed(3, None)))
        .expect("target");
    expect_op(&mut p);
    p.resume(StoreResult::Object(receipt("TERMINAL")))
        .expect("receipt");
    let create = expect_op(&mut p);
    assert_eq!(
        create,
        StoreOp::Create {
            key: key("tombstone-a"),
            spec: "delete a".to_owned(),
            origin: origin(),
        }
    );
    let tombstone = tombstone_from(create);
    p.resume(StoreResult::Object(tombstone.clone()))
        .expect("tombstone");
    assert_delete_at(&expect_op(&mut p), 3);
    p.resume(StoreResult::Object(boxed(3, None)))
        .expect("deleted");
    assert_eq!(expect_done(&mut p), DeleteOutcome::Deleted(tombstone));
}

#[test]
fn a_conflicting_or_uncertain_delete_reads_again_and_deletes_the_same_incarnation() {
    let mut p = delete_a();
    let tombstone = delete_to_tombstone(&mut p, 3);
    assert_delete_at(&expect_op(&mut p), 3);
    p.resume(StoreResult::Conflict).expect("conflict");
    assert!(is_get(&expect_op(&mut p)));
    p.resume(StoreResult::Object(boxed(4, None)))
        .expect("reread");
    assert_delete_at(&expect_op(&mut p), 4);
    p.resume(StoreResult::Uncertain).expect("uncertain");
    assert!(is_get(&expect_op(&mut p)));
    p.resume(StoreResult::NotFound).expect("gone");
    assert_eq!(expect_done(&mut p), DeleteOutcome::Deleted(tombstone));

    let mut p = delete_a();
    delete_to_tombstone(&mut p, 3);
    expect_op(&mut p);
    p.resume(StoreResult::Uncertain).expect("uncertain");
    expect_op(&mut p);
    let mut recreated = object(5, None);
    recreated.uid = uid("uid-a-again");
    p.resume(StoreResult::Object(Box::new(recreated)))
        .expect("reread");
    assert_eq!(
        expect_done(&mut p),
        DeleteOutcome::Missing(Missing::Replaced {
            uid: uid("uid-a-again")
        })
    );
}

#[test]
fn a_delete_finding_its_target_absent_ends_deleted_only_with_its_tombstone() {
    let mut p = delete_a();
    expect_op(&mut p);
    p.resume(StoreResult::NotFound).expect("absent");
    assert_eq!(
        expect_op(&mut p),
        StoreOp::Get {
            key: key("tombstone-a")
        }
    );
    let mut tombstone = object(2, None);
    tombstone.key = key("tombstone-a");
    p.resume(StoreResult::Object(Box::new(tombstone.clone())))
        .expect("tombstone");
    assert_eq!(
        expect_done(&mut p),
        DeleteOutcome::Deleted(Box::new(tombstone))
    );

    let mut p = delete_a();
    expect_op(&mut p);
    p.resume(StoreResult::NotFound).expect("absent");
    expect_op(&mut p);
    p.resume(StoreResult::NotFound).expect("no tombstone");
    assert_eq!(
        expect_done(&mut p),
        DeleteOutcome::Missing(Missing::NotFound)
    );
}

#[test]
fn a_delete_whose_tombstone_name_holds_another_origin_leaves_the_target() {
    let mut p = delete_a();
    expect_op(&mut p);
    p.resume(StoreResult::Object(boxed(1, None)))
        .expect("target");
    expect_op(&mut p);
    p.resume(StoreResult::Object(receipt("TERMINAL")))
        .expect("receipt");
    expect_op(&mut p);
    p.resume(StoreResult::AlreadyExists).expect("exists");
    assert_eq!(
        expect_op(&mut p),
        StoreOp::Get {
            key: key("tombstone-a")
        }
    );
    let mut other = object(2, None);
    other.key = key("tombstone-a");
    other.origin.context_uid = uid("another-context");
    p.resume(StoreResult::Object(Box::new(other.clone())))
        .expect("read");
    assert_eq!(
        expect_done(&mut p),
        DeleteOutcome::TombstoneTaken(Box::new(other))
    );
}

#[test]
fn a_delete_refuses_a_result_that_does_not_answer_its_operation() {
    let mut p = delete_a();
    let get = expect_op(&mut p);
    assert_eq!(
        p.resume(StoreResult::Conflict),
        Err(ProtocolError::Unexpected {
            op: OpKind::Get,
            result: "Conflict"
        })
    );
    assert_eq!(expect_op(&mut p), get);
    p.resume(StoreResult::Unavailable).expect("unavailable");
    assert_eq!(expect_op(&mut p), get);
}
