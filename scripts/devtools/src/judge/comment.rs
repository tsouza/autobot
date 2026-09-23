//! The `judge` report as one pull request comment, kept up to date.
//!
//! [`render`] turns a [`Report`] into the comment body. Its first line is [`MARKER`], a hidden
//! HTML comment, so the body never starts with the review gate's verdict prefix and the gate
//! never reads it as a verdict. A table follows with one row per `judged` entry: its id, the
//! answer, the confidence, the entry's threshold, and whether it blocks or falls back to the
//! `review` backstop. When the service was not asked or did not answer, a line says why and
//! that the `review` backstop decides every judged entry. The body ends with the model that
//! answered, the token usage and the request digest.
//!
//! [`publish`] edits the report comment of the pull request when there is one and creates it
//! otherwise, so every run leaves exactly one. The report comment is the first issue comment
//! on the pull request whose first line is [`MARKER`] and whose author is [`AUTHOR`].
//!
//! Choices the design leaves open:
//!
//! - **Author.** The workflow posts with its own token, whose identity is [`AUTHOR`]. Only a
//!   comment by that account is edited, so a comment someone else starts with the marker is
//!   never overwritten.
//! - **Confidence shown.** A "violates" row shows the confidence the decision used; a
//!   "complies" or "unsure" row shows the probability of the chosen option, or the answer's
//!   `confidence` when it has none. The decision does not read the latter; it is recorded
//!   only.

use super::{MODEL, Outcome, Reason, Report};
use crate::github::settings::Api;
use crate::github::{Method, pages};
use crate::{Error, Result};
use serde_json::{Value, json};
use std::fmt::Write as _;

/// The first line of every report comment.
pub const MARKER: &str = "<!-- judge -->";

/// The login of the account the workflow's token posts as.
pub const AUTHOR: &str = "github-actions[bot]";

/// A table cell or a line of prose: pipes escaped and line breaks folded, so an error text
/// cannot break the table or start a line of its own.
fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace(['\r', '\n'], " ")
}

/// The answer and the confidence a row shows for `outcome`.
fn answer(outcome: &Outcome) -> (String, Option<f64>) {
    let text = |t: &str| t.to_owned();
    match outcome {
        Outcome::Blocks(c) | Outcome::Backstop(Reason::LowConfidence(c)) => {
            (text("violates"), Some(*c))
        }
        Outcome::Backstop(Reason::Complies(c)) => (text("complies"), *c),
        Outcome::Backstop(Reason::Unsure(c)) => (text("unsure"), *c),
        Outcome::Backstop(Reason::Malformed(why)) => (format!("malformed: {why}"), None),
        Outcome::Backstop(Reason::Missing) => (text("no answer"), None),
        Outcome::Backstop(Reason::Transport(_)) => (text("not answered"), None),
        Outcome::Backstop(Reason::Truncated | Reason::NoKey | Reason::Unavailable(_)) => {
            (text("not asked"), None)
        }
    }
}

/// Whether `reason` means the service was not asked or did not answer.
fn is_outage(reason: &Reason) -> bool {
    matches!(
        reason,
        Reason::Truncated | Reason::NoKey | Reason::Transport(_) | Reason::Unavailable(_)
    )
}

