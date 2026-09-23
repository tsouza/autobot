use super::*;
use autobot_fakes::store::MemStore;
use autobot_kernel::digest::digest;
use autobot_kernel::store::{Create, CreateOutcome, ObjectKey, Origin};
use autobot_kernel::types::Uid;
use std::collections::BTreeSet;

const BOUNDS: Bounds = Bounds {
    steps: 64,
    ticks: 0,
};

/// The toy protocol's shared state: a counter and its version.
#[derive(Debug, Default)]
struct Counter {
    value: u32,
    version: u32,
}

/// Increments the counter by reading it in one step and writing the read value plus one in
/// the next, without checking that nothing wrote in between: the planted race.
#[derive(Debug, Default)]
struct BlindIncrement {
    read: Option<u32>,
}

impl Actor<Counter> for BlindIncrement {
    fn step(&mut self, world: &mut Counter) -> Progress {
        match self.read.take() {
            None => {
                self.read = Some(world.value);
                Progress::Running
            }
            Some(value) => {
                world.value = value + 1;
                world.version += 1;
                Progress::Done
            }
        }
    }
}

/// Increments the counter by compare-and-set on its version, rereading after a conflict.
#[derive(Debug, Default)]
struct CasIncrement {
    read: Option<(u32, u32)>,
}

impl Actor<Counter> for CasIncrement {
    fn step(&mut self, world: &mut Counter) -> Progress {
        match self.read.take() {
            None => {
                self.read = Some((world.value, world.version));
                Progress::Running
            }
            Some((value, version)) if version == world.version => {
                world.value = value + 1;
                world.version += 1;
                Progress::Done
            }
            Some(_) => Progress::Running,
        }
    }
}

fn both_increments_land<A, B>(world: &Counter, _: &A, _: &B) -> Result<(), String> {
    if world.value == 2 {
        Ok(())
    } else {
        Err(format!("lost update: the counter is {}", world.value))
    }
}

fn blind() -> (Counter, BlindIncrement, BlindIncrement) {
    Default::default()
}

fn cas() -> (Counter, CasIncrement, CasIncrement) {
    Default::default()
}

use Who::{A, B};

#[test]
fn every_position_runs_b_at_each_boundary_of_a_and_finds_the_planted_race() {
    let found = every_position(blind, both_increments_land, &BOUNDS).expect("within bounds");
    assert_eq!(
        found.schedules,
        vec![vec![B, B, A, A], vec![A, B, B, A], vec![A, A, B, B]]
    );
    assert_eq!(
        found.violations,
        vec![Violation {
            schedule: vec![A, B, B, A],
            message: "lost update: the counter is 1".to_owned(),
        }]
    );
}

#[test]
fn every_interleaving_enumerates_each_merge_once_and_finds_every_racing_one() {
    let found = every_interleaving(blind, both_increments_land, &BOUNDS).expect("within bounds");
    let schedules: BTreeSet<Schedule> = found.schedules.iter().cloned().collect();
    // Two steps each: the four-choose-two merges, each run exactly once.
    let expected: BTreeSet<Schedule> = [
        vec![A, A, B, B],
        vec![A, B, A, B],
        vec![A, B, B, A],
        vec![B, A, A, B],
        vec![B, A, B, A],
        vec![B, B, A, A],
    ]
    .into_iter()
    .collect();
    assert_eq!(found.schedules.len(), 6);
    assert_eq!(schedules, expected);
    // The race is every merge in which the second read comes before the first write.
    let racing: BTreeSet<Schedule> = found
        .violations
        .iter()
        .map(|v| v.schedule.clone())
        .collect();
    let expected_racing: BTreeSet<Schedule> = [
        vec![A, B, A, B],
        vec![A, B, B, A],
        vec![B, A, A, B],
        vec![B, A, B, A],
    ]
    .into_iter()
    .collect();
    assert_eq!(racing, expected_racing);
}

#[test]
fn the_compare_and_set_protocol_survives_every_interleaving() {
    let found = every_interleaving(cas, both_increments_land, &BOUNDS).expect("within bounds");
    assert_eq!(found.violations, Vec::new());
    // A conflict makes the loser reread: the schedules where both read first take six steps.
    assert!(
        found.schedules.contains(&vec![A, B, A, B, B, B]),
        "{:?}",
        found.schedules
    );
    let distinct: BTreeSet<&Schedule> = found.schedules.iter().collect();
    assert_eq!(distinct.len(), found.schedules.len());
}

/// An actor that never finishes.
struct Spin;

impl Actor<Counter> for Spin {
    fn step(&mut self, _: &mut Counter) -> Progress {
        Progress::Running
    }
}

#[test]
fn a_schedule_longer_than_the_step_bound_is_reported() {
    let setup = || (Counter::default(), Spin, BlindIncrement::default());
    let bounds = Bounds { steps: 8, ticks: 0 };
    let ok = |_: &Counter, _: &Spin, _: &BlindIncrement| Ok(());
    assert_eq!(every_position(setup, ok, &bounds), Err(Exceeded::Steps(8)));
    assert_eq!(
        every_interleaving(setup, ok, &bounds),
        Err(Exceeded::Steps(8))
    );
}

fn key() -> ObjectKey {
    ObjectKey {
        kind: "Probe".parse().expect("kind"),
        namespace: "testkit".parse().expect("namespace"),
        name: "shared".parse().expect("name"),
    }
}

fn create(receipt: &str) -> ProtocolActor<Create> {
    let origin = Origin {
        create_receipt_uid: receipt.parse().expect("uid"),
        input_digest: digest(receipt).expect("digest"),
        context_uid: "context".parse().expect("uid"),
    };
    ProtocolActor::new(Create::new(key(), String::new(), origin))
}

#[test]
fn two_store_creates_of_one_name_leave_exactly_one_winner_in_every_interleaving() {
    let setup = || (MemStore::new(), create("first"), create("second"));
    let one_winner = |store: &MemStore, a: &ProtocolActor<Create>, b: &ProtocolActor<Create>| {
        let created = |actor: &ProtocolActor<Create>| {
            matches!(actor.run(), Some(Run::Done(CreateOutcome::Created(_))))
        };
        let taken = |actor: &ProtocolActor<Create>| {
            matches!(actor.run(), Some(Run::Done(CreateOutcome::Taken(_))))
        };
        let winner = store
            .object(&key())
            .map(|o| o.origin.create_receipt_uid.clone());
        let receipt = |r: &str| r.parse::<Uid>().ok();
        match (created(a), created(b)) {
            (true, false) if taken(b) && winner == receipt("first") => Ok(()),
            (false, true) if taken(a) && winner == receipt("second") => Ok(()),
            _ => Err(format!(
                "a {:?}, b {:?}, stored {winner:?}",
                a.run(),
                b.run()
            )),
        }
    };
    let found = every_interleaving(setup, one_winner, &BOUNDS).expect("within bounds");
    assert_eq!(found.violations, Vec::new());
    // The first create wins; the second is refused and reads the name back.
    assert_eq!(found.schedules, vec![vec![A, B, B], vec![B, A, A]]);
}
