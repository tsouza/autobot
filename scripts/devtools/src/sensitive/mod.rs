//! The `sensitive-terms` check: a pull request's diff, commit messages and text match no term
//! of a term list held outside the repository (CHARTER L-2).
//!
//! The term list is the value of [`TERMS_VAR`], which the workflow fills from the repository
//! secret of the same name. It has the format of the pre-push hook's deny-terms file, read by
//! [`crate::hooks::parse_patterns`]: one case-insensitive regular expression per line, blank
//! lines and lines starting with `#` ignored. When the variable holds no term (unset, blank or only
//! comments), the check prints that nothing was screened and passes, so it never depends on the secret to pass;
//! the review screens L-2 either way.
//!
//! [`evaluate`] reads from the GitHub API, when the check runs, the pull request's title and
//! body, its commit messages and its changed files. It screens the title, every line of the
//! body and of each commit message, every changed path and every line each patch adds. The
//! check fails when any of them matches a term, and prints only where each match is
//! ([`Location`]): never the matched text, the term, or the term's line in the list. A file
//! whose path matches is named by its position in the file list instead of its path. A term
//! list line that is not a valid expression fails the check, naming only its line number.
//!
//! Choices the design leaves open:
//!
//! - **Removed lines** are not screened: removing a term is the fix, not a match.
//! - **A file GitHub shows no patch for** (binary, or a diff too large to show) cannot be
//!   screened; it is printed as unscreened and does not fail the check, whose failure means a
//!   match was found.

use crate::github::settings::Api;
use crate::github::{Client, Method, pages, pr_number};
use crate::{Error, Result, hooks, scope};
use regex::Regex;
use std::fmt;
use std::process::ExitCode;

/// The name of the workflow job and check.
pub const CONTEXT: &str = "sensitive-terms";

/// The environment variable, and repository secret, holding the term list.
pub const TERMS_VAR: &str = "SENSITIVE_TERMS";

/// Where a match is. Line numbers are 1-based; a file line is its line in the new version.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Location {
    /// The pull request title.
    Title,
    /// A line of the pull request body.
    Body(usize),
    /// A line of the message of the commit with this SHA.
    Commit(String, usize),
    /// The path, or the previous path, of the changed file at this 1-based position in
    /// GitHub's file list.
    Path(usize),
    /// A line a patch adds to a file, named by its path, or by its position as in
    /// [`Location::Path`] when its path matches a term.
    Added(String, usize),
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Title => write!(f, "the pull request title"),
            Self::Body(n) => write!(f, "line {n} of the pull request body"),
            Self::Commit(sha, n) => write!(f, "line {n} of the message of commit {sha}"),
            Self::Path(k) => write!(f, "the path of changed file {k}"),
            Self::Added(path, n) => write!(f, "{path}:{n}, an added line"),
        }
    }
}

/// The terms of the term list `text`.
///
/// # Errors
/// Fails when a line is not a valid regular expression, naming only its line number.
pub fn terms(text: &str) -> Result<Vec<Regex>> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let parsed = hooks::parse_patterns(line).map_err(|_| {
            Error::Parse(format!(
                "{TERMS_VAR} line {} is not a valid regular expression",
                i + 1
            ))
        })?;
        out.extend(parsed.into_iter().map(|(_, re)| re));
    }
    Ok(out)
}

/// Each line a unified-diff patch adds, with its line number in the new file.
fn added(patch: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut next = 0;
    for line in patch.lines() {
        if let Some(hunk) = line.strip_prefix("@@ ") {
            next = hunk
                .split_whitespace()
                .find_map(|range| range.strip_prefix('+'))
                .and_then(|range| range.split(',').next())
                .and_then(|start| start.parse().ok())
                .unwrap_or(0);
        } else if let Some(text) = line.strip_prefix('+') {
            out.push((next, text));
            next += 1;
        } else if line.starts_with(' ') || line.is_empty() {
            next += 1;
        }
    }
    out
}

/// What [`evaluate`] found in one pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The pull request number.
    pub pr: u64,
    /// Where a term matches, in screening order.
    pub matches: Vec<Location>,
    /// The changed files whose content GitHub shows no patch for, named as in
    /// [`Location::Added`].
    pub unscreened: Vec<String>,
}

