//! One git worktree per pull request, under `.worktrees/` in the main working tree.
//!
//! A worktree for issue `N` lives at `.worktrees/N-<slug>` on branch `N-<slug>`, created from
//! `origin/main`; the slug comes from the issue title. `.worktrees` may be a symlink to
//! another volume: it is followed, and nothing is created when its target is missing. A
//! plain `.worktrees` directory is created when absent.
//!
//! The machine-local file `~/.config/autobot/local.toml` is optional. When it sets
//! `require_mount_uuid`, the filesystem holding the resolved worktree directory must carry
//! that UUID, as reported by `findmnt -no UUID --target <dir>`. Only top-level
//! `key = "string"` lines of that file are read.
//!
//! Refusals are reported as [`Error::Command`] naming the refused operation.

use crate::process::Cmd;
use crate::{Error, Result};
use std::path::{Path, PathBuf};

/// Longest slug `new` derives from an issue title, in bytes.
pub const MAX_SLUG_LEN: usize = 40;

/// The directory holding the machine-local AutoBot settings: `$HOME/.config/autobot`.
///
/// # Errors
/// Fails if `HOME` is not set.
pub fn local_config_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".config").join("autobot"))
        .ok_or_else(|| refusal("locate ~/.config/autobot", "HOME is not set"))
}

/// Machine-local settings read from `local.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    /// UUID the filesystem holding the worktree directory must have.
    pub require_mount_uuid: Option<String>,
}

impl Settings {
    /// Reads the settings file at `path`; an absent file yields the defaults.
    ///
    /// # Errors
    /// Fails if the file exists but cannot be read or parsed.
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(io_error(&format!("read {}", path.display()), &e)),
        }
    }

    /// Parses the settings from the text of `local.toml`.
    ///
    /// # Errors
    /// Fails if `require_mount_uuid` is present but its value is not a basic string.
    pub fn parse(text: &str) -> Result<Self> {
        Ok(Self {
            require_mount_uuid: top_level_string(text, "require_mount_uuid")?,
        })
    }
}

/// The value of top-level key `key` in a flat TOML document, if it is present.
fn top_level_string(text: &str, key: &str) -> Result<Option<String>> {
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            break;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        if k.trim() != key {
            continue;
        }
        let v = v.trim();
        let value = v
            .strip_prefix('"')
            .and_then(|rest| rest.split_once('"'))
            .filter(|(inner, tail)| {
                !inner.contains('\\') && {
                    let tail = tail.trim();
                    tail.is_empty() || tail.starts_with('#')
                }
            })
            .map(|(inner, _)| inner.to_owned())
            .ok_or_else(|| Error::Parse(format!("`{key}` must be a plain string, got `{v}`")))?;
        return Ok(Some(value));
    }
    Ok(None)
}

/// The UUID of the filesystem holding `path`, from `findmnt -no UUID --target <path>`.
///
/// Returns `None` when that filesystem has no UUID.
///
/// # Errors
/// Fails if `findmnt` fails.
pub fn findmnt_uuid(path: &Path) -> Result<Option<String>> {
    let out = Cmd::new("findmnt")
        .args(["-no", "UUID", "--target"])
        .args([path.to_string_lossy()])
        .output()?;
    let uuid = out.trim();
    Ok((!uuid.is_empty()).then(|| uuid.to_owned()))
}

/// The branch and directory name for issue `issue`: `<issue>-<slug of title>`.
///
/// The slug keeps the ASCII letters and digits of the title, lowercased, with every other
/// run of characters turned into one `-`, and is cut at a word boundary to at most
/// [`MAX_SLUG_LEN`] bytes.
///
/// # Errors
/// Fails if the title has no ASCII letter or digit.
pub fn branch_name(issue: u64, title: &str) -> Result<String> {
    let mut slug = String::new();
    for word in title
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        let extra = usize::from(!slug.is_empty()) + word.len();
        if !slug.is_empty() && slug.len() + extra > MAX_SLUG_LEN {
            break;
        }
        if !slug.is_empty() {
            slug.push('-');
        }
        slug.push_str(&word.to_ascii_lowercase());
    }
    slug.truncate(MAX_SLUG_LEN);
    if slug.is_empty() {
        return Err(refusal(
            &format!("wt new {issue}"),
            "the issue title has no ASCII letter or digit to build a slug from",
        ));
    }
    Ok(format!("{issue}-{slug}"))
}

