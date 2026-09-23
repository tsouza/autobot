//! The `review-gate` commit status: a pull request's head passes only when the latest review
//! verdict names that exact head SHA.
//!
//! A review verdict is an issue comment on the pull request whose first line starts with
//! [`VERDICT_PREFIX`]. A well-formed verdict line is exactly
//! `Review verdict: PASS @ <sha>` or `Review verdict: FAIL @ <sha>`, where `<sha>` is a full
//! 40-character lowercase hexadecimal commit SHA. A line with the prefix and any other text is
//! a malformed verdict: it counts as a verdict and never passes, so a mistyped FAIL cannot
//! leave an earlier PASS standing.
//!
//! Only verdicts written by an account with write access to the repository count. An author
//! qualifies when the comment's `author_association` is `OWNER`, `MEMBER` or `COLLABORATOR`
//! and the repository permission GitHub reports for the account is `admin` or `write`; the
//! association filter keeps accounts that cannot be collaborators (bots, deleted users,
//! outsiders) away from the permission lookup, which fails for them.
//!
//! The latest counting verdict, in comment order, decides the status posted on the head SHA
//! with context [`CONTEXT`]:
//!
//! - `success` when it is a PASS naming the current head SHA;
//! - `failure` when it is a FAIL, a malformed verdict, or a PASS naming another SHA, so any
//!   new push voids an earlier PASS;
//! - `pending` when no verdict counts yet.
//!
//! The status is a commit status rather than a check run: a check run started by an
//! `issue_comment` event does not attach to the pull request's head commit.

use super::settings::Api;
use super::{Client, Method};
use crate::{Error, Result};
use serde_json::Value;

/// The commit status context, and the name of the required check.
pub const CONTEXT: &str = "review-gate";

/// The start of every verdict line.
pub const VERDICT_PREFIX: &str = "Review verdict: ";

/// Comments requested per page; a shorter page is the last one.
const PAGE_SIZE: usize = 100;

/// A parsed verdict line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// `Review verdict: PASS @ <sha>`.
    Pass(String),
    /// `Review verdict: FAIL @ <sha>`.
    Fail(String),
    /// A line that starts with [`VERDICT_PREFIX`] but is neither form above.
    Malformed,
}

/// Parses the first line of a comment body; `None` when it is not a verdict line.
#[must_use]
pub fn parse_verdict(body: &str) -> Option<Verdict> {
    let rest = body.lines().next()?.strip_prefix(VERDICT_PREFIX)?;
    let full_sha =
        |sha: &str| sha.len() == 40 && sha.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    let verdict = match rest.split_once(" @ ") {
        Some(("PASS", sha)) if full_sha(sha) => Verdict::Pass(sha.to_owned()),
        Some(("FAIL", sha)) if full_sha(sha) => Verdict::Fail(sha.to_owned()),
        _ => Verdict::Malformed,
    };
    Some(verdict)
}

/// The state of a commit status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// No verdict counts yet.
    Pending,
    /// The latest verdict is a PASS naming the head SHA.
    Success,
    /// The latest verdict does not pass the head SHA.
    Failure,
}

impl State {
    /// The value of the commit status API's `state` field.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Success => "success",
            Self::Failure => "failure",
        }
    }
}

/// The commit status to post on a pull request's head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// The head SHA the status is posted on and was evaluated against.
    pub sha: String,
    /// The status state.
    pub state: State,
    /// A one-line explanation, shown next to the check.
    pub description: String,
    /// The deciding verdict comment, when there is one.
    pub target_url: Option<String>,
}