/// The comment body for `report`, as the module docs describe it.
#[must_use]
pub fn render(report: &Report) -> String {
    let mut out = format!("{MARKER}\n### `judge`: answers for the `judged` charter entries\n\n");
    if report.outcomes.is_empty() {
        out.push_str("No judged entry was read.\n\n");
    } else {
        out.push_str("| Entry | Answer | Confidence | Threshold | Result |\n");
        out.push_str("|---|---|---|---|---|\n");
        for (entry, outcome) in &report.outcomes {
            let (answer, confidence) = answer(outcome);
            let confidence = confidence.map_or_else(|| "—".to_owned(), |c| c.to_string());
            let result = match outcome {
                Outcome::Blocks(_) => "blocks",
                Outcome::Backstop(_) => "falls back to `review`",
            };
            let _ = writeln!(
                out,
                "| {} | {} | {confidence} | {} | {result} |",
                cell(&entry.id),
                cell(&answer),
                entry.threshold
            );
        }
        out.push('\n');
    }
    let mut outages: Vec<String> = Vec::new();
    for (_, outcome) in &report.outcomes {
        if let Outcome::Backstop(reason) = outcome
            && is_outage(reason)
        {
            let text = cell(&reason.to_string());
            if !outages.contains(&text) {
                outages.push(text);
            }
        }
    }
    for outage in &outages {
        let _ = writeln!(
            out,
            "**Not judged:** {outage}. The `review` backstop decides every judged entry.\n"
        );
    }
    for note in &report.notes {
        let _ = writeln!(out, "- {}", cell(note));
    }
    if !report.notes.is_empty() {
        out.push('\n');
    }
    let _ = write!(
        out,
        "Model {} (requested {MODEL}) · usage {} · request sha256 {}",
        cell(report.model_text()),
        report.usage_text(),
        report.digest_text()
    );
    out
}

/// What [`publish`] did, with the comment's URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Posted {
    /// The pull request had no report comment; one was created.
    Created(String),
    /// The report comment was edited in place.
    Updated(String),
}

impl std::fmt::Display for Posted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Created(url) => write!(f, "report comment created: {url}"),
            Self::Updated(url) => write!(f, "report comment updated: {url}"),
        }
    }
}

/// Whether `comment` is a report comment: its first line is [`MARKER`] and its author is
/// [`AUTHOR`].
fn is_report(comment: &Value) -> bool {
    comment["user"]["login"].as_str() == Some(AUTHOR)
        && comment["body"].as_str().and_then(|b| b.lines().next()) == Some(MARKER)
}

