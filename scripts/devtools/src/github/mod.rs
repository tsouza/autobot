//! A minimal GitHub REST client.
//!
//! The token comes from the first non-empty value of `GITHUB_TOKEN` and `GH_TOKEN`; when both
//! are unset or empty, it is read from `<cli> auth token`, where `<cli>` is `AUTOBOT_GH_CLI` if
//! set, otherwise `gh`.

pub mod settings;
pub mod verdict;
pub mod graph;

use crate::process::Cmd;
use crate::{Error, Result};

const API: &str = "https://api.github.com";

/// Resolves the API token from an environment lookup and a CLI fallback.
///
/// # Errors
/// Fails if the CLI fails or prints an empty token.
pub fn resolve_token(
    env: impl Fn(&str) -> Option<String>,
    cli_token: impl FnOnce(&str) -> Result<String>,
) -> Result<String> {
    let set = |key: &str| env(key).filter(|v| !v.trim().is_empty());
    if let Some(token) = set("GITHUB_TOKEN").or_else(|| set("GH_TOKEN")) {
        return Ok(token.trim().to_owned());
    }
    let cli = set("AUTOBOT_GH_CLI").unwrap_or_else(|| "gh".to_owned());
    let token = cli_token(&cli)?.trim().to_owned();
    if token.is_empty() {
        return Err(Error::Token(format!("`{cli} auth token` printed no token")));
    }
    Ok(token)
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
        let token = resolve_token(
            |key| std::env::var(key).ok(),
            |cli| Cmd::new(cli).args(["auth", "token"]).output(),
        )?;
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
        let auth = format!("Bearer {}", self.token);
        let http = |e: ureq::Error| Error::Http(format!("{method:?} {url}: {e}"));
        let response = match method {
            Method::Get => headers(self.agent.get(&url), &auth).call(),
            Method::Post => headers(self.agent.post(&url), &auth).send_json(body),
            Method::Put => headers(self.agent.put(&url), &auth).send_json(body),
            Method::Patch => headers(self.agent.patch(&url), &auth).send_json(body),
        };
        response.map_err(http)?.body_mut().read_json().map_err(http)
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