/// Reads pull request `pr` and its comments and decides its `review-gate` status.
///
/// # Errors
/// Fails if a GitHub call fails or a response lacks the head SHA.
pub fn evaluate(api: &impl Api, pr: u64) -> Result<Status> {
    let pull = api.request(Method::Get, &format!("pulls/{pr}"), None)?;
    let Some(head) = pull["head"]["sha"].as_str() else {
        return Err(Error::Parse(format!("pulls/{pr}: no `head.sha`")));
    };
    let short = |sha: &str| sha.chars().take(7).collect::<String>();
    let comments = comments(api, pr)?;
    for comment in comments.iter().rev() {
        let Some(verdict) = comment["body"].as_str().and_then(parse_verdict) else {
            continue;
        };
        if !has_write_access(api, comment)? {
            continue;
        }
        let (state, description) = match verdict {
            Verdict::Pass(sha) if sha == head => {
                (State::Success, format!("PASS @ {}", short(head)))
            }
            Verdict::Pass(sha) => (
                State::Failure,
                format!("PASS names {}, head is {}", short(&sha), short(head)),
            ),
            Verdict::Fail(sha) => (State::Failure, format!("FAIL @ {}", short(&sha))),
            Verdict::Malformed => (State::Failure, "latest verdict is malformed".to_owned()),
        };
        return Ok(Status {
            sha: head.to_owned(),
            state,
            description,
            target_url: comment["html_url"].as_str().map(str::to_owned),
        });
    }
    Ok(Status {
        sha: head.to_owned(),
        state: State::Pending,
        description: format!("awaiting a review verdict for {}", short(head)),
        target_url: None,
    })
}

/// Posts `status` on its SHA with context [`CONTEXT`].
///
/// # Errors
/// Fails if the GitHub call fails.
pub fn post(api: &impl Api, status: &Status) -> Result<()> {
    let mut body = serde_json::json!({
        "state": status.state.as_str(),
        "context": CONTEXT,
        "description": status.description,
    });
    if let Some(url) = &status.target_url {
        body["target_url"] = Value::String(url.clone());
    }
    api.request(
        Method::Post,
        &format!("statuses/{}", status.sha),
        Some(&body),
    )?;
    Ok(())
}

/// Entry point of `scripts/review_gate.rs`: evaluates the pull request whose number is the
/// only argument, posts the status and prints it. The repository is `GITHUB_REPOSITORY`.
///
/// # Errors
/// Fails on a missing or non-numeric argument, an unset `GITHUB_REPOSITORY`, or a failed
/// GitHub call.
pub fn main(args: impl IntoIterator<Item = String>) -> Result<()> {
    let pr = parse_args(args)?;
    let repo = std::env::var("GITHUB_REPOSITORY")
        .ok()
        .filter(|r| !r.is_empty())
        .ok_or_else(|| Error::Parse("GITHUB_REPOSITORY must name owner/name".to_owned()))?;
    let client = Client::new(repo)?;
    let status = evaluate(&client, pr)?;
    post(&client, &status)?;
    println!(
        "{CONTEXT}: {} on {}: {}",
        status.state.as_str(),
        status.sha,
        status.description
    );
    Ok(())
}

/// The pull request number, the only argument.
fn parse_args(args: impl IntoIterator<Item = String>) -> Result<u64> {
    let mut args = args.into_iter();
    match (args.next(), args.next()) {
        (Some(pr), None) => pr
            .parse()
            .map_err(|_| Error::Parse(format!("not a pull request number: `{pr}`"))),
        _ => Err(Error::Parse(
            "usage: review_gate <pull-request-number>".to_owned(),
        )),
    }
}

/// Every comment on pull request `pr`, oldest first.
fn comments(api: &impl Api, pr: u64) -> Result<Vec<Value>> {
    let mut all = Vec::new();
    for page in 1.. {
        let path = format!("issues/{pr}/comments?per_page={PAGE_SIZE}&page={page}");
        let Value::Array(batch) = api.request(Method::Get, &path, None)? else {
            return Err(Error::Parse(format!("{path}: not a JSON array")));
        };
        let last = batch.len() < PAGE_SIZE;
        all.extend(batch);
        if last {
            break;
        }
    }
    Ok(all)
}

