//! The `labels` check: a pull request that changes a human-lane path carries the labels that
//! path requires.
//!
//! | Changed path | Required labels |
//! |---|---|
//! | `.github/workflows/**`, `.github/rulesets/**`, `.github/settings.json` | [`HUMAN_LANE`] |
//! | `docs/design/**` | [`HUMAN_LANE`] and [`DESIGN_CHANGE`] |
//!
//! A renamed file is judged by both its new and its previous path, so moving a file out of a
//! guarded directory needs the same labels as editing it. Any other path requires no label.
//!
//! The changed files and the labels are read from the GitHub API when the check runs, so a
//! re-run after a label is added or removed sees the pull request's current labels.

use super::settings::Api;
use super::{Client, Method, pages, pr_number};
use crate::{Error, Result};
use std::collections::BTreeSet;
use std::process::ExitCode;

/// The name of the workflow job, and of the required check.
pub const CONTEXT: &str = "labels";

/// The label every human-lane change carries.
pub const HUMAN_LANE: &str = "human-lane";

/// The label a change to the design set carries, in addition to [`HUMAN_LANE`].
pub const DESIGN_CHANGE: &str = "design-change";

/// The labels a change to `path` requires, empty when it requires none.
#[must_use]
pub fn required_labels(path: &str) -> &'static [&'static str] {
    if path.starts_with("docs/design/") {
        &[HUMAN_LANE, DESIGN_CHANGE]
    } else if path.starts_with(".github/workflows/")
        || path.starts_with(".github/rulesets/")
        || path == ".github/settings.json"
    {
        &[HUMAN_LANE]
    } else {
        &[]
    }
}

/// A changed path that lacks a label it requires.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Missing {
    /// The changed path, new or previous.
    pub path: String,
    /// The label the pull request lacks.
    pub label: &'static str,
}

impl std::fmt::Display for Missing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} needs the `{}` label", self.path, self.label)
    }
}

/// Every label that `paths` require and `labels` lacks, sorted by path, then label.
#[must_use]
pub fn missing<'a>(
    paths: impl IntoIterator<Item = &'a str>,
    labels: &BTreeSet<String>,
) -> Vec<Missing> {
    let mut out = BTreeSet::new();
    for path in paths {
        for &label in required_labels(path) {
            if !labels.contains(label) {
                out.insert(Missing {
                    path: path.to_owned(),
                    label,
                });
            }
        }
    }
    out.into_iter().collect()
}

/// Reads pull request `pr`, its labels and its changed files, and returns every missing label.
///
/// # Errors
/// Fails if a GitHub call fails or a response lacks the labels or a file name.
pub fn evaluate(api: &impl Api, pr: u64) -> Result<Vec<Missing>> {
    let pull = api.request(Method::Get, &format!("pulls/{pr}"), None)?;
    let Some(labels) = pull["labels"].as_array() else {
        return Err(Error::Parse(format!("pulls/{pr}: no `labels`")));
    };
    let labels = labels
        .iter()
        .map(|l| {
            l["name"]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| Error::Parse(format!("pulls/{pr}: a label has no `name`")))
        })
        .collect::<Result<BTreeSet<_>>>()?;
    let files = pages(api, &format!("pulls/{pr}/files"))?;
    let mut paths = Vec::new();
    for file in &files {
        let Some(name) = file["filename"].as_str() else {
            return Err(Error::Parse(format!(
                "pulls/{pr}/files: a file has no `filename`"
            )));
        };
        paths.push(name);
        paths.extend(file["previous_filename"].as_str());
    }
    Ok(missing(paths, &labels))
}

