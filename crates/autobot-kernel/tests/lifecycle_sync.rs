//! Keeps `autobot_kernel::lifecycle` equal to its single design home, KERNEL §10.
//!
//! The section is read with the shared parser, `autobot_devtools::design::lifecycle`, and
//! diffed against a list of tables, [`tables`] for the kernel's own. The check covers:
//!
//! - the same machines and fields, and the same `as` copies;
//! - the same states in the same order, so the same initial state;
//! - the same transitions once `any` and `any non-terminal` are expanded;
//! - the same sibling-field moves;
//! - requirements on exactly the annotated transitions. A transition is *annotated* when the
//!   parser gives it an annotation or a line note, or a note of its machine names it. Each such
//!   transition carries a [`Requirement`] that quotes every one of its annotations. Each phrase
//!   of a transition's requirement is printed on the transition, or is in a note (naming it or
//!   not) that holds one of the transition's two states as a word;
//! - the same event types as [`LifecycleEvent`].
//!
//! A requirement on a transition §10 does not annotate is a condition stated only in a
//! machine's prose. The check can bind it to the states its phrase names, but it cannot tell
//! when such a requirement is missing.
//!
//! A failure means the two disagree. The design is the authority, so the fix is a `design`
//! finding that decides which one is wrong, not an edit of either side to match.

use autobot_devtools::design::lifecycle::{self as design, Field, Machine, Source, Transition};
use autobot_devtools::markdown;
use autobot_kernel::lifecycle::{LifecycleEvent, Requirement, Table, tables};
use std::collections::{BTreeMap, BTreeSet};

const KERNEL: &str = include_str!("../../../docs/design/AUTOBOT-KERNEL.md");

/// Whitespace runs as one space, so a note printed over several lines matches a phrase.
fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What §10 prints about one transition.
#[derive(Default)]
struct Printed {
    /// Its annotation and line note.
    own: Vec<String>,
    /// The machine notes that name it.
    naming: Vec<String>,
}

impl Printed {
    fn all(&self) -> impl Iterator<Item = &String> {
        self.own.iter().chain(&self.naming)
    }
}

/// The `(from, to)` transitions `field` of `m` prints, with `any` and `any non-terminal`
/// expanded: `any` to every state, `any non-terminal` to every state with an outgoing
/// transition from a named source; neither to the target itself.
fn printed_edges(m: &Machine, field: &Field) -> BTreeMap<(String, String), Printed> {
    let own: Vec<&Transition> = field
        .transitions
        .iter()
        .filter(|t| t.assigns.is_none())
        .collect();
    let non_terminal: BTreeSet<&str> = own
        .iter()
        .filter_map(|t| match &t.from {
            Source::State(s) => Some(s.as_str()),
            Source::Any | Source::AnyNonTerminal => None,
        })
        .collect();
    let mut out: BTreeMap<(String, String), Printed> = BTreeMap::new();
    for t in own {
        let sources: Vec<&str> = match &t.from {
            Source::State(s) => vec![s.as_str()],
            Source::Any => field.states.iter().map(String::as_str).collect(),
            Source::AnyNonTerminal => non_terminal.iter().copied().collect(),
        };
        for from in sources.into_iter().filter(|s| *s != t.to) {
            let p = out.entry((from.to_owned(), t.to.clone())).or_default();
            p.own
                .extend(t.note.iter().chain(&t.line_note).map(|n| squash(n)));
            p.naming = m
                .notes_naming(from, &t.to)
                .into_iter()
                .map(squash)
                .collect();
        }
    }
    out
}

/// The `(from, field, to)` moves of `field` that set a sibling field.
fn printed_sets(field: &Field) -> BTreeSet<(String, String, String)> {
    field
        .transitions
        .iter()
        .filter_map(|t| {
            let sibling = t.assigns.as_ref()?;
            let Source::State(from) = &t.from else {
                return None;
            };
            Some((from.clone(), sibling.clone(), t.to.clone()))
        })
        .collect()
}

fn field_label(field: Option<&str>) -> String {
    field.map_or_else(|| "its own states".to_owned(), |f| format!("field `{f}`"))
}

/// Every disagreement between `kernel`'s §10 and the kernel's own tables, one line each.
fn diff(kernel: &str) -> Vec<String> {
    diff_tables(kernel, &tables())
}

