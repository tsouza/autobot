//! Git helpers.

use crate::process::Cmd;
use crate::{Error, Result};
use std::path::Path;

/// The merge base of `a` and `b`.
///
/// # Errors
/// Fails if git fails, for example when a revision does not exist.
pub fn merge_base(dir: impl AsRef<Path>, a: &str, b: &str) -> Result<String> {
    Ok(Cmd::new("git")
        .args(["merge-base", a, b])
        .current_dir(dir)
        .output()?
        .trim()
        .to_owned())
}

/// Paths changed between the merge base of `base` and `head`, and `head`.
///
/// # Errors
/// Fails if git fails.
pub fn changed_paths(dir: impl AsRef<Path>, base: &str, head: &str) -> Result<Vec<String>> {
    let range = format!("{base}...{head}");
    let out = Cmd::new("git")
        .args(["diff", "--name-only", &range])
        .current_dir(dir)
        .output()?;
    Ok(out
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect())
}

/// The top-level directory of the working tree containing `dir`.
///
/// # Errors
/// Fails if `dir` is not inside a git working tree.
pub fn toplevel(dir: impl AsRef<Path>) -> Result<String> {
    Ok(Cmd::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(dir)
        .output()?
        .trim()
        .to_owned())
}

/// The URL of the `origin` remote of the repository containing `dir`, trimmed.
///
/// # Errors
/// Fails if git fails, for example when there is no `origin` remote.
pub fn origin_url(dir: impl AsRef<Path>) -> Result<String> {
    Ok(Cmd::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(dir)
        .output()?
        .trim()
        .to_owned())
}

/// The `owner/name` a remote URL points at, whatever the host or host alias: HTTPS or SSH
/// URL form (`https://github.com/owner/name(.git)`, `ssh://git@host/owner/name(.git)`) or
/// scp-like form (`git@host:owner/name(.git)`, `git@github.com-alias:owner/name`).
///
/// # Errors
/// Fails if the URL has neither a scheme nor a `host:` prefix (a local path), or its path
/// does not end in a non-empty `owner/name`.
pub fn remote_repo(url: &str) -> Result<String> {
    let invalid = || Error::Parse(format!("not a GitHub remote URL: `{}`", url.trim()));
    let trimmed = url.trim().trim_end_matches('/');
    let trimmed = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    let path = match trimmed.split_once("://") {
        Some((_, rest)) => rest.split_once('/').ok_or_else(invalid)?.1,
        None => trimmed.split_once(':').ok_or_else(invalid)?.1,
    };
    let mut parts = path.rsplit('/');
    match (parts.next(), parts.next()) {
        (Some(name), Some(owner)) if !name.is_empty() && !owner.is_empty() => {
            Ok(format!("{owner}/{name}"))
        }
        _ => Err(invalid()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_repo_reads_https_ssh_and_host_alias_urls() {
        for url in [
            "https://github.com/tsouza/autobot.git",
            "https://github.com/tsouza/autobot/",
            "ssh://git@github.com/tsouza/autobot.git",
            "git@github.com:tsouza/autobot.git",
            "git@github.com:tsouza/autobot",
            "git@github.com-tsouza:tsouza/autobot.git\n",
        ] {
            assert_eq!(remote_repo(url).unwrap(), "tsouza/autobot", "{url}");
        }
    }

    #[test]
    fn remote_repo_rejects_urls_without_owner_and_name() {
        for url in [
            "",
            "/local/path/autobot",
            "git@github.com:autobot.git",
            "https://github.com/autobot",
            "https://github.com",
        ] {
            let err = remote_repo(url).unwrap_err();
            assert!(
                err.to_string().contains("not a GitHub remote URL"),
                "{url}: {err}"
            );
        }
    }
}
