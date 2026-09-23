//! The `delivery-ledger` report: how the pull requests merged in a date range were delivered.
//!
//! [`main`] takes a first and an optional last date, `YYYY-MM-DD` in UTC and inclusive (the
//! last defaults to the first), reads the repository through the GitHub API with [`collect`]
//! and prints [`render`]: a Markdown table with one row per pull request merged in the range,
//! in merge order, then a summary block. Nothing is written anywhere but stdout.
//!
//! What is read:
//!
//! - **Merged pull requests.** `pulls?state=closed&sort=updated&direction=desc`, page by
//!   page, until a page ends with a pull request last updated before the range: a pull request
//!   is updated when it merges, so none merged in the range comes later. A pull request counts
//!   when the date of its `merged_at` is in the range. `pulls/{n}` gives its additions and
//!   deletions.
//! - **Review verdicts.** The issue comments of the pull request whose first line
//!   [`parse_verdict`] reads as a PASS or a FAIL, in comment order. A line with the verdict
//!   prefix that is neither is not a round. Each verdict is one review round.
//! - **Blocking findings.** In a FAIL verdict, one per list item of its `Blocking` or `Defects`
//!   section, or of its leading numbered list when it has no such section
//!   ([`verdicts::blocking_findings`]). A FAIL verdict in which none can be read counts one
//!   finding with no class, since a FAIL has at least one.
//! - **Judge answers.** The `judge` report comment ([`judge_report`]): its table gives one
//!   answer per `judged` charter entry.
//! - **Red runs on `main`.** The runs started on [`main_red::BRANCH`] by one of
//!   [`main_red::EVENTS`] and created in the range, whose conclusion [`main_red::Run`] calls
//!   red. A run counts when its workflow is required: some job of the workflow is named like a
//!   required status check of the committed ruleset ([`required_contexts`]).
//!
//! What is reported, per pull request and in total:
//!
//! - review rounds, FAIL verdicts, and the pull requests failed two or more times;
//! - blocking findings by class slug, and each class that recurs on a pull request, that is,
//!   appears in two or more of its FAIL verdicts;
//! - whole minutes from `created_at` to `merged_at`, and additions plus deletions;
//! - per `judged` entry: the answers, the "violates" answers, those that blocked, the false
//!   alarms and the misses ([`judge::Call`]);
//! - the red runs of required workflows on `main`.
//!
//! Choices the design leaves open:
//!
//! - **Verdict authors.** A verdict counts when its comment's `author_association` is
//!   `OWNER`, `MEMBER` or `COLLABORATOR`. The `review-gate` check also asks GitHub for the
//!   author's permission; the ledger measures what was reviewed and does not decide a merge,
//!   so it spares one request per verdict.
//! - **Class slugs.** A blocking finding names its classes in parentheses right after its bold
//!   title, `**Title** (pr-body)` or `**Title** (contract/rule)`: slugs separated by `/` or `,`,
//!   optionally followed by `: explanation`, and nothing else; or it starts with `slug:`. A slug
//!   is lowercase ASCII letters, digits and hyphens, starting with a letter, so a parenthesis
//!   holding a path or prose names no class. A finding that names none counts under
//!   [`verdicts::UNTAGGED`], which never recurs.
//! - **Judge reference.** The report comment holds the latest answers only. An entry counts
//!   as found by review on a pull request when the text of one of its blocking findings names
//!   the entry's id. A "violates" answer, at any confidence, on an entry review did not find
//!   is a false alarm; a "complies" or "unsure" answer on an entry review found is a miss. An
//!   entry the service did not answer is neither.
//! - **Times.** Dates and times are UTC, as GitHub reports them.

pub mod judge;
pub mod verdicts;

use crate::github::main_red::{self, Run};
use crate::github::settings::{Api, RULESET_PATH};
use crate::github::verdict::{Verdict, parse_verdict};
use crate::github::{Client, Method, PAGE_SIZE, pages};
use crate::judge::comment::{AUTHOR, MARKER};
use crate::{Error, Result};
use judge::{Call, Row};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use verdicts::{Finding, UNTAGGED};

/// Comment authors whose verdicts count.
pub const REVIEWER_ASSOCIATIONS: [&str; 3] = ["OWNER", "MEMBER", "COLLABORATOR"];

/// An inclusive range of UTC dates, `YYYY-MM-DD`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Range {
    /// The first date.
    pub from: String,
    /// The last date.
    pub to: String,
}

