//! A red `main` opens an `urgent` issue.
//!
//! The `main-red` workflow hands a completed run of a gate-lane workflow on `main` to [`main`],
//! and starts only for the runs this module calls red, which [`Run::skip_reason`] checks
//! again. A run is red when it was started by a push to [`BRANCH`] and its conclusion is one
//! of [`RED_CONCLUSIONS`], which GitHub reports when a job of the run failed or timed out, or
//! when the run could not start. Any other run is skipped.
//!
//! For a red run, the open issues labelled `urgent` are searched for one whose title starts
//! with the workflow's [`title_prefix`]:
//!
//! - when there is one, a comment naming the head SHA and linking the run is added to it, so
//!   a workflow that stays red keeps one issue;
//! - otherwise a finding is opened, titled `main is red: <workflow> failed at <sha>`, with the
//!   labels [`LABELS`] and a capsule body linking the run, as a sub-issue of the gate epic of
//!   the earliest open milestone and in that epic's milestone.
//!
//! The earliest open milestone is the open milestone with the lowest number, since milestones
//! are numbered in plan order and carry no due dates. Its gate epic is the open issue
//! labelled `type:epic` in that milestone whose body starts with [`GATE_EPIC_PREFIX`]. When
//! the milestone has none, the open epic whose body carries the plan key [`FALLBACK_EPIC`]
//! is the parent, in its own milestone.
//!
//! Nothing is ever reverted or closed here.

use super::settings::Api;
use super::{Client, Method};
use crate::{Error, Result};
use serde_json::{Value, json};

/// The branch whose push runs are watched.
pub const BRANCH: &str = "main";

/// Run conclusions that make `main` red.
pub const RED_CONCLUSIONS: [&str; 3] = ["failure", "timed_out", "startup_failure"];

/// Labels of an opened issue.
pub const LABELS: [&str; 3] = ["urgent", "finding", "bug"];

/// How the body of a gate epic starts.
pub const GATE_EPIC_PREFIX: &str = "Gate epic for ";

/// The plan key of the parent epic used when the earliest open milestone has no gate epic.
pub const FALLBACK_EPIC: &str = "E-FOUND-CI";

/// Items requested per page; a shorter page is the last one.
const PAGE_SIZE: usize = 100;

/// A completed workflow run, as found in a `workflow_run` event or `GET actions/runs/{id}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// The workflow's name.
    pub workflow: String,
    /// The event that started the run.
    pub event: String,
    /// The branch the run's head belongs to.
    pub branch: String,
    /// The head commit SHA.
    pub sha: String,
    /// The run's conclusion; empty while the run is not complete.
    pub conclusion: String,
    /// The run's page.
    pub url: String,
}

impl Run {
    /// Reads a run from its REST representation.
    ///
    /// # Errors
    /// Fails if `name`, `event`, `head_branch`, `head_sha` or `html_url` is missing.
    pub fn from_json(run: &Value) -> Result<Self> {
        let field = |key: &str| {
            run[key]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| Error::Parse(format!("workflow run: no `{key}`")))
        };
        Ok(Self {
            workflow: field("name")?,
            event: field("event")?,
            branch: field("head_branch")?,
            sha: field("head_sha")?,
            conclusion: run["conclusion"].as_str().unwrap_or_default().to_owned(),
            url: field("html_url")?,
        })
    }

    /// Why the run leaves `main` alone, or `None` when it turns `main` red.
    #[must_use]
    pub fn skip_reason(&self) -> Option<String> {
        if self.event != "push" || self.branch != BRANCH {
            return Some(format!(
                "`{}` run on {} of `{}`, not a push to {BRANCH}",
                self.workflow, self.event, self.branch
            ));
        }
        if !RED_CONCLUSIONS.contains(&self.conclusion.as_str()) {
            return Some(format!(
                "`{}` concluded `{}`",
                self.workflow, self.conclusion
            ));
        }
        None
    }
}

/// How the title of every issue for `workflow` starts.
#[must_use]
pub fn title_prefix(workflow: &str) -> String {
    format!("main is red: {workflow} failed at ")
}

