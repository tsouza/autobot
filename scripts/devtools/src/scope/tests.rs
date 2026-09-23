use super::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Serves recorded GET responses by path and fails any other path with 404, the way the
/// client reports GitHub's answer for an unknown resource.
struct Recorded(BTreeMap<String, Value>);

impl Api for Recorded {
    fn request(&self, method: Method, path: &str, _body: Option<&Value>) -> Result<Value> {
        assert_eq!(method, Method::Get, "the scope check only reads");
        self.0
            .get(path)
            .cloned()
            .ok_or_else(|| Error::Http(format!("Get {path}: http status: 404")))
    }
}

const FILES: &str = "pulls/40/files?per_page=100&page=1";
const COMMENTS: &str = "issues/9/comments?per_page=100&page=1";

/// A pull request #40 that closes task #9, whose Allowed paths are those of #363, changing
/// `files`; shaped like GET pulls/{n}, pulls/{n}/files, issues/{n} and issues/{n}/comments,
/// keeping the fields read.
fn github(files: Value) -> Recorded {
    Recorded(BTreeMap::from([
        (
            "pulls/40".to_owned(),
            json!({"number": 40, "title": "feat(devtools): add the scope check", "body": "Closes #9\n\n## Resolution\n"}),
        ),
        (FILES.to_owned(), files),
        (
            "issues/9".to_owned(),
            json!({
                "number": 9,
                "labels": [{"name": "type:task"}, {"name": "human-lane"}],
                "body": "**Objective**\nx\n\n**Allowed paths**\nCHARTER.md, CONTRIBUTING.md, \
                    Justfile, .github/workflows/**, scripts/*.rs, scripts/devtools/src/lib.rs, \
                    scripts/devtools/src/scope/**, CONTRIBUTING.md (one paragraph)\n\n\
                    **Non-goals**\nscripts/README.md\n",
            }),
        ),
        (COMMENTS.to_owned(), json!([])),
    ]))
}

fn file(path: &str, status: &str, patch: &str) -> Value {
    json!({"filename": path, "status": status, "changes": patch.lines().count(), "patch": patch})
}

fn comment(association: &str, body: &str) -> Value {
    json!({"author_association": association, "body": body})
}

fn seeded() -> Value {
    json!([
        file("CHARTER.md", "modified", "@@ -1 +1 @@\n-a\n+b"),
        file(
            ".github/workflows/scope.yml",
            "added",
            "@@ -0,0 +1 @@\n+name: scope"
        ),
        file("scripts/README.md", "modified", "@@ -1 +1 @@\n-a\n+b"),
    ])
}

#[test]
fn a_seeded_out_of_scope_path_fails_until_a_scope_extension_names_it() {
    let mut api = github(seeded());
    let verdict = evaluate(&api, 40).unwrap();
    assert_eq!(verdict.outside, ["scripts/README.md"]);
    assert!(!verdict.passes());
    let text = verdict.to_string();
    assert!(
        text.contains("scripts/README.md: outside the allowed paths of #9\n"),
        "{text}"
    );
    assert!(
        text.contains("scope: #40 changes 1 path(s) outside the allowed paths of #9"),
        "{text}"
    );
    assert!(
        text.contains("the Allowed paths entry `CONTRIBUTING.md (one paragraph)` of #9"),
        "{text}"
    );

    api.0.insert(
        COMMENTS.to_owned(),
        json!([
            comment("OWNER", "Looks right."),
            comment(
                "OWNER",
                "Scope extension: also `scripts/README.md` (the feature table row)."
            ),
        ]),
    );
    let verdict = evaluate(&api, 40).unwrap();
    assert_eq!(verdict.outside, Vec::<String>::new());
    assert!(verdict.passes());
    let text = verdict.to_string();
    assert!(
        text.contains("scope extensions on #9 allow scripts/README.md\n"),
        "{text}"
    );
    assert!(
        text.contains("every changed path GitHub lists for #40 is allowed by #9"),
        "{text}"
    );
}

