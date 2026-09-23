use super::*;
use autobot_kernel::profile::Profile;
use autobot_kernel::store::conformance::{
    self, ClearStep, CommitStep, CreateStep, InitializeStep, InjectStep, ScriptStep,
};
use autobot_kernel::store::{Commit, CommitOutcome, CommitRequest, Pin, conformance::CheckStep};
use autobot_kernel::types::Lane;
use std::num::NonZeroU32;

const M0_PROFILE: &str = include_str!("../../../../profiles/m0.toml");

fn ring() -> ControlRing {
    Profile::parse(M0_PROFILE)
        .expect("profiles/m0.toml parses")
        .values()
        .control_ring
}

fn script(name: &str) -> Script {
    conformance::scripts()
        .expect("the suite parses")
        .into_iter()
        .find(|s| s.name == name)
        .expect("the script exists")
}

#[test]
fn the_conformance_suite_passes_on_the_in_memory_store() {
    for script in conformance::scripts().expect("the suite parses") {
        let mut store = MemStore::new();
        if let Err(failure) = run_script(&mut store, &script, ring()) {
            panic!("{failure}");
        }
    }
}

/// Create, initialize, commit and clear: four writes, each checked by the last step.
fn four_writes() -> Vec<ScriptStep> {
    vec![
        ScriptStep::Create(CreateStep {
            object: "a".to_owned(),
            receipt: None,
            spec: String::new(),
            expect: "created".to_owned(),
        }),
        ScriptStep::Initialize(InitializeStep {
            object: "a".to_owned(),
            control_lane: false,
            domain: String::new(),
            control: String::new(),
            expect: "initialized".to_owned(),
        }),
        ScriptStep::Commit(CommitStep {
            object: "a".to_owned(),
            command: "c1".to_owned(),
            lane: Lane::Domain,
            pin: Some(0),
            fields: "one".to_owned(),
            require_control: None,
            interleave: None,
            expect: "committed".to_owned(),
        }),
        ScriptStep::Clear(ClearStep {
            object: "a".to_owned(),
            command: "c1".to_owned(),
            expect: "cleared".to_owned(),
        }),
        ScriptStep::Check(CheckStep {
            object: "a".to_owned(),
            state_revision: Some(1),
            control_revision: Some(0),
            commit_sequence: Some(1),
            domain: Some("one".to_owned()),
            control: None,
            slot: Some("cleared".to_owned()),
            slot_command: Some("c1".to_owned()),
            ring_entries: None,
        }),
    ]
}

#[test]
fn a_crash_after_any_write_is_recovered_by_a_fresh_protocol() {
    for write in 0..4 {
        let mut steps = four_writes();
        steps.insert(
            write,
            ScriptStep::Inject(InjectStep {
                fault: Fault::Crash {
                    after_writes: NonZeroU32::MIN,
                },
            }),
        );
        let script = Script {
            name: format!("crash after write {}", write + 1),
            summary: String::new(),
            steps,
        };
        let mut store = MemStore::new();
        if let Err(failure) = run_script(&mut store, &script, ring()) {
            panic!("{failure}");
        }
        assert_eq!(store.version, 4, "no write was repeated after the crash");
    }
}

#[test]
fn an_armed_crash_fires_on_the_write_that_applies() {
    let steps = four_writes();
    let ScriptStep::Create(create) = &steps[0] else {
        panic!("the first step creates");
    };
    let mut store = MemStore::new();
    store.arm(Fault::Crash {
        after_writes: NonZeroU32::MIN,
    });
    let key = ObjectKey {
        kind: conformance::KIND.parse().expect("kind"),
        namespace: conformance::NAMESPACE.parse().expect("namespace"),
        name: create.object.parse().expect("name"),
    };
    let origin = autobot_kernel::store::Origin {
        create_receipt_uid: "r".parse().expect("uid"),
        input_digest: autobot_kernel::store::fields_digest(""),
        context_uid: "ctx".parse().expect("uid"),
    };
    let mut create = autobot_kernel::store::Create::new(key.clone(), String::new(), origin);
    assert_eq!(run(&mut store, &mut create), Err(RunError::Crashed));
    assert!(store.object(&key).is_some(), "the crashed write applied");
}

/// A driver that ignores the resource-version condition of status updates.
fn run_ignoring_resource_versions(script: &Script) -> Result<(), Failure> {
    let mut store = MemStore::new();
    let mut script_run = ScriptRun::new(script, ring());
    loop {
        match script_run.step() {
            Action::Done(result) => return result,
            Action::Arm(fault) => store.arm(fault),
            Action::Op(StoreOp::UpdateStatus {
                key, uid, status, ..
            }) => {
                let current = store
                    .object(&key)
                    .map(|o| o.resource_version.clone())
                    .expect("the object exists");
                let op = StoreOp::UpdateStatus {
                    key,
                    uid,
                    resource_version: current,
                    status,
                };
                if let Execution::Result(result) = store.execute(op) {
                    script_run.resume(result);
                }
            }
            Action::Op(op) => {
                if let Execution::Result(result) = store.execute(op) {
                    script_run.resume(result);
                }
            }
        }
    }
}

