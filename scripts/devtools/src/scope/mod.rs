//! The `scope` check: a pull request changes only the allowed paths of the task issue it
//! closes and the paths CONTRIBUTING lets every task change (CHARTER R-1).
//!
//! [`evaluate`] reads, from the GitHub API when the check runs, the pull request's body and
//! changed files, the task issue the body links ([`capsule::linked_issue`]), and that issue's
//! comments. A task issue is an issue labelled with one of [`TASK_LABELS`]: a task, or a
//! finding, which carries the same **Allowed paths** section. A changed path, and the previous
//! path of a renamed file, is allowed when it matches ([`glob`]):
//!
//! - an entry of the issue's **Allowed paths** ([`capsule::allowed`]);
//! - a glob named by a scope-extension comment on the issue ([`capsule::extension`]) whose
//!   author is the repository's owner, a member of its organization or a collaborator
//!   ([`EXTENDERS`]); a scope-extension comment by anyone else is reported and not counted;
//! - or an inherited path ([`inherited`]).
//!
//! The check fails, naming each path, when any path is allowed by none of them. It also fails
//! when the body links no issue, the issue does not exist, is a pull request or carries none
//! of the [`TASK_LABELS`], and when the file list reaches GitHub's limit of
//! [`GITHUB_FILE_LIMIT`] files, past which the changed paths cannot all be read. An **Allowed
//! paths** entry that is not a glob allows nothing and is printed.
//!
//! Choices the design leaves open:
//!
//! - **Inherited paths.** CONTRIBUTING's inherited paths are checked by path, and by line
//!   where the line is what makes them inherited: the `mod` line of a parent module and the
//!   `#[ignore = "awaiting #N"]` lines of a gate test group. Which `[workspace.dependencies]`
//!   entries of the root `Cargo.toml` change, and which entry of `all_controllers()` changes,
//!   is not read; the path is allowed. The testkit's [`INSTALLED`] registrations are read by
//!   line, because each is one `registry.register::<Port>(make);` line: the file may only
//!   gain such lines. Whether the port a line registers is the task's own is not read.
//! - **Who extends a scope.** Only the associations in [`EXTENDERS`] count, because anyone
//!   can comment on an issue of a public repository.
//! - **When an extension takes effect.** The check runs on pull request events; a
//!   scope-extension comment posted afterwards counts from the check's next run, which a
//!   re-run of the failed check starts.

pub mod capsule;
pub mod glob;

use crate::github::settings::Api;
use crate::github::{Client, Method, pages, pr_number};
use crate::{Error, Result};
use std::collections::BTreeSet;
use std::fmt;
use std::process::ExitCode;

/// The name of the workflow job, and of the required check.
pub const CONTEXT: &str = "scope";

/// The labels of the issues a pull request may close as its task: a task or a finding.
pub const TASK_LABELS: [&str; 2] = ["type:task", "finding"];

/// The comment author associations whose scope-extension comments count.
pub const EXTENDERS: [&str; 3] = ["OWNER", "MEMBER", "COLLABORATOR"];

/// The files GitHub lists for a pull request at most; a longer list is cut.
pub const GITHUB_FILE_LIMIT: usize = 3000;

/// The module file holding the controller registry `all_controllers()`.
pub const REGISTRY: &str = "crates/autobot-controllers/src/lib.rs";

/// The module file holding the testkit's installed port registrations.
pub const INSTALLED: &str = "crates/autobot-testkit/src/registry/installed.rs";

/// The registries a task adds its own entry to: each file, and the directory under which the
/// task's own globs must allow a changed path, the implementation the entry registers. A
/// registered port may be implemented in any crate.
pub const REGISTRIES: [(&str, &str); 2] = [
    (REGISTRY, "crates/autobot-controllers/src"),
    (INSTALLED, "crates"),
];