/// Every disagreement between `kernel`'s §10 and `tables`, one line each.
fn diff_tables(kernel: &str, tables: &[Table]) -> Vec<String> {
    let machines = match design::parse(kernel) {
        Ok(m) => m,
        Err(e) => return vec![format!("§10 does not parse: {e}")],
    };
    let mut out = Vec::new();

    for m in &machines {
        for f in &m.fields {
            if !tables
                .iter()
                .any(|t| t.machine == m.name && t.field == f.name.as_deref())
            {
                out.push(format!(
                    "§10 prints `{}` {} and the kernel has no table for it",
                    m.name,
                    field_label(f.name.as_deref())
                ));
            }
        }
    }

    for t in tables {
        let at = format!("`{}` {}", t.machine, field_label(t.field));
        let Some(field) = machines
            .iter()
            .find(|m| m.name == t.machine)
            .and_then(|m| m.field(t.field))
        else {
            out.push(format!(
                "the kernel has a table for {at} and §10 prints none"
            ));
            continue;
        };
        if field.same_as.as_deref() != t.same_as {
            out.push(format!(
                "{at}: §10 prints it as a copy of {:?}, the kernel of {:?}",
                field.same_as, t.same_as
            ));
        }
        if field.states != t.states {
            out.push(format!(
                "{at}: §10 lists the states {:?}, the kernel {:?}",
                field.states, t.states
            ));
        }

        // A copy is annotated where the field it copies is printed.
        let home = field
            .same_as
            .as_deref()
            .and_then(|kind| design::find(&machines, kind))
            .or_else(|| machines.iter().find(|m| m.name == t.machine));
        let Some(home) = home else { continue };
        let home_field = home.field(t.field).unwrap_or(field);
        let printed = printed_edges(home, home_field);
        let kernel: BTreeMap<(String, String), Option<Requirement>> = t
            .edges
            .iter()
            .map(|e| ((e.from.to_owned(), e.to.to_owned()), e.requires))
            .collect();
        for (from, to) in printed.keys().filter(|k| !kernel.contains_key(*k)) {
            out.push(format!(
                "{at}: §10 prints {from} → {to} and the kernel does not allow it"
            ));
        }
        for (from, to) in kernel.keys().filter(|k| !printed.contains_key(*k)) {
            out.push(format!(
                "{at}: the kernel allows {from} → {to} and §10 does not print it"
            ));
        }

        let prose: Vec<String> = home.notes.iter().map(|n| squash(n)).collect();
        let empty = Printed::default();
        for ((from, to), requires) in &kernel {
            let p = printed.get(&(from.clone(), to.clone())).unwrap_or(&empty);
            let Some(r) = requires else {
                for a in p.all() {
                    out.push(format!(
                        "{at}: §10 annotates {from} → {to} with `{a}` and the kernel gives it no requirement"
                    ));
                }
                continue;
            };
            let phrases: Vec<String> = r.phrases().iter().map(|s| squash(s)).collect();
            for phrase in &phrases {
                let bound =
                    design::contains_whole(phrase, from) || design::contains_whole(phrase, to);
                let on_it = p.own.iter().any(|a| a.contains(phrase.as_str()));
                let noted = p
                    .naming
                    .iter()
                    .chain(&prose)
                    .any(|n| n.contains(phrase.as_str()));
                if !on_it && !(noted && bound) {
                    out.push(format!(
                        "{at}: {from} → {to} carries {r:?}, whose phrase `{phrase}` §10 prints neither on it nor in a note holding {from} or {to}"
                    ));
                }
            }
            for a in p.all() {
                if !phrases.iter().any(|ph| a.contains(ph.as_str())) {
                    out.push(format!(
                        "{at}: §10 annotates {from} → {to} with `{a}`, which {r:?} does not quote"
                    ));
                }
            }
        }

        let printed = printed_sets(field);
        let kernel: BTreeSet<(String, String, String)> = t
            .sibling_sets
            .iter()
            .map(|s| (s.from.to_owned(), s.field.to_owned(), s.to.to_owned()))
            .collect();
        for (from, sibling, to) in printed.symmetric_difference(&kernel) {
            out.push(format!(
                "{at}: {from} → {sibling} := {to} is in only one of §10 and the kernel"
            ));
        }
    }

    let printed = printed_events(kernel, &machines);
    let kernel: BTreeSet<String> = LifecycleEvent::ALL
        .iter()
        .map(|e| e.as_str().to_owned())
        .collect();
    for name in printed.symmetric_difference(&kernel) {
        out.push(format!(
            "event type `{name}` is in only one of §10 and LifecycleEvent"
        ));
    }
    out
}