#[test]
fn a_scope_extension_by_an_outsider_is_not_counted() {
    let mut api = github(seeded());
    api.0.insert(
        COMMENTS.to_owned(),
        json!([comment(
            "NONE",
            "Scope extension: `**/*.md`, `scripts/README.md`"
        )]),
    );
    let verdict = evaluate(&api, 40).unwrap();
    assert_eq!(verdict.outside, ["scripts/README.md"]);
    assert!(
        verdict.to_string().contains(
            "a scope-extension comment on #9 by an author with association NONE is not counted"
        ),
        "{verdict}"
    );
}

#[test]
fn a_pull_request_without_a_task_issue_fails() {
    let mut unlinked = github(seeded());
    unlinked
        .0
        .insert("pulls/40".to_owned(), json!({"body": null}));
    let mut missing = github(seeded());
    missing.0.remove("issues/9");
    let mut pull = github(seeded());
    pull.0.insert(
        "issues/9".to_owned(),
        json!({"labels": [], "pull_request": {"url": "x"}}),
    );
    let mut finding = github(seeded());
    finding.0.insert(
        "issues/9".to_owned(),
        json!({"labels": [{"name": "finding"}], "body": "**Allowed paths**\n**/*"}),
    );
    for (api, why) in [
        (unlinked, NoTask::Unlinked),
        (missing, NoTask::Missing(9)),
        (pull, NoTask::PullRequest(9)),
        (finding, NoTask::NotATask(9)),
    ] {
        let verdict = evaluate(&api, 40).unwrap();
        assert_eq!(verdict.task, Err(why.clone()));
        assert!(!verdict.passes(), "{why}");
        assert_eq!(
            verdict.to_string(),
            format!("scope: #40 closes no task issue: {why}\n")
        );
    }
}

#[test]
fn a_failed_read_other_than_a_missing_issue_is_an_error() {
    let mut api = github(seeded());
    api.0.remove(COMMENTS);
    assert!(evaluate(&api, 40).is_err());
    let mut api = github(seeded());
    api.0.remove(FILES);
    assert!(evaluate(&api, 40).is_err());
}

#[test]
fn a_rename_needs_both_its_paths_allowed() {
    let api = github(json!([{
        "filename": "scripts/devtools/src/scope/x.rs",
        "previous_filename": "scripts/devtools/src/hooks.rs",
        "status": "renamed",
        "changes": 0
    }]));
    assert_eq!(
        evaluate(&api, 40).unwrap().outside,
        ["scripts/devtools/src/hooks.rs"]
    );
}

#[test]
fn a_file_list_at_github_s_limit_fails() {
    let page = |range: std::ops::Range<usize>| -> Value {
        range
            .map(|i| file(&format!("scripts/s{i}.rs"), "added", "+x"))
            .collect()
    };
    let mut api = github(json!([]));
    for p in 0..30 {
        api.0.insert(
            format!("pulls/40/files?per_page=100&page={}", p + 1),
            page(p * 100..(p + 1) * 100),
        );
    }
    api.0
        .insert("pulls/40/files?per_page=100&page=31".to_owned(), json!([]));
    let verdict = evaluate(&api, 40).unwrap();
    assert!(verdict.cut && verdict.outside.is_empty() && !verdict.passes());
    assert!(
        verdict
            .to_string()
            .contains("GitHub lists at most 3000 files"),
        "{verdict}"
    );
}

/// The paths of `files` that task #9 of [`github`] does not allow.
fn outside_of(files: Value) -> Vec<String> {
    evaluate(&github(files), 40).unwrap().outside
}

#[test]
fn the_lock_file_and_the_root_manifest_are_always_inherited() {
    let files = json!([
        file("Cargo.lock", "modified", "@@ -1 +1 @@\n-a\n+b"),
        file("Cargo.toml", "modified", "@@ -1 +1 @@\n-a\n+b"),
    ]);
    assert_eq!(outside_of(files), Vec::<String>::new());
}

