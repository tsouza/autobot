//! The replay-identity scenario (F-1): duplicate, different-payload, different-principal and
//! expired commands.

use super::harness::{
    Driver, MemDriver, aggregate, key, namespace, parse, profile, read, run, status,
};
use super::ports::{self, DomainCommand, ReceiptState};
use autobot_kernel::digest::{command_receipt_name, digest};
use autobot_kernel::types::{
    CommitObservation, CommitSequence, LaneRevision, RejectionProof, StateRevision,
};

/// One command commits once. Within the replay window its replay returns the original
/// receipt and never commits again; the same key with another payload or another principal
/// is rejected with the receipt and digest the key is bound to; after the window the replay
/// is `REPLAY_EXPIRED`. None of them changes the target or rewrites the receipt.
pub(crate) fn replay_identity(driver: &mut dyn Driver) {
    let commands = ports::commands();
    let target = aggregate(driver, "replay", false);
    let once = DomainCommand {
        idempotency_key: "k-once".to_owned(),
        principal: parse("writer"),
        target: target.0.clone(),
        target_uid: target.1.clone(),
        expected_revision: StateRevision::ZERO,
        input: "once".to_owned(),
        issued_day: 0,
    };
    let submit = |driver: &mut dyn Driver, command: &DomainCommand, today: u32| {
        run(driver, &mut *commands.submit(command, today)).done()
    };

    let first = submit(driver, &once, 0);
    assert_eq!(
        first,
        CommitObservation::Committed {
            revision: LaneRevision::State(StateRevision::new(1).unwrap_or_else(|e| panic!("{e}"))),
            commit_sequence: CommitSequence::new(1).unwrap_or_else(|e| panic!("{e}")),
        }
    );
    let committed = status(driver, &target.0);
    assert_eq!(committed.domain, "once");
    let receipt_name = command_receipt_name("k-once").unwrap_or_else(|e| panic!("{e}"));
    let receipt = read(driver, &key("CommandReceipt", receipt_name.as_str()))
        .unwrap_or_else(|| panic!("no receipt named {receipt_name}"));

    let replay = profile().values().replay;
    let last_day = replay.window_days.get() + replay.margin_days;
    assert_eq!(submit(driver, &once, 1), first);
    assert_eq!(submit(driver, &once, last_day - 1), first);

    let bound = RejectionProof::ReplayConflict {
        existing_receipt_uid: receipt.uid.clone(),
        bound_digest: digest("once").unwrap_or_else(|e| panic!("{e}")),
    };
    let other_payload = DomainCommand {
        input: "twice".to_owned(),
        ..once.clone()
    };
    assert_eq!(
        submit(driver, &other_payload, 1),
        CommitObservation::Rejected(bound.clone())
    );
    let other_principal = DomainCommand {
        principal: parse("intruder"),
        ..once.clone()
    };
    assert_eq!(
        submit(driver, &other_principal, 1),
        CommitObservation::Rejected(bound)
    );

    assert_eq!(
        submit(driver, &once, last_day + 1),
        CommitObservation::ReplayExpired
    );

    assert_eq!(status(driver, &target.0), committed);
    assert_eq!(
        run(driver, &mut *commands.receipt(&namespace(), "k-once")).done(),
        ReceiptState::Terminal(first)
    );
}

#[test]
#[ignore = "awaiting #83"]
fn f1_idempotency() {
    replay_identity(&mut MemDriver::new());
}