/// One file a pull request changes, as GitHub lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    /// The path after the change.
    pub path: String,
    /// The path before a rename.
    pub previous: Option<String>,
    /// GitHub's status: `added`, `modified`, `removed`, `renamed` and so on.
    pub status: String,
    /// The number of changed lines.
    pub changes: u64,
    /// The unified-diff hunks, absent when GitHub omits them (a binary or very large diff).
    pub patch: Option<String>,
}

/// Every file pull request `pr` changes, and whether the list reached [`GITHUB_FILE_LIMIT`],
/// so that GitHub may have cut it.
///
/// # Errors
/// Fails if a GitHub call fails or a file has no `filename`.
pub fn changed_files(api: &impl Api, pr: u64) -> Result<(Vec<ChangedFile>, bool)> {
    let files = pages(api, &format!("pulls/{pr}/files"))?;
    let cut = files.len() >= GITHUB_FILE_LIMIT;
    let files = files
        .iter()
        .map(|file| {
            let Some(path) = file["filename"].as_str() else {
                return Err(Error::Parse(format!(
                    "pulls/{pr}/files: a file has no `filename`"
                )));
            };
            Ok(ChangedFile {
                path: path.to_owned(),
                previous: file["previous_filename"].as_str().map(str::to_owned),
                status: file["status"].as_str().unwrap_or("changed").to_owned(),
                changes: file["changes"].as_u64().unwrap_or(0),
                patch: file["patch"].as_str().map(str::to_owned),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((files, cut))
}

/// Whether `error` is GitHub's answer for a resource that does not exist, as the client
/// reports a 404 status.
#[must_use]
pub fn is_not_found(error: &Error) -> bool {
    matches!(error, Error::Http(msg) if msg.ends_with("http status: 404"))
}

/// The scope of one task issue.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Task {
    /// The issue number.
    pub number: u64,
    /// The **Allowed paths** entries.
    pub allowed: capsule::Allowed,
    /// The globs the counted scope-extension comments name.
    pub extensions: Vec<String>,
    /// The author association of each scope-extension comment that is not counted.
    pub uncounted: Vec<String>,
}

impl Task {
    /// Whether `path` matches an **Allowed paths** entry or a scope extension.
    #[must_use]
    pub fn grants(&self, path: &str) -> bool {
        self.allowed
            .globs
            .iter()
            .chain(&self.extensions)
            .any(|g| glob::matches(g, path))
    }
}

/// Why a pull request has no task scope to be checked against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoTask {
    /// The body links no issue.
    Unlinked,
    /// The linked issue does not exist.
    Missing(u64),
    /// The linked number is a pull request.
    PullRequest(u64),
    /// The linked issue carries none of the [`TASK_LABELS`].
    NotATask(u64),
}

impl fmt::Display for NoTask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unlinked => write!(f, "its body links no issue with `Closes #N`"),
            Self::Missing(n) => write!(f, "it links #{n}, which does not exist"),
            Self::PullRequest(n) => write!(f, "it links #{n}, which is a pull request"),
            Self::NotATask(n) => {
                let [task, finding] = TASK_LABELS;
                write!(
                    f,
                    "it links #{n}, which carries neither the `{task}` nor the `{finding}` label"
                )
            }
        }
    }
}