/// The main working tree of the repository containing `dir`, even when `dir` is inside a
/// linked worktree.
///
/// # Errors
/// Fails if `dir` is not inside a git repository with a working tree.
pub fn main_worktree(dir: &Path) -> Result<PathBuf> {
    let common = Cmd::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(dir)
        .output()?;
    Path::new(common.trim())
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| Error::Parse(format!("git common dir `{}` has no parent", common.trim())))
}

/// Resolves `path` through symlinks and, when `want` is set, checks that the filesystem
/// holding it has that UUID according to `mount_uuid`. Returns the resolved path.
///
/// Refusals are reported under operation `op`.
///
/// # Errors
/// Fails if `path` does not lead to an existing directory, or if `want` is set and the UUID
/// differs or is unknown.
pub fn check_volume(
    op: &str,
    path: &Path,
    want: Option<&str>,
    mount_uuid: &dyn Fn(&Path) -> Result<Option<String>>,
) -> Result<PathBuf> {
    let resolved = std::fs::canonicalize(path)
        .map_err(|e| e.to_string())
        .and_then(|p| {
            if p.is_dir() {
                Ok(p)
            } else {
                Err(format!("{} is not a directory", p.display()))
            }
        })
        .map_err(|why| {
            refusal(
                op,
                format!(
                    "{} does not lead to an existing directory ({why})",
                    path.display()
                ),
            )
        })?;
    if let Some(want) = want {
        let got = mount_uuid(&resolved)?;
        if got.as_deref() != Some(want) {
            return Err(refusal(
                op,
                format!(
                    "{} is on a filesystem with UUID {}, but local.toml requires {want}; \
                     is the volume mounted?",
                    resolved.display(),
                    got.as_deref().unwrap_or("<none>"),
                ),
            ));
        }
    }
    Ok(resolved)
}

/// Resolves `<main worktree>/.worktrees` and applies the volume check (see [`check_volume`]).
///
/// Creates `.worktrees` as a directory when nothing exists at that path.
///
/// # Errors
/// Fails if `.worktrees` cannot be created, or on any refusal of [`check_volume`] with
/// `settings.require_mount_uuid` as the wanted UUID.
pub fn worktrees_dir(
    main: &Path,
    settings: &Settings,
    mount_uuid: &dyn Fn(&Path) -> Result<Option<String>>,
) -> Result<PathBuf> {
    let link = main.join(".worktrees");
    if std::fs::symlink_metadata(&link).is_err() {
        std::fs::create_dir(&link)
            .map_err(|e| io_error(&format!("create {}", link.display()), &e))?;
    }
    check_volume(
        "check the worktree volume",
        &link,
        settings.require_mount_uuid.as_deref(),
        mount_uuid,
    )
}

/// Creates the worktree for issue `issue` titled `title` and returns its path.
///
/// Resolves and checks the worktree directory first (see [`worktrees_dir`]), then fetches
/// `origin` and adds branch `<issue>-<slug>` from `origin/main` without an upstream.
///
/// # Errors
/// Fails on any refusal of [`branch_name`] or [`worktrees_dir`], or if git fails, for
/// example when the branch already exists.
pub fn new(
    repo: &Path,
    issue: u64,
    title: &str,
    settings: &Settings,
    mount_uuid: &dyn Fn(&Path) -> Result<Option<String>>,
) -> Result<PathBuf> {
    let name = branch_name(issue, title)?;
    let main = main_worktree(repo)?;
    let path = worktrees_dir(&main, settings, mount_uuid)?.join(&name);
    Cmd::new("git")
        .args(["fetch", "origin"])
        .current_dir(&main)
        .run()?;
    Cmd::new("git")
        .args(["worktree", "add", "--no-track", "-b", &name])
        .args([path.to_string_lossy()])
        .args(["origin/main"])
        .current_dir(&main)
        .run()?;
    Ok(path)
}

