//! A minimal GitHub REST and GraphQL client.
//!
//! The token comes from the first non-empty value of `GITHUB_TOKEN` and `GH_TOKEN`; when both
//! are unset or empty, creating a client fails.
//!
//! The repository is resolved once, by [`repository`]: `GITHUB_REPOSITORY` when it is set
//! and not blank (the value is trimmed), otherwise the repository the `origin` remote points at.
//!
//! The pull request scripts share [`pr_number`] for their only argument and [`pages`] for
//! reading a paged list resource.

pub mod automerge;
pub mod graph;
pub mod label_gate;
pub mod main_red;
pub mod settings;
pub mod verdict;

use crate::{Error, Result, git};
use serde_json::Value;
use settings::Api;
use std::path::Path;

const API: &str = "https://api.github.com";
const GRAPHQL: &str = "https://api.github.com/graphql";

/// Items requested per page by [`pages`]; a shorter page is the last one.
pub const PAGE_SIZE: usize = 100;

/// Every item of the paged list resource `path` relative to the repository (for example
/// `pulls/12/files`), in the order GitHub returns them. Pages of [`PAGE_SIZE`] items are read
/// until one is shorter, so a list whose length is a multiple of [`PAGE_SIZE`] ends with an
/// empty page.
///
/// # Errors
/// Fails if a GitHub call fails or a page is not a JSON array.
pub fn pages(api: &impl Api, path: &str) -> Result<Vec<Value>> {
    let mut all = Vec::new();
    for page in 1.. {
        let path = format!("{path}?per_page={PAGE_SIZE}&page={page}");
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

/// The pull request number that is the only element of `args`; `script` names the script in
/// the usage error.
///
/// # Errors
/// Fails when `args` is empty, has more than one element, or its element is not a number.
pub fn pr_number(args: impl IntoIterator<Item = String>, script: &str) -> Result<u64> {
    let mut args = args.into_iter();
    match (args.next(), args.next()) {
        (Some(pr), None) => pr
            .parse()
            .map_err(|_| Error::Parse(format!("not a pull request number: `{pr}`"))),
        _ => Err(Error::Parse(format!(
            "usage: {script} <pull-request-number>"
        ))),
    }
}

/// Resolves the API token from an environment lookup: the first non-empty (after trimming)
/// value of `GITHUB_TOKEN` and `GH_TOKEN`, trimmed.
///
/// # Errors
/// Fails if both are unset or empty.
pub fn resolve_token(env: impl Fn(&str) -> Option<String>) -> Result<String> {
    let set = |key: &str| env(key).filter(|v| !v.trim().is_empty());
    set("GITHUB_TOKEN")
        .or_else(|| set("GH_TOKEN"))
        .map(|token| token.trim().to_owned())
        .ok_or_else(|| Error::Token("set `GITHUB_TOKEN` or `GH_TOKEN`".to_owned()))
}

/// Resolves the `owner/name` of the repository: `env` (the value of `GITHUB_REPOSITORY`)
/// when it is set and not blank, otherwise the repository the URL returned by `remote`
/// points at, parsed by [`git::remote_repo`]. `remote` runs only when `env` is unusable.
///
/// # Errors
/// Fails if `remote` fails or returns a URL that does not name `owner/name`.
pub fn resolve_repo(
    env: Option<String>,
    remote: impl FnOnce() -> Result<String>,
) -> Result<String> {
    match env.map(|repo| repo.trim().to_owned()) {
        Some(repo) if !repo.is_empty() => Ok(repo),
        _ => git::remote_repo(&remote()?),
    }
}

/// The `owner/name` of the repository the scripts act on, resolved by [`resolve_repo`] from
/// `GITHUB_REPOSITORY` and the `origin` remote of the checkout containing `dir`.
///
/// # Errors
/// Fails if `GITHUB_REPOSITORY` is unset or blank and the `origin` remote is missing or does
/// not name `owner/name`.
pub fn repository(dir: impl AsRef<Path>) -> Result<String> {
    resolve_repo(std::env::var("GITHUB_REPOSITORY").ok(), || {
        git::origin_url(dir)
    })
}

/// An authenticated GitHub API client for one repository.
#[derive(Debug)]
pub struct Client {
    agent: ureq::Agent,
    token: String,
    repo: String,
}

impl Client {
    /// Creates a client for `owner/name`, resolving the token as described in the module docs.
    ///
    /// # Errors
    /// Fails if no token can be found.
    pub fn new(repo: impl Into<String>) -> Result<Self> {
        let token = resolve_token(|key| std::env::var(key).ok())?;
        Ok(Self {
            agent: ureq::Agent::new_with_defaults(),
            token,
            repo: repo.into(),
        })
    }

    /// GETs `path` relative to the repository (for example `pulls/12`) and returns the JSON body.
    ///
    /// # Errors
    /// Fails on a transport error, a non-success status, or a non-JSON body.
    pub fn get(&self, path: &str) -> Result<serde_json::Value> {
        self.request(Method::Get, path, None)
    }

    /// Sends `method` to `path` relative to the repository resource (`""` is the repository
    /// itself, `"pulls/12"` a pull request) with an optional JSON body, and returns the JSON
    /// response.
    ///
    /// # Errors
    /// Fails on a transport error, a non-success status, or a non-JSON body.
    pub fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let path = path.trim_start_matches('/');
        let url = if path.is_empty() {
            format!("{API}/repos/{}", self.repo)
        } else {
            format!("{API}/repos/{}/{path}", self.repo)
        };
        self.send(method, &url, body)
    }

    /// POSTs a GraphQL `query` (or mutation) with its `variables` and returns the `data` of
    /// the response, as [`graphql_data`] extracts it.
    ///
    /// # Errors
    /// Fails on a transport error, a non-success status, a non-JSON body, or a response that
    /// carries GraphQL errors or no `data`.
    pub fn graphql(&self, query: &str, variables: &Value) -> Result<Value> {
        let body = serde_json::json!({ "query": query, "variables": variables });
        graphql_data(self.send(Method::Post, GRAPHQL, Some(&body))?)
    }

    /// Sends `method` to the absolute `url` with the headers every request carries.
    fn send(&self, method: Method, url: &str, body: Option<&Value>) -> Result<Value> {
        let auth = format!("Bearer {}", self.token);
        let http = |e: ureq::Error| Error::Http(format!("{method:?} {url}: {e}"));
        let response = match method {
            Method::Get => headers(self.agent.get(url), &auth).call(),
            Method::Post => headers(self.agent.post(url), &auth).send_json(body),
            Method::Put => headers(self.agent.put(url), &auth).send_json(body),
            Method::Patch => headers(self.agent.patch(url), &auth).send_json(body),
        };
        response.map_err(http)?.body_mut().read_json().map_err(http)
    }
}

/// The `data` of a GraphQL response. GitHub answers a failed query with a success status and
/// an `errors` array, so a response with a non-empty `errors` is an error, whose message joins
/// every error's `message`; so is a response without `data`.
///
/// # Errors
/// Fails as described above.
pub fn graphql_data(mut response: Value) -> Result<Value> {
    if let Some(errors) = response["errors"].as_array().filter(|e| !e.is_empty()) {
        let messages: Vec<&str> = errors
            .iter()
            .map(|e| e["message"].as_str().unwrap_or("error without a message"))
            .collect();
        return Err(Error::Http(format!("GraphQL: {}", messages.join("; "))));
    }
    match response.get_mut("data").map(Value::take) {
        Some(data) if !data.is_null() => Ok(data),
        _ => Err(Error::Parse("GraphQL response has no `data`".to_owned())),
    }
}

/// The GraphQL calls a module makes, so tests can serve recorded responses in place of
/// [`Client`].
pub trait GraphQl {
    /// Runs `query` with `variables` and returns the `data` of the response.
    ///
    /// # Errors
    /// Fails as [`Client::graphql`] does.
    fn graphql(&self, query: &str, variables: &Value) -> Result<Value>;
}

impl GraphQl for Client {
    fn graphql(&self, query: &str, variables: &Value) -> Result<Value> {
        Client::graphql(self, query, variables)
    }
}

/// An HTTP method used by [`Client::request`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// Read a resource.
    Get,
    /// Create a resource.
    Post,
    /// Replace a resource.
    Put,
    /// Update some fields of a resource.
    Patch,
}