#[test]
fn a_crate_manifest_is_inherited_only_beside_an_allowed_change_in_its_crate() {
    let manifest = file(
        "scripts/devtools/Cargo.toml",
        "modified",
        "@@ -1 +1 @@\n-a\n+b",
    );
    let own = file("scripts/devtools/src/scope/mod.rs", "added", "+//! x");
    assert_eq!(
        outside_of(json!([manifest, own.clone()])),
        Vec::<String>::new()
    );
    let other = file("crates/k/Cargo.toml", "modified", "@@ -1 +1 @@\n-a\n+b");
    assert_eq!(outside_of(json!([other, own])), ["crates/k/Cargo.toml"]);
}

#[test]
fn a_parent_module_is_inherited_only_for_added_declarations_of_added_children() {
    let child = |path: &str| file(path, "added", "@@ -0,0 +1 @@\n+//! x");
    let parent = |path: &str, patch: &str| file(path, "modified", patch);
    let mut task = github(json!([]));
    task.0.insert(
        "issues/9".to_owned(),
        json!({"labels": [{"name": "type:task"}], "body": "**Allowed paths**\na/src/scope/**, a/src/b/c.rs, a/src/b/d.rs"}),
    );
    let mut outside = |files: Value| {
        task.0.insert(FILES.to_owned(), files);
        evaluate(&task, 40).unwrap().outside
    };
    let none = Vec::<String>::new();
    // Valid: a `mod.rs` child is declared in the directory above, a named file beside its
    // siblings, with its doc comment and a blank line; two children in two places.
    let scope = "@@ -5,2 +5,5 @@\n pub mod markdown;\n+/// The scope check.\n+pub mod scope;\n+\n pub mod worktree;\n";
    assert_eq!(
        outside(json!([
            child("a/src/scope/mod.rs"),
            parent("a/src/lib.rs", scope)
        ])),
        none
    );
    let two = "@@ -1,2 +1,4 @@\n+mod c;\n pub mod a;\n+pub(crate) mod d;\n pub mod e;\n";
    for declaring in ["a/src/b.rs", "a/src/b/mod.rs"] {
        assert_eq!(
            outside(json!([
                child("a/src/b/c.rs"),
                child("a/src/b/d.rs"),
                parent(declaring, two)
            ])),
            none,
            "{declaring}"
        );
    }
    // Each bypass: removing another module, compiling an existing one out, removing a test
    // gate, declaring a module the pull request does not add, and an attribute on the child.
    let lib = ["a/src/lib.rs"];
    for patch in [
        "@@ -1,2 +1,2 @@\n-pub mod billing;\n+pub mod scope;\n",
        "@@ -1,2 +1,4 @@\n+pub mod scope;\n pub mod a;\n+#[cfg(any())]\n pub mod billing;\n",
        "@@ -1,3 +1,3 @@\n+pub mod scope;\n pub mod a;\n-#[cfg(test)]\n mod tests;\n",
        "@@ -1,1 +1,3 @@\n pub mod a;\n+pub mod scope;\n+pub mod unrelated;\n",
        "@@ -1,1 +1,3 @@\n pub mod a;\n+#[cfg(feature = \"github\")]\n+pub mod scope;\n",
        "@@ -1,1 +1,3 @@\n pub mod a;\n+pub mod scope;\n+pub fn b() {}\n",
        "@@ -1,1 +1,2 @@\n pub mod a;\n+/// A comment alone.\n",
    ] {
        assert_eq!(
            outside(json!([
                child("a/src/scope/mod.rs"),
                parent("a/src/lib.rs", patch)
            ])),
            lib,
            "{patch}"
        );
    }
    // A parent of nothing added, and a child the task does not allow.
    assert_eq!(outside(json!([parent("a/src/lib.rs", scope)])), lib);
    let other = "@@ -1 +1,2 @@\n pub mod a;\n+pub mod other;\n";
    assert_eq!(
        outside(json!([
            child("a/src/other/mod.rs"),
            parent("a/src/lib.rs", other)
        ])),
        ["a/src/lib.rs", "a/src/other/mod.rs"]
    );
}