impl Range {
    /// Parses the arguments `<from> [<to>]`.
    ///
    /// # Errors
    /// Fails on no argument, more than two, a date that is not `YYYY-MM-DD`, or a last date
    /// before the first.
    pub fn from_args(args: impl IntoIterator<Item = String>) -> Result<Self> {
        let usage = || Error::Parse("usage: delivery_ledger <from> [<to>] (YYYY-MM-DD)".into());
        let mut args = args.into_iter();
        let from = args.next().ok_or_else(usage)?;
        let to = args.next().unwrap_or_else(|| from.clone());
        if args.next().is_some() {
            return Err(usage());
        }
        for date in [&from, &to] {
            if !is_date(date) {
                return Err(Error::Parse(format!("not a YYYY-MM-DD date: `{date}`")));
            }
        }
        if to < from {
            return Err(Error::Parse(format!("{to} is before {from}")));
        }
        Ok(Self { from, to })
    }

    /// Whether the date of the timestamp `at` is in the range.
    #[must_use]
    pub fn contains(&self, at: &str) -> bool {
        at.get(..10)
            .is_some_and(|day| self.from.as_str() <= day && day <= self.to.as_str())
    }
}

/// Whether `text` is a `YYYY-MM-DD` date with a month in 1..=12 and a day in 1..=31.
fn is_date(text: &str) -> bool {
    let b = text.as_bytes();
    let digits = |r: std::ops::Range<usize>| b[r].iter().all(u8::is_ascii_digit);
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && digits(0..4)
        && digits(5..7)
        && digits(8..10)
        && (1..=12).contains(&text[5..7].parse::<u8>().unwrap_or(0))
        && (1..=31).contains(&text[8..10].parse::<u8>().unwrap_or(0))
}

/// Seconds since the Unix epoch of a `YYYY-MM-DDTHH:MM:SSZ` timestamp.
///
/// # Errors
/// Fails when `at` is not in that form.
pub fn epoch_seconds(at: &str) -> Result<i64> {
    let bad = || Error::Parse(format!("not a UTC timestamp: `{at}`"));
    let b = at.as_bytes();
    let clock = |i: usize| b[i].is_ascii_digit();
    if b.len() != 20
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
        || b[19] != b'Z'
        || ![11, 12, 14, 15, 17, 18].into_iter().all(clock)
        || !at.get(..10).is_some_and(is_date)
    {
        return Err(bad());
    }
    let num = |r: std::ops::Range<usize>| {
        at.get(r)
            .and_then(|t| t.parse::<i64>().ok())
            .ok_or_else(bad)
    };
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hh, mm, ss) = (num(11..13)?, num(14..16)?, num(17..19)?);
    // Days from the civil date (proleptic Gregorian), counting from 1970-01-01.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Ok(days * 86_400 + hh * 3600 + mm * 60 + ss)
}

/// One review verdict of a pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Round {
    /// Whether the verdict is a FAIL.
    pub fail: bool,
    /// The blocking findings of a FAIL; empty for a PASS.
    pub findings: Vec<Finding>,
}

/// What the ledger reads about one merged pull request.
#[derive(Debug, Clone, PartialEq)]
pub struct Pull {
    /// The pull request number.
    pub number: u64,
    /// When it was opened.
    pub created_at: String,
    /// When it merged.
    pub merged_at: String,
    /// Lines added plus lines deleted.
    pub changed: u64,
    /// Its review verdicts, in comment order.
    pub rounds: Vec<Round>,
    /// The rows of its `judge` report, when it has one.
    pub judge: Option<Vec<Row>>,
}

impl Pull {
    /// The number of FAIL verdicts.
    #[must_use]
    pub fn fails(&self) -> usize {
        self.rounds.iter().filter(|r| r.fail).count()
    }

    /// Whole minutes from opening to merging.
    ///
    /// # Errors
    /// Fails when a timestamp is malformed.
    pub fn minutes(&self) -> Result<i64> {
        Ok((epoch_seconds(&self.merged_at)? - epoch_seconds(&self.created_at)?) / 60)
    }

