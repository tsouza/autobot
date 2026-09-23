//! Keeps `autobot_kernel::lifecycle` equal to its single design home, KERNEL §10.
//!
//! The section is read with the shared parser, `autobot_devtools::design::lifecycle`, and
//! diffed against a list of tables, [`tables`] for the kernel's own. The check covers:
//!
//! - the same machines and fields, and the same `as` copies;
//! - the same states in the same order, so the same initial state;
//! - the same transitions once `any` and `any non-terminal` are expanded, and the same
//!   sibling-field moves;
//! - every `;`-separated clause of every note of a machine. Each clause is quoted verbatim by
//!   exactly one [`Requirement`], or listed once in [`DESCRIPTIVE`] with the reason it
//!   constrains no transition. The quoting requirement is:
//!   - on every transition the clause annotates: through the note printed on it, the line note
//!     of its arrow, or a note that names it by `FROM → TO` or a `TO:` clause;
//!   - elsewhere only on transitions into or out of a state the clause holds as a word. A clause
//!     that holds no state and annotates nothing may sit on any transition of its machine.
//!
//!   Every requirement a table uses quotes a clause of its machine, and every [`DESCRIPTIVE`]
//!   entry is still printed;
//! - the same event types as [`LifecycleEvent`].
//!
//! A failure means the two disagree. The design is the authority, so the fix is a `design`
//! finding that decides which one is wrong, not an edit of either side to match.

use autobot_devtools::design::lifecycle::{
    self as design, Field, Machine, Source, Transition, contains_whole,
};
use autobot_devtools::markdown;
use autobot_kernel::lifecycle::{Edge, LifecycleEvent, Requirement, Table, tables};
use std::collections::{BTreeMap, BTreeSet};

const KERNEL: &str = include_str!("../../../docs/design/AUTOBOT-KERNEL.md");

