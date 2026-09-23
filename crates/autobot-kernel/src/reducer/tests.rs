use super::*;
use crate::types::{CommitSequence, ControlRevision, StateRevision};
use schemars::schema_for;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};

const FORMAL: &str = include_str!("../../../../docs/design/AUTOBOT-FORMAL-SURFACE.md");
const SNAPSHOT: &str = include_str!("transition_receipt.schema.json");

/// A toy aggregate: one domain field and one control field.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Toy {
    count: u64,
    held: bool,
}

fn sha(parts: &[&[u8]]) -> Digest {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    Digest::from_bytes(h.finalize().into())
}

impl ReducerState for Toy {
    fn digests(&self) -> StateDigests {
        StateDigests {
            domain: sha(&[b"domain", &self.count.to_be_bytes()]),
            control: sha(&[b"control", &[u8::from(self.held)]]),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ToyCommand {
    /// Domain: adds to the count, refused while held.
    Add(u64),
    /// Domain: adds one and emits this many effect intents.
    Emit(u32),
    /// Control: holds.
    Hold,
    /// Control: releases.
    Release,
    /// Control: holds and also adds to the count, touching a domain field.
    HoldAndAdd(u64),
    /// Domain: adds one and also flips the hold, touching a control field.
    AddAndFlip,
    /// Domain: commits under an invalid action name.
    Misnamed,
}

impl ToyCommand {
    /// The lane the command belongs to.
    fn lane(&self) -> Lane {
        match self {
            Self::Hold | Self::Release | Self::HoldAndAdd(_) => Lane::Control,
            _ => Lane::Domain,
        }
    }
}

fn intent(index: u32) -> SlotEffectIntent {
    SlotEffectIntent {
        effect_index: index,
        installation_lineage: "install-a".to_owned(),
        payload_digest: sha(&[b"payload", &index.to_be_bytes()]),
        provider_binding: "fake-forge".to_owned(),
        desired_outcome: "comment-posted".to_owned(),
        target_identity: "pr/7".to_owned(),
        contract_revision: "v1".to_owned(),
    }
}

struct ToyReducer;

impl Reducer for ToyReducer {
    type State = Toy;
    type Command = ToyCommand;
    const VERSION: &'static str = "toy-1";

    fn reduce(state: &Toy, command: &ToyCommand, _: &Guards) -> Decision<Toy> {
        let with_count = |count| Toy { count, ..*state };
        match *command {
            ToyCommand::Add(_) | ToyCommand::Emit(_) | ToyCommand::Hold if state.held => {
                Decision::Refuse(RefusalGround::Precondition("not held"))
            }
            ToyCommand::Add(n) => match state.count.checked_add(n) {
                Some(c) => Decision::Commit(Transition::domain("AddCount", with_count(c), vec![])),
                None => Decision::Refuse(RefusalGround::Precondition("count fits")),
            },
            ToyCommand::Emit(k) => Decision::Commit(Transition::domain(
                "EmitEffects",
                with_count(state.count.saturating_add(1)),
                (0..k).map(intent).collect(),
            )),
            ToyCommand::Hold => Decision::Commit(Transition::control(
                "RequestHold",
                Toy {
                    held: true,
                    ..*state
                },
            )),
            ToyCommand::Release if !state.held => {
                Decision::Refuse(RefusalGround::Precondition("held"))
            }
            ToyCommand::Release => Decision::Commit(Transition::control(
                "ReleaseHold",
                Toy {
                    held: false,
                    ..*state
                },
            )),
            ToyCommand::HoldAndAdd(n) => Decision::Commit(Transition::control(
                "RequestHold",
                Toy {
                    count: state.count.saturating_add(n),
                    held: true,
                },
            )),
            ToyCommand::AddAndFlip => Decision::Commit(Transition::domain(
                "AddCount",
                Toy {
                    count: state.count.saturating_add(1),
                    held: !state.held,
                },
                vec![],
            )),
            ToyCommand::Misnamed => {
                Decision::Commit(Transition::domain("add count", state.clone(), vec![]))
            }
        }
    }
}

/// Calls of [`ImpureReducer::reduce`] so far: state kept outside the arguments.
static IMPURE_CALLS: AtomicU64 = AtomicU64::new(0);

/// The toy reducer, except that `Add` also adds how often it has been called.
struct ImpureReducer;

impl Reducer for ImpureReducer {
    type State = Toy;
    type Command = ToyCommand;
    const VERSION: &'static str = "impure-1";

    fn reduce(state: &Toy, command: &ToyCommand, guards: &Guards) -> Decision<Toy> {
        match *command {
            ToyCommand::Add(n) => {
                let calls = IMPURE_CALLS.fetch_add(1, Ordering::Relaxed);
                ToyReducer::reduce(state, &ToyCommand::Add(n.saturating_add(calls)), guards)
            }
            _ => ToyReducer::reduce(state, command, guards),
        }
    }
}

/// splitmix64: a small deterministic generator for the property tests.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn initial() -> Versioned<Toy> {
    Versioned {
        uid: "toy-uid".parse().expect("uid"),
        counters: Counters::default(),
        state: Toy {
            count: 0,
            held: false,
        },
    }
}

fn pins(index: usize, expected_revision: LaneRevision) -> CommandPins {
    CommandPins {
        command_uid: format!("cmd-{index}").parse().expect("uid"),
        principal: "operator".parse().expect("principal"),
        input_digest: sha(&[b"input", &index.to_be_bytes()]),
        expected_revision,
    }
}

/// The pins of `command` against `aggregate`: its current revision in the command's lane.
fn current_pins(index: usize, aggregate: &Versioned<Toy>, command: &ToyCommand) -> CommandPins {
    pins(index, aggregate.counters.revision(command.lane()))
}

fn random_command(rng: &mut Rng) -> ToyCommand {
    match rng.below(6) {
        0 | 1 => ToyCommand::Add(rng.below(10)),
        2 => ToyCommand::Emit(u32::try_from(rng.below(4)).expect("small")),
        3 => ToyCommand::Hold,
        4 => ToyCommand::Release,
        _ => ToyCommand::HoldAndAdd(rng.below(3)),
    }
}

type Log = Vec<(CommandPins, ToyCommand)>;
type Outcomes = Vec<Result<Step<Toy>, ReducerError>>;

/// Feeds `log` to `step::<R>` from [`initial`], each command against the aggregate the
/// previous commits left.
fn replay<R: Reducer<State = Toy, Command = ToyCommand>>(log: &Log) -> Outcomes {
    let mut aggregate = initial();
    log.iter()
        .map(|(p, c)| {
            let outcome = step::<R>(&aggregate, p, c, &Guards::all());
            if let Ok(Step::Committed {
                aggregate: next, ..
            }) = &outcome
            {
                aggregate = next.clone();
            }
            outcome
        })
        .collect()
}

/// A random log from `seed`, recorded while running it through `step::<R>`: most commands pin
/// the current revision of their lane, one in eight pins a stale or future one.
fn record<R: Reducer<State = Toy, Command = ToyCommand>>(seed: u64) -> (Log, Outcomes) {
    let mut rng = Rng(seed);
    let mut aggregate = initial();
    let mut log = Log::new();
    let mut outcomes = Outcomes::new();
    for i in 0..usize::try_from(rng.below(40)).expect("small") {
        let command = random_command(&mut rng);
        let mut p = current_pins(i, &aggregate, &command);
        if rng.below(8) == 0 {
            let off = rng.below(3).saturating_add(1);
            let value = p.expected_revision.get() ^ off;
            p.expected_revision = match p.expected_revision {
                LaneRevision::State(_) => {
                    LaneRevision::State(StateRevision::new(value).expect("in range"))
                }
                LaneRevision::Control(_) => {
                    LaneRevision::Control(ControlRevision::new(value).expect("in range"))
                }
            };
        }
        let outcome = step::<R>(&aggregate, &p, &command, &Guards::all());
        if let Ok(Step::Committed {
            aggregate: next, ..
        }) = &outcome
        {
            aggregate = next.clone();
        }
        log.push((p, command));
        outcomes.push(outcome);
    }
    (log, outcomes)
}

/// The encoded receipts of `outcomes`, in order.
fn encoded_receipts(outcomes: &Outcomes) -> Vec<String> {
    outcomes
        .iter()
        .filter_map(|o| match o {
            Ok(Step::Committed { receipt, .. }) => {
                Some(serde_json::to_string(receipt).expect("serializes"))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn replaying_a_log_reproduces_every_decision_and_receipt() {
    let mut commits = 0;
    let mut refusals = 0;
    for seed in 0..256 {
        let (log, recorded) = record::<ToyReducer>(seed);
        let replayed = replay::<ToyReducer>(&log);
        assert_eq!(recorded, replayed, "seed {seed}: replay diverged");
        assert_eq!(
            encoded_receipts(&recorded),
            encoded_receipts(&replayed),
            "seed {seed}: receipt bytes diverged"
        );
        for outcome in &recorded {
            match outcome
                .as_ref()
                .expect("the toy reducer makes valid receipts")
            {
                Step::Committed { .. } => commits += 1,
                Step::Refused(_) => refusals += 1,
            }
        }
    }
    assert!(
        commits > 1000 && refusals > 100,
        "{commits} commits, {refusals} refusals"
    );
}

#[test]
fn replay_detects_a_reducer_that_keeps_state_between_calls() {
    let diverged = (0..64).any(|seed| {
        let (log, recorded) = record::<ImpureReducer>(seed);
        recorded != replay::<ImpureReducer>(&log)
    });
    assert!(
        diverged,
        "an impure reducer replayed identically on every seed"
    );
}

#[test]
fn receipts_chain_the_aggregate_from_its_initial_state() {
    for seed in 0..64 {
        let (log, outcomes) = record::<ToyReducer>(seed);
        let mut aggregate = initial();
        for ((p, _), outcome) in log.iter().zip(&outcomes) {
            match outcome.as_ref().expect("valid") {
                Step::Committed {
                    aggregate: next,
                    receipt,
                } => {
                    let f = receipt.fields();
                    assert_eq!(f.before, aggregate.counters);
                    assert_eq!(f.after, next.counters);
                    assert_eq!(f.before_digests, aggregate.state.digests());
                    assert_eq!(f.after_digests, next.state.digests());
                    assert_eq!(receipt.expected_revision(), p.expected_revision);
                    assert_eq!(f.command_uid, p.command_uid);
                    assert_eq!(f.input_digest, p.input_digest);
                    assert_eq!(f.aggregate_uid, aggregate.uid);
                    assert_eq!(f.reducer_version, "toy-1");
                    assert!(f.disabled_guards.is_empty());
                    aggregate = next.clone();
                }
                Step::Refused(r) => {
                    assert_eq!(
                        r.read_revision,
                        aggregate.counters.revision(p.expected_revision.lane())
                    );
                }
            }
        }
    }
}

/// The committed outcome of `command` against `aggregate` at its current revision.
fn commit(
    aggregate: &Versioned<Toy>,
    command: &ToyCommand,
    guards: &Guards,
) -> (Versioned<Toy>, TransitionReceipt) {
    let p = current_pins(0, aggregate, command);
    match step::<ToyReducer>(aggregate, &p, command, guards).expect("steps") {
        Step::Committed { aggregate, receipt } => (aggregate, receipt),
        Step::Refused(r) => panic!("refused: {r:?}"),
    }
}

fn refusal(
    aggregate: &Versioned<Toy>,
    p: &CommandPins,
    command: &ToyCommand,
    guards: &Guards,
) -> Refusal {
    match step::<ToyReducer>(aggregate, p, command, guards).expect("steps") {
        Step::Refused(r) => r,
        Step::Committed { receipt, .. } => panic!("committed: {receipt:?}"),
    }
}

#[test]
fn a_domain_commit_advances_the_state_revision_and_the_sequence() {
    let (after, receipt) = commit(&initial(), &ToyCommand::Emit(2), &Guards::all());
    assert_eq!(after.state.count, 1);
    assert_eq!(after.counters.state_revision.get(), 1);
    assert_eq!(after.counters.control_revision.get(), 0);
    assert_eq!(after.counters.commit_sequence.get(), 1);
    assert_eq!(receipt.fields().lane, Lane::Domain);
    assert_eq!(receipt.fields().action.as_str(), "EmitEffects");
    assert_eq!(receipt.fields().effect_intents, vec![intent(0), intent(1)]);
    assert_eq!(
        receipt.proposed_revision(),
        LaneRevision::State(StateRevision::new(1).expect("in range"))
    );
}

#[test]
fn a_control_commit_advances_the_control_revision_and_keeps_the_domain_digest() {
    let (after, receipt) = commit(&initial(), &ToyCommand::Hold, &Guards::all());
    assert!(after.state.held);
    assert_eq!(after.counters.state_revision.get(), 0);
    assert_eq!(after.counters.control_revision.get(), 1);
    assert_eq!(after.counters.commit_sequence.get(), 1);
    let f = receipt.fields();
    assert_eq!(f.before_digests.domain, f.after_digests.domain);
    assert_ne!(f.before_digests.control, f.after_digests.control);
}

#[test]
fn a_stale_expected_revision_is_refused_before_the_reducer_runs() {
    let (after, _) = commit(&initial(), &ToyCommand::Add(1), &Guards::all());
    let stale = pins(1, LaneRevision::State(StateRevision::ZERO));
    let r = refusal(&after, &stale, &ToyCommand::Misnamed, &Guards::all());
    assert_eq!(r.ground, RefusalGround::RevisionMismatch);
    assert_eq!(r.read_revision, after.counters.revision(Lane::Domain));
}

#[test]
fn a_reducer_refusal_records_the_revision_it_read() {
    let (held, _) = commit(&initial(), &ToyCommand::Hold, &Guards::all());
    let p = current_pins(1, &held, &ToyCommand::Add(1));
    let r = refusal(&held, &p, &ToyCommand::Add(1), &Guards::all());
    assert_eq!(r.ground, RefusalGround::Precondition("not held"));
    assert_eq!(r.read_revision, LaneRevision::State(StateRevision::ZERO));
}

#[test]
fn a_control_commit_touching_a_domain_field_is_refused_under_every_guard() {
    let command = ToyCommand::HoldAndAdd(2);
    let p = current_pins(0, &initial(), &command);
    let r = refusal(&initial(), &p, &command, &Guards::all());
    assert_eq!(r.ground, RefusalGround::Guard(GuardId::ControlFieldsOnly));
}

#[test]
fn without_the_control_fields_guard_the_domain_digest_moves_under_a_control_commit() {
    let guards = Guards::all().without(GuardId::ControlFieldsOnly);
    let (after, receipt) = commit(&initial(), &ToyCommand::HoldAndAdd(2), &guards);
    assert_eq!(after.state.count, 2);
    let f = receipt.fields();
    assert_eq!(f.lane, Lane::Control);
    assert_ne!(f.before_digests.domain, f.after_digests.domain);
    assert_eq!(f.disabled_guards, vec![GuardId::ControlFieldsOnly]);
}

#[test]
fn a_domain_commit_touching_a_control_field_is_refused_even_without_the_guard() {
    let command = ToyCommand::AddAndFlip;
    let p = current_pins(0, &initial(), &command);
    for guards in [
        Guards::all(),
        Guards::all().without(GuardId::ControlFieldsOnly),
    ] {
        let r = refusal(&initial(), &p, &command, &guards);
        assert_eq!(r.ground, RefusalGround::DomainCommitTouchesControl);
    }
}

#[test]
fn a_transition_on_another_lane_than_the_pinned_one_is_an_error() {
    let p = pins(0, LaneRevision::State(StateRevision::ZERO));
    let err = step::<ToyReducer>(&initial(), &p, &ToyCommand::Hold, &Guards::all());
    assert_eq!(
        err,
        Err(ReducerError::LaneMismatch {
            pinned: Lane::Domain,
            committed: Lane::Control
        })
    );
}

#[test]
fn an_invalid_action_name_is_an_error() {
    let p = current_pins(0, &initial(), &ToyCommand::Misnamed);
    let err = step::<ToyReducer>(&initial(), &p, &ToyCommand::Misnamed, &Guards::all());
    assert_eq!(
        err,
        Err(ReducerError::Receipt(ReceiptError::ActionName(
            "add count".to_owned()
        )))
    );
}

#[test]
fn an_exhausted_commit_sequence_is_an_error() {
    let mut aggregate = initial();
    aggregate.counters.commit_sequence = CommitSequence::new(i64::MAX.unsigned_abs()).expect("max");
    let p = current_pins(0, &aggregate, &ToyCommand::Add(1));
    let err = step::<ToyReducer>(&aggregate, &p, &ToyCommand::Add(1), &Guards::all());
    assert!(matches!(err, Err(ReducerError::Counter(_))), "{err:?}");
}

/// A receipt of a domain commit with two effect intents, as JSON.
fn receipt_json() -> (TransitionReceipt, Value) {
    let (_, receipt) = commit(&initial(), &ToyCommand::Emit(2), &Guards::all());
    let value = serde_json::to_value(&receipt).expect("serializes");
    (receipt, value)
}

#[test]
fn a_receipt_round_trips_through_its_text_form() {
    let (receipt, value) = receipt_json();
    let text = serde_json::to_string(&receipt).expect("serializes");
    let parsed: TransitionReceipt = serde_json::from_str(&text).expect("parses");
    assert_eq!(parsed, receipt);
    assert_eq!(serde_json::to_string(&parsed).expect("serializes"), text);
    assert_eq!(value["schema_version"], json!(1));
    assert_eq!(value["lane"], json!("DOMAIN"));
    assert_eq!(value["action"], json!("EmitEffects"));
    assert_eq!(value["before"]["commit_sequence"], json!(0));
    assert_eq!(value["after"]["commit_sequence"], json!(1));
    assert_eq!(value["disabled_guards"], json!([]));
}

#[test]
fn a_receipt_made_without_a_guard_round_trips_with_that_guard_listed() {
    let guards = Guards::all().without(GuardId::ControlFieldsOnly);
    let (_, receipt) = commit(&initial(), &ToyCommand::HoldAndAdd(1), &guards);
    let value = serde_json::to_value(&receipt).expect("serializes");
    assert_eq!(value["disabled_guards"], json!(["control-fields-only"]));
    let parsed: TransitionReceipt = serde_json::from_value(value).expect("parses");
    assert_eq!(parsed, receipt);
}

/// A named edit of an encoded receipt.
type Edit = (&'static str, fn(&mut Value));

#[test]
fn parsing_refuses_a_receipt_that_breaks_its_invariants() {
    let (_, value) = receipt_json();
    let edits: [Edit; 7] = [
        ("schema version", |v| v["schema_version"] = json!(2)),
        ("sequence skips", |v| {
            v["after"]["commit_sequence"] = json!(2);
        }),
        ("other lane advanced", |v| {
            v["after"]["control_revision"] = json!(1);
        }),
        ("control lane with intents", |v| {
            v["lane"] = json!("CONTROL");
            v["after"]["state_revision"] = json!(0);
            v["after"]["control_revision"] = json!(1);
        }),
        ("intents out of order", |v| {
            v["effect_intents"][1]["effect_index"] = json!(0);
        }),
        ("repeated guard", |v| {
            v["disabled_guards"] = json!(["control-fields-only", "control-fields-only"]);
        }),
        ("action name", |v| v["action"] = json!("emitEffects")),
    ];
    assert!(serde_json::from_value::<TransitionReceipt>(value.clone()).is_ok());
    for (what, edit) in edits {
        let mut tampered = value.clone();
        edit(&mut tampered);
        assert!(
            serde_json::from_value::<TransitionReceipt>(tampered).is_err(),
            "{what}: accepted"
        );
    }
}

#[test]
fn the_receipt_schema_matches_its_snapshot() {
    let schema = schema_for!(TransitionReceipt);
    let actual = format!(
        "{}\n",
        serde_json::to_string_pretty(&schema).expect("serializes")
    );
    if std::env::var_os("AUTOBOT_UPDATE_SNAPSHOTS").is_some() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/reducer/transition_receipt.schema.json"
        );
        std::fs::write(path, &actual).expect("writes the snapshot");
        return;
    }
    assert!(
        actual == SNAPSHOT,
        "the TransitionReceipt schema differs from src/reducer/transition_receipt.schema.json; \
         rerun with AUTOBOT_UPDATE_SNAPSHOTS=1 to accept it. Actual schema:\n{actual}"
    );
}

/// The rows of the FORMAL §5 table: its "Guard removed" and "Must violate" cells.
fn formal_section_5_rows() -> Vec<(String, String)> {
    let section = FORMAL
        .split_once("## 5. Negative variants")
        .and_then(|(_, rest)| rest.split_once("\n## 6."))
        .map(|(s, _)| s)
        .expect("FORMAL has §5 and §6");
    section
        .lines()
        .filter_map(|l| l.strip_prefix("| ")?.strip_suffix(" |"))
        .filter(|row| *row != "Guard removed | Must violate")
        .map(|row| {
            let (removed, violates) = row.split_once(" | ").expect("two cells");
            (removed.to_owned(), violates.to_owned())
        })
        .collect()
}

#[test]
fn the_guards_are_the_formal_section_5_rows_in_order() {
    let rows = formal_section_5_rows();
    assert_eq!(rows.len(), 20, "FORMAL §5 rows: {rows:?}");
    let guards: Vec<(String, String)> = GuardId::ALL
        .iter()
        .map(|g| (g.removed().to_owned(), g.must_violate().to_owned()))
        .collect();
    assert_eq!(guards, rows);
}

#[test]
fn every_guard_is_enabled_by_default_and_without_disables_exactly_one() {
    assert_eq!(Guards::default(), Guards::all());
    assert!(GuardId::ALL.iter().all(|&g| Guards::all().is_enabled(g)));
    assert!(Guards::all().disabled().is_empty());
    for &g in GuardId::ALL {
        let guards = Guards::all().without(g);
        assert_eq!(guards.disabled(), vec![g]);
        assert!(
            GuardId::ALL
                .iter()
                .all(|&o| guards.is_enabled(o) == (o != g))
        );
    }
}

#[test]
fn guard_ids_round_trip_through_their_kebab_case_names() {
    let names: Vec<Value> = GuardId::ALL
        .iter()
        .map(|g| serde_json::to_value(g).expect("serializes"))
        .collect();
    assert_eq!(names[5], json!("control-fields-only"));
    let parsed: Vec<GuardId> = names
        .iter()
        .map(|n| serde_json::from_value(n.clone()).expect("parses"))
        .collect();
    assert_eq!(parsed, GuardId::ALL);
}
