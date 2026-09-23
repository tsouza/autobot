//! The issue graph: its lint, its Mermaid render and its critical path.
//!
//! The graph is every issue of the repository (pull requests excluded) with its labels,
//! milestone and parent, plus the native "blocked by" edges of every open issue. An edge
//! `a → b` reads "`a` is blocked by `b`". Everything here is read-only.
//!
//! [`lint`] checks every open issue against these rules:
//!
//! - it has a milestone whose title starts with a milestone prefix (`M-1`, `M0-Q`, `M0-QF`,
//!   `M1` … `M9`); the prefixes order the milestones by number, then by suffix;
//! - unless it is an epic (`type:epic`), its parent is an epic in the same milestone;
//! - it carries exactly one of `type:epic`, `type:task` and `finding`, and no priority label
//!   (`priority…`, `prio:…`, `prio/…` or `p0`…`p9`; `urgent` is not one);
//! - if the paths under its **Allowed paths** lie in, or above, `.github/workflows/`,
//!   `.github/rulesets/` or `docs/design/`, it carries `human-lane`;
//! - an epic has a non-empty `## Acceptance` section (outside fenced code) and is blocked only by epics;
//! - it has at most 100 sub-issues and at most 50 blockers;
//! - no blocker sits in a later milestone;
//! - the blocked-by graph is acyclic, and every edge except those between two gate epics is
//!   transitively reduced (no edge `a → b` where `b` is also reachable through another
//!   blocker of `a`);
//! - the edges between gate epics equal the Depends-on column of M0 §4 exactly.
//!
//! A gate epic is an epic whose body starts with `Gate epic for G-X.`. A Depends-on cell
//! names gates either by gate id (`G-QUAL`) or by milestone prefix (`M0-Q`); a milestone
//! prefix maps to the gate id that appears in that milestone's title. Other text in the
//! cell (`frozen design`, `alongside M0`) names no gate. Gate-epic edges are compared only
//! for open gate epics, since closed issues keep no blocked-by edges of interest.

use crate::github::Client;
use crate::github::settings::repo_from_remote;
use crate::process::Cmd;
use crate::{Error, Result, git, markdown};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::process::ExitCode;

/// The design document holding the gate table, relative to the repository root.
pub const GATES_DOCUMENT: &str = "docs/design/AUTOBOT-M0-AND-GATES.md";
/// The heading prefix of the section holding the gate table.
const GATES_SECTION: &str = "4. Milestones and gates";
/// GitHub's limit of sub-issues per parent.
pub const SUB_ISSUE_LIMIT: u64 = 100;
/// GitHub's limit of blocked-by edges per issue.
pub const BLOCKED_BY_LIMIT: u64 = 50;
const EPIC: &str = "type:epic";
const KIND_LABELS: [&str; 3] = [EPIC, "type:task", "finding"];
const HUMAN_LANE: &str = "human-lane";
const HUMAN_LANE_PATHS: [&str; 3] = [".github/workflows", ".github/rulesets", "docs/design"];
const GATE_EPIC_PREFIX: &str = "Gate epic for ";

/// One issue, reduced to what the graph rules read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// Issue number.
    pub number: u64,
    /// Title.
    pub title: String,
    /// Whether the issue is open.
    pub open: bool,
    /// Label names.
    pub labels: BTreeSet<String>,
    /// Milestone title, if any.
    pub milestone: Option<String>,
    /// Parent issue number, if the issue is a sub-issue.
    pub parent: Option<u64>,
    /// Body text (empty when the issue has none).
    pub body: String,
    /// Number of sub-issues.
    pub sub_issues: u64,
    /// Number of blocked-by edges as GitHub counts them.
    pub blocked_by_total: u64,
}

impl Issue {
    /// Parses one issue object as returned by the GitHub issues API.
    ///
    /// # Errors
    /// Fails if the object has no numeric `number`.
    pub fn from_api(v: &Value) -> Result<Self> {
        let number = v["number"]
            .as_u64()
            .ok_or_else(|| Error::Parse(format!("issue without a number: {v}")))?;
        let text = |key: &str| v[key].as_str().unwrap_or_default().to_owned();
        let labels = v["labels"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|l| l["name"].as_str().or_else(|| l.as_str()))
            .map(str::to_owned)
            .collect();
        Ok(Self {
            number,
            title: text("title"),
            open: v["state"].as_str() == Some("open"),
            labels,
            milestone: v["milestone"]["title"].as_str().map(str::to_owned),
            parent: v["parent_issue_url"]
                .as_str()
                .and_then(|url| url.rsplit('/').next())
                .and_then(|n| n.parse().ok()),
            body: text("body"),
            sub_issues: v["sub_issues_summary"]["total"].as_u64().unwrap_or(0),
            blocked_by_total: v["issue_dependencies_summary"]["total_blocked_by"]
                .as_u64()
                .unwrap_or(0),
        })
    }

    /// Whether the issue carries `type:epic`.
    #[must_use]
    pub fn is_epic(&self) -> bool {
        self.labels.contains(EPIC)
    }

    /// The gate id `G-X` if this is a gate epic (an epic whose body starts with
    /// `Gate epic for G-X.`).
    #[must_use]
    pub fn gate(&self) -> Option<&str> {
        if !self.is_epic() {
            return None;
        }
        let rest = self.body.trim_start().strip_prefix(GATE_EPIC_PREFIX)?;
        let (id, _) = rest.split_once('.')?;
        is_gate_id(id).then_some(id)
    }
}

/// The issues of a repository and the blocked-by edges of its open issues.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Graph {
    /// Every issue, by number.
    pub issues: BTreeMap<u64, Issue>,
    /// For each open issue, the numbers of the issues blocking it.
    pub blocked_by: BTreeMap<u64, BTreeSet<u64>>,
}

impl Graph {
    /// Builds the graph from API responses: `issues` is the concatenated issues listing,
    /// `blocked_by` maps an issue number to its `dependencies/blocked_by` listing.
    /// Pull requests in the listing are skipped; a blocker missing from the listing is
    /// taken from the blocked-by response.
    ///
    /// # Errors
    /// Fails if a response is not an array of issue objects.
    pub fn from_api(issues: &Value, blocked_by: &BTreeMap<u64, Value>) -> Result<Self> {
        let mut graph = Self::default();
        for v in array(issues)? {
            if v.get("pull_request").is_none() {
                let issue = Issue::from_api(v)?;
                graph.issues.insert(issue.number, issue);
            }
        }
        for (&number, listing) in blocked_by {
            let edges = graph.blocked_by.entry(number).or_default();
            for v in array(listing)? {
                let blocker = Issue::from_api(v)?;
                edges.insert(blocker.number);
                graph.issues.entry(blocker.number).or_insert(blocker);
            }
        }
        Ok(graph)
    }

    /// Fetches the graph of the client's repository.
    ///
    /// # Errors
    /// Fails on any API error.
    pub fn fetch(client: &Client) -> Result<Self> {
        let mut issues = Vec::new();
        for page in 1.. {
            let Value::Array(batch) =
                client.get(&format!("issues?state=all&per_page=100&page={page}"))?
            else {
                return Err(Error::Parse("issues listing is not an array".to_owned()));
            };
            let last = batch.len() < 100;
            issues.extend(batch);
            if last {
                break;
            }
        }
        let issues = Value::Array(issues);
        let mut blocked_by = BTreeMap::new();
        for issue in array(&issues)? {
            let issue = Issue::from_api(issue)?;
            if issue.open && issue.blocked_by_total > 0 {
                let path = format!(
                    "issues/{}/dependencies/blocked_by?per_page=100",
                    issue.number
                );
                blocked_by.insert(issue.number, client.get(&path)?);
            }
        }
        Self::from_api(&issues, &blocked_by)
    }

    fn blockers(&self, number: u64) -> impl Iterator<Item = u64> + '_ {
        self.blocked_by.get(&number).into_iter().flatten().copied()
    }

    fn open(&self) -> impl Iterator<Item = &Issue> {
        self.issues.values().filter(|i| i.open)
    }

    /// Every issue reachable from `start` through one or more edges.
    fn reach(&self, start: u64) -> BTreeSet<u64> {
        let mut seen = BTreeSet::new();
        let mut stack: Vec<u64> = self.blockers(start).collect();
        while let Some(n) = stack.pop() {
            if seen.insert(n) {
                stack.extend(self.blockers(n));
            }
        }
        seen
    }
}

