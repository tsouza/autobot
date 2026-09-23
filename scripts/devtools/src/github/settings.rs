//! Applies the committed branch ruleset and repository settings to GitHub, idempotently.
//!
//! The desired state lives in two committed files at the repository root:
//!
//! - [`RULESET_PATH`], a ruleset in the shape of the GitHub rulesets REST API. It is matched
//!   by `name` against the repository's own rulesets: created when absent, replaced with
//!   `PUT` when it differs.
//! - [`SETTINGS_PATH`], a JSON object of fields of the repository REST resource. Only the
//!   fields that differ are sent, in one `PATCH`.
//!
//! A field is compared only when the desired file names it; fields GitHub adds to its
//! responses (ids, links, timestamps) are ignored. Rules are compared in `type` order, so
//! the order GitHub returns them in does not matter. Each difference is one line of the
//! printed diff, and an empty diff means nothing is sent.
//!
//! Before anything is compared, the ruleset is checked against the merge policy: no
//! required approvals, an empty bypass list, squash as the only merge method, linear
//! history, deletion and non-fast-forward blocked, and every required status check pinned
//! to the GitHub Actions app ([`GITHUB_ACTIONS_APP_ID`]) with the strict up-to-date policy
//! off, and none of the [`SECRET_CHECKS`] required (CHARTER L-5). A ruleset that breaks the
//! policy is rejected and nothing is sent.

use super::{Client, Method};
use crate::{Error, Result};
use serde_json::{Map, Value};
use std::path::Path;

/// The ruleset file, relative to the repository root.
pub const RULESET_PATH: &str = ".github/rulesets/main.json";

/// The repository settings file, relative to the repository root.
pub const SETTINGS_PATH: &str = ".github/settings.json";

/// The app id of GitHub Actions, the only accepted source of a required status check.
pub const GITHUB_ACTIONS_APP_ID: u64 = 15368;

/// The checks that read a repository secret, which no ruleset may require (CHARTER L-5): the
/// `judge` check (its service key) and the `sensitive-terms` check (its term list).
pub const SECRET_CHECKS: [&str; 2] = ["judge", "sensitive-terms"];

/// The repository-scoped GitHub REST calls this module makes, so tests can serve recorded
/// responses in place of [`Client`].
pub trait Api {
    /// Sends `method` to `path` relative to the repository resource (`""` is the repository
    /// itself, `"rulesets"` its rulesets) with an optional JSON body, and returns the JSON
    /// response.
    ///
    /// # Errors
    /// Fails on a transport error, a non-success status, or a non-JSON body.
    fn request(&self, method: Method, path: &str, body: Option<&Value>) -> Result<Value>;
}

impl Api for Client {
    fn request(&self, method: Method, path: &str, body: Option<&Value>) -> Result<Value> {
        Client::request(self, method, path, body)
    }
}

/// What has to happen to the ruleset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RulesetAction {
    /// The ruleset already matches.
    Keep,
    /// No ruleset of that name exists; create it.
    Create,
    /// The ruleset with this id differs; replace it.
    Update(u64),
}

/// The changes needed to bring the repository to the committed state.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    ruleset: Value,
    ruleset_action: RulesetAction,
    settings_patch: Map<String, Value>,
    diff: Vec<String>,
}

impl Plan {
    /// One line per difference; empty when the repository already matches.
    #[must_use]
    pub fn diff(&self) -> &[String] {
        &self.diff
    }

    /// What has to happen to the ruleset.
    #[must_use]
    pub fn ruleset_action(&self) -> &RulesetAction {
        &self.ruleset_action
    }

    /// The settings fields that will be sent, with their desired values.
    #[must_use]
    pub fn settings_patch(&self) -> &Map<String, Value> {
        &self.settings_patch
    }
}