/// A linked worktree whose branch is named after an issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    /// Issue number: the leading digits of the branch name.
    pub issue: u64,
    /// Branch name, without `refs/heads/`.
    pub branch: String,
    /// Worktree directory.
    pub path: PathBuf,
}

/// The issue number a branch named `<issue>-<slug>` belongs to.
#[must_use]
pub fn issue_of(branch: &str) -> Option<u64> {
    let (number, slug) = branch.split_once('-')?;
    if slug.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    number.parse().ok()
}

/// Parses `git worktree list --porcelain`, keeping the linked worktrees on issue branches.
///
/// The first entry, which git always reports as the main working tree, is skipped.
#[must_use]
pub fn parse_porcelain(text: &str) -> Vec<Worktree> {
    let mut out = Vec::new();
    for block in text.split("\n\n").skip(1) {
        let mut path = None;
        let mut branch = None;
        for line in block.lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                path = Some(PathBuf::from(p));
            } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
                branch = Some(b.to_owned());
            }
        }
        if let (Some(path), Some(branch)) = (path, branch)
            && let Some(issue) = issue_of(&branch)
        {
            out.push(Worktree {
                issue,
                branch,
                path,
            });
        }
    }
    out
}

/// The linked worktrees of the repository containing `repo` that are on issue branches.
///
/// # Errors
/// Fails if git fails.
pub fn list(repo: &Path) -> Result<Vec<Worktree>> {
    let text = Cmd::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo)
        .output()?;
    Ok(parse_porcelain(&text))
}

/// Number of commits on `branch` that no remote-tracking branch contains.
///
/// # Errors
/// Fails if git fails.
pub fn unpushed_commits(repo: &Path, branch: &str) -> Result<usize> {
    let count = Cmd::new("git")
        .args(["rev-list", "--count", branch, "--not", "--remotes"])
        .current_dir(repo)
        .output()?;
    count.trim().parse().map_err(|e| {
        Error::Parse(format!(
            "`git rev-list --count` printed `{}`: {e}",
            count.trim()
        ))
    })
}

/// Head commits of the merged pull requests in a `GET /repos/{repo}/pulls` response.
///
/// A pull request counts as merged when its `merged_at` is set; entries without a
/// `head.sha` are skipped. A body that is not an array yields nothing.
#[must_use]
pub fn merged_heads(pulls: &serde_json::Value) -> Vec<String> {
    pulls
        .as_array()
        .into_iter()
        .flatten()
        .filter(|pr| !pr["merged_at"].is_null())
        .filter_map(|pr| pr["head"]["sha"].as_str().map(str::to_owned))
        .collect()
}

/// Removes the worktree of issue `issue` and deletes its branch; returns the removed path.
///
/// Removal proceeds when every commit of the branch is on a remote-tracking branch, or when
/// the branch tip is the head of a merged pull request. The second case covers a branch
/// whose remote copy was deleted on merge and then pruned locally. `merged_heads` returns
/// the head commits of the merged pull requests whose head branch is the given name; it
/// runs only when the branch has commits that no remote-tracking branch contains.
///
/// # Errors
/// Fails if no worktree or more than one is on a branch of that issue, if the branch has
/// commits that no remote-tracking branch contains and its tip is not the head of a merged
/// pull request, if `merged_heads` fails, or if git fails, for example when the worktree
/// has uncommitted changes.
pub fn rm(
    repo: &Path,
    issue: u64,
    merged_heads: &dyn Fn(&str) -> Result<Vec<String>>,
) -> Result<PathBuf> {
    let op = format!("wt rm {issue}");
    let mut found = list(repo)?.into_iter().filter(|w| w.issue == issue);
    let (Some(wt), None) = (found.next(), found.next()) else {
        return Err(refusal(
            &op,
            "expected exactly one worktree on a branch of this issue",
        ));
    };
    let unpushed = unpushed_commits(repo, &wt.branch)?;
    if unpushed > 0 {
        let tip = Cmd::new("git")
            .args(["rev-parse", "--verify"])
            .args([format!("refs/heads/{}^{{commit}}", wt.branch)])
            .current_dir(repo)
            .output()?;
        if !merged_heads(&wt.branch)?
            .iter()
            .any(|sha| sha == tip.trim())
        {
            return Err(refusal(
                &op,
                format!(
                    "branch {} has {unpushed} commit(s) not on any remote and its tip is not \
                     the head of a merged pull request; push them first, or discard them with \
                     `git worktree remove {}` and `git branch -D {}`",
                    wt.branch,
                    wt.path.display(),
                    wt.branch
                ),
            ));
        }
    }
    let main = main_worktree(repo)?;
    Cmd::new("git")
        .args(["worktree", "remove"])
        .args([wt.path.to_string_lossy()])
        .current_dir(&main)
        .run()?;
    // Every commit is on a remote or in a merged pull request (checked above), so a forced
    // delete loses nothing.
    Cmd::new("git")
        .args(["branch", "-D", &wt.branch])
        .current_dir(&main)
        .run()?;
    Ok(wt.path)
}