fn array(v: &Value) -> Result<&[Value]> {
    v.as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| Error::Parse(format!("expected a JSON array, got {v}")))
}

fn is_gate_id(s: &str) -> bool {
    s.strip_prefix("G-").is_some_and(|rest| {
        !rest.is_empty()
            && rest
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-')
    })
}

/// The ordering key of a milestone: its number, then its suffix.
pub type MilestoneKey = (i64, String);

/// The ordering key of a milestone prefix such as `M-1`, `M0-QF` or `M8`: its number, then
/// its suffix. `None` if `prefix` is not one.
#[must_use]
pub fn milestone_key(prefix: &str) -> Option<MilestoneKey> {
    let rest = prefix.strip_prefix('M')?;
    let digits_end = rest
        .char_indices()
        .find(|&(i, c)| !(c.is_ascii_digit() || (i == 0 && c == '-')))
        .map_or(rest.len(), |(i, _)| i);
    let (num, suffix) = rest.split_at(digits_end);
    let num: i64 = num.parse().ok()?;
    let suffix = match suffix {
        "" => "",
        s => s
            .strip_prefix('-')
            .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_uppercase()))?,
    };
    Some((num, suffix.to_owned()))
}

/// The ordering key of a milestone title: that of its first word.
fn title_key(title: &str) -> Option<MilestoneKey> {
    milestone_key(title.split_whitespace().next()?)
}

/// The gate id (`G-X`) named in a milestone title, if any.
fn title_gate(title: &str) -> Option<&str> {
    title.split_whitespace().find(|w| is_gate_id(w))
}

/// The Depends-on column of the M0 §4 gate table: for each gate id, the raw entries of its
/// Depends-on cell (gate ids, milestone prefixes or free text).
///
/// # Errors
/// Fails if the document has no §4 table with `Gate` and `Depends on` columns, or a row's
/// Gate cell names no gate id.
pub fn gate_table(design: &str) -> Result<BTreeMap<String, Vec<String>>> {
    let missing = || Error::Parse(format!("no gate table under \"{GATES_SECTION}\""));
    let section = markdown::section(design, GATES_SECTION).ok_or_else(missing)?;
    let table = markdown::tables(section)
        .into_iter()
        .find(|t| {
            t.first()
                .is_some_and(|h| h.iter().any(|c| c == "Depends on"))
        })
        .ok_or_else(missing)?;
    let header = table.first().ok_or_else(missing)?;
    let col = |name: &str| header.iter().position(|c| c == name).ok_or_else(missing);
    let (gate_col, deps_col) = (col("Gate")?, col("Depends on")?);
    let mut out = BTreeMap::new();
    for row in table.iter().skip(1) {
        let cell = |i: usize| row.get(i).map_or("", String::as_str);
        let gate = cell(gate_col)
            .trim_matches('*')
            .rsplit('/')
            .next()
            .unwrap_or("")
            .trim();
        if !is_gate_id(gate) {
            return Err(Error::Parse(format!(
                "gate table row names no gate: {row:?}"
            )));
        }
        let deps = cell(deps_col)
            .split(',')
            .map(|d| d.trim().to_owned())
            .collect();
        out.insert(gate.to_owned(), deps);
    }
    Ok(out)
}

/// The rule a [`Violation`] breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rule {
    /// No milestone, no parent epic, or a parent that is not an epic.
    Orphan,
    /// A milestone title without a milestone prefix.
    MilestoneTitle,
    /// A sub-issue in a different milestone than its parent epic.
    ParentMilestone,
    /// An epic without a non-empty `## Acceptance` section.
    Acceptance,
    /// Not exactly one of `type:epic`, `type:task` and `finding`.
    KindLabel,
    /// A priority label.
    PriorityLabel,
    /// Allowed paths touching workflows, the ruleset or the design without `human-lane`.
    HumanLane,
    /// Part of a blocked-by cycle.
    Cycle,
    /// A blocked-by edge implied by another path.
    Redundant,
    /// A blocker in a later milestone.
    LaterMilestone,
    /// A gate-epic edge that is not in the M0 §4 Depends-on column, an M0 §4 edge missing
    /// between gate epics, or a second gate epic for the same gate.
    GateEdges,
    /// An epic blocked by an issue that is not an epic.
    EpicBlocker,
    /// Over the sub-issue or blocked-by limit.
    Limit,
}

/// One broken rule on one issue.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Violation {
    /// The offending issue.
    pub issue: u64,
    /// The rule broken.
    pub rule: Rule,
    /// What is wrong.
    pub detail: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{} {:?}: {}", self.issue, self.rule, self.detail)
    }
}

/// The text of a capsule heading line: a bold line (`**Name**`) or a Markdown heading.
fn capsule_heading(line: &str) -> Option<&str> {
    let t = line.trim();
    let bold = t.len() > 4 && t.starts_with("**") && t.ends_with("**");
    let text = if bold {
        t.get(2..t.len() - 2)
    } else {
        markdown::heading_of(t).map(|(_, text)| text)
    };
    text.map(str::trim)
}

/// The text under a capsule heading written either as a bold line (`**Allowed paths**`) or
/// as a Markdown heading, up to the next such heading.
fn capsule_section(body: &str, name: &str) -> Option<String> {
    let mut lines = body.lines();
    lines.by_ref().find(|l| capsule_heading(l) == Some(name))?;
    Some(
        lines
            .take_while(|l| capsule_heading(l).is_none())
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// Whether an Allowed paths text names a path under, or an ancestor of, the human-lane areas.
///
/// Each token is read as a glob over `/`-separated segments: a `**` segment spans any number of
/// segments, and inside a segment `*` spans any run of characters, `?` one character and
/// `[...]` one character. A token touches an area when some path it names lies inside the
/// area (`docs/design/x.md`, `**/*.yml`), is the area itself (`.github/work*`), or is a
/// directory above it (`.github`, `.github/**`, `docs/`). A token made only of `*` (`*`, `**`)
/// never matches, since it cannot be told apart from a Markdown bullet or bold marker in the
/// capsule text.
fn touches_human_lane(paths: &str) -> bool {
    paths
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '(' | ')' | '`'))
        .filter(|token| !token.is_empty() && !token.chars().all(|c| c == '*'))
        .any(|token| {
            let pattern: Vec<&str> = token.trim_end_matches('/').split('/').collect();
            HUMAN_LANE_PATHS.iter().any(|area| {
                let area: Vec<&str> = area.split('/').collect();
                glob_overlaps(&pattern, &area)
            })
        })
}

/// Whether a path named by the segment glob `pattern` lies inside `area`, is it, or is a
/// directory above it.
fn glob_overlaps(pattern: &[&str], area: &[&str]) -> bool {
    match (pattern.split_first(), area.split_first()) {
        // Either the area is consumed, so the rest of the pattern names the area or a path
        // inside it, or the pattern is consumed first, so it names a directory above the area.
        (_, None) | (None, Some(_)) => true,
        (Some((&"**", rest)), Some((_, area_rest))) => {
            glob_overlaps(rest, area) || glob_overlaps(pattern, area_rest)
        }
        (Some((seg, rest)), Some((name, area_rest))) => {
            segment_matches(seg.as_bytes(), name.as_bytes()) && glob_overlaps(rest, area_rest)
        }
    }
}

/// Whether one glob segment matches one path segment.
fn segment_matches(pattern: &[u8], name: &[u8]) -> bool {
    match pattern.split_first() {
        None => name.is_empty(),
        Some((b'*', rest)) => (0..=name.len()).any(|i| segment_matches(rest, &name[i..])),
        Some((b'?', rest)) => !name.is_empty() && segment_matches(rest, &name[1..]),
        Some((b'[', rest)) if rest.contains(&b']') => {
            let close = rest.iter().position(|&b| b == b']').map_or(0, |i| i + 1);
            !name.is_empty() && segment_matches(&rest[close..], &name[1..])
        }
        Some((c, rest)) => name.first() == Some(c) && segment_matches(rest, &name[1..]),
    }
}

