//! The pull request title check behind `just pr-title`: a title is a
//! [Conventional Commits 1.0.0](https://www.conventionalcommits.org/en/v1.0.0/) header, since
//! merges are squash-only and the title becomes the commit on `main`.
//!
//! The grammar, in the order the parts appear:
//!
//! 1. **type**: one or more ASCII letters. Any noun is accepted, in any case, because the
//!    specification allows types other than `feat` and `fix` and treats every unit except
//!    `BREAKING CHANGE` as case-insensitive; no list of types is enforced.
//! 2. **scope** (optional): `(`, one or more characters that are neither whitespace nor
//!    parentheses, `)`.
//! 3. **`!`** (optional): marks a breaking change.
//! 4. `:` followed by exactly one space.
//! 5. **description**: non-empty, starting with a character that is not whitespace, and
//!    without line breaks.
//!
//! Only the title is checked; commit messages inside the branch are not, because the squash
//! merge replaces them with the title.

use crate::{Error, Result};
use std::fmt;
use std::process::ExitCode;

/// A title that follows the grammar, split into its parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header<'a> {
    /// The type, such as `feat` or `chore`.
    pub kind: &'a str,
    /// The scope without its parentheses, when there is one.
    pub scope: Option<&'a str>,
    /// Whether the `!` breaking-change marker is present.
    pub breaking: bool,
    /// The text after `: `.
    pub description: &'a str,
}

/// Why a title does not follow the grammar: the first rule it breaks, reading left to right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Invalid {
    /// The title does not start with an ASCII letter.
    MissingType,
    /// The type is followed by a character that cannot come after it.
    BadTypeEnd,
    /// A `(` opens a scope that no `)` closes.
    UnclosedScope,
    /// The parentheses enclose nothing.
    EmptyScope,
    /// The scope contains whitespace or a parenthesis.
    BadScope,
    /// No `:` follows the type, scope and `!`.
    MissingColon,
    /// The `:` is not followed by exactly one space and then a character that is not
    /// whitespace.
    BadSeparator,
    /// Nothing follows `: `.
    EmptyDescription,
    /// The title contains a line break.
    LineBreak,
}

impl fmt::Display for Invalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MissingType => "it does not start with a type of ASCII letters",
            Self::BadTypeEnd => "the type is not followed by `(scope)`, `!` or `: `",
            Self::UnclosedScope => "the scope's `(` has no closing `)`",
            Self::EmptyScope => "the scope is empty",
            Self::BadScope => "the scope contains whitespace or a parenthesis",
            Self::MissingColon => "the type, scope and `!` are not followed by `:`",
            Self::BadSeparator => "the `:` is not followed by exactly one space",
            Self::EmptyDescription => "the description after `: ` is empty",
            Self::LineBreak => "it contains a line break",
        })
    }
}

/// Splits `title` into its parts, or names the first rule of the grammar it breaks.
///
/// # Errors
/// Returns the [`Invalid`] reason when `title` does not follow the grammar.
pub fn parse(title: &str) -> std::result::Result<Header<'_>, Invalid> {
    if title.contains(['\n', '\r']) {
        return Err(Invalid::LineBreak);
    }
    let type_len = title
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(title.len());
    if type_len == 0 {
        return Err(Invalid::MissingType);
    }
    let (kind, mut rest) = title.split_at(type_len);

    let mut scope = None;
    if let Some(after_paren) = rest.strip_prefix('(') {
        let Some(close) = after_paren.find(')') else {
            return Err(Invalid::UnclosedScope);
        };
        let inner = &after_paren[..close];
        if inner.is_empty() {
            return Err(Invalid::EmptyScope);
        }
        if inner.contains(|c: char| c.is_whitespace() || c == '(') {
            return Err(Invalid::BadScope);
        }
        scope = Some(inner);
        rest = &after_paren[close + 1..];
    }

    let breaking = rest.starts_with('!');
    if breaking {
        rest = &rest[1..];
    }

    let Some(after_colon) = rest.strip_prefix(':') else {
        return Err(if scope.is_none() && !breaking && !rest.is_empty() {
            Invalid::BadTypeEnd
        } else {
            Invalid::MissingColon
        });
    };
    let Some(description) = after_colon.strip_prefix(' ') else {
        return Err(if after_colon.is_empty() {
            Invalid::EmptyDescription
        } else {
            Invalid::BadSeparator
        });
    };
    if description.is_empty() {
        return Err(Invalid::EmptyDescription);
    }
    if description.starts_with(char::is_whitespace) {
        return Err(if description.trim().is_empty() {
            Invalid::EmptyDescription
        } else {
            Invalid::BadSeparator
        });
    }
    Ok(Header {
        kind,
        scope,
        breaking,
        description,
    })
}