    /// Blocking findings per class slug, over every FAIL verdict.
    #[must_use]
    pub fn classes(&self) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        for finding in self.rounds.iter().flat_map(|r| &r.findings) {
            for class in finding.class_names() {
                *counts.entry(class.to_owned()).or_insert(0) += 1;
            }
        }
        counts
    }

    /// The class slugs, other than [`UNTAGGED`], found in two or more FAIL verdicts.
    #[must_use]
    pub fn recurring(&self) -> Vec<String> {
        let mut verdicts: BTreeMap<&str, usize> = BTreeMap::new();
        for round in &self.rounds {
            let classes: BTreeSet<&str> = round
                .findings
                .iter()
                .flat_map(Finding::class_names)
                .filter(|c| *c != UNTAGGED)
                .collect();
            for class in classes {
                *verdicts.entry(class).or_insert(0) += 1;
            }
        }
        verdicts
            .into_iter()
            .filter(|(_, n)| *n >= 2)
            .map(|(c, _)| c.to_owned())
            .collect()
    }

    /// Each judge row with how it compares with the review, as [`judge::call`] decides.
    #[must_use]
    pub fn judge_calls(&self) -> Option<Vec<(&Row, Call)>> {
        let findings: Vec<&Finding> = self.rounds.iter().flat_map(|r| &r.findings).collect();
        self.judge.as_ref().map(|rows| {
            rows.iter()
                .map(|row| {
                    let found = findings.iter().any(|f| judge::names(&f.text, &row.entry));
                    (row, judge::call(row, found))
                })
                .collect()
        })
    }
}

/// Everything the report shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Ledger {
    /// The date range.
    pub range: Range,
    /// The merged pull requests, in merge order.
    pub pulls: Vec<Pull>,
    /// The red runs of required workflows on `main`, oldest first.
    pub red_runs: Vec<Run>,
}

/// The contexts of the required status checks in a ruleset, in the shape of
/// [`RULESET_PATH`].
#[must_use]
pub fn required_contexts(ruleset: &Value) -> BTreeSet<String> {
    ruleset["rules"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|rule| rule["type"] == "required_status_checks")
        .flat_map(|rule| {
            rule["parameters"]["required_status_checks"]
                .as_array()
                .into_iter()
                .flatten()
        })
        .filter_map(|check| check["context"].as_str().map(str::to_owned))
        .collect()
}

/// The first `judge` report comment among `comments`: its first line is [`MARKER`] and its
/// author is [`AUTHOR`].
#[must_use]
pub fn judge_report(comments: &[Value]) -> Option<&str> {
    comments.iter().find_map(|c| {
        let body = c["body"].as_str()?;
        (c["user"]["login"].as_str() == Some(AUTHOR) && body.lines().next() == Some(MARKER))
            .then_some(body)
    })
}

/// The review rounds among `comments`, as the module docs describe them.
#[must_use]
pub fn rounds(comments: &[Value]) -> Vec<Round> {
    comments
        .iter()
        .filter(|c| {
            REVIEWER_ASSOCIATIONS.contains(&c["author_association"].as_str().unwrap_or_default())
        })
        .filter_map(|c| {
            let body = c["body"].as_str()?;
            match parse_verdict(body)? {
                Verdict::Pass(_) => Some(Round {
                    fail: false,
                    findings: Vec::new(),
                }),
                Verdict::Fail(_) => {
                    let mut findings = verdicts::blocking_findings(body);
                    if findings.is_empty() {
                        findings.push(Finding {
                            classes: Vec::new(),
                            text: String::new(),
                        });
                    }
                    Some(Round {
                        fail: true,
                        findings,
                    })
                }
                Verdict::Malformed => None,
            }
        })
        .collect()
}

/// A string field of a JSON object.
fn text(value: &Value, key: &str, what: &str) -> Result<String> {
    value[key]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| Error::Parse(format!("{what}: no `{key}`")))
}

/// The numbers of the pull requests merged in `range`, in merge order.
///
/// # Errors
/// Fails if a GitHub call fails or a page is not a JSON array of pull requests.
pub fn merged_pulls(api: &impl Api, range: &Range) -> Result<Vec<u64>> {
    let mut merged: Vec<(String, u64)> = Vec::new();
    for page in 1.. {
        let path = format!(
            "pulls?state=closed&sort=updated&direction=desc&per_page={PAGE_SIZE}&page={page}"
        );
        let Value::Array(batch) = api.request(Method::Get, &path, None)? else {
            return Err(Error::Parse(format!("{path}: not a JSON array")));
        };
        let mut older = false;
        for pull in &batch {
            let updated = text(pull, "updated_at", &path)?;
            older |= updated.get(..10).is_some_and(|d| d < range.from.as_str());
            if let Some(at) = pull["merged_at"].as_str()
                && range.contains(at)
            {
                let Some(number) = pull["number"].as_u64() else {
                    return Err(Error::Parse(format!(
                        "{path}: a pull request has no `number`"
                    )));
                };
                merged.push((at.to_owned(), number));
            }
        }
        if older || batch.len() < PAGE_SIZE {
            break;
        }
    }
    merged.sort();
    Ok(merged.into_iter().map(|(_, n)| n).collect())
}