fn is_priority_label(label: &str) -> bool {
    let l = label.to_ascii_lowercase();
    let mut chars = l.chars();
    l.starts_with("priority")
        || l.starts_with("prio:")
        || l.starts_with("prio/")
        || (chars.next() == Some('p')
            && chars.next().is_some_and(|c| c.is_ascii_digit())
            && chars.next().is_none())
}

/// Whether the first exact `## Acceptance` heading in `body` has a non-empty section.
///
/// Headings are found with [`markdown::section`], so a heading inside a fenced code block is
/// never taken for the section. An earlier heading that only starts with `Acceptance` (such as
/// `## Acceptance evidence`) is skipped, and the search resumes on the line after it.
fn has_acceptance(body: &str) -> bool {
    let mut rest = body;
    while let Some(section) = markdown::section(rest, "Acceptance") {
        // `section` borrows from `rest` and starts right after its heading line.
        let start = section.as_ptr() as usize - rest.as_ptr() as usize;
        let heading = rest[..start].trim_end().lines().last().unwrap_or_default();
        if markdown::heading_of(heading.trim_end()) == Some((2, "Acceptance")) {
            return !section.trim().is_empty();
        }
        rest = &rest[start..];
    }
    false
}

/// Checks `graph` against every rule in the module docs, with the gate edges taken from
/// `gates` (see [`gate_table`]). Violations come sorted by issue, then rule.
///
/// # Errors
/// Fails if a Depends-on entry looks like a milestone prefix that no milestone title
/// carries, or names a gate that has no gate epic.
pub fn lint(graph: &Graph, gates: &BTreeMap<String, Vec<String>>) -> Result<Vec<Violation>> {
    let mut out = BTreeSet::new();
    let mut flag = |issue: u64, rule: Rule, detail: String| {
        out.insert(Violation {
            issue,
            rule,
            detail,
        });
    };
    let key_of = |n: u64| {
        graph
            .issues
            .get(&n)
            .and_then(|i| i.milestone.as_deref())
            .and_then(title_key)
    };

    for issue in graph.open() {
        let n = issue.number;
        let kinds: Vec<&str> = KIND_LABELS
            .into_iter()
            .filter(|k| issue.labels.contains(*k))
            .collect();
        if kinds.len() != 1 {
            flag(
                n,
                Rule::KindLabel,
                format!("carries {kinds:?}; needs exactly one of {KIND_LABELS:?}"),
            );
        }
        for label in issue.labels.iter().filter(|l| is_priority_label(l)) {
            flag(n, Rule::PriorityLabel, format!("priority label `{label}`"));
        }
        match issue.milestone.as_deref() {
            None => flag(n, Rule::Orphan, "no milestone".to_owned()),
            Some(t) if title_key(t).is_none() => {
                flag(
                    n,
                    Rule::MilestoneTitle,
                    format!("milestone `{t}` has no milestone prefix"),
                );
            }
            Some(_) => {}
        }
        if issue.is_epic() {
            if !has_acceptance(&issue.body) {
                flag(
                    n,
                    Rule::Acceptance,
                    "epic without a `## Acceptance` section".to_owned(),
                );
            }
        } else {
            match issue.parent.and_then(|p| graph.issues.get(&p)) {
                None => flag(n, Rule::Orphan, "no parent epic".to_owned()),
                Some(p) if !p.is_epic() => {
                    flag(
                        n,
                        Rule::Orphan,
                        format!("parent #{} is not an epic", p.number),
                    );
                }
                Some(p) if p.milestone != issue.milestone => flag(
                    n,
                    Rule::ParentMilestone,
                    format!("milestone differs from parent epic #{}", p.number),
                ),
                Some(_) => {}
            }
        }
        if capsule_section(&issue.body, "Allowed paths").is_some_and(|p| touches_human_lane(&p))
            && !issue.labels.contains(HUMAN_LANE)
        {
            flag(
                n,
                Rule::HumanLane,
                format!("allowed paths need `{HUMAN_LANE}`"),
            );
        }
        if issue.sub_issues > SUB_ISSUE_LIMIT {
            flag(
                n,
                Rule::Limit,
                format!("{} sub-issues (limit {SUB_ISSUE_LIMIT})", issue.sub_issues),
            );
        }
        let blockers = issue.blocked_by_total.max(graph.blockers(n).count() as u64);
        if blockers > BLOCKED_BY_LIMIT {
            flag(
                n,
                Rule::Limit,
                format!("{blockers} blockers (limit {BLOCKED_BY_LIMIT})"),
            );
        }
        for b in graph.blockers(n) {
            if let (Some(own), Some(theirs)) = (key_of(n), key_of(b))
                && theirs > own
            {
                flag(
                    n,
                    Rule::LaterMilestone,
                    format!("blocked by #{b} in a later milestone"),
                );
            }
            if issue.is_epic() && !graph.issues.get(&b).is_some_and(Issue::is_epic) {
                flag(
                    n,
                    Rule::EpicBlocker,
                    format!("epic blocked by #{b}, which is not an epic"),
                );
            }
        }
    }

    let gate_of = |n: u64| graph.issues.get(&n).and_then(Issue::gate);
    let reach: BTreeMap<u64, BTreeSet<u64>> = graph
        .blocked_by
        .keys()
        .map(|&n| (n, graph.reach(n)))
        .collect();
    let mut cyclic = false;
    for (&n, r) in &reach {
        if r.contains(&n) {
            cyclic = true;
            let members: Vec<u64> = r
                .iter()
                .copied()
                .filter(|m| reach.get(m).is_some_and(|rm| rm.contains(&n)))
                .collect();
            flag(
                n,
                Rule::Cycle,
                format!("in a blocked-by cycle with {members:?}"),
            );
        }
    }
    if !cyclic {
        for (&n, blockers) in &graph.blocked_by {
            for &b in blockers {
                if gate_of(n).is_some() && gate_of(b).is_some() {
                    continue;
                }
                let via = blockers
                    .iter()
                    .copied()
                    .find(|&w| w != b && reach.get(&w).is_some_and(|r| r.contains(&b)));
                if let Some(w) = via {
                    flag(
                        n,
                        Rule::Redundant,
                        format!("edge to #{b} is implied through #{w}"),
                    );
                }
            }
        }
    }

    for v in gate_edges(graph, gates)? {
        out.insert(v);
    }
    Ok(out.into_iter().collect())
}

/// Compares the edges between open gate epics with the gate table.
fn gate_edges(graph: &Graph, gates: &BTreeMap<String, Vec<String>>) -> Result<Vec<Violation>> {
    let mut epics: BTreeMap<&str, u64> = BTreeMap::new();
    let mut out = Vec::new();
    for issue in graph.issues.values() {
        if let Some(g) = issue.gate()
            && let Some(first) = epics.insert(g, issue.number)
        {
            out.push(Violation {
                issue: issue.number,
                rule: Rule::GateEdges,
                detail: format!("second gate epic for {g} (first: #{first})"),
            });
        }
    }
    let milestone_gates: BTreeMap<&str, &str> = graph
        .issues
        .values()
        .filter_map(|i| i.milestone.as_deref())
        .filter_map(|t| Some((t.split_whitespace().next()?, title_gate(t)?)))
        .collect();
    let epic_of = |gate: &str| {
        epics
            .get(gate)
            .copied()
            .ok_or_else(|| Error::Parse(format!("M0 §4 names {gate}, which has no gate epic")))
    };
    for (gate, deps) in gates {
        let epic = epic_of(gate)?;
        if !graph.issues.get(&epic).is_some_and(|i| i.open) {
            continue;
        }
        let mut expected = BTreeSet::new();
        for dep in deps {
            let dep_gate = if is_gate_id(dep) {
                dep.as_str()
            } else if milestone_key(dep).is_some() {
                milestone_gates.get(dep.as_str()).copied().ok_or_else(|| {
                    Error::Parse(format!(
                        "M0 §4 names milestone {dep}, which no milestone title maps to a gate"
                    ))
                })?
            } else {
                continue;
            };
            expected.insert(epic_of(dep_gate)?);
        }
        let actual: BTreeSet<u64> = graph
            .blockers(epic)
            .filter(|b| graph.issues.get(b).and_then(Issue::gate).is_some())
            .collect();
        for missing in expected.difference(&actual) {
            out.push(Violation {
                issue: epic,
                rule: Rule::GateEdges,
                detail: format!("M0 §4 edge to gate epic #{missing} is missing"),
            });
        }
        for extra in actual.difference(&expected) {
            out.push(Violation {
                issue: epic,
                rule: Rule::GateEdges,
                detail: format!("edge to gate epic #{extra} is not in M0 §4"),
            });
        }
    }
    Ok(out)
}