/// An issue to open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewIssue {
    /// The issue title.
    pub title: String,
    /// The issue body.
    pub body: String,
    /// The milestone number.
    pub milestone: u64,
    /// The number of the parent epic.
    pub parent: u64,
}

/// What a red run does to the issue list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Open an issue.
    Open(NewIssue),
    /// Comment on the open issue with this number.
    Comment {
        /// The issue number.
        issue: u64,
        /// The comment body.
        body: String,
    },
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open(issue) => write!(
                f,
                "open `{}` labelled {} in milestone {} under #{}:\n\n{}",
                issue.title,
                LABELS.join(", "),
                issue.milestone,
                issue.parent,
                issue.body
            ),
            Self::Comment { issue, body } => write!(f, "comment on #{issue}:\n\n{body}"),
        }
    }
}

/// Decides what red run `run` does: a comment on the workflow's open issue, or a new issue
/// placed under the gate epic of the earliest open milestone.
///
/// # Errors
/// Fails if a GitHub call fails, there is no open milestone, or no parent epic is found.
pub fn plan(api: &impl Api, run: &Run) -> Result<Action> {
    let prefix = title_prefix(&run.workflow);
    let open = list(api, "issues?labels=urgent&state=open")?;
    let existing = open.iter().find(|issue| {
        issue.get("pull_request").is_none()
            && issue["title"]
                .as_str()
                .is_some_and(|t| t.starts_with(&prefix))
    });
    if let Some(issue) = existing {
        let Some(number) = issue["number"].as_u64() else {
            return Err(Error::Parse(format!("issue `{prefix}…`: no `number`")));
        };
        return Ok(Action::Comment {
            issue: number,
            body: format!(
                "`{}` failed again at {}: {} (conclusion `{}`).",
                run.workflow, run.sha, run.url, run.conclusion
            ),
        });
    }
    let (parent, milestone) = parent_epic(api)?;
    Ok(Action::Open(NewIssue {
        title: format!("{prefix}{}", run.sha),
        body: issue_body(run),
        milestone,
        parent,
    }))
}

/// Carries out `action` and returns a one-line account of it.
///
/// # Errors
/// Fails if a GitHub call fails or the created issue lacks its number or id.
pub fn apply(api: &impl Api, action: &Action) -> Result<String> {
    match action {
        Action::Open(issue) => {
            let created = api.request(
                Method::Post,
                "issues",
                Some(&json!({
                    "title": issue.title,
                    "body": issue.body,
                    "labels": LABELS,
                    "milestone": issue.milestone,
                })),
            )?;
            let (Some(number), Some(id)) = (created["number"].as_u64(), created["id"].as_u64())
            else {
                return Err(Error::Parse(
                    "created issue: no `number` or `id`".to_owned(),
                ));
            };
            api.request(
                Method::Post,
                &format!("issues/{}/sub_issues", issue.parent),
                Some(&json!({ "sub_issue_id": id })),
            )?;
            Ok(format!(
                "opened #{number} `{}` under #{} in milestone {}",
                issue.title, issue.parent, issue.milestone
            ))
        }
        Action::Comment { issue, body } => {
            api.request(
                Method::Post,
                &format!("issues/{issue}/comments"),
                Some(&json!({ "body": body })),
            )?;
            Ok(format!("commented on #{issue}"))
        }
    }
}