/// Checks a ruleset against the merge policy described in the module docs.
///
/// # Errors
/// Fails with a message naming the first rule the ruleset breaks.
pub fn validate_ruleset(ruleset: &Value) -> Result<()> {
    let invalid = |msg: &str| Err(Error::Parse(format!("{RULESET_PATH}: {msg}")));
    if !ruleset["name"].as_str().is_some_and(|n| !n.is_empty()) {
        return invalid("`name` must be a non-empty string");
    }
    if !ruleset["bypass_actors"]
        .as_array()
        .is_some_and(Vec::is_empty)
    {
        return invalid("`bypass_actors` must be present and empty");
    }
    let Some(rules) = ruleset["rules"].as_array() else {
        return invalid("`rules` must be an array");
    };
    let rule = |kind: &str| rules.iter().find(|r| r["type"] == kind);
    for kind in ["deletion", "non_fast_forward", "required_linear_history"] {
        if rule(kind).is_none() {
            return invalid(&format!("the `{kind}` rule is required"));
        }
    }
    let Some(pull_request) = rule("pull_request") else {
        return invalid("the `pull_request` rule is required");
    };
    let params = &pull_request["parameters"];
    if params["required_approving_review_count"] != 0 {
        return invalid("`required_approving_review_count` must be 0");
    }
    if params["allowed_merge_methods"] != serde_json::json!(["squash"]) {
        return invalid("`allowed_merge_methods` must be exactly [\"squash\"]");
    }
    if let Some(checks) = rule("required_status_checks") {
        let params = &checks["parameters"];
        if params["strict_required_status_checks_policy"] != false {
            return invalid("`strict_required_status_checks_policy` must be false");
        }
        let Some(list) = params["required_status_checks"].as_array() else {
            return invalid("`required_status_checks` must be an array");
        };
        if let Some(check) = list
            .iter()
            .find(|c| c["integration_id"] != GITHUB_ACTIONS_APP_ID)
        {
            return invalid(&format!(
                "required check {} must set `integration_id` to {GITHUB_ACTIONS_APP_ID}",
                check["context"]
            ));
        }
        if let Some(check) = list
            .iter()
            .find(|c| SECRET_CHECKS.iter().any(|s| c["context"] == *s))
        {
            return invalid(&format!(
                "required check {} reads a secret, which no required check may (CHARTER L-5)",
                check["context"]
            ));
        }
    }
    Ok(())
}

/// Compares the committed ruleset and settings with the repository's current state.
///
/// # Errors
/// Fails if the ruleset breaks the policy, the settings are not a JSON object, or a GitHub
/// call fails.
pub fn plan(api: &impl Api, ruleset: &Value, settings: &Value) -> Result<Plan> {
    validate_ruleset(ruleset)?;
    let Some(settings) = settings.as_object() else {
        return Err(Error::Parse(format!(
            "{SETTINGS_PATH}: must be a JSON object"
        )));
    };
    let name = ruleset["name"].as_str().unwrap_or_default();
    let mut diff = Vec::new();

    let existing = api.request(Method::Get, "rulesets?includes_parents=false", None)?;
    let id = existing
        .as_array()
        .into_iter()
        .flatten()
        .find(|r| r["name"] == name && r["source_type"] == "Repository")
        .and_then(|r| r["id"].as_u64());
    let ruleset_action = match id {
        None => {
            diff.push(format!("ruleset {name}: create"));
            RulesetAction::Create
        }
        Some(id) => {
            let current = api.request(Method::Get, &format!("rulesets/{id}"), None)?;
            let before = diff.len();
            compare(
                &format!("ruleset {name}: "),
                &sorted_rules(ruleset),
                Some(&sorted_rules(&current)),
                &mut diff,
            );
            if diff.len() == before {
                RulesetAction::Keep
            } else {
                RulesetAction::Update(id)
            }
        }
    };

    let repo = api.request(Method::Get, "", None)?;
    let mut settings_patch = Map::new();
    for (key, want) in settings {
        let have = repo.get(key);
        if have != Some(want) {
            diff.push(change(&format!("settings: {key}"), have, want));
            settings_patch.insert(key.clone(), want.clone());
        }
    }

    Ok(Plan {
        ruleset: ruleset.clone(),
        ruleset_action,
        settings_patch,
        diff,
    })
}