/// The `owner/name` of a GitHub remote URL (`https://…/owner/name(.git)` or
/// `git@host:owner/name(.git)`, whatever the host alias).
#[must_use]
pub fn github_repo(url: &str) -> Option<String> {
    let path = url.trim().trim_end_matches('/').trim_end_matches(".git");
    let path = match path.split_once("://") {
        Some((_, rest)) => rest.split_once('/')?.1,
        None => path.split_once(':')?.1,
    };
    let mut parts = path.rsplit('/');
    let name = parts.next().filter(|s| !s.is_empty())?;
    let owner = parts.next().filter(|s| !s.is_empty())?;
    Some(format!("{owner}/{name}"))
}

/// Entry point of `scripts/wt.rs`: `new <issue#>`, `list` or `rm <issue#>`, run from the
/// current directory. `new` reads the issue title, and `rm` the merged pull requests of the
/// branch, from the GitHub repository of `origin`; `rm` asks GitHub only when the branch
/// has commits that no remote-tracking branch contains.
///
/// # Errors
/// Fails on bad arguments and on any failure of the subcommand.
#[cfg(feature = "github")]
pub fn cli(args: &[String]) -> Result<()> {
    let here = Path::new(".");
    let usage = || refusal("wt", "usage: wt new <issue#> | wt list | wt rm <issue#>");
    let issue = |arg: &str| arg.parse::<u64>().map_err(|_| usage());
    let origin_repo = || -> Result<String> {
        let url = Cmd::new("git")
            .args(["remote", "get-url", "origin"])
            .current_dir(here)
            .output()?;
        github_repo(&url)
            .ok_or_else(|| Error::Parse(format!("not a GitHub remote: {}", url.trim())))
    };
    match args {
        [cmd, n] if cmd == "new" => {
            let issue = issue(n)?;
            let title = crate::github::Client::new(origin_repo()?)?
                .get(&format!("issues/{issue}"))?["title"]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| Error::Parse(format!("issue {issue} has no title")))?;
            let settings = Settings::load(&local_config_dir()?.join("local.toml"))?;
            let path = new(here, issue, &title, &settings, &findmnt_uuid)?;
            println!("{}", path.display());
        }
        [cmd] if cmd == "list" => {
            for wt in list(here)? {
                println!("{}\t{}\t{}", wt.issue, wt.branch, wt.path.display());
            }
        }
        [cmd, n] if cmd == "rm" => {
            let lookup = |branch: &str| -> Result<Vec<String>> {
                let repo = origin_repo()?;
                let owner = repo.split_once('/').map_or(repo.as_str(), |(o, _)| o);
                let pulls = crate::github::Client::new(repo.as_str())?.get(&format!(
                    "pulls?state=closed&head={owner}:{branch}&per_page=100"
                ))?;
                Ok(merged_heads(&pulls))
            };
            let path = rm(here, issue(n)?, &lookup)?;
            println!("removed {}", path.display());
        }
        _ => return Err(usage()),
    }
    Ok(())
}