/// The blocked-by graph of the open issues as a Mermaid flowchart, one subgraph per
/// milestone in milestone order; an arrow runs from a blocker to the issue it blocks.
#[must_use]
pub fn mermaid(graph: &Graph) -> String {
    let mut by_milestone: BTreeMap<(Option<MilestoneKey>, &str), Vec<&Issue>> = BTreeMap::new();
    for issue in graph.open() {
        let title = issue.milestone.as_deref().unwrap_or("no milestone");
        by_milestone
            .entry((title_key(title), title))
            .or_default()
            .push(issue);
    }
    let mut out = String::from("flowchart LR\n");
    for (i, ((_, title), issues)) in by_milestone.iter().enumerate() {
        out.push_str(&format!("  subgraph m{i}[\"{}\"]\n", escape(title)));
        for issue in issues {
            let label = escape(&format!("#{} {}", issue.number, issue.title));
            let (open, close) = if issue.is_epic() {
                ("[[", "]]")
            } else {
                ("[", "]")
            };
            out.push_str(&format!("    i{}{open}\"{label}\"{close}\n", issue.number));
        }
        out.push_str("  end\n");
    }
    for (n, blockers) in &graph.blocked_by {
        for b in blockers {
            if graph.issues.get(b).is_some_and(|i| i.open)
                && graph.issues.get(n).is_some_and(|i| i.open)
            {
                out.push_str(&format!("  i{b} --> i{n}\n"));
            }
        }
    }
    out
}

/// Escapes text for a quoted Mermaid label.
fn escape(s: &str) -> String {
    s.replace('#', "#35;").replace('"', "#quot;")
}

/// The longest chain of open issues along open blocked-by edges, first to do first. Ties
/// go to the lowest issue number.
///
/// # Errors
/// Fails if the open issues form a cycle.
pub fn critical_path(graph: &Graph) -> Result<Vec<u64>> {
    let open: BTreeSet<u64> = graph.open().map(|i| i.number).collect();
    let open_blockers = |n: u64| graph.blockers(n).filter(|b| open.contains(b));
    // Kahn's algorithm over "blocker before blocked".
    let mut pending: BTreeMap<u64, usize> = open
        .iter()
        .map(|&n| (n, open_blockers(n).count()))
        .collect();
    let mut blocks: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    for &n in &open {
        for b in open_blockers(n) {
            blocks.entry(b).or_default().push(n);
        }
    }
    let mut ready: Vec<u64> = pending
        .iter()
        .filter(|&(_, &c)| c == 0)
        .map(|(&n, _)| n)
        .collect();
    let mut order = Vec::new();
    while let Some(n) = ready.pop() {
        order.push(n);
        for &m in blocks.get(&n).into_iter().flatten() {
            if let Some(c) = pending.get_mut(&m) {
                *c -= 1;
                if *c == 0 {
                    ready.push(m);
                }
            }
        }
    }
    if order.len() != open.len() {
        return Err(Error::Parse(
            "the open issues form a blocked-by cycle".to_owned(),
        ));
    }
    let mut length: BTreeMap<u64, (usize, Option<u64>)> = BTreeMap::new();
    for &n in &order {
        let best = open_blockers(n)
            .filter_map(|b| length.get(&b).map(|&(l, _)| (l, b)))
            .max_by(|(la, a), (lb, b)| la.cmp(lb).then(b.cmp(a)));
        length.insert(n, best.map_or((1, None), |(l, b)| (l + 1, Some(b))));
    }
    let end = length
        .iter()
        .max_by(|(a, (la, _)), (b, (lb, _))| la.cmp(lb).then(b.cmp(a)))
        .map(|(&n, _)| n);
    let mut chain = Vec::new();
    let mut cur = end;
    while let Some(n) = cur {
        chain.push(n);
        cur = length.get(&n).and_then(|&(_, prev)| prev);
    }
    chain.reverse();
    Ok(chain)
}

/// `owner/name` from `GITHUB_REPOSITORY` when it is set and non-empty, or else parsed
/// by [`repo_from_remote`] from the URL `remote` returns (the `origin` remote).
fn repository(env: Option<String>, remote: impl FnOnce() -> Result<String>) -> Result<String> {
    match env {
        Some(repo) if !repo.is_empty() => Ok(repo),
        _ => repo_from_remote(&remote()?),
    }
}

/// Loads the repository's graph and the gate table from the checkout containing the
/// current directory.
fn load() -> Result<(Graph, BTreeMap<String, Vec<String>>)> {
    let top = git::toplevel(".")?;
    let design_path = std::path::Path::new(&top).join(GATES_DOCUMENT);
    let design = std::fs::read_to_string(&design_path)
        .map_err(|e| Error::Parse(format!("{}: {e}", design_path.display())))?;
    let gates = gate_table(&design)?;
    let repo = repository(std::env::var("GITHUB_REPOSITORY").ok(), || {
        Cmd::new("git")
            .args(["remote", "get-url", "origin"])
            .current_dir(&top)
            .output()
    })?;
    let graph = Graph::fetch(&Client::new(repo)?)?;
    Ok((graph, gates))
}

