//! Every `run:` step of every workflow is a single `just` recipe (GHA -> just -> scripts).
//!
//! This guards against a raw command being added to a workflow by mistake; workflow edits are
//! human-lane and reviewed, so it does not try to resist deliberate evasion. The workflows are
//! parsed as YAML, so how a step or its `run` key is spelled does not matter: every step of
//! every job is checked by value.

use std::path::Path;
use yaml_rust2::{Yaml, YamlLoader};

/// Shell syntax that chains, pipes, backgrounds, redirects or substitutes commands.
const SHELL_CONTROL: [&str; 7] = [";", "|", "&", "`", "$(", ">", "<"];

/// Whether `value` is one `just …` command with no other command beside it.
fn is_single_just(value: &str) -> bool {
    (value.starts_with("just ") || value == "just")
        && !value.contains('\n')
        && !SHELL_CONTROL.iter().any(|op| value.contains(op))
}

/// The `run` values of every step of every job in `text` that are not a single `just` command.
fn offending_steps(text: &str) -> Vec<String> {
    let docs = YamlLoader::load_from_str(text).expect("workflow is valid YAML");
    let mut out = Vec::new();
    for doc in &docs {
        let Some(jobs) = doc["jobs"].as_hash() else {
            continue;
        };
        for job in jobs.values() {
            for step in job["steps"].as_vec().into_iter().flatten() {
                match &step["run"] {
                    Yaml::BadValue => {}
                    Yaml::String(value) if is_single_just(value.trim_end_matches('\n')) => {}
                    other => out.push(format!("{other:?}")),
                }
            }
        }
    }
    out
}

#[test]
fn every_workflow_run_step_is_a_just_recipe() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows");
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "yml" || e == "yaml") {
            let offending = offending_steps(&std::fs::read_to_string(&path).unwrap());
            assert!(offending.is_empty(), "{}: {offending:?}", path.display());
            checked += 1;
        }
    }
    assert!(checked > 0, "no workflow found in {}", dir.display());
}

#[test]
fn raw_chained_and_multiline_steps_are_found() {
    let text = r#"
jobs:
  a:
    steps:
      - run: just ci
      - run: just label-gate "$PR"
      - run: rustup toolchain install
      - run: just ci && cargo test
      - run: |
          just ci
          cargo test
      - run: |
          just ci
  b:
    steps: [{run: cargo test}, {"r\x75n": cargo build}]
"#;
    assert_eq!(
        offending_steps(text).len(),
        5,
        "{:?}",
        offending_steps(text)
    );
}

#[test]
fn single_just_commands_pass() {
    for ok in [
        "just",
        "just ci",
        "just label-gate \"$PR\"",
        "just main-red --dry-run \"$RUN_URL\"",
    ] {
        assert!(is_single_just(ok), "{ok}");
    }
    for bad in [
        "just ci; ls",
        "just ci | sh",
        "just x > f",
        "just $(id)",
        "cargo test",
        "just ci\nls",
    ] {
        assert!(!is_single_just(bad), "{bad}");
    }
}
