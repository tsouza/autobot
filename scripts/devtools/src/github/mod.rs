//! A minimal GitHub REST client.
//!
//! The token comes from `GITHUB_TOKEN` or `GH_TOKEN`; when neither is set, it is read from
//! `<cli> auth token`, where `<cli>` is `AUTOBOT_GH_CLI` if set, otherwise `gh`.

use crate::process::Cmd;
use crate::{Error, Result};

const API: &str = "https://api.github.com";

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
        let token = match std::env::var("GITHUB_TOKEN").or_else(|_| std::env::var("GH_TOKEN")) {
            Ok(token) if !token.is_empty() => token,
            _ => {
                let cli = std::env::var("AUTOBOT_GH_CLI").unwrap_or_else(|_| "gh".to_owned());
                Cmd::new(cli)
                    .args(["auth", "token"])
                    .output()?
                    .trim()
                    .to_owned()
            }
        };
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