/// Entry point of `scripts/label_gate.rs`: checks the pull request whose number is the only
/// argument, prints each missing label or a clean line, and returns the exit code. The
/// repository is resolved by [`super::repository`] from the current directory.
///
/// # Errors
/// Fails on a missing or non-numeric argument, an unresolvable repository, or a failed
/// GitHub call.
pub fn run(args: impl IntoIterator<Item = String>) -> Result<ExitCode> {
    let pr = pr_number(args, "label_gate")?;
    let repo = super::repository(".")?;
    let missing = evaluate(&Client::new(repo)?, pr)?;
    if missing.is_empty() {
        println!("{CONTEXT}: #{pr} carries every label its changed paths require");
        return Ok(ExitCode::SUCCESS);
    }
    for m in &missing {
        println!("{m}");
    }
    println!("{CONTEXT}: #{pr} lacks {} required label(s)", missing.len());
    Ok(ExitCode::FAILURE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::PAGE_SIZE;
    use crate::github::settings::GITHUB_ACTIONS_APP_ID;
    use serde_json::{Value, json};
    use std::collections::BTreeMap;

    // Recorded from GET repos/tsouza/autobot/pulls/248, keeping `number`, `state`, the label
    // names and `head`.
    const PULL_248: &str = r#"{"number":248,"state":"closed","labels":[{"name":"human-lane"},{"name":"area:ci"}],"head":{"ref":"54-review-gate","sha":"f1c9c9b525e8602a2873e56a544643705e72ef3f"}}"#;

    // Recorded from GET repos/tsouza/autobot/pulls/248/files?per_page=100&page=1, keeping
    // `filename`, `status` and `previous_filename`.
    const FILES_248: &str = r#"[{"filename":".github/rulesets/main.json","status":"modified"},{"filename":".github/workflows/review-gate.yml","status":"added"},{"filename":"Justfile","status":"modified"},{"filename":"scripts/devtools/src/github/mod.rs","status":"modified"},{"filename":"scripts/devtools/src/github/verdict.rs","status":"added"},{"filename":"scripts/review_gate.rs","status":"added"}]"#;

    // Recorded from GET repos/tsouza/autobot/pulls/237, same fields as PULL_248.
    const PULL_237: &str = r#"{"number":237,"state":"closed","labels":[{"name":"design"},{"name":"human-lane"},{"name":"design-change"}],"head":{"ref":"223-design-rulings","sha":"6b6bcf0485ea97ded8315b2ce4dbf2e26676a934"}}"#;

    // Recorded from GET repos/tsouza/autobot/pulls/237/files?per_page=100&page=1, same fields
    // as FILES_248.
    const FILES_237: &str = r#"[{"filename":"docs/design/AUTOBOT-FORMAL-SURFACE.md","status":"modified"},{"filename":"docs/design/AUTOBOT-KERNEL.md","status":"modified"},{"filename":"docs/design/AUTOBOT-M0-AND-GATES.md","status":"modified"}]"#;

    const WORKFLOW: &str = include_str!("../../../../.github/workflows/labels.yml");
    const MAIN_JSON: &str = include_str!("../../../../.github/rulesets/main.json");

    fn parse(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    /// Serves recorded GET responses by path and fails anything else the way GitHub answers an
    /// unknown resource.
    struct FakeRepo(BTreeMap<String, Value>);

    impl FakeRepo {
        fn new(pr: u64, pull: Value, files: Value) -> Self {
            let mut responses = BTreeMap::new();
            responses.insert(format!("pulls/{pr}"), pull);
            responses.insert(format!("pulls/{pr}/files?per_page=100&page=1"), files);
            Self(responses)
        }

        /// The recorded pull request `pr` (248 or 237) with its labels replaced by `labels`.
        fn relabelled(pr: u64, labels: &[&str]) -> Self {
            let (pull, files) = match pr {
                248 => (PULL_248, FILES_248),
                237 => (PULL_237, FILES_237),
                _ => unreachable!("no recording for #{pr}"),
            };
            let mut pull = parse(pull);
            pull["labels"] = labels.iter().map(|l| json!({"name": l})).collect();
            Self::new(pr, pull, parse(files))
        }
    }

    impl Api for FakeRepo {
        fn request(&self, method: Method, path: &str, _body: Option<&Value>) -> Result<Value> {
            assert_eq!(method, Method::Get, "the check only reads");
            self.0
                .get(path)
                .cloned()
                .ok_or_else(|| Error::Http(format!("404 {path}")))
        }
    }

    fn miss(path: &str, label: &'static str) -> Missing {
        Missing {
            path: path.to_owned(),
            label,
        }
    }

    #[test]
    fn workflow_change_with_human_lane_passes() {
        let api = FakeRepo::new(248, parse(PULL_248), parse(FILES_248));
        assert_eq!(evaluate(&api, 248).unwrap(), []);
    }

    #[test]
    fn workflow_change_without_human_lane_fails() {
        let api = FakeRepo::relabelled(248, &["area:ci"]);
        assert_eq!(
            evaluate(&api, 248).unwrap(),
            [
                miss(".github/rulesets/main.json", HUMAN_LANE),
                miss(".github/workflows/review-gate.yml", HUMAN_LANE),
            ]
        );
    }

    #[test]
    fn design_change_with_both_labels_passes() {
        let api = FakeRepo::new(237, parse(PULL_237), parse(FILES_237));
        assert_eq!(evaluate(&api, 237).unwrap(), []);
    }

    #[test]
    fn design_change_with_only_human_lane_fails() {
        let api = FakeRepo::relabelled(237, &["design", HUMAN_LANE]);
        assert_eq!(
            evaluate(&api, 237).unwrap(),
            [
                miss("docs/design/AUTOBOT-FORMAL-SURFACE.md", DESIGN_CHANGE),
                miss("docs/design/AUTOBOT-KERNEL.md", DESIGN_CHANGE),
                miss("docs/design/AUTOBOT-M0-AND-GATES.md", DESIGN_CHANGE),
            ]
        );
    }

    #[test]
    fn design_change_with_only_design_change_fails_for_human_lane() {
        let api = FakeRepo::relabelled(237, &[DESIGN_CHANGE]);
        let missing = evaluate(&api, 237).unwrap();
        assert_eq!(missing.len(), 3);
        assert!(missing.iter().all(|m| m.label == HUMAN_LANE), "{missing:?}");
    }

    #[test]
    fn unguarded_paths_need_no_label() {
        let mut files = parse(FILES_248);
        files.as_array_mut().unwrap().drain(..2);
        let mut pull = parse(PULL_248);
        pull["labels"] = json!([]);
        let api = FakeRepo::new(248, pull, files);
        assert_eq!(evaluate(&api, 248).unwrap(), []);
    }

    #[test]
    fn a_rename_is_judged_by_its_previous_path_too() {
        let files = json!([{
            "filename": "docs/AUTOBOT-KERNEL.md",
            "previous_filename": "docs/design/AUTOBOT-KERNEL.md",
            "status": "renamed",
        }]);
        let api = FakeRepo::new(237, json!({"labels": [{"name": HUMAN_LANE}]}), files);
        assert_eq!(
            evaluate(&api, 237).unwrap(),
            [miss("docs/design/AUTOBOT-KERNEL.md", DESIGN_CHANGE)]
        );
    }

    #[test]
    fn guarded_paths_are_exactly_the_human_lane_set() {
        for (path, labels) in [
            (".github/workflows/ci.yml", &[HUMAN_LANE][..]),
            (".github/rulesets/main.json", &[HUMAN_LANE]),
            (".github/settings.json", &[HUMAN_LANE]),
            (
                "docs/design/extensions/forge-mirroring.md",
                &[HUMAN_LANE, DESIGN_CHANGE],
            ),
            (".github/ISSUE_TEMPLATE/task.md", &[]),
            (".github/settings.json.bak", &[]),
            (".github/workflows", &[]),
            ("docs/README.md", &[]),
            ("docs/designs/x.md", &[]),
            ("scripts/devtools/src/github/label_gate.rs", &[]),
        ] {
            assert_eq!(required_labels(path), labels, "{path}");
        }
    }

    #[test]
    fn files_on_later_pages_are_read() {
        let mut filler = vec![json!({"filename": "README.md"}); PAGE_SIZE];
        filler[0] = json!({"filename": "Justfile"});
        let mut api = FakeRepo::new(248, json!({"labels": []}), Value::Array(filler));
        api.0.insert(
            "pulls/248/files?per_page=100&page=2".to_owned(),
            json!([{"filename": ".github/settings.json"}]),
        );
        assert_eq!(
            evaluate(&api, 248).unwrap(),
            [miss(".github/settings.json", HUMAN_LANE)]
        );
    }

    #[test]
    fn malformed_responses_are_errors() {
        let api = FakeRepo::new(248, json!({"number": 248}), parse(FILES_248));
        let err = evaluate(&api, 248).unwrap_err();
        assert!(err.to_string().contains("no `labels`"), "{err}");
        let api = FakeRepo::new(248, parse(PULL_248), json!([{"status": "added"}]));
        let err = evaluate(&api, 248).unwrap_err();
        assert!(err.to_string().contains("no `filename`"), "{err}");
        let api = FakeRepo::new(248, parse(PULL_248), json!({}));
        let err = evaluate(&api, 248).unwrap_err();
        assert!(err.to_string().contains("not a JSON array"), "{err}");
    }

    #[test]
    fn missing_labels_print_path_and_label() {
        assert_eq!(
            miss("docs/design/AUTOBOT-KERNEL.md", DESIGN_CHANGE).to_string(),
            "docs/design/AUTOBOT-KERNEL.md needs the `design-change` label"
        );
    }

    #[test]
    fn the_only_argument_is_a_pull_request_number() {
        assert_eq!(pr_number(["52".to_owned()], "label_gate").unwrap(), 52);
        for args in [
            vec![],
            vec!["x".to_owned()],
            vec!["1".to_owned(), "2".to_owned()],
        ] {
            assert!(pr_number(args.clone(), "label_gate").is_err(), "{args:?}");
        }
    }

    #[test]
    fn adding_or_removing_a_label_reruns_the_labels_job() {
        let on = WORKFLOW
            .split("\njobs:")
            .next()
            .unwrap()
            .split_once("\non:\n")
            .unwrap()
            .1;
        assert!(
            on.contains(
                "  pull_request:\n    types: [opened, synchronize, reopened, labeled, unlabeled, edited]\n"
            ),
            "{on}"
        );
        let jobs = WORKFLOW.split_once("\njobs:\n").unwrap().1;
        assert!(jobs.starts_with(&format!("  {CONTEXT}:\n")), "{jobs}");
        assert!(jobs.contains("just label-gate \"$PR\""), "{jobs}");
    }

    #[test]
    fn labels_is_a_required_check_of_the_ruleset() {
        let ruleset = parse(MAIN_JSON);
        let checks = ruleset["rules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["type"] == "required_status_checks")
            .unwrap()["parameters"]["required_status_checks"]
            .clone();
        assert!(
            checks
                .as_array()
                .unwrap()
                .contains(&json!({"context": CONTEXT, "integration_id": GITHUB_ACTIONS_APP_ID})),
            "{checks}"
        );
    }
}