/// Entry point of `scripts/pr_title.rs`: checks the title that is the only argument, prints
/// the verdict and returns the exit code.
///
/// # Errors
/// Fails when `args` does not hold exactly one element.
pub fn run(args: impl IntoIterator<Item = String>) -> Result<ExitCode> {
    let mut args = args.into_iter();
    let (Some(title), None) = (args.next(), args.next()) else {
        return Err(Error::Parse("usage: pr_title <title>".to_owned()));
    };
    match parse(&title) {
        Ok(_) => {
            println!("pr-title: `{title}` is a Conventional Commits header");
            Ok(ExitCode::SUCCESS)
        }
        Err(reason) => {
            println!("pr-title: `{title}` is not a Conventional Commits header: {reason}");
            Ok(ExitCode::FAILURE)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header<'a>(
        kind: &'a str,
        scope: Option<&'a str>,
        breaking: bool,
        description: &'a str,
    ) -> Header<'a> {
        Header {
            kind,
            scope,
            breaking,
            description,
        }
    }

    #[test]
    fn valid_titles_split_into_their_parts() {
        let cases = [
            (
                "chore(deps): bump serde_json from 1.0.150 to 1.0.151",
                header(
                    "chore",
                    Some("deps"),
                    false,
                    "bump serde_json from 1.0.150 to 1.0.151",
                ),
            ),
            (
                "chore(deps-dev): bump the cargo group across 1 directory with 3 updates",
                header(
                    "chore",
                    Some("deps-dev"),
                    false,
                    "bump the cargo group across 1 directory with 3 updates",
                ),
            ),
            (
                "ci: check Conventional Commits PR titles in the labels job",
                header(
                    "ci",
                    None,
                    false,
                    "check Conventional Commits PR titles in the labels job",
                ),
            ),
            (
                "feat(kernel)!: drop the v0 wire format",
                header("feat", Some("kernel"), true, "drop the v0 wire format"),
            ),
            (
                "refactor!: rename Error::Parse",
                header("refactor", None, true, "rename Error::Parse"),
            ),
            (
                "fix(docs/design): correct a cross-reference",
                header(
                    "fix",
                    Some("docs/design"),
                    false,
                    "correct a cross-reference",
                ),
            ),
            (
                "FEAT: types are case-insensitive",
                header("FEAT", None, false, "types are case-insensitive"),
            ),
            (
                "docs: a: colon later is part of the description",
                header(
                    "docs",
                    None,
                    false,
                    "a: colon later is part of the description",
                ),
            ),
            ("chore: x", header("chore", None, false, "x")),
        ];
        for (title, expected) in cases {
            assert_eq!(parse(title), Ok(expected), "{title}");
        }
    }

    #[test]
    fn invalid_titles_name_the_first_broken_rule() {
        let cases = [
            ("", Invalid::MissingType),
            (
                "Bump serde_json from 1.0.150 to 1.0.151",
                Invalid::BadTypeEnd,
            ),
            (": no type", Invalid::MissingType),
            (" feat: leading space", Invalid::MissingType),
            ("feat2: digits in the type", Invalid::BadTypeEnd),
            ("feat-x: hyphen in the type", Invalid::BadTypeEnd),
            ("feat (scope): space before the scope", Invalid::BadTypeEnd),
            ("feat add a thing", Invalid::BadTypeEnd),
            ("Revert \"feat: add a thing\"", Invalid::BadTypeEnd),
            ("feat", Invalid::MissingColon),
            ("feat(scope) add a thing", Invalid::MissingColon),
            ("feat! add a thing", Invalid::MissingColon),
            ("feat!!: twice", Invalid::MissingColon),
            ("feat(scope)(other): two scopes", Invalid::MissingColon),
            ("feat(scope: unclosed", Invalid::UnclosedScope),
            ("feat(): empty scope", Invalid::EmptyScope),
            ("feat(two words): spaced scope", Invalid::BadScope),
            ("feat((nested)): nested", Invalid::BadScope),
            ("feat:no space", Invalid::BadSeparator),
            ("feat:  two spaces", Invalid::BadSeparator),
            ("feat:\tadd a thing", Invalid::BadSeparator),
            ("feat:", Invalid::EmptyDescription),
            ("feat: ", Invalid::EmptyDescription),
            ("feat:   ", Invalid::EmptyDescription),
            ("feat: add\nbody", Invalid::LineBreak),
            ("feat: add\r", Invalid::LineBreak),
        ];
        for (title, expected) in cases {
            assert_eq!(parse(title), Err(expected), "{title:?}");
        }
    }

    #[test]
    fn run_reports_the_verdict_in_the_exit_code() {
        let run_one = |title: &str| run([title.to_owned()]).unwrap();
        assert_eq!(
            run_one("chore(deps): bump ureq from 3.4.1 to 3.4.2"),
            ExitCode::SUCCESS
        );
        assert_eq!(run_one("Update README"), ExitCode::FAILURE);
    }

    #[test]
    fn run_requires_exactly_one_argument() {
        for args in [vec![], vec!["feat: a".to_owned(), "b".to_owned()]] {
            let err = run(args).unwrap_err();
            assert!(
                matches!(&err, Error::Parse(m) if m == "usage: pr_title <title>"),
                "{err}"
            );
        }
    }

    const WORKFLOW: &str = include_str!("../../../.github/workflows/labels.yml");

    #[test]
    fn the_labels_job_checks_the_title_from_the_event_through_the_environment() {
        let jobs = WORKFLOW.split_once("\njobs:\n").unwrap().1;
        assert!(jobs.starts_with("  labels:\n"), "{jobs}");
        let step = jobs
            .split("\n      - ")
            .find(|s| s.contains("just pr-title"))
            .unwrap_or_else(|| panic!("no pr-title step in the labels job:\n{jobs}"));
        // The title reaches the shell only as an environment variable, never interpolated into
        // the command line, so a title cannot inject shell syntax.
        assert!(
            step.contains("run: just pr-title \"$PR_TITLE\"\n"),
            "{step}"
        );
        assert!(
            step.contains("PR_TITLE: ${{ github.event.pull_request.title }}"),
            "{step}"
        );
        // It runs even when the label gate before it failed, so both verdicts are reported.
        assert!(step.contains("if: ${{ !cancelled() }}"), "{step}");
        let on = WORKFLOW.split_once("\non:\n").unwrap().1;
        let types = on
            .lines()
            .find_map(|l| l.trim().strip_prefix("types: "))
            .unwrap();
        assert!(
            types
                .trim_matches(['[', ']'])
                .split(", ")
                .any(|t| t == "edited"),
            "{types}"
        );
    }
}
