//! Every `run:` step of every workflow is a `just` recipe (GHA -> just -> scripts).

use std::path::Path;

/// The value of a `run:` key on this line, if the line holds one.
fn run_value(line: &str) -> Option<&str> {
    let key = line.trim_start().trim_start_matches("- ");
    key.strip_prefix("run:").map(str::trim)
}

/// Shell syntax that chains, pipes, backgrounds or substitutes commands.
const SHELL_CONTROL: [&str; 5] = [";", "|", "&", "`", "$("];

/// Whether `value` is one `just …` command with no other command beside it.
fn is_single_just(value: &str) -> bool {
    (value.starts_with("just ") || value == "just")
        && !SHELL_CONTROL.iter().any(|op| value.contains(op))
}

/// The `run:` steps in `text` that are not a single `just …` command.
fn offending_steps(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(run_value)
        .filter(|value| !is_single_just(value))
        .map(str::to_owned)
        .collect()
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
fn raw_multiline_and_non_just_steps_are_found() {
    let text = "jobs:\n  a:\n    steps:\n      - run: just ci\n      - run: rustup toolchain install\n      - run: |\n          just ci\n      - name: x\n        run: cargo test\n";
    assert_eq!(
        offending_steps(text),
        vec!["rustup toolchain install", "|", "cargo test"]
    );
    assert!(offending_steps("      - run: just toolchain\n").is_empty());
    assert!(offending_steps("      - run: just label-gate \"$PR\"\n").is_empty());
}

#[test]
fn just_steps_chained_with_shell_commands_are_found() {
    let chained = [
        "just ci && cargo test",
        "just ci || cargo test",
        "just ci; rustup toolchain install",
        "just x | sh",
        "just ci & cargo test",
        "just ci `cargo test`",
        "just ci $(cargo test)",
    ];
    for value in chained {
        assert_eq!(
            offending_steps(&format!("      - run: {value}\n")),
            vec![value],
            "{value}"
        );
    }
}