/// Reads pull request `number`, its verdicts and its judge report.
///
/// # Errors
/// Fails if a GitHub call fails or the pull request lacks its times or line counts.
pub fn read_pull(api: &impl Api, number: u64) -> Result<Pull> {
    let path = format!("pulls/{number}");
    let pull = api.request(Method::Get, &path, None)?;
    let count = |key: &str| {
        pull[key]
            .as_u64()
            .ok_or_else(|| Error::Parse(format!("{path}: no `{key}`")))
    };
    let comments = pages(api, &format!("issues/{number}/comments"))?;
    Ok(Pull {
        number,
        created_at: text(&pull, "created_at", &path)?,
        merged_at: text(&pull, "merged_at", &path)?,
        changed: count("additions")? + count("deletions")?,
        rounds: rounds(&comments),
        judge: judge_report(&comments).map(judge::rows),
    })
}

/// The runs on `main` created in `range` and started by `event`, as their REST objects.
fn runs(api: &impl Api, range: &Range, event: &str) -> Result<Vec<Value>> {
    let mut all = Vec::new();
    for page in 1.. {
        let path = format!(
            "actions/runs?branch={}&event={event}&created={}..{}&per_page={PAGE_SIZE}&page={page}",
            main_red::BRANCH,
            range.from,
            range.to
        );
        let Some(batch) = api.request(Method::Get, &path, None)?["workflow_runs"]
            .as_array()
            .cloned()
        else {
            return Err(Error::Parse(format!("{path}: no `workflow_runs` array")));
        };
        let last = batch.len() < PAGE_SIZE;
        all.extend(batch);
        if last {
            break;
        }
    }
    Ok(all)
}

/// The job names of run `id`.
fn job_names(api: &impl Api, id: u64) -> Result<Vec<String>> {
    let path = format!("actions/runs/{id}/jobs?per_page={PAGE_SIZE}");
    let jobs = api.request(Method::Get, &path, None)?;
    let Some(jobs) = jobs["jobs"].as_array() else {
        return Err(Error::Parse(format!("{path}: no `jobs` array")));
    };
    Ok(jobs
        .iter()
        .filter_map(|j| j["name"].as_str().map(str::to_owned))
        .collect())
}

/// The red runs of required workflows on `main` created in `range`, oldest first. Whether a
/// workflow is required is decided from the jobs of its red run, or, when that run has no
/// jobs (it could not start), of its other runs in the range, newest first, until one has
/// jobs.
///
/// # Errors
/// Fails if a GitHub call fails or a run lacks the fields [`Run::from_json`] needs.
pub fn red_runs(api: &impl Api, range: &Range, required: &BTreeSet<String>) -> Result<Vec<Run>> {
    let mut all = Vec::new();
    for event in main_red::EVENTS {
        all.extend(runs(api, range, event)?);
    }
    all.sort_by(|a, b| b["created_at"].as_str().cmp(&a["created_at"].as_str()));
    let mut decided: BTreeMap<u64, bool> = BTreeMap::new();
    let mut red = Vec::new();
    for value in &all {
        let run = Run::from_json(value)?;
        if run.skip_reason().is_some() {
            continue;
        }
        let Some(workflow) = value["workflow_id"].as_u64() else {
            return Err(Error::Parse(format!("{}: no `workflow_id`", run.url)));
        };
        let is_required = match decided.get(&workflow) {
            Some(known) => *known,
            None => {
                let known = workflow_is_required(api, value, &all, required)?;
                decided.insert(workflow, known);
                known
            }
        };
        if is_required {
            red.push(run);
        }
    }
    red.reverse();
    Ok(red)
}