#[test]
fn the_registry_is_inherited_beside_an_allowed_controller_change() {
    let mut api = github(json!([
        file(REGISTRY, "modified", "@@ -1 +1 @@\n-a\n+b"),
        file(
            "crates/autobot-controllers/src/intake/brief.rs",
            "added",
            "+x"
        ),
    ]));
    api.0.insert(
        "issues/9".to_owned(),
        json!({"labels": [{"name": "type:task"}], "body": "**Allowed paths**\ncrates/autobot-controllers/src/intake/brief.rs"}),
    );
    assert_eq!(evaluate(&api, 40).unwrap().outside, Vec::<String>::new());
    assert_eq!(
        outside_of(json!([file(REGISTRY, "modified", "@@ -1 +1 @@\n-a\n+b")])),
        [REGISTRY]
    );
}

#[test]
fn a_gate_group_is_inherited_only_for_removing_the_task_s_own_ignore_lines() {
    let path = "crates/autobot-kernel/tests/g_commit/tests.rs";
    let removal = |n: u64| {
        format!(
            "@@ -12,5 +12,3 @@\n #[test]\n-#[ignore = \"awaiting #{n}\"]\n fn f1() {{}}\n #[test]\n-    #[ignore = \"awaiting #{n}\"]\n"
        )
    };
    assert_eq!(
        outside_of(json!([file(path, "modified", &removal(9))])),
        Vec::<String>::new()
    );
    assert_eq!(
        outside_of(json!([file(path, "modified", &removal(83))])),
        [path]
    );
    let edit = format!("{}+fn f2() {{}}\n", removal(9));
    assert_eq!(outside_of(json!([file(path, "modified", &edit)])), [path]);
    assert_eq!(outside_of(json!([file(path, "modified", "")])), [path]);
    assert_eq!(
        outside_of(json!([{"filename": path, "status": "modified", "changes": 2}])),
        [path]
    );
}

#[test]
fn a_declaration_is_a_plain_or_pub_mod_line() {
    for (line, name) in [
        ("mod a;", "a"),
        ("pub mod a_b;", "a_b"),
        ("  pub(crate) mod x1 ;", "x1"),
    ] {
        assert_eq!(declared_module(line), Some(name), "{line:?}");
    }
    for line in [
        "mod a",
        "pub mod a { }",
        "pub(in crate::a) mod x;",
        "pub(super) mod x;",
        "#[cfg(test)]",
        "mod ;",
        "use a::b;",
    ] {
        assert_eq!(declared_module(line), None, "{line:?}");
    }
    assert_eq!(module_name("a/src/scope/mod.rs"), Some("scope"));
    assert_eq!(module_name("a/src/b/c.rs"), Some("c"));
    assert_eq!(module_name("a/src/b/c.md"), None);
}

#[test]
fn the_workflow_runs_the_check_from_the_default_branch_and_never_cancels_a_run() {
    const WORKFLOW: &str = include_str!("../../../../.github/workflows/scope.yml");
    let jobs = WORKFLOW.split_once("\njobs:\n").unwrap().1;
    assert!(jobs.starts_with(&format!("  {CONTEXT}:\n")), "{jobs}");
    assert!(
        jobs.contains("ref: ${{ github.event.repository.default_branch }}"),
        "{jobs}"
    );
    let runs: Vec<&str> = jobs
        .lines()
        .filter_map(|l| l.trim().strip_prefix("- run: "))
        .collect();
    assert_eq!(runs, ["just toolchain", "just scope \"$PR\""]);
    assert!(!WORKFLOW.contains("concurrency:"), "{WORKFLOW}");
    assert!(!WORKFLOW.contains("secrets."), "{WORKFLOW}");
}
