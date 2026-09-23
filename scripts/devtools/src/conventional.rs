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
        assert!(step.contains("    just pr-title \"$PR_TITLE\"\n"), "{step}");
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

    /// The `run:` block of the pr-title step, dedented, as GitHub hands it to bash.
    fn title_step_script() -> String {
        let (_, after) = WORKFLOW
            .split_once("\n        run: |\n")
            .unwrap_or_else(|| panic!("no block `run:` in labels.yml:\n{WORKFLOW}"));
        let block: Vec<&str> = after
            .lines()
            .take_while(|l| l.starts_with("          "))
            .map(|l| &l["          ".len()..])
            .collect();
        assert!(
            block.iter().any(|l| l.contains("just pr-title")),
            "{block:?}"
        );
        block.join("\n") + "\n"
    }

    /// Runs the pr-title step with a stub `just` that has the `pr-title` recipe only when
    /// `has_recipe`, and whose `pr-title` recipe records its arguments and rejects the title.
    fn run_title_step(has_recipe: bool, title: &str) -> (Option<i32>, String, String) {
        use crate::worktree::test_support::TempDir;
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new();
        let log = tmp.0.join("calls");
        let stub = tmp.0.join("just");
        std::fs::write(
            &stub,
            format!(
                "#!/bin/sh\n\
                 if [ \"$1\" = --show ]; then [ {has} = 1 ] && [ \"$2\" = pr-title ]; exit; fi\n\
                 printf '%s\\n' \"$@\" >> '{log}'\n\
                 exit 1\n",
                has = u8::from(has_recipe),
                log = log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = format!(
            "{}:{}",
            tmp.0.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let out = std::process::Command::new("bash")
            .args(["--noprofile", "--norc", "-eo", "pipefail", "-c"])
            .arg(title_step_script())
            .env("PATH", path)
            .env("PR_TITLE", title)
            .output()
            .unwrap();
        let calls = std::fs::read_to_string(&log).unwrap_or_default();
        (
            out.status.code(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            calls,
        )
    }

    #[test]
    fn the_title_step_warns_instead_of_failing_before_the_recipe_is_on_the_default_branch() {
        let (code, stdout, calls) = run_title_step(false, "Update README");
        assert_eq!(code, Some(0), "{stdout}");
        assert!(stdout.starts_with("::warning::"), "{stdout}");
        assert_eq!(calls, "");
    }

    #[test]
    fn the_title_step_runs_the_recipe_with_the_literal_title_and_keeps_its_verdict() {
        let title = "Update \"README\" $HOME `id`";
        let (code, stdout, calls) = run_title_step(true, title);
        assert_eq!(code, Some(1), "{stdout}");
        assert_eq!(calls, format!("pr-title\n{title}\n"));
    }
}