/// The clauses of §10 notes that constrain no transition: machine, clause, and why.
const DESCRIPTIVE: &[(&str, &str, &str)] = &[
    (
        "CommandReceipt",
        "it is never evaluated, never read as new intent and never rewrites a terminal receipt, and it is not reached from UNCERTAIN, which may already have committed",
        "what a REPLAY_EXPIRED receipt never does; the table has no UNCERTAIN → REPLAY_EXPIRED edge",
    ),
    (
        "CommandReceipt",
        "the retained cancellation receipt is the proof",
        "names the record that proves a CANCELLED receipt",
    ),
    (
        "ExternalOperation, ToolInvocation",
        "same key, attempt_index+1, send_attempt moved to prior_send_attempts",
        "what RECONCILING → REQUESTED writes, not a condition on taking it",
    ),
    (
        "ExternalOperation, ToolInvocation",
        "\"accepted, not sent\" is PERMITTED with a ledger entry ACCEPTED_NOT_SENT",
        "defines a name for a PERMITTED record; no edge",
    ),
    (
        "ExternalOperation, ToolInvocation",
        "DISPATCHING always carries send_attempt",
        "a field invariant of the DISPATCHING record",
    ),
    (
        "EffectIntent",
        "the source receipt is retained, §2, while any of its intents is not ACKNOWLEDGED",
        "constrains the retention of the source receipt, not an EffectIntent edge",
    ),
    (
        "EffectReceipt",
        "immutable, one per attempt",
        "the record never changes: the machine has no edge",
    ),
    (
        "pending commit slot",
        "per aggregate",
        "the scope of the machine",
    ),
    (
        "control receipt",
        "ring entry",
        "names where the record lives",
    ),
    (
        "reservation phase",
        "active_manager_transaction",
        "names the field that holds the machine",
    ),
    (
        "ledger entry",
        "send_state",
        "names the field that holds the machine",
    ),
    (
        "expected record",
        "TaskRun.status.expected_records.outcome, set PENDING at admission, and each expected_records.usage[producer], committed PENDING before its producer spends, §8",
        "which records the machine applies to and when each is created PENDING, its initial state",
    ),
    (
        "expected record",
        "GAP is final: a record committed after the gap closes the TelemetryGap and leaves the entry GAP",
        "GAP has no exit in the table; the late record moves the TelemetryGap",
    ),
    (
        "WorkContext",
        "new epoch",
        "what DRAINING → ACTIVE installs, not a condition on taking it",
    ),
    (
        "WorkContext",
        "hold_state and each manager_authority[plan].phase are independent of each other",
        "relates two fields; no edge of either depends on the other",
    ),
    (
        "WorkContext",
        "AcceptDispatch checks every field",
        "a check of the dispatch action, which moves none of these fields",
    ),
    (
        "WorkContext",
        "a keyed entry exists from the action that creates it until its retirement, §3.1, and its absence is no state: plan_authority[plan] is created ACTIVE by the first ActivatePlanRevision, and absent means no active revision",
        "how an entry is created and removed; its initial state, not an edge",
    ),
    (
        "WorkContext",
        "integration_authority[basis] is created RESERVED by ReserveIntegrationBasis, and INVALIDATED is final for that key",
        "creation in the initial state, and INVALIDATED having no exit in the table",
    ),
    (
        "Plan.phase",
        "the register still holds the previous revision QUIESCING",
        "the register while ACTIVATION_FAILED → QUIESCING holds; the edge's condition is `replacement revision only`",
    ),
    (
        "Plan.phase",
        "RevisionPending is a condition, not a phase",
        "says a name is not a state",
    ),
    (
        "Plan.phase",
        "ACTIVATING runs from the Plan controller's start of snapshot verification through MEMBERS_VERIFIED and the submission of ActivatePlanRevision, or of SupersedePlanRevision for a replacement",
        "what the ACTIVATING phase covers; its exits carry their own conditions",
    ),
    (
        "Plan.phase",
        "a plan that ended is retired from the registers, §3.1",
        "what happens to the registers after a terminal phase",
    ),
    (
        "WorkBrief",
        "immutable",
        "the record never changes: the machine has no edge",
    ),
    (
        "Charter, ProjectCharter",
        "both kinds carry revisions[rev]",
        "says which kinds hold the field",
    ),
    (
        "Charter, ProjectCharter",
        "an ACCEPTED revision is immutable and digested",
        "a property of the ACCEPTED record",
    ),
    (
        "Charter, ProjectCharter",
        "a SUPERSEDED revision stays pinned by every plan revision that pinned it",
        "a property of the SUPERSEDED record, which has no exit",
    ),
    (
        "ManagerLease",
        "acknowledgement of manager_authority",
        "what the record is",
    ),
    ("ManagerLease", "never authority", "what the record is not"),
    (
        "ManagerLease",
        "an EXPIRED lease never returns: AdvanceManagerEpoch installs a new lease",
        "EXPIRED has no exit in the table",
    ),
    (
        "Task, Milestone",
        "it lists the UID and digest of every EvidenceBundle it relies on",
        "what the acceptance adjudication records, not a condition on taking the edge",
    ),
    (
        "TaskRun",
        "custody keeps the candidate, and a replacement TaskRun verifies it again",
        "what happens to the candidate after the run ends",
    ),
    (
        "AgentRun",
        "the copy acknowledging the fence requested on the TaskRun",
        "says which copy of fence_state the sibling move sets",
    ),
    (
        "AgentRun",
        "fence_state and execution_epoch are acknowledged copies of its TaskRun's and authorize nothing",
        "what the copied fields are",
    ),
    (
        "AgentRun",
        "a fence of an AgentRun is requested on its TaskRun",
        "where a fence is requested; the TaskRun table carries that edge",
    ),
    (
        "FenceSession",
        "the Broker's evidence of one fence of one TaskRun at one execution_epoch, created for the TaskRun's FENCE_PENDING commit",
        "what the record is and when it is created in its initial state",
    ),
    (
        "FenceSession",
        "CONFIRMED records FenceConfirmed, §6",
        "the event the edge into CONFIRMED records, not a condition on taking it",
    ),
    (
        "Workspace",
        "retirement needs a new PRESERVED",
        "RETIRED is reached only from PRESERVED, which the table already fixes",
    ),
    (
        "ArtifactCommit",
        "the Workspace's PRESERVED follows it, §7",
        "constrains the Workspace, not an ArtifactCommit edge",
    ),
    (
        "IntegrationBasis",
        "the register is the authority",
        "says which record decides; the edges carry RegisterInvalidated",
    ),
    (
        "IntegrationBasis",
        "RetireIntegrationBasis then removes the entry, §3.1",
        "what happens to the register after RELEASED",
    ),
    (
        "EvidenceBundle",
        "a bundle has no accepted state: an accepted bundle is one a committed acceptance adjudication references",
        "says a name is not a state",
    ),
    (
        "TelemetryGap",
        "the gap is never removed, and every count uses the linked record from then on as an append-only correction, as for a CENSORED receipt later SETTLED",
        "how counts use a closed gap, not a condition on an edge",
    ),
    (
        "Decision",
        "immutable",
        "the record never changes: the machine has no edge",
    ),
    (
        "gate",
        "a gate of M0 §4, not a kind: its evidence is a signed manifest, M0 §5, and NOT_RUN is a gate with no manifest",
        "what the record is and what its initial state means",
    ),
];

