//! Keeps `autobot_kernel::lifecycle` equal to its single design home, KERNEL §10.
//!
//! The section is read with the shared parser, `autobot_devtools::design::lifecycle`, and
//! diffed against [`tables`]: the same machines and fields, the same states in the same order
//! (so the same initial state), the same transitions once `any` and `any non-terminal` are
//! expanded, the same sibling-field moves, and every [`Requirement`] a table uses stated in the
//! notes of its machine. The event types §10 names are [`LifecycleEvent`]. A failure means the
//! two disagree; the design is the authority, so the fix is a `design` finding that decides which
//! one is wrong, not an edit of either side to match.

use autobot_devtools::design::lifecycle::{self as design, Field, Machine, Source};
use autobot_devtools::markdown;
use autobot_kernel::lifecycle::{LifecycleEvent, Requirement, Table, tables};
use std::collections::BTreeSet;

const KERNEL: &str = include_str!("../../../docs/design/AUTOBOT-KERNEL.md");

/// Whitespace runs as one space, so a note printed over several lines matches a phrase.
fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The `(from, to)` pairs `field` prints, with `any` and `any non-terminal` expanded: `any`
/// to every state, `any non-terminal` to every state with an outgoing transition from a named
/// source; neither to the target itself.
fn printed_edges(field: &Field) -> BTreeSet<(String, String)> {
    let own = field.transitions.iter().filter(|t| t.assigns.is_none());
    let non_terminal: BTreeSet<&str> = own
        .clone()
        .filter_map(|t| match &t.from {
            Source::State(s) => Some(s.as_str()),
            Source::Any | Source::AnyNonTerminal => None,
        })
        .collect();
    let mut out = BTreeSet::new();
    for t in own {
        let sources: Vec<&str> = match &t.from {
            Source::State(s) => vec![s.as_str()],
            Source::Any => field.states.iter().map(String::as_str).collect(),
            Source::AnyNonTerminal => non_terminal.iter().copied().collect(),
        };
        for from in sources.into_iter().filter(|s| *s != t.to) {
            out.insert((from.to_owned(), t.to.clone()));
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

/// The note text a requirement of `table` may be stated in: the notes of the machine the
/// field is printed on (for a copy, the machine it copies) and the annotations of the field's
/// transitions.
fn notes_for(machines: &[Machine], table: &Table) -> String {
    let home = table.same_as.unwrap_or(table.machine);
    let Some(m) = machines.iter().find(|m| m.name == home) else {
        return String::new();
    };
    let annotations = m
        .field(table.field)
        .into_iter()
        .flat_map(|f| f.transitions.iter().filter_map(|t| t.note.as_deref()));
    squash(
        &m.notes
            .iter()
            .map(String::as_str)
            .chain(annotations)
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn field_label(field: Option<&str>) -> String {
    field.map_or_else(|| "its own states".to_owned(), |f| format!("field `{f}`"))
}

/// Every disagreement between `kernel`'s §10 and [`tables`], one line each.
fn diff(kernel: &str) -> Vec<String> {
    let machines = match design::parse(kernel) {
        Ok(m) => m,
        Err(e) => return vec![format!("§10 does not parse: {e}")],
    };
    let tables = tables();
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

    for t in &tables {
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

        let printed = printed_edges(field);
        let kernel: BTreeSet<(String, String)> = t
            .edges
            .iter()
            .map(|e| (e.from.to_owned(), e.to.to_owned()))
            .collect();
        for (from, to) in printed.difference(&kernel) {
            out.push(format!(
                "{at}: §10 prints {from} → {to} and the kernel does not allow it"
            ));
        }
        for (from, to) in kernel.difference(&printed) {
            out.push(format!(
                "{at}: the kernel allows {from} → {to} and §10 does not print it"
            ));
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

        let notes = notes_for(&machines, t);
        let used: BTreeSet<Requirement> = t.edges.iter().filter_map(|e| e.requires).collect();
        for r in used {
            for phrase in r.phrases() {
                if !notes.contains(&squash(phrase)) {
                    out.push(format!(
                        "{at}: requirement {r:?} cites `{phrase}`, which the notes of `{}` do not state",
                        t.same_as.unwrap_or(t.machine)
                    ));
                }
            }
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
    assert!(
        problems.contains(
            &"`TaskRun` field `fence_state`: the kernel allows FENCED_UNCERTAIN → FENCED and §10 does not print it"
                .to_owned()
        ),
        "{problems:#?}"
    );
    assert!(
        problems.iter().any(|p| p.starts_with(
            "`AgentRun` field `fence_state`: the kernel allows FENCED_UNCERTAIN → FENCED"
        )),
        "{problems:#?}"
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
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with("`AgentRun` its own states: §10 lists the states")),
        "{problems:#?}"
    );
    assert!(
        problems.contains(
            &"`AgentRun` its own states: §10 prints RUNNING → HEARTBEAT_MISSED and the kernel does not allow it"
                .to_owned()
        ),
        "{problems:#?}"
    );
    assert!(
        problems.contains(
            &"`AgentRun` its own states: HEARTBEAT_LOST → fence_state := FENCE_PENDING is in only one of §10 and the kernel"
                .to_owned()
        ),
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
fn a_dropped_requirement_fails() {
    let problems = diff(&edited(
        "CENSORED → SETTLED (append-only correction)",
        "CENSORED → SETTLED",
    ));
    assert_eq!(
        problems,
        [
            "`UsageReceipt` its own states: requirement AppendOnlyCorrection cites `append-only correction`, which the notes of `UsageReceipt` do not state"
        ],
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
