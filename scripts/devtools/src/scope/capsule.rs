//! What the `scope` check reads from a pull request body, a task issue and its comments.
//!
//! - The task issue is the first issue the pull request body links with `Closes`, `Fixes` or
//!   `Resolves` ([`linked_issue`]).
//! - A capsule section is the text under a bold line (`**Allowed paths**`) or a Markdown
//!   heading of that name, up to the next such line ([`section`]).
//! - The **Allowed paths** section holds paths and globs separated by commas, semicolons or
//!   line breaks; a list bullet and backticks around an entry are ignored. An entry that is
//!   not one glob ([`super::glob::parse`]), such as a path followed by a qualifier, allows
//!   nothing and is reported ([`allowed`]).
//! - A scope-extension comment starts with `Scope extension` (any case, after leading
//!   whitespace and Markdown emphasis or quote marks) and names its paths and globs in
//!   backticks: a backticked span counts when it holds a `/`, a `.` or a `*` and is a glob, so a
//!   word such as `scope` or `Justfile` in backticks does not ([`extension`]).

use super::glob;
use crate::markdown;

/// The number of the first issue `body` links with `Closes`, `Fixes` or `Resolves` (any case)
/// followed by `#N`.
#[must_use]
pub fn linked_issue(body: &str) -> Option<u64> {
    let lower = body.to_ascii_lowercase();
    let mut first: Option<(usize, u64)> = None;
    for keyword in ["closes #", "fixes #", "resolves #"] {
        let mut from = 0;
        while let Some(at) = lower[from..].find(keyword).map(|i| i + from) {
            let digits: String = lower[at + keyword.len()..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            from = at + keyword.len();
            if let Ok(n) = digits.parse()
                && first.is_none_or(|(pos, _)| at < pos)
            {
                first = Some((at, n));
                break;
            }
        }
    }
    first.map(|(_, n)| n)
}

/// The text of a capsule heading line: a bold line (`**Name**`) or a Markdown heading.
fn heading(line: &str) -> Option<&str> {
    let t = line.trim();
    let bold = t.len() > 4 && t.starts_with("**") && t.ends_with("**");
    let text = if bold {
        t.get(2..t.len() - 2)
    } else {
        markdown::heading_of(t).map(|(_, text)| text)
    };
    text.map(str::trim)
}

/// The text under the capsule heading `name` in `body`, up to the next capsule heading.
#[must_use]
pub fn section(body: &str, name: &str) -> Option<String> {
    let mut lines = body.lines();
    lines.by_ref().find(|l| heading(l) == Some(name))?;
    Some(
        lines
            .take_while(|l| heading(l).is_none())
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// The entries of an **Allowed paths** section.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Allowed {
    /// The entries that are globs, as [`glob::parse`] returns them.
    pub globs: Vec<String>,
    /// The entries that are not, as written; they allow nothing.
    pub ignored: Vec<String>,
}

/// The entries of the **Allowed paths** section text `text`.
#[must_use]
pub fn allowed(text: &str) -> Allowed {
    let mut out = Allowed::default();
    for entry in text.split([',', ';', '\n']) {
        let entry = entry.trim();
        let entry = ["- ", "* ", "+ "]
            .iter()
            .find_map(|bullet| entry.strip_prefix(bullet))
            .unwrap_or(entry)
            .trim();
        if entry.is_empty() {
            continue;
        }
        match glob::parse(&entry.replace('`', "")) {
            Some(g) => out.globs.push(g),
            None => out.ignored.push(entry.to_owned()),
        }
    }
    out
}

/// The globs a scope-extension comment names, or `None` when `comment` is not one.
#[must_use]
pub fn extension(comment: &str) -> Option<Vec<String>> {
    let lead = comment.trim_start_matches(|c: char| c.is_whitespace() || "*_#>".contains(c));
    let prefix = "scope extension";
    let starts = lead
        .get(..prefix.len())
        .is_some_and(|p| p.eq_ignore_ascii_case(prefix));
    if !starts {
        return None;
    }
    Some(
        comment
            .split('`')
            .skip(1)
            .step_by(2)
            .filter(|span| span.contains(['/', '.', '*']))
            .filter_map(glob::parse)
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linked_issue_takes_the_first_closing_keyword() {
        assert_eq!(linked_issue("Closes #315\n\nFixes #2"), Some(315));
        assert_eq!(linked_issue("see #4; fixes #12, closes #13"), Some(12));
        assert_eq!(linked_issue("RESOLVES #7"), Some(7));
        assert_eq!(linked_issue("closes #x, closes #8"), Some(8));
        assert_eq!(linked_issue("refs #5"), None);
    }

    // The capsule of #363 as filed, trimmed to the sections read here.
    const CAPSULE: &str = "**Objective**\nMove three entries.\n\n**Allowed paths**\n\
        CHARTER.md, CONTRIBUTING.md, Justfile, .github/workflows/**, scripts/*.rs\n\n\
        **Non-goals**\nNo change to the review checklist.\n";

    #[test]
    fn the_allowed_paths_section_ends_at_the_next_heading() {
        assert_eq!(
            section(CAPSULE, "Allowed paths").as_deref(),
            Some("CHARTER.md, CONTRIBUTING.md, Justfile, .github/workflows/**, scripts/*.rs\n")
        );
        let heading = "## Allowed paths\n- `a/**`\n## Non-goals\nb/**\n";
        assert_eq!(
            section(heading, "Allowed paths").as_deref(),
            Some("- `a/**`")
        );
        assert_eq!(section(CAPSULE, "Owns"), None);
    }

    #[test]
    fn allowed_splits_entries_and_ignores_qualified_ones() {
        let got = allowed(
            "- `crates/k/src/**`, Cargo.toml (the `members` entry)\n\
             * docs/a.md; crates/c/src/lib.rs (its `all_controllers()` entry only)\n\
             docs/design/X*.md §1\n\n**",
        );
        assert_eq!(got.globs, ["crates/k/src/**", "docs/a.md"]);
        assert_eq!(
            got.ignored,
            [
                "Cargo.toml (the `members` entry)",
                "crates/c/src/lib.rs (its `all_controllers()` entry only)",
                "docs/design/X*.md §1",
                "**",
            ]
        );
    }

    #[test]
    fn an_extension_names_its_globs_in_backticks() {
        // Shaped like the scope-extension comments filed on #317 and #392.
        let got = extension(
            "Scope extension: this task may also edit the `MODEL` constant in \
             `scripts/devtools/src/judge.rs` and `crates/a/src/**`; not `#[ignore = \"x\"]` or `**`.",
        );
        assert_eq!(
            got.unwrap(),
            ["scripts/devtools/src/judge.rs", "crates/a/src/**"]
        );
        assert_eq!(
            extension("**Scope extension** `a/b.rs`").unwrap(),
            ["a/b.rs"]
        );
        assert_eq!(
            extension("> scope extension: `a/b.rs`").unwrap(),
            ["a/b.rs"]
        );
        assert_eq!(extension("Not a scope extension: `a/b.rs`"), None);
        assert_eq!(extension("Scope"), None);
        assert_eq!(
            extension("Scope extension: `a.rs`, but not `Justfile` or `scope`").unwrap(),
            ["a.rs"]
        );
    }
}
