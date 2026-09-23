//! Tests over GitHub API responses recorded for 2026-09-23 under `fixtures/`, with logins and
//! the repository replaced by placeholders and fields the ledger does not read dropped. The
//! comment bodies keep the verdict line and the `Blocking` section.

use super::judge::{Answer, Call, Row, names, rows};
use super::verdicts::{Finding, UNTAGGED, blocking_findings, classes, is_slug};
use super::*;
use serde_json::json;
use std::cell::RefCell;

const PULLS_CLOSED: &str = include_str!("fixtures/pulls_closed.json");
const PULL_349: &str = include_str!("fixtures/pull_349.json");
const PULL_378: &str = include_str!("fixtures/pull_378.json");
const PULL_405: &str = include_str!("fixtures/pull_405.json");
const COMMENTS_349: &str = include_str!("fixtures/comments_349.json");
const COMMENTS_378: &str = include_str!("fixtures/comments_378.json");
const COMMENTS_405: &str = include_str!("fixtures/comments_405.json");
const RUNS_PUSH: &str = include_str!("fixtures/runs_push.json");
const RUNS_SCHEDULE: &str = include_str!("fixtures/runs_schedule.json");
const JOBS_CI: &str = include_str!("fixtures/jobs_ci.json");
const JOBS_FORMAL: &str = include_str!("fixtures/jobs_formal.json");
const RULESET: &str = include_str!("../../../../.github/rulesets/main.json");

const DAY: &str = "2026-09-23";
const CLOSED_PAGE_1: &str = "pulls?state=closed&sort=updated&direction=desc&per_page=100&page=1";

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn runs_path(event: &str) -> String {
    format!("actions/runs?branch=main&event={event}&created={DAY}..{DAY}&per_page=100&page=1")
}

fn jobs_path(id: u64) -> String {
    format!("actions/runs/{id}/jobs?per_page=100")
}

fn day() -> Range {
    Range::from_args([DAY.to_owned()]).unwrap()
}

fn required() -> BTreeSet<String> {
    required_contexts(&parse(RULESET))
}

/// Serves recorded GET responses by path, fails an unknown path the way GitHub answers an
/// unknown resource, refuses writes, and records every path asked for.
struct Recorded {
    responses: BTreeMap<String, Value>,
    gets: RefCell<Vec<String>>,
}

impl Recorded {
    fn day() -> Self {
        let mut responses = BTreeMap::new();
        responses.insert(CLOSED_PAGE_1.to_owned(), parse(PULLS_CLOSED));
        for (n, pull, comments) in [
            (349, PULL_349, COMMENTS_349),
            (378, PULL_378, COMMENTS_378),
            (405, PULL_405, COMMENTS_405),
        ] {
            responses.insert(format!("pulls/{n}"), parse(pull));
            responses.insert(
                format!("issues/{n}/comments?per_page=100&page=1"),
                parse(comments),
            );
        }
        responses.insert(runs_path("push"), parse(RUNS_PUSH));
        responses.insert(runs_path("schedule"), parse(RUNS_SCHEDULE));
        responses.insert(jobs_path(35_855_661_934), parse(JOBS_CI));
        responses.insert(jobs_path(35_851_425_794), parse(JOBS_FORMAL));
        Self {
            responses,
            gets: RefCell::new(Vec::new()),
        }
    }

    fn edit(&mut self, path: &str, edit: impl FnOnce(&mut Value)) {
        edit(self.responses.get_mut(path).unwrap());
    }
}

impl Api for Recorded {
    fn request(&self, method: Method, path: &str, _body: Option<&Value>) -> Result<Value> {
        assert_eq!(method, Method::Get, "the ledger only reads");
        self.gets.borrow_mut().push(path.to_owned());
        self.responses
            .get(path)
            .cloned()
            .ok_or_else(|| Error::Http(format!("404 {path}")))
    }
}

fn pull(ledger: &Ledger, number: u64) -> &Pull {
    ledger.pulls.iter().find(|p| p.number == number).unwrap()
}

fn classes_of(pull: &Pull) -> Vec<(String, usize)> {
    pull.classes().into_iter().collect()
}