/// The event types §10 names: the backticked mixed-case names of the prose after the block,
/// and the `CamelCase` names ending in `ed` of the machines' notes that are not a kind.
fn printed_events(kernel: &str, machines: &[Machine]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Some(section) = markdown::section(kernel, design::SECTION) else {
        return out;
    };
    let after = section.rsplit("```").next().unwrap_or_default();
    for (i, span) in after.split('`').enumerate() {
        if i % 2 == 1
            && span.chars().all(|c| c.is_ascii_alphabetic())
            && span.starts_with(|c: char| c.is_ascii_uppercase())
            && span.chars().any(|c| c.is_ascii_lowercase())
        {
            out.insert(span.to_owned());
        }
    }
    let kinds: BTreeSet<&str> = machines
        .iter()
        .flat_map(|m| m.subjects.iter().map(String::as_str))
        .collect();
    for note in machines.iter().flat_map(|m| &m.notes) {
        for word in note.split(|c: char| !c.is_ascii_alphanumeric()) {
            if word.len() > 2
                && word.ends_with("ed")
                && word.starts_with(|c: char| c.is_ascii_uppercase())
                && word.chars().any(|c| c.is_ascii_lowercase())
                && word.chars().skip(1).any(|c| c.is_ascii_uppercase())
                && !kinds.contains(word)
            {
                out.insert(word.to_owned());
            }
        }
    }
    out
}

/// `KERNEL` with `from` replaced by `to`, which must occur in it.
fn edited(from: &str, to: &str) -> String {
    assert!(
        KERNEL.contains(from),
        "the edit's anchor `{from}` is gone from KERNEL"
    );
    KERNEL.replacen(from, to, 1)
}

/// The kernel's tables with the requirement of `from → to` of `machine` moved to `onto`.
fn moved(machine: &str, from: &str, to: &str, onto: (&str, &str)) -> Vec<Table> {
    let mut all = tables();
    let t = all
        .iter_mut()
        .find(|t| t.machine == machine && t.field.is_none())
        .unwrap();
    let source = t
        .edges
        .iter()
        .position(|e| e.from == from && e.to == to)
        .unwrap();
    let target = t.edges.iter().position(|e| (e.from, e.to) == onto).unwrap();
    t.edges[target].requires = t.edges[source].requires.take();
    all
}

#[test]
fn kernel_tables_match_kernel_section_10() {
    let problems = diff(KERNEL);
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn a_missing_edge_fails() {
    let problems = diff(&edited(
        "FENCED | FENCED_UNCERTAIN ; FENCED_UNCERTAIN → FENCED",
        "FENCED | FENCED_UNCERTAIN",
    ));
    assert_eq!(
        problems,
        [
            "`TaskRun` field `fence_state`: the kernel allows FENCED_UNCERTAIN → FENCED and §10 does not print it",
            "`AgentRun` field `fence_state`: the kernel allows FENCED_UNCERTAIN → FENCED and §10 does not print it",
        ],
    );
}

#[test]
fn an_extra_edge_fails() {
    let problems = diff(&edited(
        "                     VERIFIED → STALE\n",
        "                     VERIFIED → STALE | QUARANTINED\n",
    ));
    assert_eq!(
        problems,
        [
            "`AgentCheckpoint` its own states: §10 prints VERIFIED → QUARANTINED and the kernel does not allow it"
        ],
    );
}

#[test]
fn an_edge_hidden_behind_any_non_terminal_fails() {
    let problems = diff(&edited(
        "RECONCILED → MAPPED → DISPATCH_ENABLED ; any non-terminal → FAILED",
        "RECONCILED → MAPPED → DISPATCH_ENABLED ; any → FAILED",
    ));
    assert_eq!(
        problems,
        [
            "`RestoreRequest` its own states: §10 prints DISPATCH_ENABLED → FAILED and the kernel does not allow it"
        ],
    );
}

#[test]
fn a_renamed_state_fails() {
    let problems = diff(&KERNEL.replace("HEARTBEAT_LOST", "HEARTBEAT_MISSED"));
    for want in [
        "`AgentRun` its own states: §10 prints RUNNING → HEARTBEAT_MISSED and the kernel does not allow it",
        "`AgentRun` its own states: the kernel allows RUNNING → HEARTBEAT_LOST and §10 does not print it",
        "`AgentRun` its own states: HEARTBEAT_LOST → fence_state := FENCE_PENDING is in only one of §10 and the kernel",
    ] {
        assert!(problems.contains(&want.to_owned()), "{problems:#?}");
    }
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with("`AgentRun` its own states: §10 lists the states")),
        "{problems:#?}"
    );
}