/// Reads task issue `number`: its **Allowed paths** and its scope-extension comments.
///
/// # Errors
/// Fails if a GitHub call other than a 404 for the issue fails, or a response lacks a field
/// read here.
pub fn read_task(api: &impl Api, number: u64) -> Result<std::result::Result<Task, NoTask>> {
    let issue = match api.request(Method::Get, &format!("issues/{number}"), None) {
        Ok(issue) => issue,
        Err(e) if is_not_found(&e) => return Ok(Err(NoTask::Missing(number))),
        Err(e) => return Err(e),
    };
    if !issue["pull_request"].is_null() {
        return Ok(Err(NoTask::PullRequest(number)));
    }
    let Some(labels) = issue["labels"].as_array() else {
        return Err(Error::Parse(format!("issues/{number}: no `labels`")));
    };
    if !labels
        .iter()
        .any(|l| TASK_LABELS.iter().any(|t| l["name"] == *t))
    {
        return Ok(Err(NoTask::NotATask(number)));
    }
    let body = issue["body"].as_str().unwrap_or_default();
    let mut task = Task {
        number,
        allowed: capsule::section(body, "Allowed paths")
            .map(|s| capsule::allowed(&s))
            .unwrap_or_default(),
        ..Task::default()
    };
    for comment in pages(api, &format!("issues/{number}/comments"))? {
        let Some(globs) = comment["body"].as_str().and_then(capsule::extension) else {
            continue;
        };
        let association = comment["author_association"].as_str().unwrap_or("NONE");
        if EXTENDERS.contains(&association) {
            task.extensions.extend(globs);
        } else {
            task.uncounted.push(association.to_owned());
        }
    }
    Ok(Ok(task))
}

/// The lines a patch adds (`+`) or removes (`-`), with their sign.
fn changed_lines(patch: &str) -> impl Iterator<Item = (char, &str)> {
    patch.lines().filter_map(|l| {
        let mut chars = l.chars();
        match chars.next() {
            Some(sign @ ('+' | '-')) => Some((sign, chars.as_str())),
            _ => None,
        }
    })
}

/// The module name `line` declares when it is `mod <name>;`, `pub mod <name>;` or
/// `pub(crate) mod <name>;`, surrounding whitespace allowed.
fn declared_module(line: &str) -> Option<&str> {
    let t = line.trim();
    let t = t
        .strip_prefix("pub(crate) ")
        .or_else(|| t.strip_prefix("pub "))
        .unwrap_or(t);
    let name = t.strip_prefix("mod ")?.trim().strip_suffix(';')?.trim();
    let ident = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    ident.then_some(name)
}

/// Whether `patch` only adds declarations of the modules in `children`: it removes no line,
/// and every run of consecutive added lines holds at least one `mod` line naming one of
/// `children` ([`declared_module`]), with every other line of the run a `///` doc comment or
/// blank.
fn adds_only_child_declarations(patch: &str, children: &BTreeSet<&str>) -> bool {
    let mut runs: Vec<Vec<&str>> = Vec::new();
    let mut open = false;
    for line in patch.lines() {
        match line.strip_prefix('+') {
            Some(added) => {
                if !open {
                    runs.push(Vec::new());
                    open = true;
                }
                if let Some(run) = runs.last_mut() {
                    run.push(added);
                }
            }
            None if line.starts_with('-') => return false,
            None => open = false,
        }
    }
    !runs.is_empty()
        && runs.iter().all(|run| {
            let declares = |l: &&str| declared_module(l).is_some_and(|n| children.contains(n));
            run.iter().any(declares)
                && run.iter().all(|l| {
                    declares(l) || l.trim().is_empty() || l.trim_start().starts_with("///")
                })
        })
}

/// Whether `line` is one installed registration, `registry.register::<Port>(make);`,
/// surrounding whitespace allowed.
fn is_registration(line: &str) -> bool {
    line.trim()
        .strip_prefix("registry.register::<")
        .and_then(|rest| rest.strip_suffix(");"))
        .and_then(|rest| rest.split_once(">("))
        .is_some_and(|(port, make)| !port.trim().is_empty() && !make.trim().is_empty())
}

/// Whether `patch` only adds registrations to [`INSTALLED`]: it removes no line, adds at
/// least one [`is_registration`] line, and every other line it adds is blank.
fn adds_only_registrations(patch: &str) -> bool {
    let lines: Vec<(char, &str)> = changed_lines(patch).collect();
    lines.iter().any(|(_, l)| is_registration(l))
        && lines
            .iter()
            .all(|(sign, l)| *sign == '+' && (is_registration(l) || l.trim().is_empty()))
}