/// Whether the workflow of `run` is required, from the jobs of `run` or, when it has none, of
/// the other runs of its workflow in `all`, in order, until one has jobs. A workflow none of
/// whose runs has jobs is not required.
fn workflow_is_required(
    api: &impl Api,
    run: &Value,
    all: &[Value],
    required: &BTreeSet<String>,
) -> Result<bool> {
    let workflow = &run["workflow_id"];
    let others = all
        .iter()
        .filter(|v| v["workflow_id"] == *workflow && *v != run);
    for candidate in std::iter::once(run).chain(others) {
        let Some(id) = candidate["id"].as_u64() else {
            continue;
        };
        let names = job_names(api, id)?;
        if !names.is_empty() {
            return Ok(names.iter().any(|n| required.contains(n)));
        }
    }
    Ok(false)
}

/// Reads everything the report for `range` shows; `required` holds the required status
/// check contexts.
///
/// # Errors
/// Fails if a GitHub call fails or a response lacks a field the report needs.
pub fn collect(api: &impl Api, range: &Range, required: &BTreeSet<String>) -> Result<Ledger> {
    let pulls = merged_pulls(api, range)?
        .into_iter()
        .map(|n| read_pull(api, n))
        .collect::<Result<_>>()?;
    Ok(Ledger {
        range: range.clone(),
        pulls,
        red_runs: red_runs(api, range, required)?,
    })
}