#[test]
fn a_reordered_initial_state_fails() {
    let problems = diff(&edited(
        "Artifact             PENDING → VERIFIED | EXPIRED",
        "Artifact             VERIFIED | EXPIRED ; PENDING → VERIFIED | EXPIRED",
    ));
    assert_eq!(
        problems,
        [
            r#"`Artifact` its own states: §10 lists the states ["VERIFIED", "EXPIRED", "PENDING"], the kernel ["PENDING", "VERIFIED", "EXPIRED"]"#
        ],
    );
}

#[test]
fn a_dropped_annotation_fails() {
    let problems = diff(&edited(
        "CENSORED → SETTLED (append-only correction)",
        "CENSORED → SETTLED",
    ));
    assert_eq!(
        problems,
        [
            "`UsageReceipt` its own states: CENSORED → SETTLED carries AppendOnlyCorrection, whose phrase `append-only correction` §10 prints neither on it nor in a note holding CENSORED or SETTLED"
        ],
    );
}

#[test]
fn a_new_annotation_fails() {
    let problems = diff(&edited(
        "CustodyPolicy        ACTIVE → RETIRED",
        "CustodyPolicy        ACTIVE → RETIRED (superseded)",
    ));
    assert_eq!(
        problems,
        [
            "`CustodyPolicy` its own states: §10 annotates ACTIVE → RETIRED with `superseded` and the kernel gives it no requirement"
        ],
    );
}

#[test]
fn an_annotation_moved_to_another_edge_fails() {
    let problems = diff(&edited(
        "PENDING | PARTIAL → UNKNOWN → CENSORED ; CENSORED → SETTLED (append-only correction)",
        "PENDING | PARTIAL → UNKNOWN → CENSORED (append-only correction) ; CENSORED → SETTLED",
    ));
    assert_eq!(
        problems,
        [
            "`UsageReceipt` its own states: CENSORED → SETTLED carries AppendOnlyCorrection, whose phrase `append-only correction` §10 prints neither on it nor in a note holding CENSORED or SETTLED",
            "`UsageReceipt` its own states: §10 annotates UNKNOWN → CENSORED with `append-only correction` and the kernel gives it no requirement",
        ],
    );
}

#[test]
fn a_requirement_moved_to_another_edge_fails() {
    let annotated = diff_tables(
        KERNEL,
        &moved(
            "UsageReceipt",
            "CENSORED",
            "SETTLED",
            ("PARTIAL", "SETTLED"),
        ),
    );
    assert_eq!(
        annotated,
        [
            "`UsageReceipt` its own states: §10 annotates CENSORED → SETTLED with `append-only correction` and the kernel gives it no requirement",
            "`UsageReceipt` its own states: PARTIAL → SETTLED carries AppendOnlyCorrection, whose phrase `append-only correction` §10 prints neither on it nor in a note holding PARTIAL or SETTLED",
        ],
    );
    // A requirement stated only in prose is bound to the states its phrase names.
    let prose = diff_tables(
        KERNEL,
        &moved(
            "TaskRun",
            "PREPARING",
            "EXECUTING",
            ("ADMITTED", "PREPARING"),
        ),
    );
    assert_eq!(
        prose,
        [
            "`TaskRun` its own states: ADMITTED → PREPARING carries FenceActive, whose phrase `once fence_state leaves ACTIVE the phase never moves to EXECUTING or SUCCEEDED` §10 prints neither on it nor in a note holding ADMITTED or PREPARING"
        ],
    );
}

#[test]
fn a_copy_printed_as_its_own_table_fails() {
    let problems = diff(&edited(
        "fence_state: as TaskRun",
        "fence_state:  ACTIVE → FENCE_PENDING → FENCED | FENCED_UNCERTAIN ; FENCED_UNCERTAIN → FENCED",
    ));
    assert!(
        problems.contains(
            &r#"`AgentRun` field `fence_state`: §10 prints it as a copy of None, the kernel of Some("TaskRun")"#
                .to_owned()
        ),
        "{problems:#?}"
    );
}

#[test]
fn a_new_machine_or_field_fails() {
    let problems = diff(&edited(
        "Decision             RECORDED  (immutable)",
        "Decision             RECORDED  (immutable)\n                     review:  OPEN → CLOSED",
    ));
    assert_eq!(
        problems,
        ["§10 prints `Decision` field `review` and the kernel has no table for it"],
    );
}

#[test]
fn a_new_event_type_fails() {
    let problems = diff(&edited(
        "`Dispatched`, `ReceiptObserved`",
        "`Dispatched`, `Escalated`, `ReceiptObserved`",
    ));
    assert_eq!(
        problems,
        ["event type `Escalated` is in only one of §10 and LifecycleEvent"],
    );
}