/// The module name of the Rust file `path`: its stem, or its directory's name for `mod.rs`.
fn module_name(path: &str) -> Option<&str> {
    let (dir, file) = path.rsplit_once('/')?;
    match file.strip_suffix(".rs")? {
        "mod" => dir.rsplit('/').next(),
        stem => Some(stem),
    }
}

/// The files that can declare a module whose file lies in directory `dir`.
fn parent_modules(dir: &str) -> Vec<String> {
    let mut out: Vec<String> = ["mod.rs", "lib.rs", "main.rs"]
        .iter()
        .map(|f| format!("{dir}/{f}"))
        .collect();
    out.push(format!("{dir}.rs"));
    out
}

/// The files that can declare the module of the Rust file `path`.
fn declaring_files(path: &str) -> Vec<String> {
    let Some((dir, file)) = path.rsplit_once('/') else {
        return Vec::new();
    };
    if file == "mod.rs" {
        dir.rsplit_once('/')
            .map_or_else(Vec::new, |(parent, _)| parent_modules(parent))
    } else {
        parent_modules(dir)
    }
}

/// Whether `path` lies in a gate test group, `crates/<crate>/tests/g_<group>/`.
fn in_gate_group(path: &str) -> bool {
    let segments: Vec<&str> = path.split('/').collect();
    segments.len() > 4
        && segments[0] == "crates"
        && segments[2] == "tests"
        && segments[3].starts_with("g_")
}

/// Whether `path`, changed by `file`, is a path CONTRIBUTING lets every task change, given
/// every file of the pull request and its `task`:
///
/// - `Cargo.lock` and the root `Cargo.toml` (its `[workspace.dependencies]` entries);
/// - `<dir>/Cargo.toml` when the task's own globs allow a changed path under `<dir>/`;
/// - a file of [`REGISTRIES`] when the task's own globs allow a changed path under that
///   registry's directory, and for [`INSTALLED`] only when it removes no line and adds only
///   `registry.register::<Port>(make);` lines and blank lines;
/// - the parent module of a Rust file the pull request adds under the task's own globs, when
///   it removes no line and adds only `mod` declarations of such files, each with the `///`
///   doc comments and blank lines added next to it;
/// - a modified file of a gate test group when every changed line is a removed
///   `#[ignore = "awaiting #N"]` line naming the task.
#[must_use]
pub fn inherited(path: &str, file: &ChangedFile, files: &[ChangedFile], task: &Task) -> bool {
    if path == "Cargo.lock" || path == "Cargo.toml" {
        return true;
    }
    let granted_under = |dir: &str| {
        files
            .iter()
            .any(|f| f.path.starts_with(&format!("{dir}/")) && task.grants(&f.path))
    };
    if let Some(dir) = path.strip_suffix("/Cargo.toml")
        && granted_under(dir)
    {
        return true;
    }
    let patch = file.patch.as_deref();
    if let Some((registry, dir)) = REGISTRIES.iter().find(|(registry, _)| *registry == path)
        && granted_under(dir)
        && (*registry != INSTALLED || patch.is_some_and(adds_only_registrations))
    {
        return true;
    }
    let children: BTreeSet<&str> = files
        .iter()
        .filter(|f| {
            f.status == "added"
                && task.grants(&f.path)
                && declaring_files(&f.path).iter().any(|p| p == path)
        })
        .filter_map(|f| module_name(&f.path))
        .collect();
    if !children.is_empty() && patch.is_some_and(|p| adds_only_child_declarations(p, &children)) {
        return true;
    }
    if in_gate_group(path) && file.status == "modified" {
        let marker = format!("#[ignore = \"awaiting #{}\"]", task.number);
        return patch.is_some_and(|p| {
            let mut lines = changed_lines(p).peekable();
            lines.peek().is_some() && lines.all(|(sign, l)| sign == '-' && l.trim() == marker)
        });
    }
    false
}

/// The changed paths, new and previous, that `task` does not allow, sorted.
#[must_use]
pub fn outside(files: &[ChangedFile], task: &Task) -> Vec<String> {
    let mut out = BTreeSet::new();
    for file in files {
        for path in std::iter::once(&file.path).chain(&file.previous) {
            if !task.grants(path) && !inherited(path, file, files, task) {
                out.insert(path.clone());
            }
        }
    }
    out.into_iter().collect()
}