/// An [`Error::Command`] reporting that operation `op` was refused or failed.
pub(crate) fn refusal(op: &str, detail: impl Into<String>) -> Error {
    Error::Command {
        command: op.to_owned(),
        detail: detail.into(),
    }
}

fn io_error(op: &str, e: &std::io::Error) -> Error {
    refusal(op, e.to_string())
}

#[cfg(test)]
pub(crate) mod test_support {
    //! Temporary git repositories for tests.

    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A directory under the system temporary directory, removed on drop.
    pub(crate) struct TempDir(pub(crate) PathBuf);

    impl TempDir {
        pub(crate) fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "autobot-devtools-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path.canonicalize().unwrap())
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Runs git in `dir` with a fixed identity and no signing, returning trimmed stdout.
    pub(crate) fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    /// Writes `file` with `content` in `dir` and commits it with `message`.
    pub(crate) fn commit(dir: &Path, file: &str, content: &str, message: &str) {
        std::fs::write(dir.join(file), content).unwrap();
        git(dir, &["add", file]);
        git(dir, &["commit", "-q", "-m", message]);
    }

    /// An `origin` repository with one commit on `main`, and a clone of it at `<tmp>/work`.
    pub(crate) fn clone_with_origin(tmp: &Path) -> PathBuf {
        let origin = tmp.join("origin");
        std::fs::create_dir(&origin).unwrap();
        git(&origin, &["init", "-q", "-b", "main"]);
        commit(&origin, "README", "hello\n", "initial");
        git(tmp, &["clone", "-q", "origin", "work"]);
        tmp.join("work")
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{TempDir, clone_with_origin, commit, git};
    use super::*;
    use std::cell::RefCell;

    fn no_mount(_: &Path) -> Result<Option<String>> {
        panic!("the mount lookup must not run without require_mount_uuid")
    }

    fn no_pulls(_: &str) -> Result<Vec<String>> {
        panic!("the pull request lookup must not run while every commit is on a remote")
    }

    /// `GET pulls?state=closed&head=<owner>:47-workspace-crates` on the AutoBot repository,
    /// recorded and trimmed to the fields read, followed by a closed, unmerged entry.
    const CLOSED_PULLS: &str = r#"[
      {"number": 235, "state": "closed", "merged_at": "2026-09-23T00:24:48Z",
       "head": {"ref": "47-workspace-crates", "sha": "ba95cd31c93460244e378816def9bef96eccb5dc"}},
      {"number": 236, "state": "closed", "merged_at": null,
       "head": {"ref": "47-workspace-crates", "sha": "0000000000000000000000000000000000000001"}}
    ]"#;

    #[test]
    fn branch_name_slugs_the_title() {
        assert_eq!(
            branch_name(56, "Worktree-per-PR recipes: `just wt new|list|rm`").unwrap(),
            "56-worktree-per-pr-recipes-just-wt-new-list"
        );
        assert_eq!(branch_name(7, "  Fix   CI!! ").unwrap(), "7-fix-ci");
        let long = branch_name(1, &"abcdefghij ".repeat(10)).unwrap();
        assert_eq!(long, "1-abcdefghij-abcdefghij-abcdefghij");
        let one_huge_word = branch_name(2, &"x".repeat(60)).unwrap();
        assert_eq!(one_huge_word.len(), 2 + MAX_SLUG_LEN);
        assert!(branch_name(3, "¿¡ —").is_err());
    }