/// Entry point of `scripts/main_red.rs`. The repository is `GITHUB_REPOSITORY`.
///
/// With no argument, reads the `workflow_run` event at `GITHUB_EVENT_PATH` and, when the run
/// is red, carries out its [`Action`]. With `--dry-run <run-url>`, reads the run behind the
/// URL (`https://github.com/<owner>/<name>/actions/runs/<id>`, from any repository), prints
/// whether the workflow would act on it and the action it would take, and writes nothing.
///
/// # Errors
/// Fails on bad arguments, an unset `GITHUB_REPOSITORY`, an unreadable event, or a failed
/// GitHub call.
pub fn main(args: impl IntoIterator<Item = String>) -> Result<()> {
    let dry_run = parse_args(args)?;
    let var = |key: &str| {
        std::env::var(key)
            .ok()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| Error::Parse(format!("{key} is not set")))
    };
    let client = Client::new(var("GITHUB_REPOSITORY")?)?;
    let Some((run_repo, id)) = dry_run else {
        let path = var("GITHUB_EVENT_PATH")?;
        let text =
            std::fs::read_to_string(&path).map_err(|e| Error::Parse(format!("{path}: {e}")))?;
        let event: Value =
            serde_json::from_str(&text).map_err(|e| Error::Parse(format!("{path}: {e}")))?;
        let run = Run::from_json(&event["workflow_run"])?;
        if let Some(reason) = run.skip_reason() {
            println!("main-red: skipped: {reason}");
            return Ok(());
        }
        println!("main-red: {}", apply(&client, &plan(&client, &run)?)?);
        return Ok(());
    };
    let run = Run::from_json(&Client::new(run_repo)?.get(&format!("actions/runs/{id}"))?)?;
    match run.skip_reason() {
        Some(reason) => println!("main-red would skip this run: {reason}"),
        None => println!("main-red would act on this run"),
    }
    println!(
        "dry run, nothing written; for a red run it would {}",
        plan(&client, &run)?
    );
    Ok(())
}

/// `None` for no argument, or the repository and run id of `--dry-run <run-url>`.
fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Option<(String, u64)>> {
    let args: Vec<String> = args.into_iter().collect();
    match args.as_slice() {
        [] => Ok(None),
        [flag, url] if flag == "--dry-run" => parse_run_url(url).map(Some),
        _ => Err(Error::Parse(
            "usage: main_red [--dry-run <run-url>]".to_owned(),
        )),
    }
}

/// The `owner/name` and run id of `https://github.com/<owner>/<name>/actions/runs/<id>`,
/// optionally followed by more path segments.
fn parse_run_url(url: &str) -> Result<(String, u64)> {
    let bad = || Error::Parse(format!("not a workflow run URL: `{url}`"));
    let path = url.strip_prefix("https://github.com/").ok_or_else(bad)?;
    let parts: Vec<&str> = path.split('/').collect();
    match parts.as_slice() {
        [owner, name, "actions", "runs", id, ..] if !owner.is_empty() && !name.is_empty() => {
            Ok((format!("{owner}/{name}"), id.parse().map_err(|_| bad())?))
        }
        _ => Err(bad()),
    }
}

/// The capsule body of a new issue for red run `run`.
fn issue_body(run: &Run) -> String {
    let Run {
        workflow,
        sha,
        url,
        conclusion,
        ..
    } = run;
    format!(
        "**Objective**\n\
         Make `{workflow}` pass on {BRANCH} again. Its run on the push of {sha} concluded \
         `{conclusion}`: {url}\n\n\
         **Design refs**\n\
         Gate lane (CI on push to main)\n\n\
         **Allowed paths**\n\
         The paths the failure's cause is in, named when it is triaged.\n\n\
         **Non-goals**\n\
         Changes the failure does not need.\n\n\
         **Acceptance evidence**\n\
         A `{workflow}` run on a push to {BRANCH} concludes `success`.\n"
    )
}

/// The parent epic and its milestone, as described in the module docs.
fn parent_epic(api: &impl Api) -> Result<(u64, u64)> {
    let earliest = list(api, "milestones?state=open")?
        .iter()
        .filter_map(|m| m["number"].as_u64())
        .min()
        .ok_or_else(|| Error::Parse("no open milestone".to_owned()))?;
    let epics = list(api, "issues?labels=type:epic&state=open")?;
    let body = |epic: &&Value| epic["body"].as_str().unwrap_or_default().to_owned();
    let milestone = |epic: &Value| epic["milestone"]["number"].as_u64();
    let plan_key = format!("<!-- plan-key: {FALLBACK_EPIC} -->");
    let parent = epics
        .iter()
        .find(|e| milestone(e) == Some(earliest) && body(e).starts_with(GATE_EPIC_PREFIX))
        .or_else(|| epics.iter().find(|e| body(e).contains(&plan_key)))
        .ok_or_else(|| {
            Error::Parse(format!(
                "milestone {earliest} has no open gate epic and no open epic is {FALLBACK_EPIC}"
            ))
        })?;
    match (parent["number"].as_u64(), milestone(parent)) {
        (Some(number), Some(milestone)) => Ok((number, milestone)),
        _ => Err(Error::Parse(
            "parent epic: no `number` or `milestone.number`".to_owned(),
        )),
    }
}