fn owner_comment(body: &str) -> Value {
    json!({"user": {"login": "octo-owner"}, "author_association": "OWNER", "body": body})
}

fn judge_comment(rows: &[(&str, &str, &str)]) -> Value {
    let mut body = format!(
        "{MARKER}\n### `judge`: answers for the `judged` charter entries\n\n| Entry | Answer | Confidence | Threshold | Result |\n|---|---|---|---|---|\n"
    );
    for (entry, answer, result) in rows {
        let _ = writeln!(body, "| {entry} | {answer} | 0.5 | 0.8 | {result} |");
    }
    json!({"user": {"login": AUTHOR}, "author_association": "NONE", "body": body})
}

#[test]
fn the_recorded_day_gives_each_pull_request_its_metrics() {
    let api = Recorded::day();
    let ledger = collect(&api, &day(), &required()).unwrap();
    let order: Vec<u64> = ledger.pulls.iter().map(|p| p.number).collect();
    assert_eq!(order, [349, 378, 405], "merge order");

    // Two PASS and two FAIL verdicts; `rule` is in both FAILs.
    let p = pull(&ledger, 378);
    assert_eq!((p.rounds.len(), p.fails()), (4, 2));
    assert_eq!(
        classes_of(p),
        [
            ("correctness".to_owned(), 1),
            ("pr-body".to_owned(), 1),
            ("rule".to_owned(), 2)
        ]
    );
    assert_eq!(p.recurring(), ["rule"]);
    assert_eq!(p.minutes().unwrap(), 79, "11:26:59 to 12:46:16");
    assert_eq!(p.changed, 708 + 238);

    // The service did not answer: every row is unanswered.
    let calls = p.judge_calls().unwrap();
    assert_eq!(calls.len(), 4);
    assert!(calls.iter().all(|(_, c)| *c == Call::Unanswered));

    // One FAIL whose finding names no class but names CHARTER L-2, then a PASS; no report.
    let p = pull(&ledger, 349);
    assert_eq!((p.rounds.len(), p.fails()), (2, 1));
    assert_eq!(classes_of(p), [(UNTAGGED.to_owned(), 1)]);
    assert!(p.recurring().is_empty());
    assert_eq!(p.minutes().unwrap(), 29);
    assert_eq!(p.judge, None);

    // A single PASS; the judge answered "violates" on L-2 and C-1, below their thresholds.
    let p = pull(&ledger, 405);
    assert_eq!((p.rounds.len(), p.fails()), (1, 0));
    assert!(p.classes().is_empty());
    let calls: Vec<(String, Call)> = p
        .judge_calls()
        .unwrap()
        .into_iter()
        .map(|(row, call)| (row.entry.clone(), call))
        .collect();
    assert_eq!(
        calls,
        [
            ("L-1".to_owned(), Call::Agree),
            ("L-2".to_owned(), Call::FalseAlarm),
            ("L-3".to_owned(), Call::Agree),
            ("C-1".to_owned(), Call::FalseAlarm),
        ]
    );

    // ci failed twice and formal once; the cancelled formal run and the green runs do not count.
    let red: Vec<(&str, &str)> = ledger
        .red_runs
        .iter()
        .map(|r| (r.workflow.as_str(), &r.sha[..7]))
        .collect();
    assert_eq!(
        red,
        [("ci", "90b5c30"), ("formal", "6e64dee"), ("ci", "9b26e50")]
    );
    // One jobs lookup per workflow decides it.
    let gets = api.gets.borrow();
    let jobs: Vec<&String> = gets
        .iter()
        .filter(|g| g.ends_with("/jobs?per_page=100"))
        .collect();
    assert_eq!(jobs.len(), 2, "{jobs:?}");
}