/// Sends the requests a [`Plan`] calls for; sends nothing for an empty plan.
///
/// # Errors
/// Fails if a GitHub call fails.
pub fn apply(api: &impl Api, plan: &Plan) -> Result<()> {
    match plan.ruleset_action {
        RulesetAction::Keep => {}
        RulesetAction::Create => {
            api.request(Method::Post, "rulesets", Some(&plan.ruleset))?;
        }
        RulesetAction::Update(id) => {
            api.request(Method::Put, &format!("rulesets/{id}"), Some(&plan.ruleset))?;
        }
    }
    if !plan.settings_patch.is_empty() {
        api.request(
            Method::Patch,
            "",
            Some(&Value::Object(plan.settings_patch.clone())),
        )?;
    }
    Ok(())
}

/// Entry point of `scripts/repo_settings.rs`: prints the diff, then applies it unless the
/// only argument is `--dry-run`.
///
/// The repository is resolved by [`super::repository`] from the checkout's root.
///
/// # Errors
/// Fails on an unknown argument, an unreadable or invalid file, or a failed GitHub call.
pub fn main(args: impl IntoIterator<Item = String>) -> Result<()> {
    let mut dry_run = false;
    for arg in args {
        match arg.as_str() {
            "--dry-run" => dry_run = true,
            other => return Err(Error::Parse(format!("unknown argument `{other}`"))),
        }
    }
    let root = crate::git::toplevel(".")?;
    let repo = super::repository(&root)?;
    let ruleset = read_json(&Path::new(&root).join(RULESET_PATH))?;
    let settings = read_json(&Path::new(&root).join(SETTINGS_PATH))?;
    let client = Client::new(repo.clone())?;
    let plan = plan(&client, &ruleset, &settings)?;
    println!("{repo}: {} change(s)", plan.diff().len());
    for line in plan.diff() {
        println!("  {line}");
    }
    if dry_run {
        println!("dry run: nothing applied");
    } else {
        apply(&client, &plan)?;
    }
    Ok(())
}

fn read_json(path: &Path) -> Result<Value> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| Error::Parse(format!("{}: {e}", path.display())))?;
    serde_json::from_str(&text).map_err(|e| Error::Parse(format!("{}: {e}", path.display())))
}

/// A copy of `ruleset` whose `rules` are ordered by `type`.
fn sorted_rules(ruleset: &Value) -> Value {
    let mut ruleset = ruleset.clone();
    if let Some(rules) = ruleset.get_mut("rules").and_then(Value::as_array_mut) {
        rules.sort_by(|a, b| a["type"].to_string().cmp(&b["type"].to_string()));
    }
    ruleset
}

/// Appends one line per field of `want` that `have` does not match. Objects are compared
/// on `want`'s keys only; arrays must have the same length and are compared by position.
fn compare(path: &str, want: &Value, have: Option<&Value>, out: &mut Vec<String>) {
    match (want, have) {
        (Value::Object(want), Some(Value::Object(have))) => {
            for (key, value) in want {
                let sep = if path.ends_with(": ") { "" } else { "." };
                compare(&format!("{path}{sep}{key}"), value, have.get(key), out);
            }
        }
        (Value::Array(want_items), Some(Value::Array(have_items)))
            if want_items.len() == have_items.len() =>
        {
            for (i, (w, h)) in want_items.iter().zip(have_items).enumerate() {
                compare(&format!("{path}[{i}]"), w, Some(h), out);
            }
        }
        _ if have == Some(want) => {}
        _ => out.push(change(path, have, want)),
    }
}