/// The `scope` check's judgement of one pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    /// The pull request number.
    pub pr: u64,
    /// The task issue, or why there is none.
    pub task: std::result::Result<Task, NoTask>,
    /// The changed paths the task does not allow.
    pub outside: Vec<String>,
    /// Whether the file list reached [`GITHUB_FILE_LIMIT`].
    pub cut: bool,
}

impl Verdict {
    /// Whether the pull request passes.
    #[must_use]
    pub fn passes(&self) -> bool {
        self.task.is_ok() && self.outside.is_empty() && !self.cut
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let pr = self.pr;
        let task = match &self.task {
            Ok(task) => task,
            Err(why) => return writeln!(f, "{CONTEXT}: #{pr} closes no task issue: {why}"),
        };
        let n = task.number;
        writeln!(f, "{CONTEXT}: #{pr} closes task #{n}")?;
        for entry in &task.allowed.ignored {
            writeln!(
                f,
                "{CONTEXT}: the Allowed paths entry `{entry}` of #{n} is not a path or glob and \
                 allows nothing; a qualifier belongs in Non-goals"
            )?;
        }
        if !task.extensions.is_empty() {
            writeln!(
                f,
                "{CONTEXT}: scope extensions on #{n} allow {}",
                task.extensions.join(", ")
            )?;
        }
        for association in &task.uncounted {
            writeln!(
                f,
                "{CONTEXT}: a scope-extension comment on #{n} by an author with association \
                 {association} is not counted"
            )?;
        }
        if self.cut {
            writeln!(
                f,
                "{CONTEXT}: GitHub lists at most {GITHUB_FILE_LIMIT} files, so not every \
                 changed path of #{pr} can be checked"
            )?;
        }
        for path in &self.outside {
            writeln!(f, "{path}: outside the allowed paths of #{n}")?;
        }
        if self.outside.is_empty() {
            writeln!(
                f,
                "{CONTEXT}: every changed path GitHub lists for #{pr} is allowed by #{n}"
            )
        } else {
            writeln!(
                f,
                "{CONTEXT}: #{pr} changes {} path(s) outside the allowed paths of #{n}",
                self.outside.len()
            )
        }
    }
}

/// Reads pull request `pr`, its changed files and its task issue, and judges it.
///
/// # Errors
/// Fails if a GitHub call fails (other than a 404 for the linked issue) or a response lacks a
/// field read here.
pub fn evaluate(api: &impl Api, pr: u64) -> Result<Verdict> {
    let pull = api.request(Method::Get, &format!("pulls/{pr}"), None)?;
    let body = pull["body"].as_str().unwrap_or_default();
    let task = match capsule::linked_issue(body) {
        Some(n) => read_task(api, n)?,
        None => Err(NoTask::Unlinked),
    };
    let (files, cut) = changed_files(api, pr)?;
    let outside = task
        .as_ref()
        .map_or_else(|_| Vec::new(), |task| outside(&files, task));
    Ok(Verdict {
        pr,
        task,
        outside,
        cut,
    })
}

/// Entry point of `scripts/scope.rs`: judges the pull request whose number is the only
/// argument, prints the [`Verdict`], and returns the exit code. The repository is resolved by
/// [`crate::github::repository`] from the current directory.
///
/// # Errors
/// Fails on a missing or non-numeric argument, an unresolvable repository or token, or a
/// failed GitHub call.
pub fn run(args: impl IntoIterator<Item = String>) -> Result<ExitCode> {
    let pr = pr_number(args, "scope")?;
    let repo = crate::github::repository(".")?;
    let verdict = evaluate(&Client::new(repo)?, pr)?;
    print!("{verdict}");
    Ok(if verdict.passes() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

#[cfg(test)]
mod tests;