#[test]
fn the_report_shows_the_rows_and_the_totals() {
    let ledger = collect(&Recorded::day(), &day(), &required()).unwrap();
    let out = render(&ledger).unwrap();
    for line in [
        "## Delivery ledger: merged 2026-09-23 to 2026-09-23 (UTC)",
        "| #349 | 2 | 1 | untagged 1 | — | 29 | 50 | no report |",
        "| #378 | 4 | 2 | correctness 1, pr-body 1, rule 2 | rule | 79 | 946 | — |",
        "| #405 | 1 | 0 | — | — | 15 | 286 | false alarm L-2, false alarm C-1 |",
        "- Pull requests merged: 3",
        "- Review rounds: 7 (4 PASS, 3 FAIL); pull requests without a verdict: 0",
        "- Pull requests failed two or more times: 1 (#378)",
        "- Blocking findings: 5 (correctness 1, pr-body 1, rule 2, untagged 1)",
        "- Classes recurring on a pull request: 1 (#378 rule)",
        "- Minutes from open to merge: total 123, mean 41, longest 79 (#378)",
        "- Lines changed (additions plus deletions): 1282",
        "- Failed runs of required workflows on `main`: 3",
        "  - formal failure at 6e64dee: https://github.com/octo-org/autobot/actions/runs/35851425794",
        "- Judge reports: 2",
        "| C-1 | 1 | 1 | 0 | 1 | 0 |",
        "| L-1 | 1 | 0 | 0 | 0 | 0 |",
        "| L-2 | 1 | 1 | 0 | 1 | 0 |",
    ] {
        assert!(
            out.lines().any(|l| l == line),
            "missing `{line}` in:\n{out}"
        );
    }
}

#[test]
fn a_judge_complies_on_an_entry_review_found_is_a_miss() {
    let mut api = Recorded::day();
    api.edit("issues/349/comments?per_page=100&page=1", |c| {
        c.as_array_mut().unwrap().push(judge_comment(&[
            ("L-1", "violates", "blocks"),
            ("L-2", "complies", "falls back to `review`"),
            ("L-3", "not asked", "falls back to `review`"),
        ]));
    });
    let ledger = collect(&api, &day(), &required()).unwrap();
    let calls: Vec<Call> = pull(&ledger, 349)
        .judge_calls()
        .unwrap()
        .into_iter()
        .map(|(_, c)| c)
        .collect();
    assert_eq!(calls, [Call::FalseAlarm, Call::Miss, Call::Unanswered]);
    let totals = ledger.judge_totals();
    assert_eq!(
        totals["L-1"],
        EntryTotals {
            answered: 2,
            violates: 1,
            blocks: 1,
            false_alarms: 1,
            misses: 0
        }
    );
    assert_eq!(
        totals["L-2"].misses, 1,
        "#349 is a miss; #405 is a false alarm"
    );
    assert_eq!(totals["L-2"].false_alarms, 1);
    assert_eq!(
        totals["L-3"].answered, 1,
        "#405 answered; #349 and #378 did not"
    );
    let out = render(&ledger).unwrap();
    assert!(
        out.contains("| #349 | 2 | 1 | untagged 1 | — | 29 | 50 | false alarm L-1, miss L-2 |"),
        "{out}"
    );
}

#[test]
fn a_violates_the_review_confirms_is_a_hit_not_a_false_alarm() {
    let mut api = Recorded::day();
    api.edit("issues/349/comments?per_page=100&page=1", |c| {
        c.as_array_mut().unwrap().push(judge_comment(&[(
            "L-2",
            "violates",
            "falls back to `review`",
        )]));
    });
    let ledger = collect(&api, &day(), &required()).unwrap();
    let (row, call) = pull(&ledger, 349).judge_calls().unwrap()[0];
    assert_eq!((row.entry.as_str(), call), ("L-2", Call::Hit));
    assert_eq!(ledger.judge_totals()["L-2"].false_alarms, 1, "only #405's");
    assert_eq!(ledger.judge_totals()["L-2"].violates, 2);
}

#[test]
fn only_reviewers_verdicts_count_and_a_malformed_line_is_no_round() {
    let mut api = Recorded::day();
    api.edit("issues/405/comments?per_page=100&page=1", |c| {
        let c = c.as_array_mut().unwrap();
        let fail = format!(
            "Review verdict: FAIL @ {}\n\n## Blocking\n\n1. **x** (rule)",
            "a".repeat(40)
        );
        let mut outsider = owner_comment(&fail);
        outsider["author_association"] = json!("CONTRIBUTOR");
        c.push(outsider);
        c.push(owner_comment("Review verdict: FAIL @ d259cc4"));
        c.push(owner_comment(&format!("LGTM\n{fail}")));
    });
    let ledger = collect(&api, &day(), &required()).unwrap();
    let p = pull(&ledger, 405);
    assert_eq!((p.rounds.len(), p.fails()), (1, 0));
}