    #[test]
    fn settings_read_only_a_plain_top_level_string() {
        let text = "# c\nrequire_mount = \"/x\"\nrequire_mount_uuid = \"abc-1\"  # note\n";
        assert_eq!(
            Settings::parse(text).unwrap().require_mount_uuid.as_deref(),
            Some("abc-1")
        );
        assert_eq!(
            Settings::parse("other = \"1\"\n").unwrap(),
            Settings::default()
        );
        assert_eq!(
            Settings::parse("[t]\nrequire_mount_uuid = \"x\"\n").unwrap(),
            Settings::default()
        );
        assert!(Settings::parse("require_mount_uuid = 5\n").is_err());
        assert!(Settings::parse("require_mount_uuid = \"a\\\"b\"\n").is_err());
        let tmp = TempDir::new();
        assert_eq!(
            Settings::load(&tmp.0.join("absent.toml")).unwrap(),
            Settings::default()
        );
    }

    #[test]
    fn github_repo_parses_remote_urls() {
        for url in [
            "https://github.com/o/r.git",
            "https://github.com/o/r",
            "git@github.com:o/r.git",
            "git@github.com-alias:o/r.git\n",
            "ssh://git@github.com/o/r.git",
        ] {
            assert_eq!(github_repo(url).as_deref(), Some("o/r"), "{url}");
        }
        assert_eq!(github_repo("/local/path"), None);
    }

    #[test]
    fn porcelain_keeps_linked_issue_branches_only() {
        let text = "worktree /r\nHEAD 1\nbranch refs/heads/7-main-on-issue\n\n\
                    worktree /r/.worktrees/5-a\nHEAD 2\nbranch refs/heads/5-a\n\n\
                    worktree /d\nHEAD 3\ndetached\n\n\
                    worktree /x\nHEAD 4\nbranch refs/heads/12x-b\n";
        assert_eq!(
            parse_porcelain(text),
            vec![Worktree {
                issue: 5,
                branch: "5-a".into(),
                path: "/r/.worktrees/5-a".into(),
            }]
        );
    }

    #[test]
    fn new_list_and_rm_round_trip() {
        let tmp = TempDir::new();
        let work = clone_with_origin(&tmp.0);
        // The main working tree is never listed, even on a branch of the same issue.
        git(&work, &["checkout", "-q", "-b", "9-main-tree"]);
        let path = new(&work, 9, "Add a thing", &Settings::default(), &no_mount).unwrap();
        assert_eq!(path, work.join(".worktrees/9-add-a-thing"));
        assert!(path.join("README").is_file());
        assert_eq!(git(&path, &["branch", "--show-current"]), "9-add-a-thing");
        assert_eq!(
            git(&path, &["rev-parse", "HEAD"]),
            git(&work, &["rev-parse", "origin/main"])
        );

        // `list` works from inside the linked worktree too.
        let listed = list(&path).unwrap();
        assert_eq!(
            listed,
            vec![Worktree {
                issue: 9,
                branch: "9-add-a-thing".into(),
                path: path.clone(),
            }]
        );

        assert_eq!(rm(&work, 9, &no_pulls).unwrap(), path);
        assert!(!path.exists());
        assert!(list(&work).unwrap().is_empty());
        assert_eq!(git(&work, &["branch", "--list", "9-add-a-thing"]), "");
        assert!(rm(&work, 9, &no_pulls).is_err());
    }