/// `name count` pairs, comma-separated, or `—` when there are none.
fn counts(map: &BTreeMap<String, usize>) -> String {
    if map.is_empty() {
        return "—".to_owned();
    }
    map.iter()
        .map(|(k, n)| format!("{k} {n}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The items joined with `, `, or `—` when there are none.
fn list(items: &[String]) -> String {
    if items.is_empty() {
        "—".to_owned()
    } else {
        items.join(", ")
    }
}

/// The judge cell of a pull request's row.
fn judge_cell(pull: &Pull) -> String {
    let Some(calls) = pull.judge_calls() else {
        return "no report".to_owned();
    };
    let marked: Vec<String> = calls
        .iter()
        .filter_map(|(row, call)| match call {
            Call::FalseAlarm => Some(format!("false alarm {}", row.entry)),
            Call::Miss => Some(format!("miss {}", row.entry)),
            _ => None,
        })
        .collect();
    list(&marked)
}

/// Per-entry judge totals: answered, violates, blocks, false alarms, misses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EntryTotals {
    /// Answers "complies", "violates" or "unsure".
    pub answered: usize,
    /// "violates" answers.
    pub violates: usize,
    /// "violates" answers at or above the threshold.
    pub blocks: usize,
    /// "violates" answers on an entry review did not find.
    pub false_alarms: usize,
    /// "complies" or "unsure" answers on an entry review found.
    pub misses: usize,
}

impl Ledger {
    /// The judge totals per entry id, over every pull request with a report.
    #[must_use]
    pub fn judge_totals(&self) -> BTreeMap<String, EntryTotals> {
        let mut totals: BTreeMap<String, EntryTotals> = BTreeMap::new();
        for calls in self.pulls.iter().filter_map(Pull::judge_calls) {
            for (row, call) in calls {
                let t = totals.entry(row.entry.clone()).or_default();
                t.answered += usize::from(call != Call::Unanswered);
                t.violates += usize::from(matches!(call, Call::FalseAlarm | Call::Hit));
                t.blocks += usize::from(row.blocks);
                t.false_alarms += usize::from(call == Call::FalseAlarm);
                t.misses += usize::from(call == Call::Miss);
            }
        }
        totals
    }
}

/// The report for `ledger`, as the module docs describe it.
///
/// # Errors
/// Fails when a pull request's timestamps are malformed.
pub fn render(ledger: &Ledger) -> Result<String> {
    let mut out = String::new();
    let Range { from, to } = &ledger.range;
    let _ = writeln!(out, "## Delivery ledger: merged {from} to {to} (UTC)\n");
    out.push_str(
        "| PR | Rounds | FAILs | Blocking classes | Recurring | Minutes to merge | Lines changed | Judge |\n",
    );
    out.push_str("|---|---|---|---|---|---|---|---|\n");
    let mut minutes = Vec::new();
    for pull in &ledger.pulls {
        let m = pull.minutes()?;
        minutes.push((m, pull.number));
        let _ = writeln!(
            out,
            "| #{} | {} | {} | {} | {} | {m} | {} | {} |",
            pull.number,
            pull.rounds.len(),
            pull.fails(),
            counts(&pull.classes()),
            list(&pull.recurring()),
            pull.changed,
            judge_cell(pull)
        );
    }
    let pulls = &ledger.pulls;
    let rounds: usize = pulls.iter().map(|p| p.rounds.len()).sum();
    let fails: usize = pulls.iter().map(Pull::fails).sum();
    let unreviewed = pulls.iter().filter(|p| p.rounds.is_empty()).count();
    let twice: Vec<String> = pulls
        .iter()
        .filter(|p| p.fails() >= 2)
        .map(|p| format!("#{}", p.number))
        .collect();
    let mut classes: BTreeMap<String, usize> = BTreeMap::new();
    for (class, n) in pulls.iter().flat_map(Pull::classes) {
        *classes.entry(class).or_insert(0) += n;
    }
    let findings: usize = classes.values().sum();
    let recurring: Vec<String> = pulls
        .iter()
        .flat_map(|p| {
            p.recurring()
                .into_iter()
                .map(|c| format!("#{} {c}", p.number))
        })
        .collect();
    let total_minutes: i64 = minutes.iter().map(|(m, _)| m).sum();
    let changed: u64 = pulls.iter().map(|p| p.changed).sum();
    let reports = pulls.iter().filter(|p| p.judge.is_some()).count();

    out.push_str("\n### Summary\n\n");
    let _ = writeln!(out, "- Pull requests merged: {}", pulls.len());
    let _ = writeln!(
        out,
        "- Review rounds: {rounds} ({} PASS, {fails} FAIL); pull requests without a verdict: {unreviewed}",
        rounds - fails
    );
    let _ = writeln!(
        out,
        "- Pull requests failed two or more times: {} ({})",
        twice.len(),
        list(&twice)
    );
    let _ = writeln!(
        out,
        "- Blocking findings: {findings} ({})",
        counts(&classes)
    );
    let _ = writeln!(
        out,
        "- Classes recurring on a pull request: {} ({})",
        recurring.len(),
        list(&recurring)
    );
    let mean = i64::try_from(pulls.len())
        .ok()
        .filter(|n| *n > 0)
        .map_or(0, |n| total_minutes / n);
    let longest = minutes
        .iter()
        .max()
        .map_or_else(|| "—".to_owned(), |(m, n)| format!("{m} (#{n})"));
    let _ = writeln!(
        out,
        "- Minutes from open to merge: total {total_minutes}, mean {mean}, longest {longest}"
    );
    let _ = writeln!(out, "- Lines changed (additions plus deletions): {changed}");
    let _ = writeln!(
        out,
        "- Failed runs of required workflows on `main`: {}",
        ledger.red_runs.len()
    );
    for run in &ledger.red_runs {
        let short: String = run.sha.chars().take(7).collect();
        let _ = writeln!(
            out,
            "  - {} {} at {short}: {}",
            run.workflow, run.conclusion, run.url
        );
    }
    let _ = writeln!(out, "- Judge reports: {reports}\n");
    let totals = ledger.judge_totals();
    if !totals.is_empty() {
        out.push_str("| Entry | Answered | Violates | Blocks | False alarms | Misses |\n");
        out.push_str("|---|---|---|---|---|---|\n");
        for (entry, t) in &totals {
            let _ = writeln!(
                out,
                "| {entry} | {} | {} | {} | {} | {} |",
                t.answered, t.violates, t.blocks, t.false_alarms, t.misses
            );
        }
    }
    Ok(out)
}

/// Entry point of `scripts/delivery_ledger.rs`: prints the report for the date range the
/// arguments give. The required checks are read from [`RULESET_PATH`] in the current
/// directory, and the repository is resolved by [`crate::github::repository`].
///
/// # Errors
/// Fails on bad arguments, an unreadable ruleset, an unresolvable repository or token, or a
/// failed GitHub call.
pub fn main(args: impl IntoIterator<Item = String>) -> Result<()> {
    let range = Range::from_args(args)?;
    let ruleset = std::fs::read_to_string(RULESET_PATH)
        .map_err(|e| Error::Parse(format!("{RULESET_PATH}: {e}")))?;
    let ruleset: Value =
        serde_json::from_str(&ruleset).map_err(|e| Error::Parse(format!("{RULESET_PATH}: {e}")))?;
    let client = Client::new(crate::github::repository(".")?)?;
    let ledger = collect(&client, &range, &required_contexts(&ruleset))?;
    print!("{}", render(&ledger)?);
    Ok(())
}

#[cfg(test)]
mod tests;