#[test]
fn a_judge_comment_by_someone_else_is_not_the_report() {
    let mut forged = judge_comment(&[("L-1", "violates", "blocks")]);
    forged["user"]["login"] = json!("octo-owner");
    assert_eq!(judge_report(&[forged.clone()]), None);
    let real = judge_comment(&[("L-2", "unsure", "falls back to `review`")]);
    let comments = [forged, real];
    let body = judge_report(&comments).unwrap();
    assert_eq!(
        rows(body),
        [Row {
            entry: "L-2".to_owned(),
            answer: Answer::Unsure,
            blocks: false
        }]
    );
}

#[test]
fn a_red_run_of_a_workflow_without_required_jobs_does_not_count() {
    let mut api = Recorded::day();
    api.edit(&runs_path("push"), |runs| {
        let list = runs["workflow_runs"].as_array_mut().unwrap();
        // The recorded kind run, turned red.
        let kind = list.iter_mut().find(|r| r["name"] == "kind").unwrap();
        kind["conclusion"] = json!("failure");
    });
    api.responses.insert(
        jobs_path(35_881_515_494),
        json!({"total_count": 1, "jobs": [{"name": "integration", "conclusion": "failure"}]}),
    );
    let ledger = collect(&api, &day(), &required()).unwrap();
    assert_eq!(ledger.red_runs.len(), 3);
    assert!(ledger.red_runs.iter().all(|r| r.workflow != "kind"));
}

#[test]
fn a_run_that_never_started_is_judged_by_the_jobs_of_another_run() {
    let mut api = Recorded::day();
    api.edit(&runs_path("push"), |runs| {
        let list = runs["workflow_runs"].as_array_mut().unwrap();
        let newest_ci = list
            .iter_mut()
            .find(|r| r["id"] == 35_855_661_934_u64)
            .unwrap();
        newest_ci["conclusion"] = json!("startup_failure");
    });
    api.responses.insert(
        jobs_path(35_855_661_934),
        json!({"total_count": 0, "jobs": []}),
    );
    // The green ci run of the same workflow, newest first, has the required jobs.
    api.responses
        .insert(jobs_path(35_881_515_427), parse(JOBS_CI));
    let ledger = collect(&api, &day(), &required()).unwrap();
    let conclusions: Vec<&str> = ledger
        .red_runs
        .iter()
        .map(|r| r.conclusion.as_str())
        .collect();
    assert_eq!(conclusions, ["failure", "failure", "startup_failure"]);
}

#[test]
fn listing_stops_at_a_page_that_reaches_before_the_range() {
    let mut page: Vec<Value> = (0..100)
        .map(|n| json!({"number": 1000 + n, "merged_at": null, "updated_at": "2026-09-24T01:00:00Z"}))
        .collect();
    page[10] = json!({"number": 12, "merged_at": "2026-09-23T23:59:59Z", "updated_at": "2026-09-24T00:00:01Z"});
    page[20] = json!({"number": 11, "merged_at": "2026-09-23T08:00:00Z", "updated_at": "2026-09-23T08:00:02Z"});
    page[30] = json!({"number": 13, "merged_at": "2026-09-24T00:00:00Z", "updated_at": "2026-09-24T00:00:02Z"});
    page[99] = json!({"number": 10, "merged_at": "2026-09-22T23:59:59Z", "updated_at": "2026-09-22T23:59:59Z"});
    let api = Recorded {
        responses: BTreeMap::from([(CLOSED_PAGE_1.to_owned(), Value::Array(page))]),
        gets: RefCell::new(Vec::new()),
    };
    assert_eq!(merged_pulls(&api, &day()).unwrap(), [11, 12]);
    assert_eq!(*api.gets.borrow(), [CLOSED_PAGE_1], "page 2 is never read");
    let two = Range::from_args(["2026-09-22".to_owned(), "2026-09-24".to_owned()]).unwrap();
    // The page is full and none of it is older than the 22nd, so page 2 is read (and missing).
    let err = merged_pulls(&api, &two).unwrap_err();
    assert!(err.to_string().contains("page=2"), "{err}");
}

