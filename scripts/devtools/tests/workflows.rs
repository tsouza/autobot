//! Every `run:` step of every workflow is a `just` recipe (GHA -> just -> scripts).
//!
//! The check is strict rather than a YAML parse: a step passes only when its `run` key is
//! written plainly as `run: just …` on one line. Anywhere else on a line, the text `run`
//! followed by a `:` key indicator is reported unless a plain-scalar character (a letter, a
//! digit, `-`, `_`, `.` or `/`) comes right before it. That covers a `run` key inside a flow
//! mapping or a flow sequence, after a tag or an anchor, and after any other delimiter. An
//! explicit `?` key is reported, and so is a `run` value that continues onto a more indented
//! line. Every quoted key and every alias key is reported whatever it spells, because an
//! escape (`"\x72un"`) or an alias can spell `run` without containing that text.

use std::path::Path;

/// Shell syntax that chains, pipes, backgrounds or substitutes commands.
const SHELL_CONTROL: [&str; 5] = [";", "|", "&", "`", "$("];

/// Whether `value` is one `just …` command with no other command beside it.
fn is_single_just(value: &str) -> bool {
    (value.starts_with("just ") || value == "just")
        && !SHELL_CONTROL.iter().any(|op| value.contains(op))
}

/// Number of leading whitespace bytes in `line`.
fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// The column of the plain `run:` key on this line and the value after it, if the line
/// holds one. Any number of `-` sequence markers, each followed by whitespace, may precede it.
fn plain_run(line: &str) -> Option<(usize, &str)> {
    let mut body = line.trim_start();
    while let Some(rest) = body.strip_prefix('-') {
        if !rest.starts_with(char::is_whitespace) {
            break;
        }
        body = rest.trim_start();
    }
    let value = body.strip_prefix("run")?.trim_start().strip_prefix(':')?;
    if !value.is_empty() && !value.starts_with(char::is_whitespace) {
        return None;
    }
    Some((line.len() - body.len(), value.trim()))
}

/// Whether the text right after `at` in `line` is a `:` key indicator, after optional spaces.
fn key_follows(line: &str, at: usize) -> bool {
    line[at..].trim_start().starts_with(':')
}

/// Whether the line holds a quoted key or an alias key (`*name :`), whatever it spells.
fn quoted_or_alias_key(line: &str) -> bool {
    let quoted = line
        .match_indices(['"', '\''])
        .any(|(at, _)| key_follows(line, at + 1));
    let alias = line.match_indices('*').any(|(at, _)| {
        let name = line[at + 1..]
            .find(|c: char| c.is_whitespace() || ":,{}[]".contains(c))
            .map_or(line.len(), |end| at + 1 + end);
        name > at + 1 && key_follows(line, name)
    });
    quoted || alias
}

/// Whether `c` can sit inside a plain scalar next to `run` without ending it, so that the
/// `run` it touches is part of a longer key such as `dry-run`.
fn plain_scalar_char(c: char) -> bool {
    c.is_alphanumeric() || "-_./".contains(c)
}

/// Whether the line holds a key that is or may be `run` in any spelling other than the plain
/// one: quoted, an alias, after an explicit `?` key indicator, or a `run` key after any
/// character that ends a plain scalar (whitespace, `{`, `[`, `,`, a tag or an anchor).
fn other_run_key(line: &str) -> bool {
    if line
        .trim_start()
        .trim_start_matches(['-', ' '])
        .starts_with('?')
        || quoted_or_alias_key(line)
    {
        return true;
    }
    line.match_indices("run").any(|(at, _)| {
        let before = line[..at].chars().next_back();
        before.is_none_or(|c| !plain_scalar_char(c)) && key_follows(line, at + 3)
    })
}

/// The `run:` steps in `text` that are not a single `just …` command on one line.
fn offending_steps(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut offending = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let Some((column, value)) = plain_run(line) else {
            if other_run_key(line) {
                offending.push(line.trim().to_owned());
            }
            continue;
        };
        let continued: Vec<&str> = lines[i + 1..]
            .iter()
            .filter(|next| !next.trim().is_empty())
            .take_while(|next| indent(next) > column)
            .map(|next| next.trim())
            .collect();
        if !continued.is_empty() {
            offending.push(format!("{value} {}", continued.join(" ")).trim().to_owned());
        } else if !is_single_just(value) {
            offending.push(value.to_owned());
        }
    }
    offending
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
        vec!["rustup toolchain install", "| just ci", "cargo test"]
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

#[test]
fn run_values_continued_on_the_next_line_are_found() {
    let text = "      - run: just ci\n          && cargo test\n      - name: x\n";
    assert_eq!(offending_steps(text), vec!["just ci && cargo test"]);
    let blank_then_more = "      - run: just ci\n\n          cargo test\n";
    assert_eq!(offending_steps(blank_then_more), vec!["just ci cargo test"]);
    let value_below_key = "      - run:\n          cargo test\n";
    assert_eq!(offending_steps(value_below_key), vec!["cargo test"]);
    let next_key_same_column = "      - run: just ci\n        env:\n          A: b\n";
    assert!(offending_steps(next_key_same_column).is_empty());
}

#[test]
fn run_keys_in_other_spellings_are_found() {
    let spelled = [
        "      -   run: cargo test",
        "      - - run: cargo test",
        "      - {run: cargo test}",
        "      - { name: x, run: just ci }",
        "      - \"run\": cargo test",
        "      - 'run' : just ci",
        "      - ? run",
        "      - \"\\x72un\": cargo test",
        "      - \"r\\u0075n\": cargo test",
        "      - {\"\\x72un\": cargo test}",
        "      - !!str run: cargo test",
        "      - &k run: cargo test",
        "      - *k : cargo test",
        "    steps: [run: cargo test]",
        "    steps: [name: x, run: cargo test]",
        "      - [run: cargo test]",
    ];
    for line in spelled {
        assert_eq!(offending_steps(&format!("{line}\n")).len(), 1, "{line}");
    }
    let unrelated = "  dry-run:\n    runs-on: x\n      run-url:\n      pre_run: x\n      a.run: x\n      a/run: x\n# a push or scheduled run\n";
    assert!(offending_steps(unrelated).is_empty());
    let quoted_values = "      - run: just label-gate \"$PR\"\n        if: github.ref == 'main'\n";
    assert!(offending_steps(quoted_values).is_empty());
}