/// Writes `body` to the report comment of pull request `pr`: edits the first report comment
/// when there is one, and creates it otherwise.
///
/// # Errors
/// Fails if a GitHub call fails or the report comment has no numeric `id`.
pub fn publish(api: &impl Api, pr: u64, body: &str) -> Result<Posted> {
    let comments = pages(api, &format!("issues/{pr}/comments"))?;
    let payload = json!({ "body": body });
    let url = |posted: &Value| posted["html_url"].as_str().unwrap_or_default().to_owned();
    match comments.iter().find(|c| is_report(c)) {
        Some(existing) => {
            let Some(id) = existing["id"].as_u64() else {
                return Err(Error::Parse(format!(
                    "issues/{pr}/comments: the report comment has no `id`"
                )));
            };
            let posted = api.request(
                Method::Patch,
                &format!("issues/comments/{id}"),
                Some(&payload),
            )?;
            Ok(Posted::Updated(url(&posted)))
        }
        None => {
            let posted = api.request(
                Method::Post,
                &format!("issues/{pr}/comments"),
                Some(&payload),
            )?;
            Ok(Posted::Created(url(&posted)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::verdict::{VERDICT_PREFIX, parse_verdict};
    use crate::judge::charter::Entry;
    use std::cell::RefCell;

    fn entry(id: &str, threshold: f64) -> Entry {
        Entry {
            id: id.to_owned(),
            statement: format!("statement of {id}"),
            threshold,
        }
    }

    fn report(outcomes: Vec<(Entry, Outcome)>) -> Report {
        Report {
            outcomes,
            model: Some("jev-1.13.0".to_owned()),
            usage: Some((1200, 14)),
            digest: Some("ab12".to_owned()),
            notes: Vec::new(),
        }
    }

    fn answered() -> Report {
        report(vec![
            (entry("L-1", 0.8), Outcome::Blocks(0.93)),
            (
                entry("C-1", 0.9),
                Outcome::Backstop(Reason::Complies(Some(0.97))),
            ),
            (
                entry("C-2", 0.8),
                Outcome::Backstop(Reason::LowConfidence(0.6)),
            ),
            (entry("C-3", 0.8), Outcome::Backstop(Reason::Unsure(None))),
        ])
    }

    fn outage() -> Report {
        let down = || Outcome::Backstop(Reason::Transport("POST x: connection refused".into()));
        Report {
            model: None,
            usage: None,
            ..report(vec![
                (entry("L-1", 0.8), down()),
                (entry("C-1", 0.8), down()),
            ])
        }
    }

    fn charter_down() -> Report {
        super::super::unavailable(Vec::new(), "charter: CHARTER.md: not found")
    }

    const BACKSTOP: &str = "The `review` backstop decides every judged entry.";

    #[test]
    fn a_row_per_entry_and_a_footer_with_model_usage_and_digest() {
        let body = render(&answered());
        let rows: Vec<&str> = body.lines().filter(|l| l.starts_with("| ")).collect();
        assert_eq!(
            rows,
            [
                "| Entry | Answer | Confidence | Threshold | Result |",
                "| L-1 | violates | 0.93 | 0.8 | blocks |",
                "| C-1 | complies | 0.97 | 0.9 | falls back to `review` |",
                "| C-2 | violates | 0.6 | 0.8 | falls back to `review` |",
                "| C-3 | unsure | — | 0.8 | falls back to `review` |",
            ]
        );
        assert_eq!(
            body.lines().last(),
            Some(
                "Model jev-1.13.0 (requested jev-1.13.0) · usage 1200 input, 14 output tokens \
                 · request sha256 ab12"
            )
        );
        assert!(!body.contains(BACKSTOP), "{body}");
    }

    #[test]
    fn the_first_line_is_the_marker_and_never_a_verdict() {
        for report in [answered(), outage(), charter_down()] {
            let body = render(&report);
            assert_eq!(body.lines().next(), Some(MARKER));
            assert!(!body.starts_with("Review verdict"));
            assert!(!body.starts_with(VERDICT_PREFIX));
            assert_eq!(parse_verdict(&body), None);
        }
    }

    #[test]
    fn an_outage_says_so_once_and_names_the_backstop() {
        let body = render(&outage());
        let line =
            format!("**Not judged:** service unavailable: POST x: connection refused. {BACKSTOP}");
        assert_eq!(body.matches(&line).count(), 1, "{body}");
        assert!(body.contains("| L-1 | not answered | — | 0.8 | falls back to `review` |"));
        assert!(body.ends_with("Model none reported (requested jev-1.13.0) · usage none reported · request sha256 ab12"), "{body}");

        let body = render(&charter_down());
        assert!(body.contains("No judged entry was read."), "{body}");
        assert!(
            body.contains(
                "- inputs unavailable: charter: CHARTER.md: not found; the `review` backstop \
                 applies to every judged entry"
            ),
            "{body}"
        );
        assert!(
            body.ends_with("request sha256 none (no request built)"),
            "{body}"
        );

        let reads_down = super::super::unavailable(vec![entry("L-1", 0.8)], "pull request #7: 502");
        let body = render(&reads_down);
        assert!(body.contains(&format!(
            "**Not judged:** inputs unavailable: pull request #7: 502; not asked. {BACKSTOP}"
        )));
        assert!(body.contains("| L-1 | not asked | — | 0.8 | falls back to `review` |"));
    }

    #[test]
    fn a_pipe_or_newline_in_a_reason_cannot_break_the_table() {
        let body = render(&report(vec![(
            entry("L-1", 0.8),
            Outcome::Backstop(Reason::Malformed("a | b\n| forged | row |".into())),
        )]));
        let rows: Vec<&str> = body.lines().filter(|l| l.starts_with("| L-1")).collect();
        assert_eq!(
            rows,
            [
                "| L-1 | malformed: a \\| b \\| forged \\| row \\| | — | 0.8 | falls back to `review` |"
            ]
        );
        assert!(!body.lines().any(|l| l.starts_with("| forged")));
    }

    /// The issue comments of pull request 7, served and updated the way GitHub does: GET lists
    /// them, POST appends one by [`AUTHOR`], PATCH replaces a comment's body. Every write is
    /// recorded.
    struct Thread {
        comments: RefCell<Vec<Value>>,
        writes: RefCell<Vec<(Method, String)>>,
    }

    fn comment(id: u64, login: &str, kind: &str, body: &str) -> Value {
        json!({
            "id": id,
            "html_url": format!("https://github.com/octo-org/autobot/pull/7#issuecomment-{id}"),
            "user": {"login": login, "type": kind},
            "author_association": if kind == "Bot" { "NONE" } else { "OWNER" },
            "body": body,
        })
    }

    impl Thread {
        /// A thread holding a human comment and a human comment that starts with the marker.
        fn new() -> Self {
            Self {
                comments: RefCell::new(vec![
                    comment(101, "octo-owner", "User", "Looks fine."),
                    comment(
                        102,
                        "octo-owner",
                        "User",
                        &format!("{MARKER}\nquoted report"),
                    ),
                ]),
                writes: RefCell::new(Vec::new()),
            }
        }

        fn body_of(&self, id: u64) -> String {
            let comments = self.comments.borrow();
            let found = comments.iter().find(|c| c["id"] == id).unwrap();
            found["body"].as_str().unwrap().to_owned()
        }
    }

    impl Api for Thread {
        fn request(&self, method: Method, path: &str, body: Option<&Value>) -> Result<Value> {
            let text = || body.unwrap()["body"].as_str().unwrap().to_owned();
            match (method, path) {
                (Method::Get, "issues/7/comments?per_page=100&page=1") => {
                    Ok(Value::Array(self.comments.borrow().clone()))
                }
                (Method::Post, "issues/7/comments") => {
                    self.writes.borrow_mut().push((method, path.to_owned()));
                    let id = 200 + self.comments.borrow().len() as u64;
                    let created = comment(id, AUTHOR, "Bot", &text());
                    self.comments.borrow_mut().push(created.clone());
                    Ok(created)
                }
                (Method::Patch, _) if path.starts_with("issues/comments/") => {
                    self.writes.borrow_mut().push((method, path.to_owned()));
                    let id: u64 = path["issues/comments/".len()..].parse().unwrap();
                    let mut comments = self.comments.borrow_mut();
                    let Some(target) = comments.iter_mut().find(|c| c["id"] == id) else {
                        return Err(Error::Http(format!("Patch {path}: http status: 404")));
                    };
                    target["body"] = Value::String(text());
                    Ok(target.clone())
                }
                _ => Err(Error::Http(format!("{method:?} {path}: http status: 404"))),
            }
        }
    }

    #[test]
    fn a_first_run_creates_the_comment_and_a_second_run_edits_it() {
        let thread = Thread::new();
        let first = render(&outage());
        let posted = publish(&thread, 7, &first).unwrap();
        assert_eq!(
            posted,
            Posted::Created("https://github.com/octo-org/autobot/pull/7#issuecomment-202".into())
        );
        assert_eq!(thread.comments.borrow().len(), 3);
        assert_eq!(thread.body_of(202), first);

        let second = render(&answered());
        let posted = publish(&thread, 7, &second).unwrap();
        assert_eq!(
            posted,
            Posted::Updated("https://github.com/octo-org/autobot/pull/7#issuecomment-202".into())
        );
        assert_eq!(thread.comments.borrow().len(), 3);
        assert_eq!(thread.body_of(202), second);
        assert_eq!(thread.body_of(102), format!("{MARKER}\nquoted report"));
        assert_eq!(
            *thread.writes.borrow(),
            [
                (Method::Post, "issues/7/comments".to_owned()),
                (Method::Patch, "issues/comments/202".to_owned()),
            ]
        );
        let reports = thread
            .comments
            .borrow()
            .iter()
            .filter(|c| is_report(c))
            .count();
        assert_eq!(reports, 1);
    }

    #[test]
    fn a_failed_read_of_the_comments_writes_nothing() {
        let thread = Thread::new();
        assert!(publish(&thread, 8, "x").is_err());
        assert!(thread.writes.borrow().is_empty());
    }
}