#[test]
fn findings_are_the_top_level_items_of_the_blocking_section() {
    let body = "Review verdict: FAIL @ x\n\n## Checked\n\n1. **Not a finding** (rule)\n\n## Blocking findings\n\nIntro text.\n\n1. **First** (contract/rule: the checks must pass). Detail L-2.\n   - sub item (pr-body)\n\n   More.\n2. pr-body: the description says x.\n3. **Third, tagged inside** (`a/b.rs:3`).\n- **Fourth** (Correctness)\n\n## Advisory (not blocking)\n\n1. **Advice** (style)\n";
    let findings = blocking_findings(body);
    let tags: Vec<Vec<String>> = findings.iter().map(|f| f.classes.clone()).collect();
    assert_eq!(
        tags,
        [
            vec!["contract".to_owned(), "rule".to_owned()],
            vec!["pr-body".to_owned()],
            vec![],
            vec![],
        ]
    );
    assert!(findings[0].text.contains("sub item") && findings[0].text.contains("More."));
    assert!(names(&findings[0].text, "L-2"));
    let untagged: Vec<&str> = findings[2].class_names().collect();
    assert_eq!(untagged, [UNTAGGED]);
    assert!(blocking_findings("Review verdict: FAIL @ x\n\n## Blocking\n\nNone.\n").is_empty());
    assert!(blocking_findings("Review verdict: FAIL @ x\n\nNo sections.").is_empty());
}

#[test]
fn class_slugs_follow_the_bold_title_or_lead_the_item() {
    assert_eq!(classes("**T** (pr-body). x"), ["pr-body"]);
    assert_eq!(classes("**T** (rule, rule/pr-body)"), ["rule", "pr-body"]);
    assert_eq!(classes("**pr-body: the description** x"), ["pr-body"]);
    assert_eq!(classes("correctness: x (rule)"), ["correctness"]);
    assert!(classes("PR description, Evidence: false").is_empty());
    assert!(classes("**T (rule)** x").is_empty());
    assert!(classes("**T** (`crates/autobot-fakes/src/lib.rs:2`)").is_empty());
    assert!(classes("**T** (rule and more)").is_empty());
    assert_eq!(
        classes("**T** (contract/rule: checks, and / more)"),
        ["contract", "rule"]
    );
    assert!(classes("**T** x (rule)").is_empty());
    assert!(is_slug("merge-compile-break2") && !is_slug("2x") && !is_slug("") && !is_slug("Rule"));
    let finding = Finding {
        classes: vec!["rule".to_owned()],
        text: String::new(),
    };
    assert_eq!(finding.class_names().collect::<Vec<_>>(), ["rule"]);
}

#[test]
fn an_entry_id_is_named_only_as_a_whole_word() {
    assert!(names("breaks CHARTER L-2.", "L-2"));
    assert!(names("(L-2)", "L-2"));
    assert!(!names("L-20 and XL-2 and L-2a and L-2-b", "L-2"));
    assert!(!names("anything", ""));
}

#[test]
fn judge_rows_read_the_answer_and_whether_it_blocks() {
    let body = judge_comment(&[
        ("L-1", "violates", "blocks"),
        ("L-2", "malformed: no type", "falls back to `review`"),
        ("C-1", "complies", "falls back to `review`"),
    ])["body"]
        .as_str()
        .unwrap()
        .to_owned();
    let got: Vec<(String, Answer, bool)> = rows(&body)
        .into_iter()
        .map(|r| (r.entry, r.answer, r.blocks))
        .collect();
    assert_eq!(
        got,
        [
            ("L-1".to_owned(), Answer::Violates, true),
            ("L-2".to_owned(), Answer::None, false),
            ("C-1".to_owned(), Answer::Complies, false),
        ]
    );
    assert!(rows(&format!("{MARKER}\nNo judged entry was read.")).is_empty());
}