/// Entry point of `just dag-lint`: prints every violation and fails if there is any.
#[must_use]
pub fn lint_main() -> ExitCode {
    let result = load().and_then(|(graph, gates)| {
        let violations = lint(&graph, &gates)?;
        let open = graph.open().count();
        Ok((open, violations))
    });
    match result {
        Ok((open, violations)) if violations.is_empty() => {
            println!("issue graph OK: {open} open issues");
            ExitCode::SUCCESS
        }
        Ok((_, violations)) => {
            for v in &violations {
                println!("{v}");
            }
            eprintln!("{} violation(s)", violations.len());
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("dag-lint: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Entry point of `just dag`: prints the Mermaid render, or with `--critical` the longest
/// open chain, one issue per line.
#[must_use]
pub fn dag_main(args: impl IntoIterator<Item = String>) -> ExitCode {
    let critical = match args.into_iter().collect::<Vec<_>>().as_slice() {
        [] => false,
        [flag] if flag == "--critical" => true,
        other => {
            eprintln!("usage: dag [--critical]; got {other:?}");
            return ExitCode::FAILURE;
        }
    };
    let result = load().and_then(|(graph, _)| {
        if !critical {
            return Ok(mermaid(&graph));
        }
        Ok(critical_path(&graph)?
            .iter()
            .filter_map(|n| graph.issues.get(n))
            .map(|i| {
                let milestone = i
                    .milestone
                    .as_deref()
                    .and_then(|t| t.split_whitespace().next());
                format!("#{} [{}] {}\n", i.number, milestone.unwrap_or("-"), i.title)
            })
            .collect())
    });
    match result {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("dag: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    //! The fixtures are responses recorded from this repository's issues API, trimmed to
    //! the fields the rules read: the issues listing (bodies cut to the first line and the
    //! `## Acceptance` heading of an epic, or the **Allowed paths** of a task) and the
    //! `dependencies/blocked_by` listings of the issues that have edges. The gate table is
    //! the real M0 §4.

    use super::*;
    use serde_json::json;

    const DESIGN: &str = include_str!("../../../../docs/design/AUTOBOT-M0-AND-GATES.md");

    /// `GET issues?state=all`, recorded and trimmed.
    const ISSUES: &str = r#"[
        {"number":224,"title":"Finding: KERNEL header cites I-1 … I-9 while FORMAL cites I-1 … I-10","state":"open","labels":[{"name":"finding"},{"name":"design"},{"name":"needs-decision"}],"milestone":{"title":"M0-Q · G-QUAL — qualification slice"},"parent_issue_url":"https://api.github.com/repos/tsouza/autobot/issues/5","body":"**Allowed paths**\nNone (a finding; the owner's decision lands as a design-change PR under E-M0-DESIGN-CLOSURE).","sub_issues_summary":{"total":0},"issue_dependencies_summary":{"total_blocked_by":0}},
        {"number":223,"title":"Design-change PR: owner rulings (intake authority clause, 46-kind closure parenthetical, FORMAL §7 width sentence, DEFERRED gate-evidence entry)","state":"open","labels":[{"name":"type:task"},{"name":"design"},{"name":"human-lane"},{"name":"area:docs"}],"milestone":{"title":"M0-Q · G-QUAL — qualification slice"},"parent_issue_url":"https://api.github.com/repos/tsouza/autobot/issues/5","body":"**Allowed paths**\ndocs/design/AUTOBOT-KERNEL*.md (§1 or §4 clause), docs/design/AUTOBOT-M0-AND-GATES*.md (§1 Plan row, §5), docs/design/AUTOBOT-FORMAL-SURFACE*.md (§7)","sub_issues_summary":{"total":0},"issue_dependencies_summary":{"total_blocked_by":1}},
        {"number":222,"title":"Release process tracking issue","state":"open","labels":[{"name":"type:task"}],"milestone":{"title":"M1 · G-INTAKE — install & real intake"},"parent_issue_url":"https://api.github.com/repos/tsouza/autobot/issues/23","body":"**Allowed paths**\nNone (tracking issue).","sub_issues_summary":{"total":0},"issue_dependencies_summary":{"total_blocked_by":1}},
        {"number":204,"title":"Decompose M1 (G-INTAKE) into tasks","state":"open","labels":[{"name":"type:task"}],"milestone":{"title":"M1 · G-INTAKE — install & real intake"},"parent_issue_url":"https://api.github.com/repos/tsouza/autobot/issues/23","body":"**Allowed paths**\nNone (issues only).","sub_issues_summary":{"total":0},"issue_dependencies_summary":{"total_blocked_by":1}},
        {"number":203,"title":"G-QUAL evidence bundle and gate record","state":"open","labels":[{"name":"type:task"},{"name":"area:docs"}],"milestone":{"title":"M0-Q · G-QUAL — qualification slice"},"parent_issue_url":"https://api.github.com/repos/tsouza/autobot/issues/22","body":"**Allowed paths**\ndocs/gates/g-qual/**","sub_issues_summary":{"total":0},"issue_dependencies_summary":{"total_blocked_by":7}},
        {"number":63,"title":"Issue-graph lint and Mermaid render (`just dag-lint`, `just dag`)","state":"open","labels":[{"name":"type:task"},{"name":"human-lane"},{"name":"area:devtools"},{"name":"area:ci"},{"name":"area:docs"}],"milestone":{"title":"M-1 · Foundation — repository, tooling, CI, governance"},"parent_issue_url":"https://api.github.com/repos/tsouza/autobot/issues/4","body":"**Allowed paths**\nscripts/dag_lint.rs, scripts/dag.rs, scripts/devtools/src/github/graph.rs, .github/workflows/dag.yml, Justfile, CONTRIBUTING.md (section \"Work items\" only)","sub_issues_summary":{"total":0},"issue_dependencies_summary":{"total_blocked_by":2}},
        {"number":42,"title":"Production qualification sign-off","state":"open","labels":[{"name":"type:epic"}],"milestone":{"title":"M9 · G-PROD — production qualification"},"parent_issue_url":null,"body":"Gate epic for G-PROD.\n\n## Acceptance\nThe G-PROD record references PASSED G-QUAL, G-INTAKE, G-FENCE-CUSTODY, G-FORGE, G-OBS, G-OPS and G-F","sub_issues_summary":{"total":1},"issue_dependencies_summary":{"total_blocked_by":7}},
        {"number":41,"title":"Operational qualification","state":"open","labels":[{"name":"type:epic"}],"milestone":{"title":"M8 · G-OPS — operational qualification"},"parent_issue_url":null,"body":"Gate epic for G-OPS.\n\n## Acceptance\nThe G-OPS record lists measured scale limits, backup, upgrade, load, backpressure, recovery drills, ","sub_issues_summary":{"total":2},"issue_dependencies_summary":{"total_blocked_by":3}},
        {"number":40,"title":"Forge and runtime portability","state":"open","labels":[{"name":"type:epic"}],"milestone":{"title":"M7 · G-PORTABILITY — second forge & runtime"},"parent_issue_url":null,"body":"Gate epic for G-PORTABILITY.\n\n## Acceptance\nThe G-PORTABILITY record shows the mixed-forge context and the second runtime adapter passing the sa","sub_issues_summary":{"total":2},"issue_dependencies_summary":{"total_blocked_by":1}},
        {"number":39,"title":"Adaptive routing","state":"open","labels":[{"name":"type:epic"}],"milestone":{"title":"M6 · G-ADAPT — adaptive routing"},"parent_issue_url":null,"body":"Gate epic for G-ADAPT.\n\n## Acceptance\nThe G-ADAPT record lists qualified routing revisions, exploration escrow and expected-loss limit, ca","sub_issues_summary":{"total":2},"issue_dependencies_summary":{"total_blocked_by":2}},
        {"number":38,"title":"Evaluation ledger","state":"open","labels":[{"name":"type:epic"}],"milestone":{"title":"M5 · G-EVALUATION — evaluation ledger"},"parent_issue_url":null,"body":"Gate epic for G-EVALUATION.\n\n## Acceptance\nThe G-EVALUATION record lists the canonical ledger at scale, cohorts with one primary id, support bo","sub_issues_summary":{"total":2},"issue_dependencies_summary":{"total_blocked_by":2}},
        {"number":37,"title":"Telemetry pipeline and observability policy","state":"open","labels":[{"name":"type:epic"}],"milestone":{"title":"M4 · G-OBS — observability"},"parent_issue_url":null,"body":"Gate epic for G-OBS.\n\n## Acceptance\nThe G-OBS record lists pipeline coverage, fail-closed redaction, boot-epoch identity, backfill, rete","sub_issues_summary":{"total":2},"issue_dependencies_summary":{"total_blocked_by":1}},
        {"number":35,"title":"Forge mirroring and conformance (first forge)","state":"open","labels":[{"name":"type:epic"}],"milestone":{"title":"M3 · G-FORGE — forge mirroring & conformance"},"parent_issue_url":null,"body":"Gate epic for G-FORGE. The read-only self-intake epic in the same milestone is not G-FORGE evidence.\n\n## Acceptance\nThe G-FORGE record lists per-provider, per-operation conformance in disposable repositories, lost ac","sub_issues_summary":{"total":2},"issue_dependencies_summary":{"total_blocked_by":2}},
        {"number":34,"title":"G-FENCE-CUSTODY evidence","state":"open","labels":[{"name":"type:epic"}],"milestone":{"title":"M2 · G-FENCE-CUSTODY — real broker, isolation & custody"},"parent_issue_url":null,"body":"Gate epic for G-FENCE-CUSTODY.\n\n## Acceptance\nThe G-FENCE-CUSTODY record lists real broker, qualified isolated runtime, measured fencing, replicat","sub_issues_summary":{"total":1},"issue_dependencies_summary":{"total_blocked_by":1}},
        {"number":28,"title":"G-INTAKE evidence","state":"open","labels":[{"name":"type:epic"}],"milestone":{"title":"M1 · G-INTAKE — install & real intake"},"parent_issue_url":null,"body":"Gate epic for G-INTAKE.\n\n## Acceptance\nThe G-INTAKE record lists installation and recovery, intake and adoption on real forge metadata, dec","sub_issues_summary":{"total":1},"issue_dependencies_summary":{"total_blocked_by":1}},
        {"number":24,"title":"G-FORMAL: variants, counterexamples, liveness, cross-check, refinement, fault injection","state":"open","labels":[{"name":"type:epic"}],"milestone":{"title":"M0-QF · G-FORMAL — formal qualification alongside M0"},"parent_issue_url":null,"body":"Gate epic for G-FORMAL.\n\n## Acceptance\nThe G-FORMAL record lists non-vacuous variant checks, published counterexamples and assumptions, the","sub_issues_summary":{"total":8},"issue_dependencies_summary":{"total_blocked_by":0}},
        {"number":23,"title":"M1 planning: decomposition and release tracking","state":"open","labels":[{"name":"type:epic"}],"milestone":{"title":"M1 · G-INTAKE — install & real intake"},"parent_issue_url":null,"body":"Turns the M1 epics into tasks once G-QUAL is PASSED (M0 §4 M1 row; M0 §5 G-INTAKE lines). Keeping decomposition out of the gate epic keeps the gate epic's edges equal to the M0 §4 Depends-on column.\n\n## Acceptance\nEvery M1 epic has scope-capsule tasks, every evidence item and DEFERRED line of the M1 epic bodies i","sub_issues_summary":{"total":2},"issue_dependencies_summary":{"total_blocked_by":1}},
        {"number":22,"title":"M0-Q qualification: target, end-to-end scenario, measurements, gate evidence","state":"open","labels":[{"name":"type:epic"}],"milestone":{"title":"M0-Q · G-QUAL — qualification slice"},"parent_issue_url":null,"body":"Gate epic for G-QUAL.\n\n## Acceptance\nG-QUAL gate record PASSED with evidence bound to installation, software, policy and profile digests.","sub_issues_summary":{"total":9},"issue_dependencies_summary":{"total_blocked_by":0}},
        {"number":5,"title":"Design closure and freeze","state":"open","labels":[{"name":"type:epic"}],"milestone":{"title":"M0-Q · G-QUAL — qualification slice"},"parent_issue_url":null,"body":"M0-Q depends on a frozen design (M0 §4). The owner's four rulings land first as one design-change PR; one fresh session reviews the ruled set from a brief kept outside the repository and posts its report on the review issue; clarification tasks settle the findings through design-change PRs (permit write lanes, dependents scope, register maintenance, lifecycle exits, role rules); the owner applies the design-v1 tag. Implementation choices the design leaves open are recorded in the owning crate's rustdoc, not here. Blocking is targeted: only the consumers of a ruling or clarification wait for it.\n\n## Acceptance\nThe four rulings and every clarification are merged design-change PRs; every design finding under th","sub_issues_summary":{"total":11},"issue_dependencies_summary":{"total_blocked_by":0}},
        {"number":4,"title":"Design-set checks and the design-change process","state":"open","labels":[{"name":"type:epic"}],"milestone":{"title":"M-1 · Foundation — repository, tooling, CI, governance"},"parent_issue_url":null,"body":"Mechanical consistency of docs/design (lifecycle-state closure through the one KERNEL §10 parser, layer rule, numbers only in M0 §2, the split-file rule, retired terms, I-n -> F-n -> group -> owning-test -> Quint-invariant traceability) and the issue-graph lint. The design checks run inside the required `test` job.\n\n## Acceptance\n`just check-design`, `just retired-terms` and `just trace-lint` fail on seeded violations and pass o","sub_issues_summary":{"total":4},"issue_dependencies_summary":{"total_blocked_by":0}}
    ]"#;

    /// `GET issues/{n}/dependencies/blocked_by` for each issue with edges, recorded and trimmed.
    const BLOCKED_BY: &str = r#"{
        "23":[{"labels":[{"name":"type:epic"}],"milestone":{"title":"M0-Q · G-QUAL — qualification slice"},"number":22,"state":"open","title":"M0-Q qualification: target, end-to-end scenario, measurements, gate evidence"}],
        "28":[{"labels":[{"name":"type:epic"}],"milestone":{"title":"M0-Q · G-QUAL — qualification slice"},"number":22,"state":"open","title":"M0-Q qualification: target, end-to-end scenario, measurements, gate evidence"}],
        "34":[{"labels":[{"name":"type:epic"}],"milestone":{"title":"M0-Q · G-QUAL — qualification slice"},"number":22,"state":"open","title":"M0-Q qualification: target, end-to-end scenario, measurements, gate evidence"}],
        "35":[{"labels":[{"name":"type:epic"}],"milestone":{"title":"M1 · G-INTAKE — install & real intake"},"number":28,"state":"open","title":"G-INTAKE evidence"},{"labels":[{"name":"type:epic"}],"milestone":{"title":"M2 · G-FENCE-CUSTODY — real broker, isolation & custody"},"number":34,"state":"open","title":"G-FENCE-CUSTODY evidence"}],
        "37":[{"labels":[{"name":"type:epic"}],"milestone":{"title":"M0-Q · G-QUAL — qualification slice"},"number":22,"state":"open","title":"M0-Q qualification: target, end-to-end scenario, measurements, gate evidence"}],
        "38":[{"labels":[{"name":"type:epic"}],"milestone":{"title":"M1 · G-INTAKE — install & real intake"},"number":28,"state":"open","title":"G-INTAKE evidence"},{"labels":[{"name":"type:epic"}],"milestone":{"title":"M4 · G-OBS — observability"},"number":37,"state":"open","title":"Telemetry pipeline and observability policy"}],
        "39":[{"labels":[{"name":"type:epic"}],"milestone":{"title":"M3 · G-FORGE — forge mirroring & conformance"},"number":35,"state":"open","title":"Forge mirroring and conformance (first forge)"},{"labels":[{"name":"type:epic"}],"milestone":{"title":"M5 · G-EVALUATION — evaluation ledger"},"number":38,"state":"open","title":"Evaluation ledger"}],
        "40":[{"labels":[{"name":"type:epic"}],"milestone":{"title":"M3 · G-FORGE — forge mirroring & conformance"},"number":35,"state":"open","title":"Forge mirroring and conformance (first forge)"}],
        "41":[{"labels":[{"name":"type:epic"}],"milestone":{"title":"M2 · G-FENCE-CUSTODY — real broker, isolation & custody"},"number":34,"state":"open","title":"G-FENCE-CUSTODY evidence"},{"labels":[{"name":"type:epic"}],"milestone":{"title":"M3 · G-FORGE — forge mirroring & conformance"},"number":35,"state":"open","title":"Forge mirroring and conformance (first forge)"},{"labels":[{"name":"type:epic"}],"milestone":{"title":"M4 · G-OBS — observability"},"number":37,"state":"open","title":"Telemetry pipeline and observability policy"}],
        "42":[{"labels":[{"name":"type:epic"}],"milestone":{"title":"M0-Q · G-QUAL — qualification slice"},"number":22,"state":"open","title":"M0-Q qualification: target, end-to-end scenario, measurements, gate evidence"},{"labels":[{"name":"type:epic"}],"milestone":{"title":"M1 · G-INTAKE — install & real intake"},"number":28,"state":"open","title":"G-INTAKE evidence"},{"labels":[{"name":"type:epic"}],"milestone":{"title":"M2 · G-FENCE-CUSTODY — real broker, isolation & custody"},"number":34,"state":"open","title":"G-FENCE-CUSTODY evidence"},{"labels":[{"name":"type:epic"}],"milestone":{"title":"M3 · G-FORGE — forge mirroring & conformance"},"number":35,"state":"open","title":"Forge mirroring and conformance (first forge)"},{"labels":[{"name":"type:epic"}],"milestone":{"title":"M4 · G-OBS — observability"},"number":37,"state":"open","title":"Telemetry pipeline and observability policy"},{"labels":[{"name":"type:epic"}],"milestone":{"title":"M8 · G-OPS — operational qualification"},"number":41,"state":"open","title":"Operational qualification"},{"labels":[{"name":"type:epic"}],"milestone":{"title":"M0-QF · G-FORMAL — formal qualification alongside M0"},"number":24,"state":"open","title":"G-FORMAL: variants, counterexamples, liveness, cross-check, refinement, fault injection"}],
        "204":[{"labels":[{"name":"type:task"},{"name":"area:docs"}],"milestone":{"title":"M0-Q · G-QUAL — qualification slice"},"number":203,"state":"open","title":"G-QUAL evidence bundle and gate record"}],
        "222":[{"labels":[{"name":"type:task"},{"name":"area:docs"}],"milestone":{"title":"M0-Q · G-QUAL — qualification slice"},"number":203,"state":"open","title":"G-QUAL evidence bundle and gate record"}]
    }"#;

    struct Fixture {
        issues: Value,
        blocked_by: BTreeMap<u64, Value>,
    }

    impl Fixture {
        fn recorded() -> Self {
            let raw: BTreeMap<String, Value> = serde_json::from_str(BLOCKED_BY).unwrap();
            Self {
                issues: serde_json::from_str(ISSUES).unwrap(),
                blocked_by: raw
                    .into_iter()
                    .map(|(k, v)| (k.parse().unwrap(), v))
                    .collect(),
            }
        }

        fn issue(&mut self, number: u64) -> &mut Value {
            self.issues
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|i| i["number"] == number)
                .unwrap()
        }

        fn remove_label(&mut self, number: u64, label: &str) {
            let labels = self.issue(number)["labels"].as_array_mut().unwrap();
            let before = labels.len();
            labels.retain(|l| l["name"] != label);
            assert_eq!(labels.len(), before - 1, "#{number} had no `{label}`");
        }

        fn add_label(&mut self, number: u64, label: &str) {
            let labels = self.issue(number)["labels"].as_array_mut().unwrap();
            labels.push(json!({ "name": label }));
        }

        fn add_edge(&mut self, number: u64, blocker: u64) {
            let listing = self.blocked_by.entry(number).or_insert_with(|| json!([]));
            listing
                .as_array_mut()
                .unwrap()
                .push(json!({ "number": blocker }));
        }

        fn remove_edge(&mut self, number: u64, blocker: u64) {
            let listing = self
                .blocked_by
                .get_mut(&number)
                .unwrap()
                .as_array_mut()
                .unwrap();
            let before = listing.len();
            listing.retain(|b| b["number"] != blocker);
            assert_eq!(
                listing.len(),
                before - 1,
                "#{number} was not blocked by #{blocker}"
            );
        }

        fn graph(&self) -> Graph {
            Graph::from_api(&self.issues, &self.blocked_by).unwrap()
        }

        fn lint(&self) -> Vec<Violation> {
            lint(&self.graph(), &gate_table(DESIGN).unwrap()).unwrap()
        }
    }

    fn rules(violations: &[Violation]) -> Vec<(u64, Rule)> {
        violations.iter().map(|v| (v.issue, v.rule)).collect()
    }

    #[test]
    fn recorded_plan_is_clean() {
        let fx = Fixture::recorded();
        let graph = fx.graph();
        assert_eq!(graph.issues.len(), 20);
        assert_eq!(
            graph.blocked_by.values().map(BTreeSet::len).sum::<usize>(),
            23
        );
        let gates: BTreeSet<&str> = graph.issues.values().filter_map(Issue::gate).collect();
        assert_eq!(gates.len(), 11);
        assert_eq!(fx.lint(), vec![]);
    }

    #[test]
    fn cycle_is_rejected() {
        let mut fx = Fixture::recorded();
        fx.add_edge(204, 222);
        fx.add_edge(222, 204);
        assert_eq!(
            rules(&fx.lint()),
            vec![(204, Rule::Cycle), (222, Rule::Cycle)]
        );
    }

    #[test]
    fn edge_to_a_later_milestone_is_rejected() {
        let mut fx = Fixture::recorded();
        // #63 is in M-1, #222 in M1.
        fx.add_edge(63, 222);
        assert_eq!(rules(&fx.lint()), vec![(63, Rule::LaterMilestone)]);
        // The reverse direction (M1 blocked by M-1) is fine.
        let mut fx = Fixture::recorded();
        fx.add_edge(222, 63);
        assert_eq!(fx.lint(), vec![]);
    }

    #[test]
    fn orphans_are_rejected() {
        let mut fx = Fixture::recorded();
        fx.issue(222)["parent_issue_url"] = Value::Null;
        fx.issue(204)["parent_issue_url"] = json!("https://api.github.com/repos/o/r/issues/203");
        fx.issue(224)["milestone"] = Value::Null;
        let violations = fx.lint();
        assert_eq!(
            rules(&violations),
            vec![
                (204, Rule::Orphan),
                (222, Rule::Orphan),
                (224, Rule::Orphan),
                (224, Rule::ParentMilestone)
            ]
        );
        assert!(violations[0].detail.contains("#203 is not an epic"));
        assert_eq!(violations[1].detail, "no parent epic");
        assert_eq!(violations[2].detail, "no milestone");
    }

    #[test]
    fn epic_without_acceptance_is_rejected() {
        let mut fx = Fixture::recorded();
        fx.issue(23)["body"] = json!("Turns the M1 epics into tasks.\n\n### Acceptance\nDone.");
        fx.issue(5)["body"] = json!("Owns the rulings.\n\n## Acceptance\n\n");
        assert_eq!(
            rules(&fx.lint()),
            vec![(5, Rule::Acceptance), (23, Rule::Acceptance)]
        );
    }

    #[test]
    fn acceptance_is_read_from_the_exact_heading() {
        let mut fx = Fixture::recorded();
        // An empty section whose heading only starts with `Acceptance` comes first: the
        // epic is valid because its `## Acceptance` section is not empty.
        fx.issue(23)["body"] = json!(
            "Turns the M1 epics into tasks.\n\n## Acceptance evidence\n\n## Acceptance\nDone."
        );
        // A non-empty `### Acceptance notes` comes first: the epic is invalid because its
        // `## Acceptance` section is empty.
        fx.issue(5)["body"] =
            json!("Owns the rulings.\n\n### Acceptance notes\nfoo\n\n## Acceptance\n\n");
        assert_eq!(rules(&fx.lint()), vec![(5, Rule::Acceptance)]);
    }

    #[test]
    fn fenced_acceptance_heading_is_ignored() {
        let mut fx = Fixture::recorded();
        // The only `## Acceptance` is quoted inside a code block: the epic has none.
        fx.issue(23)["body"] =
            json!("Turns the M1 epics into tasks.\n```\n## Acceptance\nfoo\n```\n");
        // A fenced `## Acceptance` with content hides nothing: the real one is empty.
        fx.issue(5)["body"] =
            json!("Owns the rulings.\n~~~~\n## Acceptance\nfoo\n~~~~\n## Acceptance\n\n");
        assert_eq!(
            rules(&fx.lint()),
            vec![(5, Rule::Acceptance), (23, Rule::Acceptance)]
        );
    }

    #[test]
    fn redundant_task_edge_is_rejected() {
        let mut fx = Fixture::recorded();
        // #222 → #203 is already implied by #222 → #204 → #203.
        fx.add_edge(222, 204);
        let violations = fx.lint();
        assert_eq!(rules(&violations), vec![(222, Rule::Redundant)]);
        assert_eq!(violations[0].detail, "edge to #203 is implied through #204");
    }

    #[test]
    fn redundant_gate_epic_edge_is_accepted() {
        let fx = Fixture::recorded();
        let graph = fx.graph();
        // G-PROD (#42) → G-QUAL (#22) is implied by #42 → G-INTAKE (#28) → #22, as M0 §4 has it.
        assert!(graph.blocked_by[&42].contains(&22));
        assert!(graph.blocked_by[&42].contains(&28));
        assert!(graph.reach(28).contains(&22));
        assert_eq!(fx.lint(), vec![]);
    }

    #[test]
    fn gate_epic_edges_must_equal_the_design() {
        let mut fx = Fixture::recorded();
        // G-OBS (#37) depends on M0-Q only; add G-INTAKE (#28).
        fx.add_edge(37, 28);
        // G-FORGE (#35) depends on M1 and M2; drop M2's gate epic, G-FENCE-CUSTODY (#34).
        fx.remove_edge(35, 34);
        let violations = fx.lint();
        assert_eq!(
            rules(&violations),
            vec![(35, Rule::GateEdges), (37, Rule::GateEdges)]
        );
        assert_eq!(
            violations[0].detail,
            "M0 §4 edge to gate epic #34 is missing"
        );
        assert_eq!(
            violations[1].detail,
            "edge to gate epic #28 is not in M0 §4"
        );
    }

    #[test]
    fn epic_blocked_by_a_task_is_rejected() {
        let mut fx = Fixture::recorded();
        fx.add_edge(23, 203);
        assert_eq!(rules(&fx.lint()), vec![(23, Rule::EpicBlocker)]);
    }

    #[test]
    fn kind_labels() {
        let fx = Fixture::recorded();
        // A finding carries no `type:*` label and is accepted.
        let finding = &fx.graph().issues[&224];
        assert!(finding.labels.contains("finding"));
        assert!(!finding.labels.iter().any(|l| l.starts_with("type:")));
        assert_eq!(fx.lint(), vec![]);

        let mut fx = Fixture::recorded();
        fx.add_label(224, "type:task");
        fx.remove_label(204, "type:task");
        assert_eq!(
            rules(&fx.lint()),
            vec![(204, Rule::KindLabel), (224, Rule::KindLabel)]
        );
    }

    #[test]
    fn missing_human_lane_is_rejected() {
        let mut fx = Fixture::recorded();
        // #223 edits docs/design/**, #63 adds .github/workflows/dag.yml.
        fx.remove_label(223, "human-lane");
        fx.remove_label(63, "human-lane");
        assert_eq!(
            rules(&fx.lint()),
            vec![(63, Rule::HumanLane), (223, Rule::HumanLane)]
        );
    }

    #[test]
    fn ancestor_of_a_human_lane_area_needs_human_lane() {
        let mut fx = Fixture::recorded();
        fx.issue(204)["body"] = json!("**Allowed paths**\n.github/**, Justfile");
        fx.issue(222)["body"] = json!("**Allowed paths**\n- docs/");
        fx.issue(224)["body"] = json!("**Allowed paths**\n.github/work*/dag.yml");
        assert_eq!(
            rules(&fx.lint()),
            vec![
                (204, Rule::HumanLane),
                (222, Rule::HumanLane),
                (224, Rule::HumanLane)
            ]
        );
    }

    #[test]
    fn leading_glob_that_reaches_a_human_lane_area_needs_human_lane() {
        let mut fx = Fixture::recorded();
        fx.issue(204)["body"] = json!("**Allowed paths**\n**/*.yml");
        fx.issue(222)["body"] = json!("**Allowed paths**\n- `**/workflows/**`");
        fx.issue(224)["body"] = json!("**Allowed paths**\n**/dag.yml, Justfile");
        fx.issue(203)["body"] = json!("**Allowed paths**\n*/design/[A-Z]*.md");
        assert_eq!(
            rules(&fx.lint()),
            vec![
                (203, Rule::HumanLane),
                (204, Rule::HumanLane),
                (222, Rule::HumanLane),
                (224, Rule::HumanLane)
            ]
        );
    }

    #[test]
    fn paths_beside_the_human_lane_areas_do_not_need_it() {
        let mut fx = Fixture::recorded();
        fx.issue(204)["body"] =
            json!("**Allowed paths**\n* .github/ISSUE_TEMPLATE/**, docs/designs.md, **, scripts/*");
        fx.issue(222)["body"] =
            json!("**Allowed paths**\n.github/workflows-notes.md, .github/work*.yml");
        fx.issue(224)["body"] = json!("**Allowed paths**\n*.rs, **Note**: scripts/?/x");
        assert_eq!(rules(&fx.lint()), vec![]);
    }

    #[test]
    fn priority_labels_are_rejected_but_urgent_is_not() {
        let mut fx = Fixture::recorded();
        fx.add_label(204, "priority:high");
        fx.add_label(222, "P1");
        fx.add_label(203, "urgent");
        assert_eq!(
            rules(&fx.lint()),
            vec![(204, Rule::PriorityLabel), (222, Rule::PriorityLabel)]
        );
    }

    #[test]
    fn limits_are_enforced() {
        let mut fx = Fixture::recorded();
        fx.issue(22)["sub_issues_summary"]["total"] = json!(SUB_ISSUE_LIMIT + 1);
        fx.issue(203)["issue_dependencies_summary"]["total_blocked_by"] =
            json!(BLOCKED_BY_LIMIT + 1);
        fx.issue(204)["issue_dependencies_summary"]["total_blocked_by"] = json!(BLOCKED_BY_LIMIT);
        assert_eq!(
            rules(&fx.lint()),
            vec![(22, Rule::Limit), (203, Rule::Limit)]
        );
    }

    #[test]
    fn reads_the_gate_table() {
        let gates = gate_table(DESIGN).unwrap();
        assert_eq!(gates.len(), 11);
        assert_eq!(gates["G-FORGE"], vec!["M1", "M2"]);
        assert_eq!(gates["G-QUAL"], vec!["frozen design"]);
        assert_eq!(gates["G-PROD"].len(), 7);
        assert!(gate_table("# Doc\n\n## 4. Milestones and gates\n\nno table\n").is_err());
    }

    #[test]
    fn unknown_milestone_in_the_gate_table_is_an_error() {
        let fx = Fixture::recorded();
        let mut gates = gate_table(DESIGN).unwrap();
        gates.insert("G-OBS".to_owned(), vec!["M42".to_owned()]);
        assert!(lint(&fx.graph(), &gates).is_err());
    }

    #[test]
    fn milestone_order_follows_the_prefix() {
        let order = ["M-1", "M0-Q", "M0-QF", "M1", "M2", "M8", "M9", "M10"];
        let keys: Vec<MilestoneKey> = order.iter().map(|p| milestone_key(p).unwrap()).collect();
        assert!(keys.windows(2).all(|w| w[0] < w[1]), "{keys:?}");
        for bad in ["M", "Mx", "M1-", "M1-q", "G-QUAL", "frozen design"] {
            assert_eq!(milestone_key(bad), None, "{bad}");
        }
    }

    #[test]
    fn critical_path_is_the_longest_open_chain() {
        let fx = Fixture::recorded();
        assert_eq!(
            critical_path(&fx.graph()).unwrap(),
            vec![22, 28, 35, 41, 42]
        );

        // Closing #41 shortens every chain through it.
        let mut fx = Fixture::recorded();
        fx.issue(41)["state"] = json!("closed");
        assert_eq!(critical_path(&fx.graph()).unwrap(), vec![22, 28, 35, 39]);

        let mut fx = Fixture::recorded();
        fx.add_edge(204, 222);
        fx.add_edge(222, 204);
        assert!(critical_path(&fx.graph()).is_err());
    }

    #[test]
    fn mermaid_renders_open_issues_and_edges() {
        let mut fx = Fixture::recorded();
        fx.issue(41)["state"] = json!("closed");
        let out = mermaid(&fx.graph());
        assert!(out.starts_with("flowchart LR\n"));
        assert!(
            out.contains("    i42[[\"#35;42 Production qualification sign-off"),
            "{out}"
        );
        assert!(out.contains("  i28 --> i35\n"));
        assert!(out.contains("  i203 --> i204\n"));
        assert!(!out.contains("i41"), "closed issues are left out");
        let m_1 = out.find("M-1 ·").unwrap();
        let m0q = out.find("M0-Q ·").unwrap();
        let m9 = out.find("M9 ·").unwrap();
        assert!(m_1 < m0q && m0q < m9);
    }

    #[test]
    fn pull_requests_are_skipped() {
        let issues = json!([{ "number": 1, "state": "open", "pull_request": {} }, { "number": 2, "state": "open" }]);
        let graph = Graph::from_api(&issues, &BTreeMap::new()).unwrap();
        assert_eq!(graph.issues.keys().copied().collect::<Vec<_>>(), vec![2]);
        assert!(Graph::from_api(&json!({}), &BTreeMap::new()).is_err());
    }

    #[test]
    fn repository_prefers_the_environment_then_parses_the_remote() {
        let remote = || Ok("git@github.com-alias:o/r.git".to_string());
        assert_eq!(repository(Some("a/b".into()), remote).unwrap(), "a/b");
        assert_eq!(repository(Some(String::new()), remote).unwrap(), "o/r");
        assert_eq!(repository(None, remote).unwrap(), "o/r");
        assert!(repository(None, || Ok("r".to_string())).is_err());
        // The remote parser is the shared one, which rejects an owner containing `@`.
        assert!(repository(None, || Ok("ssh://git@host/r".to_string())).is_err());
        assert!(repository(None, || Err(Error::Parse("no remote".into()))).is_err());
    }
}