/// Adds the authentication and API-version headers every request carries.
fn headers<B>(req: ureq::RequestBuilder<B>, auth: &str) -> ureq::RequestBuilder<B> {
    req.header("Authorization", auth)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "autobot-devtools")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::{Cell, RefCell};
    use std::collections::BTreeMap;

    // Recorded from `automerge::PULL_REQUEST_ID` with number 236, and with
    // number 99999, which does not exist; shared with the `automerge` tests.
    pub(super) const FOUND: &str = r#"{"data":{"repository":{"pullRequest":{"id":"PR_kwDOUmSub88AAAABEq1tqw","number":236}}}}"#;
    pub(super) const NOT_FOUND: &str = r#"{"data":{"repository":{"pullRequest":null}},"errors":[{"type":"NOT_FOUND","path":["repository","pullRequest"],"locations":[{"line":1,"column":91}],"message":"Could not resolve to a PullRequest with the number of 99999."}]}"#;

    const ALIAS_REMOTE: &str = "git@github.com-alias:octo-org/autobot.git\n";

    #[test]
    fn resolve_repo_prefers_the_environment_variable() {
        let asked = Cell::new(false);
        let remote = || {
            asked.set(true);
            Ok(ALIAS_REMOTE.to_owned())
        };
        assert_eq!(resolve_repo(Some("a/b".into()), remote).unwrap(), "a/b");
        assert!(
            !asked.get(),
            "the remote must not be read when the variable is set"
        );
    }

    #[test]
    fn resolve_repo_falls_back_to_ssh_https_and_host_alias_remotes() {
        for url in [
            "git@github.com:octo-org/autobot.git",
            "https://github.com/octo-org/autobot",
            ALIAS_REMOTE,
        ] {
            for env in [None, Some(String::new()), Some("  ".to_owned())] {
                let got = resolve_repo(env.clone(), || Ok(url.to_owned())).unwrap();
                assert_eq!(got, "octo-org/autobot", "{url} with {env:?}");
            }
        }
    }

    #[test]
    fn resolve_repo_reports_a_bad_or_missing_remote() {
        let err = resolve_repo(None, || Ok("/local/autobot".to_owned())).unwrap_err();
        assert!(err.to_string().contains("not a GitHub remote URL"), "{err}");
        let err = resolve_repo(None, || Err(Error::Parse("no origin".into()))).unwrap_err();
        assert!(err.to_string().contains("no origin"), "{err}");
    }

    /// Serves GET responses by path, fails an unknown path the way GitHub answers an unknown
    /// resource, and records every path asked for.
    struct Pages {
        responses: BTreeMap<String, Value>,
        gets: RefCell<Vec<String>>,
    }

    impl Api for Pages {
        fn request(&self, method: Method, path: &str, _body: Option<&Value>) -> Result<Value> {
            assert_eq!(method, Method::Get, "paging only reads");
            self.gets.borrow_mut().push(path.to_owned());
            self.responses
                .get(path)
                .cloned()
                .ok_or_else(|| Error::Http(format!("404 {path}")))
        }
    }

    fn numbered(range: std::ops::Range<usize>) -> Value {
        range.map(|n| json!({ "n": n })).collect()
    }

    #[test]
    fn pages_reads_until_a_short_page_and_keeps_order() {
        let api = Pages {
            responses: BTreeMap::from([
                (
                    "pulls/7/files?per_page=100&page=1".to_owned(),
                    numbered(0..100),
                ),
                (
                    "pulls/7/files?per_page=100&page=2".to_owned(),
                    numbered(100..103),
                ),
            ]),
            gets: RefCell::new(Vec::new()),
        };
        let items = pages(&api, "pulls/7/files").unwrap();
        assert_eq!(Value::Array(items), numbered(0..103));
        assert_eq!(api.gets.borrow().len(), 2);
    }

    #[test]
    fn pages_reads_the_empty_page_after_an_exact_multiple() {
        let api = Pages {
            responses: BTreeMap::from([
                (
                    "issues/7/comments?per_page=100&page=1".to_owned(),
                    numbered(0..100),
                ),
                (
                    "issues/7/comments?per_page=100&page=2".to_owned(),
                    json!([]),
                ),
            ]),
            gets: RefCell::new(Vec::new()),
        };
        assert_eq!(pages(&api, "issues/7/comments").unwrap().len(), 100);
        assert_eq!(
            *api.gets.borrow(),
            [
                "issues/7/comments?per_page=100&page=1",
                "issues/7/comments?per_page=100&page=2",
            ]
        );
    }

    #[test]
    fn pages_reports_a_page_that_is_not_an_array_and_a_failed_call() {
        let api = Pages {
            responses: BTreeMap::from([(
                "pulls/7/files?per_page=100&page=1".to_owned(),
                json!({"message": "Not Found"}),
            )]),
            gets: RefCell::new(Vec::new()),
        };
        let err = pages(&api, "pulls/7/files").unwrap_err();
        assert_eq!(
            err.to_string(),
            Error::Parse("pulls/7/files?per_page=100&page=1: not a JSON array".into()).to_string()
        );
        let err = pages(&api, "pulls/8/files").unwrap_err();
        assert!(err.to_string().contains("404 pulls/8/files"), "{err}");
    }

    #[test]
    fn pr_number_takes_exactly_one_number_and_names_the_script() {
        assert_eq!(pr_number(["52".to_owned()], "label_gate").unwrap(), 52);
        let err = pr_number(Vec::new(), "label_gate").unwrap_err();
        assert!(
            err.to_string()
                .contains("usage: label_gate <pull-request-number>"),
            "{err}"
        );
        let err = pr_number(["1".to_owned(), "2".to_owned()], "review_gate").unwrap_err();
        assert!(
            err.to_string()
                .contains("usage: review_gate <pull-request-number>"),
            "{err}"
        );
        let err = pr_number(["x".to_owned()], "review_gate").unwrap_err();
        assert!(
            err.to_string().contains("not a pull request number: `x`"),
            "{err}"
        );
    }

    #[test]
    fn graphql_data_returns_the_data_of_a_successful_response() {
        let data = graphql_data(serde_json::from_str(FOUND).unwrap()).unwrap();
        assert_eq!(
            data,
            serde_json::json!({"repository": {"pullRequest": {"id": "PR_kwDOUmSub88AAAABEq1tqw", "number": 236}}})
        );
    }

    #[test]
    fn graphql_errors_fail_even_with_data_present() {
        let err = graphql_data(serde_json::from_str(NOT_FOUND).unwrap()).unwrap_err();
        assert_eq!(
            err.to_string(),
            "GitHub API error: GraphQL: Could not resolve to a PullRequest with the number of 99999."
        );
        let two = serde_json::json!({"errors": [{"message": "a"}, {"type": "X"}]});
        let err = graphql_data(two).unwrap_err();
        assert!(
            err.to_string()
                .ends_with("GraphQL: a; error without a message"),
            "{err}"
        );
    }

    #[test]
    fn graphql_response_without_data_is_an_error() {
        for response in [
            serde_json::json!({}),
            serde_json::json!({"data": null, "errors": []}),
        ] {
            let err = graphql_data(response.clone()).unwrap_err();
            assert!(err.to_string().contains("no `data`"), "{response}");
        }
    }
}
