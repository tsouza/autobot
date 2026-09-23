//! The create scenarios (F-5): a create whose acknowledgement is lost, and deletion or
//! recreation before the create receipt is terminal.

use super::store::{AGGREGATE, OnWrite, attempt, key, list, namespace, parse, port, read, run};
use autobot_kernel::digest::{CreateIndex, digest};
use autobot_kernel::store::conformance::Fault;
use autobot_kernel::store::{Object, OpKind};
use autobot_kernel::types::{RejectionProof, Uid};
use autobot_testkit::harness::{Driver, Run};
use autobot_testkit::registry::g_commit::{CreateCommand, CreateResult, CreatesPort, DeleteResult};

/// The create command for client request key `request` with `spec`.
fn create_command(request: &str, spec: &str) -> CreateCommand {
    CreateCommand {
        index: CreateIndex {
            context_uid: parse("context"),
            principal: parse("writer"),
            kind: AGGREGATE.to_owned(),
            parent_uid: None,
            client_request_key: request.to_owned(),
        },
        namespace: namespace(),
        spec: spec.to_owned(),
    }
}

/// The one object of the create kind the store holds.
fn only_object(driver: &mut dyn Driver) -> Object {
    let mut objects = list(driver, AGGREGATE);
    assert_eq!(objects.len(), 1, "{objects:#?}");
    objects.remove(0)
}

/// A create whose acknowledgement is lost resolves by reading its name to the one object it
/// made, whose origin metadata equals its receipt; a retry resolves to the same object, and a
/// changed payload under the same index conflicts instead of creating a second one.
pub(crate) fn lost_create_ack(driver: &mut dyn Driver) {
    let creates = port::<CreatesPort>();
    let command = create_command("request-1", "spec-1");

    let mut lost_ack = OnWrite::new(driver, AGGREGATE, OpKind::Create, Fault::LostCreateAck);
    let created = run(&mut lost_ack, &mut *creates.create(&command));
    let object = only_object(driver);
    assert_eq!(
        created,
        CreateResult::Created {
            uid: object.uid.clone()
        }
    );
    let target_name = command
        .index
        .target_name()
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(object.key.name, target_name);
    let receipt_name = command
        .index
        .receipt_name()
        .unwrap_or_else(|e| panic!("{e}"));
    let receipt = read(driver, &key("CommandReceipt", receipt_name.as_str()))
        .unwrap_or_else(|| panic!("no create receipt named {receipt_name}"));
    assert_eq!(object.origin.create_receipt_uid, receipt.uid);
    assert_eq!(
        object.origin.input_digest,
        digest("spec-1").unwrap_or_else(|e| panic!("{e}"))
    );
    assert_eq!(object.origin.context_uid, parse::<Uid>("context"));

    assert_eq!(run(driver, &mut *creates.create(&command)), created);
    let changed = create_command("request-1", "spec-2");
    let bound = RejectionProof::ReplayConflict {
        existing_receipt_uid: receipt.uid.clone(),
        bound_digest: object.origin.input_digest,
    };
    assert_eq!(
        run(driver, &mut *creates.create(&changed)),
        CreateResult::Rejected(bound)
    );
    assert_eq!(only_object(driver), object);
}

/// C1 creates the target and dies before its create receipt is terminal. A delete of the
/// target is refused while that receipt is not terminal, and C2's replay of the create
/// resolves to the same object instead of creating another.
pub(crate) fn no_delete_or_recreate_before_terminal_receipt(driver: &mut dyn Driver) {
    let creates = port::<CreatesPort>();
    let command = create_command("request-2", "spec");

    let mut crash = OnWrite::crash(driver, AGGREGATE, OpKind::Create);
    assert_eq!(
        attempt(&mut crash, &mut *creates.create(&command)),
        Run::Crashed
    );
    let object = only_object(driver);

    let mut delete = creates.delete(&object.key, &object.uid, &parse("writer"));
    assert_eq!(run(driver, &mut *delete), DeleteResult::Refused);
    assert_eq!(only_object(driver), object);

    assert_eq!(
        run(driver, &mut *creates.create(&command)),
        CreateResult::Created {
            uid: object.uid.clone()
        }
    );
    assert_eq!(only_object(driver).uid, object.uid);
}
