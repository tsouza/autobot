//! Git helpers.

use crate::Result;
use crate::process::Cmd;
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
