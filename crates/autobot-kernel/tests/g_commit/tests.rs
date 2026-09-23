//! The group's tests: each fixture test runs one scenario on the in-memory store of
//! `autobot-fakes` with every guard enabled, and `scenario_faults_land_on_their_kind_only`
//! checks the scenarios' own store faults.

use super::scenarios::store::{
    AGGREGATE, OnWrite, Outage, aggregate, key, namespace, parse, read, status,
};
use super::scenarios::{commands, commit, create, projection};
use autobot_fakes::store::{Execution, MemStore};
use autobot_kernel::reducer::Guards;
use autobot_kernel::store::{Object, OpKind, StoreOp, StoreResult};
use autobot_testkit::harness::Driver;

#[test]
#[ignore = "awaiting #83"]
fn f1_idempotency() {
    commands::replay_identity(&mut MemStore::new());
}

#[test]
#[ignore = "awaiting #84"]
fn f2_receipt_durability() {
    commit::c1_crash_then_c2_repair(&mut MemStore::new(), &Guards::all());
}

#[test]
#[ignore = "awaiting #84"]
fn f3_receipt_barrier() {
    commit::receipt_barrier_until_verified(&mut MemStore::new(), &Guards::all());
}

#[test]
fn domain_commit_is_held_by_an_unresolved_slot() {
    commit::domain_commit_waits_on_the_barrier(&mut MemStore::new());
}

#[test]
#[ignore = "awaiting #81"]
fn f4_lane_separation() {
    commit::lane_separation(&mut MemStore::new(), &Guards::all());
}

#[test]
#[ignore = "awaiting #82"]
fn slot_clearing_changes_only_reconciliation_fields() {
    commit::clearing_is_reconciliation_only(&mut MemStore::new());
}

#[test]
#[ignore = "awaiting #81"]
fn full_ring_refuses_the_next_control_transition() {
    commit::full_ring_refuses(&mut MemStore::new());
}

#[test]
#[ignore = "awaiting #84"]
fn hold_beside_a_pending_domain_commit_with_verified_repair() {
    commit::hold_beside_pending_commit_then_repair(&mut MemStore::new(), &Guards::all());
}

#[test]
#[ignore = "awaiting #86"]
fn f5_create_identity() {
    create::lost_create_ack(&mut MemStore::new());
}

#[test]
#[ignore = "awaiting #86"]
fn no_delete_or_recreate_before_a_terminal_create_receipt() {
    create::no_delete_or_recreate_before_terminal_receipt(&mut MemStore::new());
}

#[test]
#[ignore = "awaiting #85"]
fn f6_projection_order() {
    projection::out_of_order_and_conflicting(&mut MemStore::new(), &Guards::all());
}

/// The status write of `object`, as read, with its domain fields set to `domain`.
fn set_domain(object: &Object, domain: &str) -> StoreOp {
    let mut status = object
        .status
        .clone()
        .unwrap_or_else(|| panic!("{} has no status", object.key));
    domain.clone_into(&mut status.domain);
    StoreOp::UpdateStatus {
        key: object.key.clone(),
        uid: object.uid.clone(),
        resource_version: object.resource_version.clone(),
        status: Box::new(status),
    }
}

/// `OnWrite` arms its fault on the first write of its kind and operation and on nothing else;
/// `Outage` answers for the kinds it takes down without reaching the store and passes every
/// other kind through.
#[test]
fn scenario_faults_land_on_their_kind_only() {
    let mut store = MemStore::new();
    let (target, _) = aggregate(&mut store, "faults", false);
    let current = |store: &mut MemStore| {
        read(store, &target).unwrap_or_else(|| panic!("{target} is missing"))
    };

    let object = current(&mut store);
    let mut elsewhere = OnWrite::crash(&mut store, "CommandReceipt", OpKind::UpdateStatus);
    let applied = elsewhere.perform(set_domain(&object, "b"));
    assert!(
        matches!(applied, Execution::Result(StoreResult::Object(_))),
        "{applied:?}"
    );

    let object = current(&mut store);
    let mut crash = OnWrite::crash(&mut store, AGGREGATE, OpKind::UpdateStatus);
    let get = crash.perform(StoreOp::Get {
        key: target.clone(),
    });
    assert!(
        matches!(get, Execution::Result(StoreResult::Object(_))),
        "{get:?}"
    );
    assert_eq!(crash.perform(set_domain(&object, "c")), Execution::Crashed);
    assert_eq!(status(&mut store, &target).domain, "c");

    let object = current(&mut store);
    let mut down = Outage::new(&mut store, &[AGGREGATE]);
    let unavailable = Execution::Result(StoreResult::Unavailable);
    assert_eq!(
        down.perform(StoreOp::Get {
            key: target.clone()
        }),
        unavailable
    );
    let relist = StoreOp::List {
        kind: parse(AGGREGATE),
        namespace: namespace(),
    };
    assert_eq!(down.perform(relist), unavailable);
    assert_eq!(
        down.perform(set_domain(&object, "d")),
        Execution::Result(StoreResult::Uncertain)
    );
    let other = down.perform(StoreOp::Get {
        key: key("CommandReceipt", "absent"),
    });
    assert_eq!(other, Execution::Result(StoreResult::NotFound));
    assert_eq!(status(&mut store, &target).domain, "c");
}