#[test]
fn the_suite_fails_a_driver_that_accepts_a_stale_resource_version() {
    let failure = run_ignoring_resource_versions(&script("stale-resource-version"))
        .expect_err("a stale write must be caught");
    assert!(failure.message.contains("expected conflict"), "{failure}");
    let failure = run_ignoring_resource_versions(&script("control-between-read-and-write"))
        .expect_err("the accept write must not land over the hold");
    assert!(failure.message.contains("expected refused"), "{failure}");
}

#[test]
fn a_write_timeout_that_did_not_apply_changes_nothing_and_reports_uncertain() {
    let mut store = MemStore::new();
    store.arm(Fault::WriteTimeout { applied: false });
    let key = ObjectKey {
        kind: conformance::KIND.parse().expect("kind"),
        namespace: conformance::NAMESPACE.parse().expect("namespace"),
        name: "a".parse().expect("name"),
    };
    let origin = autobot_kernel::store::Origin {
        create_receipt_uid: "r".parse().expect("uid"),
        input_digest: autobot_kernel::store::fields_digest(""),
        context_uid: "ctx".parse().expect("uid"),
    };
    let op = StoreOp::Create {
        key: key.clone(),
        spec: String::new(),
        origin,
    };
    assert_eq!(store.execute(op), Execution::Result(StoreResult::Uncertain));
    assert!(store.object(&key).is_none());
    assert_eq!(store.version, 0);
}

#[test]
fn a_commit_on_a_missing_object_is_reported_missing() {
    let mut store = MemStore::new();
    let request = CommitRequest {
        target: ObjectKey {
            kind: conformance::KIND.parse().expect("kind"),
            namespace: conformance::NAMESPACE.parse().expect("namespace"),
            name: "absent".parse().expect("name"),
        },
        uid: "u".parse().expect("uid"),
        command_uid: "c".parse().expect("uid"),
        pin: Pin::Current,
        ring: ring(),
        transition: |_: &Object, _: &autobot_kernel::store::Status| {
            Err(autobot_kernel::store::GuardRefusal {
                guard: "unused".to_owned(),
            })
        },
    };
    let outcome = run(&mut store, &mut Commit::new(request)).expect("runs");
    assert_eq!(
        outcome,
        CommitOutcome::Missing(autobot_kernel::store::Missing::NotFound)
    );
}

/// Runs `script` on a driver whose fault hooks do nothing.
fn run_without_faults(script: &Script) -> Result<(), Failure> {
    let mut store = MemStore::new();
    let mut script_run = ScriptRun::new(script, ring());
    loop {
        match script_run.step() {
            Action::Done(result) => return result,
            Action::Arm(_) => {}
            Action::Op(op) => match store.execute(op) {
                Execution::Result(result) => script_run.resume(result),
                Execution::Crashed => script_run.crash(),
            },
        }
    }
}

#[test]
fn the_suite_fails_a_driver_whose_fault_hooks_do_nothing() {
    let scripts = conformance::scripts().expect("the suite parses");
    let with_faults: Vec<_> = scripts
        .iter()
        .filter(|s| {
            s.steps
                .iter()
                .any(|step| matches!(step, ScriptStep::Inject(_)))
        })
        .collect();
    let kinds: std::collections::BTreeSet<String> = with_faults
        .iter()
        .flat_map(|s| s.steps.iter())
        .filter_map(|step| match step {
            ScriptStep::Inject(inject) => format!("{:?}", inject.fault)
                .split([' ', '{'])
                .next()
                .map(str::to_owned),
            _ => None,
        })
        .collect();
    let expected = [
        "Crash",
        "WriteTimeout",
        "LateWrite",
        "LostCreateAck",
        "DropEvents",
        "DuplicateEvents",
        "ReorderEvents",
        "ExpireWatch",
    ];
    assert_eq!(
        kinds,
        expected.map(str::to_owned).into(),
        "every fault kind has a script"
    );
    for script in with_faults {
        let failure = run_without_faults(script).expect_err(&script.name);
        if ["uncertain-write", "lost-create-ack", "crash-between-writes"]
            .contains(&script.name.as_str())
        {
            assert!(failure.message.contains("never fired"), "{failure}");
        }
    }
}

#[test]
fn a_late_write_lands_right_after_the_next_read_is_answered() {
    let mut store = MemStore::new();
    let key = ObjectKey {
        kind: conformance::KIND.parse().expect("kind"),
        namespace: conformance::NAMESPACE.parse().expect("namespace"),
        name: "a".parse().expect("name"),
    };
    let origin = autobot_kernel::store::Origin {
        create_receipt_uid: "r".parse().expect("uid"),
        input_digest: autobot_kernel::store::fields_digest(""),
        context_uid: "ctx".parse().expect("uid"),
    };
    store.arm(Fault::LateWrite);
    let create = StoreOp::Create {
        key: key.clone(),
        spec: String::new(),
        origin,
    };
    assert_eq!(
        store.execute(create),
        Execution::Result(StoreResult::Uncertain)
    );
    assert_eq!(store.take_fired(), vec![Fault::LateWrite]);
    assert!(store.object(&key).is_none(), "not applied before the read");
    let read = store.execute(StoreOp::Get { key: key.clone() });
    assert_eq!(
        read,
        Execution::Result(StoreResult::NotFound),
        "the read sees it unapplied"
    );
    assert!(store.object(&key).is_some(), "applied right after the read");
}