#[test]
fn required_contexts_are_the_rulesets_status_checks() {
    let contexts = required();
    for context in ["review-gate", "test", "formal", "labels"] {
        assert!(contexts.contains(context), "{context}");
    }
    assert!(!contexts.contains("integration") && !contexts.contains("judge"));
    assert!(required_contexts(&json!({"rules": [{"type": "deletion"}]})).is_empty());
}

#[test]
fn the_range_takes_one_or_two_dates() {
    let one = Range::from_args(["2026-09-23".to_owned()]).unwrap();
    assert_eq!((one.from.as_str(), one.to.as_str()), (DAY, DAY));
    assert!(one.contains("2026-09-23T23:59:59Z") && !one.contains("2026-09-24T00:00:00Z"));
    assert!(!one.contains("2026-09-22T23:59:59Z") && !one.contains(""));
    for bad in [
        vec![],
        vec!["2026-9-23".to_owned()],
        vec!["2026-13-01".to_owned()],
        vec!["2026-09-00".to_owned()],
        vec!["2026-09-24".to_owned(), "2026-09-23".to_owned()],
        vec![DAY.to_owned(), DAY.to_owned(), DAY.to_owned()],
    ] {
        assert!(Range::from_args(bad.clone()).is_err(), "{bad:?}");
    }
}

#[test]
fn timestamps_convert_to_epoch_seconds() {
    assert_eq!(epoch_seconds("1970-01-01T00:00:00Z").unwrap(), 0);
    assert_eq!(epoch_seconds("2000-03-01T00:00:00Z").unwrap(), 951_868_800);
    assert_eq!(
        epoch_seconds("2026-09-23T11:26:59Z").unwrap(),
        1_790_162_819
    );
    for bad in [
        "2026-09-23 11:26:59Z",
        "2026-09-23T11:26:59",
        "2026-09-23T1a:26:59Z",
        "2026-09-23T11-26-59Z",
        "2026-09-23T+1:26:59Z",
    ] {
        assert!(epoch_seconds(bad).is_err(), "{bad}");
    }
}

#[test]
fn findings_are_also_read_from_defects_sections_and_a_leading_numbered_list() {
    let count = |body: &str| blocking_findings(body).len();
    let heading = "Review verdict: FAIL @ x\n\n## Checked\n\n- fine\n\n## Defects\n\n1. `a.rs:1`: wrong.\n2. **B** (rule). Also wrong.\n\n## Checked\n\n1. not a finding\n";
    assert_eq!(count(heading), 2);
    assert_eq!(blocking_findings(heading)[1].classes, ["rule"]);
    let bold = "Review verdict: FAIL @ x\n\n**Paths:** fine.\n\n**Defects:**\n\n1. one\n   **Fix:** indented, still the finding\n2. two\n\n**Earlier FAIL (abc):**\n\n**Defects:**\n\n1. an older one\n";
    let findings = blocking_findings(bold);
    assert_eq!(findings.len(), 2, "{findings:?}");
    assert!(findings[0].text.contains("still the finding"));
    let bare = "Review verdict: FAIL @ x\n\n1. `README.md`: no build section.\n   - detail\n2. PR body: says x.\n- a remark, not numbered\n\n## Advisory (not blocking)\n\n1. advice\n";
    let findings = blocking_findings(bare);
    assert_eq!(findings.len(), 2, "{findings:?}");
    assert!(findings[1].text.contains("a remark"));
    let fenced = "Review verdict: FAIL @ x\n\n## Blocking\n\n1. one\n\n   ```text\n## Advisory\n   ```\n2. two\n";
    assert_eq!(count(fenced), 2);
}

#[test]
fn a_fail_whose_findings_cannot_be_read_counts_one_untagged_finding() {
    let sha = "b".repeat(40);
    let comments = [
        owner_comment(&format!("Review verdict: FAIL @ {sha}\n\nSee the thread.")),
        owner_comment(&format!("Review verdict: PASS @ {sha}\n\n1. checked")),
    ];
    let rounds = rounds(&comments);
    assert_eq!(rounds.len(), 2);
    assert_eq!(rounds[0].findings.len(), 1);
    assert_eq!(
        rounds[0].findings[0].class_names().collect::<Vec<_>>(),
        [UNTAGGED]
    );
    assert!(
        rounds[1].findings.is_empty(),
        "a PASS has no blocking findings"
    );
}