fn change(path: &str, have: Option<&Value>, want: &Value) -> String {
    let have = have.map_or_else(|| "(absent)".to_owned(), Value::to_string);
    format!("{path}: {have} -> {want}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    // The committed files change as later work adds required checks, so they are only
    // checked against the policy; the plan tests use the frozen desired state below, which
    // matches the recorded repository.
    const MAIN_JSON: &str = include_str!("../../../../.github/rulesets/main.json");
    const COMMITTED_SETTINGS: &str = include_str!("../../../../.github/settings.json");

    // The desired ruleset the recorded repository state was applied from.
    const DESIRED_RULESET: &str = r#"{"name":"main","target":"branch","enforcement":"active","bypass_actors":[],"conditions":{"ref_name":{"exclude":[],"include":["~DEFAULT_BRANCH"]}},"rules":[{"type":"deletion"},{"type":"non_fast_forward"},{"type":"required_linear_history"},{"type":"pull_request","parameters":{"required_approving_review_count":0,"dismiss_stale_reviews_on_push":false,"required_reviewers":[],"require_code_owner_review":false,"require_last_push_approval":false,"required_review_thread_resolution":true,"require_extra_approval_for_unattributed_changes":true,"allowed_merge_methods":["squash"]}}]}"#;

    // The desired settings the recorded repository state was applied from.
    const SETTINGS_JSON: &str = r#"{"allow_squash_merge":true,"allow_merge_commit":false,"allow_rebase_merge":false,"squash_merge_commit_title":"PR_TITLE","squash_merge_commit_message":"PR_BODY","allow_auto_merge":true,"delete_branch_on_merge":true,"allow_update_branch":false,"has_wiki":false,"has_discussions":false}"#;

    // Recorded from the GitHub API (GET rulesets?includes_parents=false), with owner and logins
    // replaced by placeholders.
    const RULESETS_LIST: &str = r#"[{"id":23850705,"name":"main","target":"branch","source_type":"Repository","source":"octo-org/autobot","enforcement":"active","node_id":"RRS_lACqUmVwb3NpdG9yec5SZK5vzgFr7tE","_links":{"self":{"href":"https://api.github.com/repos/octo-org/autobot/rulesets/23850705"},"html":{"href":"https://github.com/octo-org/autobot/rules/23850705"}},"created_at":"2026-09-22T23:19:37.951Z","updated_at":"2026-09-22T23:19:38.004Z"}]"#;

    // Recorded from the GitHub API (GET rulesets/23850705), with owner and logins replaced by
    // placeholders.
    const RULESET_DETAIL: &str = r#"{"id":23850705,"name":"main","target":"branch","source_type":"Repository","source":"octo-org/autobot","enforcement":"active","conditions":{"ref_name":{"exclude":[],"include":["~DEFAULT_BRANCH"]}},"rules":[{"type":"deletion"},{"type":"non_fast_forward"},{"type":"required_linear_history"},{"type":"pull_request","parameters":{"required_approving_review_count":0,"dismiss_stale_reviews_on_push":false,"required_reviewers":[],"require_code_owner_review":false,"require_last_push_approval":false,"required_review_thread_resolution":true,"require_extra_approval_for_unattributed_changes":true,"allowed_merge_methods":["squash"]}}],"node_id":"RRS_lACqUmVwb3NpdG9yec5SZK5vzgFr7tE","created_at":"2026-09-22T23:19:37.951Z","updated_at":"2026-09-22T23:19:38.004Z","bypass_actors":[],"current_user_can_bypass":"never","_links":{"self":{"href":"https://api.github.com/repos/octo-org/autobot/rulesets/23850705"},"html":{"href":"https://github.com/octo-org/autobot/rules/23850705"}}}"#;

    // Recorded from the GitHub API (GET of the repository), with owner and logins replaced by
    // placeholders, keeping the merge and feature fields.
    const REPO: &str = r#"{"id":1062540311,"name":"autobot","full_name":"octo-org/autobot","private":false,"default_branch":"main","has_issues":true,"has_projects":true,"has_wiki":false,"has_discussions":false,"allow_squash_merge":true,"allow_merge_commit":false,"allow_rebase_merge":false,"allow_auto_merge":true,"delete_branch_on_merge":true,"allow_update_branch":false,"use_squash_pr_title_as_default":true,"squash_merge_commit_title":"PR_TITLE","squash_merge_commit_message":"PR_BODY","merge_commit_title":"MERGE_MESSAGE","merge_commit_message":"PR_TITLE"}"#;

    fn parse(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    /// A repository that serves recorded responses and applies writes the way GitHub does,
    /// recording every write.
    struct FakeRepo {
        state: RefCell<BTreeMap<String, Value>>,
        writes: RefCell<Vec<(Method, String, Value)>>,
    }

    impl FakeRepo {
        fn new(list: Value, detail: Option<Value>, repo: Value) -> Self {
            let mut state = BTreeMap::new();
            state.insert("rulesets?includes_parents=false".to_owned(), list);
            if let Some(detail) = detail {
                state.insert(format!("rulesets/{}", detail["id"]), detail);
            }
            state.insert(String::new(), repo);
            Self {
                state: RefCell::new(state),
                writes: RefCell::new(Vec::new()),
            }
        }

        fn recorded() -> Self {
            Self::new(
                parse(RULESETS_LIST),
                Some(parse(RULESET_DETAIL)),
                parse(REPO),
            )
        }

        fn writes(&self) -> Vec<(Method, String, Value)> {
            self.writes.borrow().clone()
        }
    }

    impl Api for FakeRepo {
        fn request(&self, method: Method, path: &str, body: Option<&Value>) -> Result<Value> {
            let mut state = self.state.borrow_mut();
            if method == Method::Get {
                return state
                    .get(path)
                    .cloned()
                    .ok_or_else(|| Error::Http(format!("404 {path}")));
            }
            let body = body.cloned().unwrap_or(Value::Null);
            self.writes
                .borrow_mut()
                .push((method, path.to_owned(), body.clone()));
            match method {
                Method::Post => {
                    let mut created = body;
                    created["id"] = json!(99);
                    created["source_type"] = json!("Repository");
                    let key = "rulesets?includes_parents=false".to_owned();
                    let list = state.get_mut(&key).and_then(Value::as_array_mut).unwrap();
                    list.push(
                        json!({"id": 99, "name": created["name"], "source_type": "Repository"}),
                    );
                    state.insert("rulesets/99".to_owned(), created.clone());
                    Ok(created)
                }
                Method::Put => {
                    let mut current = state[path].clone();
                    for (k, v) in body.as_object().unwrap() {
                        current[k] = v.clone();
                    }
                    state.insert(path.to_owned(), current.clone());
                    Ok(current)
                }
                Method::Patch => {
                    let repo = state.get_mut(path).unwrap();
                    for (k, v) in body.as_object().unwrap() {
                        repo[k] = v.clone();
                    }
                    Ok(repo.clone())
                }
                Method::Get => unreachable!(),
            }
        }
    }

    #[test]
    fn committed_files_follow_the_policy() {
        validate_ruleset(&parse(MAIN_JSON)).unwrap();
        assert!(parse(COMMITTED_SETTINGS).is_object());
    }

    #[test]
    fn matching_repository_needs_no_change() {
        let api = FakeRepo::recorded();
        let plan = plan(&api, &parse(DESIRED_RULESET), &parse(SETTINGS_JSON)).unwrap();
        assert_eq!(plan.diff(), &[] as &[String]);
        assert_eq!(plan.ruleset_action(), &RulesetAction::Keep);
        apply(&api, &plan).unwrap();
        assert!(api.writes().is_empty());
    }

    #[test]
    fn missing_ruleset_is_created_then_left_alone() {
        let api = FakeRepo::new(json!([]), None, parse(REPO));
        let ruleset = parse(DESIRED_RULESET);
        let first = plan(&api, &ruleset, &parse(SETTINGS_JSON)).unwrap();
        assert_eq!(first.diff(), ["ruleset main: create"]);
        assert_eq!(first.ruleset_action(), &RulesetAction::Create);
        apply(&api, &first).unwrap();
        assert_eq!(
            api.writes(),
            [(Method::Post, "rulesets".to_owned(), ruleset.clone())]
        );

        let second = plan(&api, &ruleset, &parse(SETTINGS_JSON)).unwrap();
        assert!(second.diff().is_empty(), "{:?}", second.diff());
        apply(&api, &second).unwrap();
        assert_eq!(api.writes().len(), 1);
    }

    #[test]
    fn drifted_ruleset_and_settings_are_updated_then_left_alone() {
        let mut detail = parse(RULESET_DETAIL);
        pull_request(&mut detail)["parameters"]["required_approving_review_count"] = json!(1);
        detail["rules"].as_array_mut().unwrap().swap(0, 2);
        detail["enforcement"] = json!("evaluate");
        let mut repo = parse(REPO);
        repo["allow_update_branch"] = json!(true);
        repo["has_wiki"] = json!(true);
        let api = FakeRepo::new(parse(RULESETS_LIST), Some(detail), repo);
        let ruleset = parse(DESIRED_RULESET);
        let settings = parse(SETTINGS_JSON);

        let first = plan(&api, &ruleset, &settings).unwrap();
        assert_eq!(
            first.diff(),
            [
                r#"ruleset main: enforcement: "evaluate" -> "active""#,
                "ruleset main: rules[2].parameters.required_approving_review_count: 1 -> 0",
                "settings: allow_update_branch: true -> false",
                "settings: has_wiki: true -> false",
            ]
        );
        assert_eq!(first.ruleset_action(), &RulesetAction::Update(23_850_705));
        apply(&api, &first).unwrap();
        assert_eq!(
            api.writes(),
            [
                (Method::Put, "rulesets/23850705".to_owned(), ruleset.clone()),
                (
                    Method::Patch,
                    String::new(),
                    json!({"allow_update_branch": false, "has_wiki": false})
                ),
            ]
        );

        let second = plan(&api, &ruleset, &settings).unwrap();
        assert!(second.diff().is_empty(), "{:?}", second.diff());
        apply(&api, &second).unwrap();
        assert_eq!(api.writes().len(), 2);
    }

    #[test]
    fn extra_rule_on_github_is_a_difference() {
        let mut detail = parse(RULESET_DETAIL);
        detail["rules"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type": "creation"}));
        let api = FakeRepo::new(parse(RULESETS_LIST), Some(detail), parse(REPO));
        let plan = plan(&api, &parse(DESIRED_RULESET), &parse(SETTINGS_JSON)).unwrap();
        assert_eq!(plan.ruleset_action(), &RulesetAction::Update(23_850_705));
        assert_eq!(plan.diff().len(), 1);
        assert!(
            plan.diff()[0].starts_with("ruleset main: rules: "),
            "{:?}",
            plan.diff()
        );
    }

    fn rejection(edit: impl FnOnce(&mut Value)) -> String {
        let mut ruleset = parse(DESIRED_RULESET);
        edit(&mut ruleset);
        let api = FakeRepo::recorded();
        let err = plan(&api, &ruleset, &parse(SETTINGS_JSON)).unwrap_err();
        assert!(api.writes().is_empty());
        err.to_string()
    }

    fn pull_request(ruleset: &mut Value) -> &mut Value {
        ruleset["rules"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|r| r["type"] == "pull_request")
            .unwrap()
    }

    #[test]
    fn rejects_required_approvals() {
        let err = rejection(|r| {
            pull_request(r)["parameters"]["required_approving_review_count"] = json!(1);
        });
        assert!(err.contains("required_approving_review_count"), "{err}");
    }

    #[test]
    fn rejects_bypass_actors() {
        let err = rejection(|r| {
            r["bypass_actors"] =
                json!([{"actor_id": 5, "actor_type": "RepositoryRole", "bypass_mode": "always"}]);
        });
        assert!(err.contains("bypass_actors"), "{err}");
        let err = rejection(|r| {
            r.as_object_mut().unwrap().remove("bypass_actors");
        });
        assert!(err.contains("bypass_actors"), "{err}");
    }

    #[test]
    fn rejects_merge_methods_other_than_squash() {
        let err = rejection(|r| {
            pull_request(r)["parameters"]["allowed_merge_methods"] = json!(["squash", "merge"]);
        });
        assert!(err.contains("allowed_merge_methods"), "{err}");
    }

    #[test]
    fn rejects_missing_protection_rules() {
        for kind in [
            "deletion",
            "non_fast_forward",
            "required_linear_history",
            "pull_request",
        ] {
            let err = rejection(|r| {
                r["rules"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|rule| rule["type"] != kind);
            });
            assert!(err.contains(kind), "{kind}: {err}");
        }
    }

    fn with_checks(parameters: Value) -> impl FnOnce(&mut Value) {
        move |r: &mut Value| {
            r["rules"]
                .as_array_mut()
                .unwrap()
                .push(json!({"type": "required_status_checks", "parameters": parameters}));
        }
    }

    #[test]
    fn required_checks_must_come_from_github_actions_without_strict_policy() {
        let err = rejection(with_checks(json!({
            "strict_required_status_checks_policy": false,
            "required_status_checks": [{"context": "ci"}]
        })));
        assert!(err.contains("integration_id"), "{err}");
        let err = rejection(with_checks(json!({
            "strict_required_status_checks_policy": true,
            "required_status_checks": [{"context": "ci", "integration_id": GITHUB_ACTIONS_APP_ID}]
        })));
        assert!(
            err.contains("strict_required_status_checks_policy"),
            "{err}"
        );

        let mut ruleset = parse(DESIRED_RULESET);
        with_checks(json!({
            "strict_required_status_checks_policy": false,
            "required_status_checks": [{"context": "ci", "integration_id": GITHUB_ACTIONS_APP_ID}]
        }))(&mut ruleset);
        validate_ruleset(&ruleset).unwrap();
    }

    #[test]
    fn a_check_that_reads_a_secret_is_never_required() {
        for context in SECRET_CHECKS {
            let err = rejection(with_checks(json!({
                "strict_required_status_checks_policy": false,
                "required_status_checks": [
                    {"context": "ci", "integration_id": GITHUB_ACTIONS_APP_ID},
                    {"context": context, "integration_id": GITHUB_ACTIONS_APP_ID}
                ]
            })));
            assert!(
                err.contains(&format!("required check \"{context}\" reads a secret")),
                "{err}"
            );
        }
    }

    #[cfg(all(feature = "judge", feature = "hooks"))]
    #[test]
    fn the_secret_checks_are_the_workflow_jobs_that_read_a_secret() {
        assert!(SECRET_CHECKS.contains(&crate::sensitive::CONTEXT));
        for (context, workflow) in [
            (
                "judge",
                include_str!("../../../../.github/workflows/judge.yml"),
            ),
            (
                crate::sensitive::CONTEXT,
                include_str!("../../../../.github/workflows/sensitive-terms.yml"),
            ),
        ] {
            let jobs = workflow.split_once("\njobs:\n").unwrap().1;
            assert!(jobs.starts_with(&format!("  {context}:\n")), "{jobs}");
            assert!(jobs.contains("${{ secrets."), "{context}: {jobs}");
        }
    }

    #[test]
    fn settings_must_be_an_object() {
        let err = plan(&FakeRepo::recorded(), &parse(DESIRED_RULESET), &json!([])).unwrap_err();
        assert!(err.to_string().contains(SETTINGS_PATH), "{err}");
    }
}