/// Whether the comment's author has write access, as described in the module docs.
fn has_write_access(api: &impl Api, comment: &Value) -> Result<bool> {
    let association = comment["author_association"].as_str().unwrap_or_default();
    if !matches!(association, "OWNER" | "MEMBER" | "COLLABORATOR") {
        return Ok(false);
    }
    let Some(login) = comment["user"]["login"].as_str() else {
        return Ok(false);
    };
    let permission = api.request(
        Method::Get,
        &format!("collaborators/{login}/permission"),
        None,
    )?;
    Ok(matches!(
        permission["permission"].as_str(),
        Some("admin" | "write")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    // Recorded from GET repos/tsouza/autobot/pulls/236, keeping `number`, `state` and `head`.
    const PULL: &str = r#"{"head":{"ref":"48-devtools-lib","sha":"15b7298d5a0de178405ccff640f71c2bfde55f3f"},"number":236,"state":"closed"}"#;

    // Recorded from GET repos/tsouza/autobot/issues/236/comments?per_page=100, keeping
    // `id`, `html_url`, `user.login`, `user.type`, `author_association`, `created_at` and the
    // first line of `body`.
    const COMMENTS: &str = r#"[{"author_association":"OWNER","body":"Review verdict: FAIL @ 2d6fe7ee926d787b7e4de9603b34753b448e3cdf","created_at":"2026-09-23T00:29:34Z","html_url":"https://github.com/tsouza/autobot/pull/236#issuecomment-5786866846","id":5786866846,"user":{"login":"tsouza","type":"User"}},{"author_association":"OWNER","body":"Review verdict: FAIL @ 0b1bef41de4192a41209204e387f98fd02813796","created_at":"2026-09-23T00:33:59Z","html_url":"https://github.com/tsouza/autobot/pull/236#issuecomment-5786921782","id":5786921782,"user":{"login":"tsouza","type":"User"}},{"author_association":"OWNER","body":"Review verdict: FAIL @ ddf67ff490f699af3920c86e83ec6ac30eaf97ce","created_at":"2026-09-23T00:38:38Z","html_url":"https://github.com/tsouza/autobot/pull/236#issuecomment-5786973062","id":5786973062,"user":{"login":"tsouza","type":"User"}},{"author_association":"OWNER","body":"Review verdict: FAIL @ 6ae0c26070bd87bf7188a39f08dad0ecf0710f45","created_at":"2026-09-23T00:43:41Z","html_url":"https://github.com/tsouza/autobot/pull/236#issuecomment-5787017374","id":5787017374,"user":{"login":"tsouza","type":"User"}},{"author_association":"OWNER","body":"Review verdict: FAIL @ c1bdd53e21c28af6bcbc8f1e16b201e795198149","created_at":"2026-09-23T00:46:38Z","html_url":"https://github.com/tsouza/autobot/pull/236#issuecomment-5787042606","id":5787042606,"user":{"login":"tsouza","type":"User"}},{"author_association":"OWNER","body":"Review verdict: PASS @ 15b7298d5a0de178405ccff640f71c2bfde55f3f","created_at":"2026-09-23T00:49:52Z","html_url":"https://github.com/tsouza/autobot/pull/236#issuecomment-5787069111","id":5787069111,"user":{"login":"tsouza","type":"User"}}]"#;

    // Recorded from GET repos/tsouza/autobot/collaborators/tsouza/permission, keeping
    // `permission`, `role_name` and `user.login`.
    const OWNER_PERMISSION: &str =
        r#"{"permission":"admin","role_name":"admin","user":{"login":"tsouza"}}"#;

    // Recorded from GET repos/tsouza/autobot/collaborators/octocat/permission, same fields.
    const READ_PERMISSION: &str =
        r#"{"permission":"read","role_name":"read","user":{"login":"octocat"}}"#;

    const HEAD: &str = "15b7298d5a0de178405ccff640f71c2bfde55f3f";
    const PASS_URL: &str = "https://github.com/tsouza/autobot/pull/236#issuecomment-5787069111";
    const COMMENTS_PAGE_1: &str = "issues/236/comments?per_page=100&page=1";

    fn parse(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    /// Serves recorded GET responses by path, fails any other GET the way GitHub answers an
    /// unknown resource, and records every write.
    struct FakeRepo {
        responses: BTreeMap<String, Value>,
        gets: RefCell<Vec<String>>,
        writes: RefCell<Vec<(Method, String, Value)>>,
    }

    impl FakeRepo {
        fn new(pull: Value, comments: Value) -> Self {
            let mut responses = BTreeMap::new();
            responses.insert("pulls/236".to_owned(), pull);
            responses.insert(COMMENTS_PAGE_1.to_owned(), comments);
            responses.insert(
                "collaborators/tsouza/permission".to_owned(),
                parse(OWNER_PERMISSION),
            );
            responses.insert(
                "collaborators/octocat/permission".to_owned(),
                parse(READ_PERMISSION),
            );
            Self {
                responses,
                gets: RefCell::new(Vec::new()),
                writes: RefCell::new(Vec::new()),
            }
        }

        fn recorded() -> Self {
            Self::new(parse(PULL), parse(COMMENTS))
        }

        fn with_comments(edit: impl FnOnce(&mut Vec<Value>)) -> Self {
            let mut comments = parse(COMMENTS);
            edit(comments.as_array_mut().unwrap());
            Self::new(parse(PULL), comments)
        }
    }

    impl Api for FakeRepo {
        fn request(&self, method: Method, path: &str, body: Option<&Value>) -> Result<Value> {
            if method == Method::Get {
                self.gets.borrow_mut().push(path.to_owned());
                return self
                    .responses
                    .get(path)
                    .cloned()
                    .ok_or_else(|| Error::Http(format!("404 {path}")));
            }
            self.writes.borrow_mut().push((
                method,
                path.to_owned(),
                body.cloned().unwrap_or(Value::Null),
            ));
            Ok(json!({}))
        }
    }

    fn comment(login: &str, association: &str, body: &str) -> Value {
        json!({
            "author_association": association,
            "body": body,
            "html_url": format!("https://github.com/tsouza/autobot/pull/236#issuecomment-{login}"),
            "user": {"login": login, "type": "User"},
        })
    }

    #[test]
    fn pass_naming_the_head_succeeds_and_is_posted_on_the_head() {
        let api = FakeRepo::recorded();
        let status = evaluate(&api, 236).unwrap();
        assert_eq!(
            status,
            Status {
                sha: HEAD.to_owned(),
                state: State::Success,
                description: "PASS @ 15b7298".to_owned(),
                target_url: Some(PASS_URL.to_owned()),
            }
        );
        post(&api, &status).unwrap();
        assert_eq!(
            *api.writes.borrow(),
            [(
                Method::Post,
                format!("statuses/{HEAD}"),
                json!({
                    "state": "success",
                    "context": "review-gate",
                    "description": "PASS @ 15b7298",
                    "target_url": PASS_URL,
                })
            )]
        );
    }

    #[test]
    fn pass_for_an_older_sha_fails() {
        let mut pull = parse(PULL);
        pull["head"]["sha"] = json!("9f0e1d2c3b4a59687766554433221100ffeeddcc");
        let api = FakeRepo::new(pull, parse(COMMENTS));
        let status = evaluate(&api, 236).unwrap();
        assert_eq!(status.state, State::Failure);
        assert_eq!(status.sha, "9f0e1d2c3b4a59687766554433221100ffeeddcc");
        assert_eq!(status.description, "PASS names 15b7298, head is 9f0e1d2");
        assert_eq!(status.target_url.as_deref(), Some(PASS_URL));
    }

    #[test]
    fn pass_from_an_account_without_write_access_is_ignored() {
        // Drop the owner's PASS: the owner's latest verdict is then a FAIL for another SHA.
        let pass = format!("Review verdict: PASS @ {HEAD}");
        for (login, association) in [
            ("octocat", "COLLABORATOR"),
            ("stranger", "NONE"),
            ("stranger", "CONTRIBUTOR"),
        ] {
            let api = FakeRepo::with_comments(|c| {
                c.pop();
                c.push(comment(login, association, &pass));
            });
            let status = evaluate(&api, 236).unwrap();
            assert_eq!(status.state, State::Failure, "{association}");
            assert_eq!(status.description, "FAIL @ c1bdd53", "{association}");
        }
        // A read-only collaborator is looked up and rejected; an outsider is never looked up.
        let api = FakeRepo::with_comments(|c| {
            c.push(comment(
                "octocat",
                "COLLABORATOR",
                "Review verdict: FAIL @ x",
            ));
            c.push(comment("stranger", "NONE", "Review verdict: FAIL @ x"));
        });
        assert_eq!(evaluate(&api, 236).unwrap().state, State::Success);
        let gets = api.gets.borrow();
        assert!(gets.contains(&"collaborators/octocat/permission".to_owned()));
        assert!(!gets.iter().any(|g| g.contains("stranger")), "{gets:?}");
    }

    #[test]
    fn later_fail_overrides_an_earlier_pass() {
        let api = FakeRepo::with_comments(|c| {
            c.push(comment(
                "tsouza",
                "OWNER",
                &format!("Review verdict: FAIL @ {HEAD}\n\n## Defects\n1. x"),
            ));
        });
        let status = evaluate(&api, 236).unwrap();
        assert_eq!(status.state, State::Failure);
        assert_eq!(status.description, "FAIL @ 15b7298");
    }

    #[test]
    fn malformed_verdict_overrides_an_earlier_pass() {
        let api = FakeRepo::with_comments(|c| {
            c.push(comment("tsouza", "OWNER", "Review verdict: FAIL @ 15b7298"));
        });
        let status = evaluate(&api, 236).unwrap();
        assert_eq!(status.state, State::Failure);
        assert_eq!(status.description, "latest verdict is malformed");
    }

    #[test]
    fn comments_that_are_not_verdicts_do_not_count() {
        let api = FakeRepo::with_comments(|c| {
            c.push(comment("tsouza", "OWNER", "LGTM"));
            c.push(comment(
                "tsouza",
                "OWNER",
                &format!("Note\nReview verdict: FAIL @ {HEAD}"),
            ));
        });
        assert_eq!(evaluate(&api, 236).unwrap().state, State::Success);
    }

    #[test]
    fn no_verdict_is_pending() {
        let api = FakeRepo::with_comments(Vec::clear);
        let status = evaluate(&api, 236).unwrap();
        assert_eq!(status.state, State::Pending);
        assert_eq!(status.description, "awaiting a review verdict for 15b7298");
        assert_eq!(status.target_url, None);
        post(&api, &status).unwrap();
        assert_eq!(
            api.writes.borrow()[0].2,
            json!({
                "state": "pending",
                "context": "review-gate",
                "description": "awaiting a review verdict for 15b7298",
            })
        );
    }

    #[test]
    fn verdicts_on_later_pages_are_read() {
        let mut api = FakeRepo::with_comments(|c| {
            c.truncate(1);
            c.resize(PAGE_SIZE, comment("tsouza", "OWNER", "chatter"));
        });
        api.responses.insert(
            "issues/236/comments?per_page=100&page=2".to_owned(),
            json!([parse(COMMENTS)[5].clone()]),
        );
        let status = evaluate(&api, 236).unwrap();
        assert_eq!(status.state, State::Success);
        assert_eq!(status.target_url.as_deref(), Some(PASS_URL));
    }

    #[test]
    fn missing_head_sha_is_an_error() {
        let api = FakeRepo::new(json!({"number": 236}), parse(COMMENTS));
        let err = evaluate(&api, 236).unwrap_err();
        assert!(err.to_string().contains("head.sha"), "{err}");
    }

    #[test]
    fn verdict_lines_parse_exactly() {
        assert_eq!(
            parse_verdict(&format!("Review verdict: PASS @ {HEAD}\r\n\nChecked")),
            Some(Verdict::Pass(HEAD.to_owned()))
        );
        assert_eq!(
            parse_verdict(&format!("Review verdict: FAIL @ {HEAD}")),
            Some(Verdict::Fail(HEAD.to_owned()))
        );
        for malformed in [
            "Review verdict: PASS @ 15b7298".to_owned(),
            format!("Review verdict: PASS @ {}", HEAD.to_uppercase()),
            format!("Review verdict: PASS @ {HEAD} (rebased)"),
            format!("Review verdict: pass @ {HEAD}"),
            format!("Review verdict: PASS  @ {HEAD}"),
            "Review verdict: ".to_owned(),
        ] {
            assert_eq!(
                parse_verdict(&malformed),
                Some(Verdict::Malformed),
                "{malformed}"
            );
        }
        for other in [
            String::new(),
            "LGTM".to_owned(),
            format!(" Review verdict: PASS @ {HEAD}"),
            format!("review verdict: PASS @ {HEAD}"),
        ] {
            assert_eq!(parse_verdict(&other), None, "{other}");
        }
    }

    #[test]
    fn the_only_argument_is_a_pull_request_number() {
        assert_eq!(parse_args(["236".to_owned()]).unwrap(), 236);
        for args in [
            vec![],
            vec!["x".to_owned()],
            vec!["1".to_owned(), "2".to_owned()],
        ] {
            assert!(parse_args(args.clone()).is_err(), "{args:?}");
        }
    }
}