/// Whitespace runs as one space, so a note printed over several lines matches a phrase.
fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One transition or sibling move of a machine: field, source, and target (`field := STATE`
/// for a sibling move).
type Key = (Option<String>, String, String);

fn key_text(k: &Key) -> String {
    let (field, from, to) = k;
    let at = field.as_ref().map_or(String::new(), |f| format!("{f}: "));
    format!("{at}{from} → {to}")
}

/// The sources `t` covers, with `any` and `any non-terminal` expanded: `any` to every state,
/// `any non-terminal` to every state with an outgoing transition from a named source; neither
/// to the target itself.
fn sources<'a>(field: &'a Field, t: &'a Transition) -> Vec<&'a str> {
    let non_terminal: BTreeSet<&str> = field
        .transitions
        .iter()
        .filter(|t| t.assigns.is_none())
        .filter_map(|t| match &t.from {
            Source::State(s) => Some(s.as_str()),
            Source::Any | Source::AnyNonTerminal => None,
        })
        .collect();
    let all: Vec<&str> = match &t.from {
        Source::State(s) => vec![s.as_str()],
        Source::Any => field.states.iter().map(String::as_str).collect(),
        Source::AnyNonTerminal => non_terminal.into_iter().collect(),
    };
    all.into_iter().filter(|s| *s != t.to).collect()
}

/// The key of each move `t` covers.
fn keys(field: &Field, t: &Transition) -> Vec<Key> {
    let to = match &t.assigns {
        Some(sibling) => format!("{sibling} := {}", t.to),
        None => t.to.clone(),
    };
    sources(field, t)
        .into_iter()
        .map(|from| (field.name.clone(), from.to_owned(), to.clone()))
        .collect()
}

/// The `(from, to)` transitions `field` prints.
fn printed_edges(field: &Field) -> BTreeSet<(String, String)> {
    field
        .transitions
        .iter()
        .filter(|t| t.assigns.is_none())
        .flat_map(|t| keys(field, t))
        .map(|(_, from, to)| (from, to))
        .collect()
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

/// Where a clause of a machine may and must be quoted.
#[derive(Default)]
struct Clause {
    /// The moves the clause annotates or names: its requirement is on each.
    required: BTreeSet<Key>,
    /// The moves its requirement may be on besides: those of a state the clause holds.
    allowed: BTreeSet<Key>,
    /// The clause holds no state and annotates nothing, so it may be on any move.
    anywhere: bool,
}

/// Every clause of the notes of `m`, with where it applies. A copied field (`as Kind`) is
/// accounted for on the machine it copies.
fn clauses(m: &Machine) -> BTreeMap<String, Clause> {
    let fields: Vec<&Field> = m.fields.iter().filter(|f| f.same_as.is_none()).collect();
    let moves: Vec<Key> = fields
        .iter()
        .flat_map(|f| f.transitions.iter().flat_map(|t| keys(f, t)))
        .collect();
    let states: BTreeSet<&str> = fields
        .iter()
        .flat_map(|f| f.states.iter().map(String::as_str))
        .collect();
    let mut out: BTreeMap<String, Clause> = BTreeMap::new();
    let add = |text: &str, on: &[Key], out: &mut BTreeMap<String, Clause>| {
        for clause in text.split(';').map(squash).filter(|c| !c.is_empty()) {
            let named: Vec<Key> = moves
                .iter()
                .filter(|(_, from, to)| {
                    contains_whole(&clause, &format!("{from} → {to}"))
                        || clause.starts_with(&format!("{to}:"))
                })
                .cloned()
                .collect();
            let holds = |s: &str| contains_whole(&clause, s);
            let allowed: Vec<Key> = moves
                .iter()
                .filter(|(_, from, to)| holds(from) || holds(to.rsplit(' ').next().unwrap_or(to)))
                .cloned()
                .collect();
            let anywhere = on.is_empty() && named.is_empty() && !states.iter().any(|s| holds(s));
            let c = out.entry(clause).or_default();
            c.required.extend(on.iter().cloned().chain(named));
            c.allowed.extend(allowed);
            c.anywhere |= anywhere;
        }
    };
    for f in &fields {
        for t in &f.transitions {
            let on = keys(f, t);
            for note in t.note.iter().chain(&t.line_note) {
                add(note, &on, &mut out);
            }
        }
    }
    for (i, note) in m.notes.iter().enumerate() {
        if !m.line_notes.contains(&i) {
            add(note, &[], &mut out);
        }
    }
    out
}

fn field_label(field: Option<&str>) -> String {
    field.map_or_else(|| "its own states".to_owned(), |f| format!("field `{f}`"))
}

/// Every disagreement between `kernel`'s §10 and the kernel's own tables, one line each.
fn diff(kernel: &str) -> Vec<String> {
    diff_with(kernel, &tables(), DESCRIPTIVE)
}

/// Every disagreement between `kernel`'s §10, `tables` and `descriptive`, one line each.
fn diff_with(kernel: &str, tables: &[Table], descriptive: &[(&str, &str, &str)]) -> Vec<String> {
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
    }

    out.extend(account(&machines, tables, descriptive));

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

/// Where each requirement of the kernel tables of `machine` sits.
fn uses(tables: &[Table], machine: &str) -> BTreeMap<Requirement, BTreeSet<Key>> {
    let mut out: BTreeMap<Requirement, BTreeSet<Key>> = BTreeMap::new();
    for t in tables
        .iter()
        .filter(|t| t.machine == machine && t.same_as.is_none())
    {
        let field = t.field.map(str::to_owned);
        let edges = t.edges.iter().map(|e: &Edge<&str>| {
            (
                (field.clone(), e.from.to_owned(), e.to.to_owned()),
                e.requires,
            )
        });
        let sets = t.sibling_sets.iter().map(|s| {
            (
                (
                    field.clone(),
                    s.from.to_owned(),
                    format!("{} := {}", s.field, s.to),
                ),
                s.requires,
            )
        });
        for (key, requires) in edges.chain(sets) {
            for r in requires {
                out.entry(*r).or_default().insert(key.clone());
            }
        }
    }
    out
}

/// Every clause of §10 that no requirement or descriptive entry accounts for, or accounts for
/// in the wrong place, and every requirement or entry that quotes no clause.
fn account(
    machines: &[Machine],
    tables: &[Table],
    descriptive: &[(&str, &str, &str)],
) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    for m in machines {
        let at = format!("`{}`", m.name);
        let used = uses(tables, &m.name);
        let clauses = clauses(m);
        for (text, c) in &clauses {
            seen.insert((m.name.clone(), text.clone()));
            let listed = descriptive
                .iter()
                .filter(|(dm, dc, _)| *dm == m.name && squash(dc) == *text)
                .count();
            let quoting: Vec<Requirement> = Requirement::ALL
                .iter()
                .copied()
                .filter(|r| squash(r.phrase()) == *text)
                .collect();
            if listed > 1 {
                out.push(format!(
                    "{at}: clause `{text}` is listed as descriptive {listed} times"
                ));
            }
            match (listed, quoting.as_slice()) {
                (0, []) => out.push(format!(
                    "{at}: clause `{text}` is neither quoted by a requirement nor listed as descriptive"
                )),
                (0, [r]) => {
                    let on = used.get(r).cloned().unwrap_or_default();
                    if on.is_empty() {
                        out.push(format!("{at}: {r:?} quotes `{text}` and no transition carries it"));
                    }
                    for k in c.required.difference(&on) {
                        out.push(format!(
                            "{at}: §10 states `{text}` on {} and it does not carry {r:?}",
                            key_text(k)
                        ));
                    }
                    for k in on.iter().filter(|k| {
                        !c.anywhere && !c.required.contains(*k) && !c.allowed.contains(*k)
                    }) {
                        out.push(format!(
                            "{at}: {} carries {r:?}, but `{text}` neither annotates it nor holds its states",
                            key_text(k)
                        ));
                    }
                }
                (0, many) => out.push(format!("{at}: clause `{text}` is quoted by {many:?}")),
                (_, []) => {}
                (_, quoted) => out.push(format!(
                    "{at}: clause `{text}` is listed as descriptive and quoted by {quoted:?}"
                )),
            }
        }
        for (r, on) in &used {
            if !clauses.contains_key(&squash(r.phrase())) {
                let on: Vec<String> = on.iter().map(key_text).collect();
                out.push(format!(
                    "{at}: {r:?} is on {} but §10 prints no clause `{}` for it",
                    on.join(", "),
                    r.phrase()
                ));
            }
        }
    }
    for (m, c, _) in descriptive {
        if !seen.contains(&((*m).to_owned(), squash(c))) {
            out.push(format!(
                "`{m}`: descriptive entry `{c}` is no longer in §10"
            ));
        }
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

/// The kernel's tables with the requirements of `from → to` on `machine`'s own states set to
/// `requires`.
fn with_requires(
    machine: &str,
    from: &str,
    to: &str,
    requires: &'static [Requirement],
) -> Vec<Table> {
    let mut all = tables();
    let t = all
        .iter_mut()
        .find(|t| t.machine == machine && t.field.is_none())
        .unwrap();
    let e = t
        .edges
        .iter_mut()
        .find(|e| e.from == from && e.to == to)
        .unwrap();
    e.requires = requires;
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
    for want in [
        "`TaskRun` field `fence_state`: the kernel allows FENCED_UNCERTAIN → FENCED and §10 does not print it",
        "`AgentRun` field `fence_state`: the kernel allows FENCED_UNCERTAIN → FENCED and §10 does not print it",
    ] {
        assert!(problems.contains(&want.to_owned()), "{problems:#?}");
    }
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
            "`AgentCheckpoint` its own states: §10 prints VERIFIED → QUARANTINED and the kernel does not allow it",
            "`AgentCheckpoint`: §10 states `QUARANTINED: out-of-scope content was detected at the checkpoint, ROLES §2` on VERIFIED → QUARANTINED and it does not carry OutOfScopeContent",
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
            "`UsageReceipt`: AppendOnlyCorrection is on CENSORED → SETTLED but §10 prints no clause `append-only correction` for it"
        ],
    );
}

#[test]
fn an_unaccounted_clause_fails() {
    let problems = diff(&edited(
        "Artifact             PENDING → VERIFIED | EXPIRED",
        "Artifact             PENDING → VERIFIED | EXPIRED   (checked by digest; EXPIRED past its retention)",
    ));
    assert_eq!(
        problems,
        [
            "`Artifact`: clause `EXPIRED past its retention` is neither quoted by a requirement nor listed as descriptive",
            "`Artifact`: clause `checked by digest` is neither quoted by a requirement nor listed as descriptive",
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
            "`UsageReceipt`: §10 states `append-only correction` on UNKNOWN → CENSORED and it does not carry AppendOnlyCorrection",
            "`UsageReceipt`: CENSORED → SETTLED carries AppendOnlyCorrection, but `append-only correction` neither annotates it nor holds its states",
        ],
    );
}

#[test]
fn a_requirement_moved_to_another_edge_fails() {
    let mut moved = with_requires("UsageReceipt", "CENSORED", "SETTLED", &[]);
    let t = moved
        .iter_mut()
        .find(|t| t.machine == "UsageReceipt")
        .unwrap();
    let e = t
        .edges
        .iter_mut()
        .find(|e| e.from == "PARTIAL" && e.to == "SETTLED")
        .unwrap();
    e.requires = &[Requirement::AppendOnlyCorrection];
    assert_eq!(
        diff_with(KERNEL, &moved, DESCRIPTIVE),
        [
            "`UsageReceipt`: §10 states `append-only correction` on CENSORED → SETTLED and it does not carry AppendOnlyCorrection",
            "`UsageReceipt`: PARTIAL → SETTLED carries AppendOnlyCorrection, but `append-only correction` neither annotates it nor holds its states",
        ],
    );
    // A clause that only a machine note states is bound to the states it holds.
    let mut prose = with_requires("TaskRun", "PREPARING", "EXECUTING", &[]);
    let t = prose
        .iter_mut()
        .find(|t| t.machine == "TaskRun" && t.field.is_none())
        .unwrap();
    let e = t
        .edges
        .iter_mut()
        .find(|e| e.from == "ADMITTED" && e.to == "PREPARING")
        .unwrap();
    e.requires = &[Requirement::FenceActive];
    assert_eq!(
        diff_with(KERNEL, &prose, DESCRIPTIVE),
        [
            "`TaskRun`: ADMITTED → PREPARING carries FenceActive, but `once fence_state leaves ACTIVE the phase never moves to EXECUTING or SUCCEEDED` neither annotates it nor holds its states"
        ],
    );
}

#[test]
fn a_weaker_requirement_on_the_right_edge_fails() {
    let weaker = with_requires(
        "TaskRun",
        "PREPARING",
        "FAILED",
        &[Requirement::Fenced, Requirement::FenceNotPending],
    );
    assert_eq!(
        diff_with(KERNEL, &weaker, DESCRIPTIVE),
        [
            "`TaskRun`: PREPARING → FAILED carries Fenced, but `fenced` neither annotates it nor holds its states",
            "`TaskRun`: SetupFailedOrFenced quotes `setup failed, or fenced` and no transition carries it",
            "`TaskRun`: §10 states `setup failed, or fenced` on PREPARING → FAILED and it does not carry SetupFailedOrFenced",
        ],
    );
}

#[test]
fn a_prose_clause_on_no_edge_fails() {
    let mut none = tables();
    for t in none.iter_mut().filter(|t| t.machine == "Plan.phase") {
        for e in &mut t.edges {
            e.requires = &[];
        }
    }
    let problems = diff_with(KERNEL, &none, DESCRIPTIVE);
    assert!(
        problems.contains(
            &"`Plan.phase`: QuiescedFirst quotes `FAILED and CANCELLED of a plan that holds a plan_authority entry follow its QuiescePlan and the §5 wait for active attempts, during which the phase stays where it was and then moves directly to FAILED or CANCELLED` and no transition carries it"
                .to_owned()
        ),
        "{problems:#?}"
    );
}

#[test]
fn descriptive_entries_are_listed_once_and_still_printed() {
    let problems = diff(&edited(
        "EffectReceipt        RECORDED  (immutable, one per attempt)",
        "EffectReceipt        RECORDED  (immutable)",
    ));
    assert_eq!(
        problems,
        [
            "`EffectReceipt`: clause `immutable` is neither quoted by a requirement nor listed as descriptive",
            "`EffectReceipt`: descriptive entry `immutable, one per attempt` is no longer in §10",
        ],
    );
    let mut twice = DESCRIPTIVE.to_vec();
    twice.push(("Decision", "immutable", "again"));
    assert_eq!(
        diff_with(KERNEL, &tables(), &twice),
        ["`Decision`: clause `immutable` is listed as descriptive 2 times"],
    );
    let mut quoted = DESCRIPTIVE.to_vec();
    quoted.push(("UsageReceipt", "append-only correction", "wrongly"));
    assert_eq!(
        diff_with(KERNEL, &tables(), &quoted),
        [
            "`UsageReceipt`: clause `append-only correction` is listed as descriptive and quoted by [AppendOnlyCorrection]"
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