    #[test]
    fn rm_refuses_unpushed_commits_until_they_are_pushed() {
        let tmp = TempDir::new();
        let work = clone_with_origin(&tmp.0);
        let path = new(&work, 4, "Work", &Settings::default(), &no_mount).unwrap();
        commit(&path, "f", "x\n", "local work");

        let err = rm(&work, 4, &|_| Ok(Vec::new())).unwrap_err().to_string();
        assert!(err.contains("1 commit(s) not on any remote"), "{err}");
        assert!(err.contains("git branch -D 4-work"), "{err}");
        assert!(path.is_dir());

        git(&path, &["push", "-q", "origin", "4-work"]);
        rm(&work, 4, &no_pulls).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn merged_heads_keeps_merged_pull_requests_only() {
        let pulls: serde_json::Value = serde_json::from_str(CLOSED_PULLS).unwrap();
        assert_eq!(
            merged_heads(&pulls),
            ["ba95cd31c93460244e378816def9bef96eccb5dc"]
        );
        assert!(merged_heads(&serde_json::json!({"message": "Not Found"})).is_empty());
    }

    #[test]
    fn rm_accepts_a_pruned_branch_only_when_its_tip_was_merged() {
        let tmp = TempDir::new();
        let work = clone_with_origin(&tmp.0);
        let path = new(&work, 6, "Merged", &Settings::default(), &no_mount).unwrap();
        commit(&path, "f", "x\n", "merged work");
        git(&path, &["push", "-q", "origin", "6-merged"]);
        let tip = git(&path, &["rev-parse", "HEAD"]);
        // The merge deletes the remote branch; a pruning fetch then drops `origin/6-merged`.
        git(&tmp.0.join("origin"), &["branch", "-D", "6-merged"]);
        git(&work, &["fetch", "-q", "--prune", "origin"]);

        let asked = RefCell::new(Vec::new());
        let heads = |answer: Vec<String>| {
            let asked = &asked;
            move |branch: &str| {
                asked.borrow_mut().push(branch.to_owned());
                Ok(answer.clone())
            }
        };
        let err = rm(&work, 6, &heads(vec!["0".repeat(40)]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("1 commit(s) not on any remote"), "{err}");
        assert!(path.is_dir());

        assert_eq!(rm(&work, 6, &heads(vec![tip])).unwrap(), path);
        assert!(!path.exists());
        assert_eq!(git(&work, &["branch", "--list", "6-merged"]), "");
        assert_eq!(asked.borrow().as_slice(), ["6-merged", "6-merged"]);
    }

    #[test]
    fn new_follows_a_worktrees_symlink() {
        let tmp = TempDir::new();
        let work = clone_with_origin(&tmp.0);
        let volume = tmp.0.join("volume");
        std::fs::create_dir(&volume).unwrap();
        std::os::unix::fs::symlink(&volume, work.join(".worktrees")).unwrap();

        let path = new(&work, 3, "Linked", &Settings::default(), &no_mount).unwrap();
        assert_eq!(path, volume.join("3-linked"));
        assert!(volume.join("3-linked/README").is_file());
        assert_eq!(list(&work).unwrap()[0].path, path);
    }

    #[test]
    fn new_refuses_a_dangling_worktrees_symlink() {
        let tmp = TempDir::new();
        let work = clone_with_origin(&tmp.0);
        let missing = tmp.0.join("unmounted/worktrees");
        std::os::unix::fs::symlink(&missing, work.join(".worktrees")).unwrap();

        let err = new(&work, 3, "Linked", &Settings::default(), &no_mount)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("does not lead to an existing directory"),
            "{err}"
        );
        assert!(!missing.exists());
        assert_eq!(git(&work, &["branch", "--list", "3-linked"]), "");
    }

    #[test]
    fn new_checks_the_mount_uuid_of_the_resolved_directory() {
        let tmp = TempDir::new();
        let work = clone_with_origin(&tmp.0);
        let volume = tmp.0.join("volume");
        std::fs::create_dir(&volume).unwrap();
        std::os::unix::fs::symlink(&volume, work.join(".worktrees")).unwrap();
        let settings = Settings {
            require_mount_uuid: Some("want-uuid".into()),
        };
        let asked = RefCell::new(Vec::new());
        let lookup = |answer: Option<&'static str>| {
            let asked = &asked;
            move |p: &Path| {
                asked.borrow_mut().push(p.to_path_buf());
                Ok(answer.map(str::to_owned))
            }
        };

        let err = new(&work, 1, "One", &settings, &lookup(Some("other-uuid")))
            .unwrap_err()
            .to_string();
        assert!(err.contains("UUID other-uuid"), "{err}");
        let err = new(&work, 1, "One", &settings, &lookup(None))
            .unwrap_err()
            .to_string();
        assert!(err.contains("UUID <none>"), "{err}");
        assert!(!volume.join("1-one").exists());
        assert_eq!(asked.borrow().as_slice(), [volume.clone(), volume.clone()]);

        let path = new(&work, 1, "One", &settings, &lookup(Some("want-uuid"))).unwrap();
        assert_eq!(path, volume.join("1-one"));
    }
}