impl Report {
    /// Non-zero exactly when a term matches.
    #[must_use]
    pub fn exit_code(&self) -> ExitCode {
        if self.matches.is_empty() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for path in &self.unscreened {
            writeln!(
                f,
                "{CONTEXT}: {path} has no patch on GitHub; its content is not screened"
            )?;
        }
        for at in &self.matches {
            writeln!(f, "{CONTEXT}: {at} matches a term")?;
        }
        match self.matches.len() {
            0 => writeln!(f, "{CONTEXT}: #{} matches no term", self.pr),
            n => writeln!(f, "{CONTEXT}: #{} matches a term in {n} place(s)", self.pr),
        }
    }
}

/// Screens pull request `pr` against `terms`.
///
/// # Errors
/// Fails if a GitHub call fails or a response lacks a field read here.
pub fn evaluate(api: &impl Api, pr: u64, terms: &[Regex]) -> Result<Report> {
    let hit = |text: &str| terms.iter().any(|t| t.is_match(text));
    let mut matches = Vec::new();
    let pull = api.request(Method::Get, &format!("pulls/{pr}"), None)?;
    let Some(title) = pull["title"].as_str() else {
        return Err(Error::Parse(format!("pulls/{pr}: no `title`")));
    };
    if hit(title) {
        matches.push(Location::Title);
    }
    let body = pull["body"].as_str().unwrap_or_default();
    for (i, line) in body.lines().enumerate() {
        if hit(line) {
            matches.push(Location::Body(i + 1));
        }
    }
    for commit in pages(api, &format!("pulls/{pr}/commits"))? {
        let (Some(sha), Some(message)) =
            (commit["sha"].as_str(), commit["commit"]["message"].as_str())
        else {
            return Err(Error::Parse(format!(
                "pulls/{pr}/commits: a commit has no `sha` or message"
            )));
        };
        let short = sha.get(..12).unwrap_or(sha);
        for (i, line) in message.lines().enumerate() {
            if hit(line) {
                matches.push(Location::Commit(short.to_owned(), i + 1));
            }
        }
    }
    let (files, _) = scope::changed_files(api, pr)?;
    let mut unscreened = Vec::new();
    for (k, file) in files.iter().enumerate() {
        let path_hit = std::iter::once(&file.path)
            .chain(&file.previous)
            .any(|p| hit(p));
        let name = if path_hit {
            matches.push(Location::Path(k + 1));
            format!("changed file {}", k + 1)
        } else {
            file.path.clone()
        };
        match &file.patch {
            Some(patch) => {
                for (n, line) in added(patch) {
                    if hit(line) {
                        matches.push(Location::Added(name.clone(), n));
                    }
                }
            }
            None if file.changes > 0 => unscreened.push(name),
            None => {}
        }
    }
    Ok(Report {
        pr,
        matches,
        unscreened,
    })
}

/// Entry point of `scripts/sensitive_terms.rs`: screens the pull request whose number is the
/// only argument against the term list in [`TERMS_VAR`], prints the [`Report`], and returns
/// its exit code. Without a term list it prints that nothing was screened and succeeds. The
/// repository is resolved by [`crate::github::repository`] from the current directory.
///
/// # Errors
/// Fails on a missing or non-numeric argument, an invalid term list, an unresolvable
/// repository or token, or a failed GitHub call.
pub fn run(args: impl IntoIterator<Item = String>) -> Result<ExitCode> {
    let pr = pr_number(args, "sensitive_terms")?;
    let list = std::env::var(TERMS_VAR).unwrap_or_default();
    let terms = terms(&list)?;
    if terms.is_empty() {
        println!("{CONTEXT}: `{TERMS_VAR}` holds no term; #{pr} is not screened");
        return Ok(ExitCode::SUCCESS);
    }
    let repo = crate::github::repository(".")?;
    let report = evaluate(&Client::new(repo)?, pr, &terms)?;
    print!("{report}");
    Ok(report.exit_code())
}

#[cfg(test)]
mod tests;