/// Every item of the list endpoint `path` (which has a query string), in page order.
fn list(api: &impl Api, path: &str) -> Result<Vec<Value>> {
    let mut all = Vec::new();
    for page in 1.. {
        let paged = format!("{path}&per_page={PAGE_SIZE}&page={page}");
        let Value::Array(batch) = api.request(Method::Get, &paged, None)? else {
            return Err(Error::Parse(format!("{paged}: not a JSON array")));
        };
        let last = batch.len() < PAGE_SIZE;
        all.extend(batch);
        if last {
            break;
        }
    }
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    // Recorded from GET repos/tsouza/runnerscout/actions/runs/35276703631, a push run on main
    // that failed, keeping the fields `Run` reads plus `id`, `status` and `run_attempt`.
    const FAILED_RUN: &str = r#"{"id":35276703631,"name":"CI","event":"push","head_branch":"main","head_sha":"47309bae54d56d0899939259a553d01e69cd2a2a","status":"completed","conclusion":"failure","html_url":"https://github.com/tsouza/runnerscout/actions/runs/35276703631","run_attempt":1}"#;

    // Recorded from GET repos/tsouza/autobot/milestones?state=open&per_page=100, keeping
    // `number`, `title` and `state`.
    const MILESTONES: &str = r#"[{"number":1,"title":"M-1 · Foundation — repository, tooling, CI, governance","state":"open"},{"number":2,"title":"M0-Q · G-QUAL — qualification slice","state":"open"},{"number":3,"title":"M0-QF · G-FORMAL — formal qualification alongside M0","state":"open"},{"number":4,"title":"M1 · G-INTAKE — install & real intake","state":"open"},{"number":5,"title":"M2 · G-FENCE-CUSTODY — real broker, isolation & custody","state":"open"},{"number":6,"title":"M3 · G-FORGE — forge mirroring & conformance","state":"open"},{"number":7,"title":"M4 · G-OBS — observability","state":"open"},{"number":8,"title":"M5 · G-EVALUATION — evaluation ledger","state":"open"},{"number":9,"title":"M6 · G-ADAPT — adaptive routing","state":"open"},{"number":10,"title":"M7 · G-PORTABILITY — second forge & runtime","state":"open"},{"number":11,"title":"M8 · G-OPS — operational qualification","state":"open"},{"number":12,"title":"M9 · G-PROD — production qualification","state":"open"}]"#;

    // Recorded from GET repos/tsouza/autobot/issues?labels=type:epic&state=open&per_page=100,
    // keeping the M-1 epics and the M0-Q gate epic with `number`, `id`, `title`, `state`,
    // `milestone.number`, and of `body` the first line and the plan-key line.
    const EPICS: &str = r#"[{"number":22,"id":5546503658,"title":"M0-Q qualification: target, end-to-end scenario, measurements, gate evidence","state":"open","milestone":{"number":2},"body":"Gate epic for G-QUAL.\n\n<!-- plan-key: E-M0-QUALIFICATION -->"},{"number":4,"id":5546495144,"title":"Design-set checks and the design-change process","state":"open","milestone":{"number":1},"body":"Mechanical consistency of docs/design.\n\n<!-- plan-key: E-FOUND-DESIGN -->"},{"number":3,"id":5546494859,"title":"CI workflows, required checks, review gate and dependency automation","state":"open","milestone":{"number":1},"body":"GHA -> just -> scripts. Gate lane only (no model, no secrets in required checks). A red main opens an `urgent` issue.\n\n<!-- plan-key: E-FOUND-CI -->"},{"number":2,"id":5546494552,"title":"Workspace crates, Justfile, devtools library, worktrees and sccache","state":"open","milestone":{"number":1},"body":"The nine crates with the lint policy, cargo-deny and rustfmt.\n\n<!-- plan-key: E-FOUND-TOOLING -->"}]"#;

    const MILESTONES_PAGE: &str = "milestones?state=open&per_page=100&page=1";
    const EPICS_PAGE: &str = "issues?labels=type:epic&state=open&per_page=100&page=1";
    const URGENT_PAGE: &str = "issues?labels=urgent&state=open&per_page=100&page=1";
    const SHA: &str = "47309bae54d56d0899939259a553d01e69cd2a2a";
    const RUN_URL: &str = "https://github.com/tsouza/runnerscout/actions/runs/35276703631";

    fn parse(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    fn failed_run() -> Run {
        Run::from_json(&parse(FAILED_RUN)).unwrap()
    }

    /// Serves GET responses by path, fails any other GET the way GitHub answers an unknown
    /// resource, and records every write; an issue creation answers with the next issue.
    struct FakeRepo {
        responses: BTreeMap<String, Value>,
        writes: RefCell<Vec<(Method, String, Value)>>,
    }

    impl FakeRepo {
        fn new(urgent: Value) -> Self {
            let responses = BTreeMap::from([
                (MILESTONES_PAGE.to_owned(), parse(MILESTONES)),
                (EPICS_PAGE.to_owned(), parse(EPICS)),
                (URGENT_PAGE.to_owned(), urgent),
            ]);
            Self {
                responses,
                writes: RefCell::new(Vec::new()),
            }
        }
    }

    impl Api for FakeRepo {
        fn request(&self, method: Method, path: &str, body: Option<&Value>) -> Result<Value> {
            if method == Method::Get {
                return self
                    .responses
                    .get(path)
                    .cloned()
                    .ok_or_else(|| Error::Http(format!("404 {path}")));
            }
            let body = body.cloned().unwrap_or(Value::Null);
            self.writes
                .borrow_mut()
                .push((method, path.to_owned(), body));
            Ok(if path == "issues" {
                json!({"number": 300, "id": 9_000_000_300_u64})
            } else {
                json!({})
            })
        }
    }

    fn expected_body() -> String {
        format!(
            "**Objective**\nMake `CI` pass on main again. Its run on the push of {SHA} \
             concluded `failure`: {RUN_URL}\n\n**Design refs**\nGate lane (CI on push to \
             main)\n\n**Allowed paths**\nThe paths the failure's cause is in, named when it \
             is triaged.\n\n**Non-goals**\nChanges the failure does not need.\n\n\
             **Acceptance evidence**\nA `CI` run on a push to main concludes `success`.\n"
        )
    }

    #[test]
    fn a_failed_run_opens_an_issue_under_the_ci_epic_while_m_minus_1_is_open() {
        let api = FakeRepo::new(json!([]));
        let run = failed_run();
        assert_eq!(run.skip_reason(), None);
        let action = plan(&api, &run).unwrap();
        assert_eq!(
            action,
            Action::Open(NewIssue {
                title: format!("main is red: CI failed at {SHA}"),
                body: expected_body(),
                milestone: 1,
                parent: 3,
            })
        );
        assert_eq!(
            apply(&api, &action).unwrap(),
            format!("opened #300 `main is red: CI failed at {SHA}` under #3 in milestone 1")
        );
        assert_eq!(
            *api.writes.borrow(),
            [
                (
                    Method::Post,
                    "issues".to_owned(),
                    json!({
                        "title": format!("main is red: CI failed at {SHA}"),
                        "body": expected_body(),
                        "labels": ["urgent", "finding", "bug"],
                        "milestone": 1,
                    })
                ),
                (
                    Method::Post,
                    "issues/3/sub_issues".to_owned(),
                    json!({"sub_issue_id": 9_000_000_300_u64})
                ),
            ]
        );
    }

    #[test]
    fn the_gate_epic_of_the_earliest_open_milestone_is_the_parent() {
        let mut api = FakeRepo::new(json!([]));
        let mut milestones = parse(MILESTONES);
        milestones.as_array_mut().unwrap().remove(0);
        api.responses.insert(MILESTONES_PAGE.to_owned(), milestones);
        let Action::Open(issue) = plan(&api, &failed_run()).unwrap() else {
            panic!("expected a new issue");
        };
        assert_eq!((issue.parent, issue.milestone), (22, 2));
    }

    #[test]
    fn no_gate_epic_and_no_fallback_epic_is_an_error() {
        let mut api = FakeRepo::new(json!([]));
        let mut epics = parse(EPICS);
        epics.as_array_mut().unwrap().retain(|e| e["number"] != 3);
        api.responses.insert(EPICS_PAGE.to_owned(), epics);
        let err = plan(&api, &failed_run()).unwrap_err();
        assert!(
            err.to_string()
                .contains("milestone 1 has no open gate epic"),
            "{err}"
        );
    }

    #[test]
    fn a_second_failure_of_the_same_workflow_comments_on_its_open_issue() {
        let first = format!("main is red: CI failed at {}", "0".repeat(40));
        let api = FakeRepo::new(json!([
            {"number": 290, "title": "main is red: ci failed at 1234", "labels": []},
            {"number": 291, "title": first, "pull_request": {}},
            {"number": 292, "title": first},
        ]));
        let action = plan(&api, &failed_run()).unwrap();
        let body = format!("`CI` failed again at {SHA}: {RUN_URL} (conclusion `failure`).");
        assert_eq!(
            action,
            Action::Comment {
                issue: 292,
                body: body.clone()
            }
        );
        assert_eq!(apply(&api, &action).unwrap(), "commented on #292");
        assert_eq!(
            *api.writes.borrow(),
            [(
                Method::Post,
                "issues/292/comments".to_owned(),
                json!({ "body": body })
            )]
        );
    }

    #[test]
    fn open_issues_on_later_pages_are_found() {
        let mut api = FakeRepo::new(json!(vec![json!({"number": 1, "title": "x"}); PAGE_SIZE]));
        api.responses.insert(
            "issues?labels=urgent&state=open&per_page=100&page=2".to_owned(),
            json!([{"number": 292, "title": format!("main is red: CI failed at {SHA}")}]),
        );
        assert!(matches!(
            plan(&api, &failed_run()).unwrap(),
            Action::Comment { issue: 292, .. }
        ));
    }

    #[test]
    fn only_red_push_runs_on_main_count() {
        let with = |key: &str, value: &str| {
            let mut run = parse(FAILED_RUN);
            run[key] = json!(value);
            Run::from_json(&run).unwrap().skip_reason()
        };
        for conclusion in RED_CONCLUSIONS {
            assert_eq!(with("conclusion", conclusion), None, "{conclusion}");
        }
        for conclusion in ["success", "cancelled", "skipped", "neutral", ""] {
            assert!(with("conclusion", conclusion).is_some(), "{conclusion}");
        }
        assert!(with("event", "pull_request").is_some());
        assert!(with("event", "workflow_dispatch").is_some());
        assert!(with("head_branch", "scratch").is_some());
        let mut run = parse(FAILED_RUN);
        run["conclusion"] = Value::Null;
        assert!(Run::from_json(&run).unwrap().skip_reason().is_some());
    }

    #[test]
    fn a_run_without_its_head_sha_is_an_error() {
        let mut run = parse(FAILED_RUN);
        run.as_object_mut().unwrap().remove("head_sha");
        let err = Run::from_json(&run).unwrap_err();
        assert!(err.to_string().contains("head_sha"), "{err}");
    }

    #[test]
    fn arguments_are_nothing_or_a_dry_run_url() {
        assert_eq!(parse_args(Vec::new()).unwrap(), None);
        assert_eq!(
            parse_args(["--dry-run".to_owned(), RUN_URL.to_owned()]).unwrap(),
            Some(("tsouza/runnerscout".to_owned(), 35_276_703_631))
        );
        assert_eq!(
            parse_run_url(&format!("{RUN_URL}/attempts/2")).unwrap(),
            ("tsouza/runnerscout".to_owned(), 35_276_703_631)
        );
        for args in [
            vec!["--dry-run"],
            vec!["--dry-run", "https://github.com/tsouza/autobot/pull/1"],
            vec![
                "--dry-run",
                "https://github.com/tsouza/autobot/actions/runs/x",
            ],
            vec![
                "--dry-run",
                "http://github.com/tsouza/autobot/actions/runs/1",
            ],
            vec!["run", RUN_URL],
        ] {
            let args: Vec<String> = args.into_iter().map(str::to_owned).collect();
            assert!(parse_args(args.clone()).is_err(), "{args:?}");
        }
    }
}
