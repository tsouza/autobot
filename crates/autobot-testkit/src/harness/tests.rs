use super::*;
use autobot_kernel::digest::digest;
use autobot_kernel::store::{Create, CreateOutcome, ObjectKey, Origin, StoreResult};

fn key(name: &str) -> ObjectKey {
    ObjectKey {
        kind: "Probe".parse().expect("kind"),
        namespace: "testkit".parse().expect("namespace"),
        name: name.parse().expect("name"),
    }
}

fn origin(receipt: &str) -> Origin {
    Origin {
        create_receipt_uid: receipt.parse().expect("uid"),
        input_digest: digest(receipt).expect("digest"),
        context_uid: "context".parse().expect("uid"),
    }
}

fn create(name: &str) -> Create {
    Create::new(key(name), String::new(), origin(name))
}

const BOUNDS: Bounds = Bounds {
    steps: 16,
    ticks: 5,
};

/// A protocol that reads one object forever.
struct Forever;

impl Protocol for Forever {
    type Outcome = ();

    fn step(&mut self) -> Step<()> {
        Step::Op(StoreOp::Get { key: key("never") })
    }

    fn resume(&mut self, _: StoreResult) -> Result<(), ProtocolError> {
        Ok(())
    }
}

#[test]
fn run_drives_a_protocol_to_its_outcome() {
    let mut store = MemStore::new();
    let run = run(&mut store, &mut create("a"), &BOUNDS);
    assert!(
        matches!(run, Run::Done(CreateOutcome::Created(_))),
        "{run:?}"
    );
    assert!(store.object(&key("a")).is_some());
}

#[test]
fn run_reports_a_stall_at_the_step_bound() {
    let run = run(&mut MemStore::new(), &mut Forever, &BOUNDS);
    assert_eq!(run, Run::Stalled(Exceeded::Steps(16)));
}

#[test]
fn a_crash_swept_over_every_write_lands_between_each_pair_of_writes() {
    // Two creates; each run records which creates finished before the crash, then a fresh
    // process re-runs the crashed create on the same store.
    let crash = Fault::Crash {
        after_writes: NonZeroU32::MIN,
    };
    let sweep = sweep(MemStore::new, crash, |driver| {
        let mut finished = Vec::new();
        for name in ["a", "b"] {
            match run(driver, &mut create(name), &BOUNDS) {
                Run::Done(CreateOutcome::Created(_)) => finished.push(name),
                Run::Crashed => {
                    let present = driver.inner().object(&key(name)).is_some();
                    let repaired = run(driver, &mut create(name), &BOUNDS);
                    let repaired = matches!(repaired, Run::Done(CreateOutcome::Created(_)));
                    return (finished, Some((name, present, repaired)));
                }
                other => panic!("create {name} gave {other:?}"),
            }
        }
        (finished, None)
    });
    assert_eq!(sweep.writes, 2);
    assert_eq!(sweep.clean, (vec!["a", "b"], None));
    let at = |n| NonZeroU32::new(n).expect("non-zero");
    assert_eq!(
        sweep.faulted,
        vec![
            (at(1), (vec![], Some(("a", true, true)))),
            (at(2), (vec!["a"], Some(("b", true, true)))),
        ]
    );
}

#[test]
fn a_timeout_positioned_at_one_write_affects_only_that_write() {
    let op = |name: &str| StoreOp::Create {
        key: key(name),
        spec: String::new(),
        origin: origin(name),
    };
    let timeout = Fault::WriteTimeout { applied: false };
    let mut driver = AtWrite::new(MemStore::new(), NonZeroU32::MIN.saturating_add(1), timeout);
    assert!(matches!(
        driver.perform(op("a")),
        Execution::Result(StoreResult::Object(_))
    ));
    assert!(!driver.reached());
    assert_eq!(
        driver.perform(op("b")),
        Execution::Result(StoreResult::Uncertain)
    );
    assert!(driver.reached());
    assert!(matches!(
        driver.perform(op("c")),
        Execution::Result(StoreResult::Object(_))
    ));
    assert_eq!(driver.writes(), 3);
    assert!(driver.inner().object(&key("b")).is_none());
}

#[test]
fn reads_do_not_count_as_write_positions() {
    let mut driver = AtWrite::counting(MemStore::new());
    driver.perform(StoreOp::Get { key: key("a") });
    assert_eq!(driver.writes(), 0);
}

#[test]
fn within_gives_the_first_tick_the_condition_holds() {
    assert_eq!(within(&BOUNDS, |t| (t >= 3).then_some(t)), Ok(3));
    assert_eq!(
        within(&BOUNDS, |t| (t > 5).then_some(t)),
        Err(Exceeded::Ticks(5))
    );
}

#[test]
fn mishaps_cover_each_drop_duplicate_and_swap_once() {
    let all = mishaps(3);
    assert_eq!(
        all,
        vec![
            Mishap::Drop(0),
            Mishap::Drop(1),
            Mishap::Drop(2),
            Mishap::Duplicate(0),
            Mishap::Duplicate(1),
            Mishap::Duplicate(2),
            Mishap::Swap(0, 1),
            Mishap::Swap(0, 2),
            Mishap::Swap(1, 2),
        ]
    );
}

#[test]
fn deliver_applies_one_mishap() {
    let messages = [1, 2, 3];
    assert_eq!(deliver(&messages, Mishap::Drop(1)), vec![1, 3]);
    assert_eq!(deliver(&messages, Mishap::Duplicate(2)), vec![1, 2, 3, 3]);
    assert_eq!(deliver(&messages, Mishap::Swap(0, 2)), vec![3, 2, 1]);
    assert_eq!(deliver(&messages, Mishap::Drop(3)), vec![1, 2, 3]);
}

#[test]
fn orders_gives_every_permutation_once() {
    let all = orders(&['a', 'b', 'c']);
    assert_eq!(
        all,
        vec![
            vec!['a', 'b', 'c'],
            vec!['a', 'c', 'b'],
            vec!['b', 'a', 'c'],
            vec!['b', 'c', 'a'],
            vec!['c', 'a', 'b'],
            vec!['c', 'b', 'a'],
        ]
    );
    assert_eq!(orders::<u8>(&[]), vec![Vec::<u8>::new()]);
    assert_eq!(orders(&[7, 7]).len(), 2);
}
