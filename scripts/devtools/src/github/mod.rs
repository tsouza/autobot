//! A minimal GitHub REST client.
//!
//! The token comes from the first non-empty value of `GITHUB_TOKEN` and `GH_TOKEN`; when both
//! are unset or empty, it is read from `<cli> auth token`, where `<cli>` is `AUTOBOT_GH_CLI` if
//! set, otherwise `gh`.

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
        return Err(Error::Http(format!("`{cli} auth token` printed no token")));
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
        let url = format!("{API}/repos/{}/{}", self.repo, path.trim_start_matches('/'));
        self.agent
            .get(&url)
            .header("Authorization", &format!("Bearer {}", self.token))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "autobot-devtools")
            .call()
            .map_err(|e| Error::Http(e.to_string()))?
            .body_mut()
            .read_json()
            .map_err(|e| Error::Http(e.to_string()))
    }
}
