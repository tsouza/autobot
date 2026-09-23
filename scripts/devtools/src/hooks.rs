//! The git pre-push hook that keeps deny-listed terms out of pushed commits.
//!
//! The deny list is the machine-local file `~/.config/autobot/deny-terms`: one
//! case-insensitive regular expression per line, blank lines and lines starting with `#`
//! ignored. When it is absent, every push passes. Otherwise a push fails when a pushed
//! commit's message, or a line one of its diffs adds, matches any expression. A merge
//! commit's diff is the one `git log --remerge-diff` shows: what the recorded merge adds
//! beyond an automatic re-merge of its parents, such as a conflict resolution.
//!
//! [`install`] writes a hook that runs `just hook-pre-push`, which calls [`cli`] with the
//! hook's arguments and standard input.

use crate::process::Cmd;
use crate::worktree::{local_config_dir, refusal};
use crate::{Error, Result};
use regex::Regex;
use std::path::{Path, PathBuf};

/// Marker line that identifies a hook written by [`install`].
pub const MARKER: &str = "# autobot-devtools pre-push hook";

/// The object name git uses for "no commit" on a pre-push input line.
const ZERO_OID: &str = "0000000000000000000000000000000000000000";

/// Installs the pre-push hook in the hooks directory of the repository containing `repo`
/// (honouring `core.hooksPath`) and returns the hook's path.
///
/// # Errors
/// Fails if git fails, if a pre-push hook not written by this function already exists, or if
/// the file cannot be written.
pub fn install(repo: &Path) -> Result<PathBuf> {
    let dir = Cmd::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-path", "hooks"])
        .current_dir(repo)
        .output()?;
    let dir = PathBuf::from(dir.trim());
    let hook = dir.join("pre-push");
    if let Ok(existing) = std::fs::read_to_string(&hook)
        && !existing.contains(MARKER)
    {
        return Err(refusal(
            "install pre-push hook",
            format!(
                "{} exists and was not written by `just hooks`",
                hook.display()
            ),
        ));
    }
    let script = format!("#!/bin/sh\n{MARKER}\nexec just hook-pre-push \"$@\"\n");
    std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::write(&hook, script))
        .and_then(|()| make_executable(&hook))
        .map_err(|e| refusal("install pre-push hook", e.to_string()))?;
    Ok(hook)
}

#[cfg(unix)]
fn make_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn make_executable(_: &Path) -> std::io::Result<()> {
    Ok(())
}

/// The deny patterns in the text of a deny-terms file, each with its 1-based line number.
///
/// # Errors
/// Fails if a line is not a valid regular expression.
pub fn parse_patterns(text: &str) -> Result<Vec<(usize, Regex)>> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| {
            let l = l.trim();
            !l.is_empty() && !l.starts_with('#')
        })
        .map(|(i, l)| {
            Regex::new(&format!("(?i){}", l.trim()))
                .map(|re| (i + 1, re))
                .map_err(|e| Error::Parse(format!("deny-terms line {}: {e}", i + 1)))
        })
        .collect()
}

/// A pushed line that matches a deny pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// Line number of the matching pattern in the deny-terms file.
    pub pattern_line: usize,
    /// The offending commit-message line or added diff line, without the leading `+`.
    pub text: String,
}

/// The texts one ref update pushes: its commit messages and the lines its diffs add.
///
/// `update` is one line of pre-push input: `<local ref> <local oid> <remote ref> <remote
/// oid>`. A new remote ref pushes every commit no ref of `remote` already has; a deletion
/// pushes nothing.
///
/// # Errors
/// Fails if the line is malformed or git fails.
pub fn pushed_lines(repo: &Path, remote: &str, update: &str) -> Result<Vec<String>> {
    let fields: Vec<&str> = update.split_whitespace().collect();
    let [_, local, _, remote_oid] = fields[..] else {
        return Err(Error::Parse(format!(
            "unexpected pre-push input `{update}`"
        )));
    };
    if local == ZERO_OID {
        return Ok(Vec::new());
    }
    let range: Vec<String> = if remote_oid == ZERO_OID {
        vec![local.into(), "--not".into(), format!("--remotes={remote}")]
    } else {
        vec![format!("{remote_oid}..{local}")]
    };
    let log = |format: &str, patch: bool| {
        Cmd::new("git")
            .args(["log", "--no-color", "--no-ext-diff", format])
            .args(
                patch
                    .then_some(["--patch", "--remerge-diff"])
                    .into_iter()
                    .flatten(),
            )
            .args(range.iter().cloned())
            .current_dir(repo)
            .output()
    };
    let messages = log("--format=%B", false)?;
    let patches = log("--format=", true)?;
    Ok(messages
        .lines()
        .chain(added_lines(&patches))
        .map(str::to_owned)
        .collect())
}

/// The lines a `git log --patch` output adds, without their leading `+`.
///
/// A file's header runs from its `diff ` line to its first `@@` hunk line; its `+++ b/<file>`
/// line is skipped there. Inside a hunk every line starting with `+` is an added line, even
/// one whose content itself starts with `++`.
pub fn added_lines(patch: &str) -> impl Iterator<Item = &str> {
    let mut in_header = false;
    patch.lines().filter_map(move |l| {
        if l.starts_with("diff ") {
            in_header = true;
        } else if l.starts_with("@@") {
            in_header = false;
        }
        if in_header { None } else { l.strip_prefix('+') }
    })
}

/// Every violation in a push, given the hook's `remote` argument, its standard input
/// `updates`, and the deny-terms file at `deny_terms`; an absent file yields none.
///
/// # Errors
/// Fails if the deny-terms file cannot be read or parsed, or on any failure of
/// [`pushed_lines`].
pub fn check_push(
    repo: &Path,
    remote: &str,
    updates: &str,
    deny_terms: &Path,
) -> Result<Vec<Violation>> {
    let text = match std::fs::read_to_string(deny_terms) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(refusal(
                &format!("read {}", deny_terms.display()),
                e.to_string(),
            ));
        }
    };
    let patterns = parse_patterns(&text)?;
    let mut out = Vec::new();
    for update in updates.lines().filter(|l| !l.trim().is_empty()) {
        for line in pushed_lines(repo, remote, update)? {
            if let Some((n, _)) = patterns.iter().find(|(_, re)| re.is_match(&line)) {
                out.push(Violation {
                    pattern_line: *n,
                    text: line,
                });
            }
        }
    }
    Ok(out)
}

/// Entry point of `scripts/hooks.rs`: `install`, or `pre-push <remote> [<url>]` with the
/// pre-push input on standard input, run from the current directory.
///
/// # Errors
/// Fails on bad arguments, on any failure of the subcommand, and when the push has a
/// violation.
pub fn cli(args: &[String]) -> Result<()> {
    let here = Path::new(".");
    match args {
        [cmd] if cmd == "install" => {
            println!("installed {}", install(here)?.display());
            Ok(())
        }
        [cmd, remote, ..] if cmd == "pre-push" => {
            let updates = std::io::read_to_string(std::io::stdin())
                .map_err(|e| refusal("read pre-push input", e.to_string()))?;
            let deny_terms = local_config_dir()?.join("deny-terms");
            let violations = check_push(here, remote, &updates, &deny_terms)?;
            for v in &violations {
                eprintln!("deny-terms line {}: {}", v.pattern_line, v.text);
            }
            if violations.is_empty() {
                Ok(())
            } else {
                Err(refusal(
                    "pre-push",
                    format!(
                        "{} pushed line(s) match {}",
                        violations.len(),
                        deny_terms.display()
                    ),
                ))
            }
        }
        _ => Err(refusal(
            "hooks",
            "usage: hooks install | hooks pre-push <remote> [<url>]",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktree::test_support::{TempDir, clone_with_origin, commit, git};

    const DENY: &str = "# comment\n\n  forbidden-[0-9]+  \n";

    fn update(repo: &Path, remote_oid: &str) -> String {
        let head = git(repo, &["rev-parse", "HEAD"]);
        format!("refs/heads/b {head} refs/heads/b {remote_oid}\n")
    }

    fn setup() -> (TempDir, PathBuf, PathBuf) {
        let tmp = TempDir::new();
        let work = clone_with_origin(&tmp.0);
        let deny = tmp.0.join("deny-terms");
        std::fs::write(&deny, DENY).unwrap();
        (tmp, work, deny)
    }

    #[test]
    fn patterns_skip_comments_and_are_case_insensitive() {
        let patterns = parse_patterns(DENY).unwrap();
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].0, 3);
        assert!(patterns[0].1.is_match("a FORBIDDEN-42 b"));
        assert!(!patterns[0].1.is_match("forbidden-x"));
        assert!(parse_patterns("(unclosed\n").is_err());
    }

    #[test]
    fn an_added_line_fails_a_new_branch_push() {
        let (_tmp, work, deny) = setup();
        commit(&work, "a", "fine\nsee Forbidden-7 here\n", "clean message");
        let v = check_push(&work, "origin", &update(&work, ZERO_OID), &deny).unwrap();
        assert_eq!(
            v,
            vec![Violation {
                pattern_line: 3,
                text: "see Forbidden-7 here".into(),
            }]
        );
        // Commits already on the remote are not re-checked.
        commit(&work, "b", "fine\n", "clean");
        git(&work, &["push", "-q", "origin", "HEAD:refs/heads/b"]);
        commit(&work, "c", "fine\n", "clean");
        let base = git(&work, &["rev-parse", "HEAD~1"]);
        assert!(
            check_push(&work, "origin", &update(&work, &base), &deny)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn added_lines_skip_file_headers_but_not_added_plus_lines() {
        let patch = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -0,0 +1,3 @@\n+plain\n+++ x\n+++\n \
                     ctx\n-gone\ndiff --git a/g b/g\nnew file mode 100644\n--- /dev/null\n+++ b/g\n\
                     @@ -0,0 +1 @@\n++y\n";
        assert_eq!(
            added_lines(patch).collect::<Vec<_>>(),
            vec!["plain", "++ x", "++", "+y"]
        );
    }

    #[test]
    fn an_added_line_starting_with_plus_plus_fails_the_push() {
        let (_tmp, work, deny) = setup();
        let base = git(&work, &["rev-parse", "HEAD"]);
        commit(&work, "a", "plain\n++ forbidden-1\n", "clean");
        let v = check_push(&work, "origin", &update(&work, &base), &deny).unwrap();
        assert_eq!(
            v,
            vec![Violation {
                pattern_line: 3,
                text: "++ forbidden-1".into(),
            }]
        );
    }

    #[test]
    fn a_line_added_by_a_merge_commit_fails_the_push() {
        let (_tmp, work, deny) = setup();
        let base = git(&work, &["rev-parse", "HEAD"]);
        git(&work, &["checkout", "-q", "-b", "side"]);
        commit(&work, "s", "side\n", "side");
        git(&work, &["checkout", "-q", "-"]);
        commit(&work, "m", "mainline\n", "mainline");
        git(&work, &["merge", "-q", "--no-commit", "--no-ff", "side"]);
        std::fs::write(work.join("r"), "resolved forbidden-3\n").unwrap();
        git(&work, &["add", "r"]);
        git(&work, &["commit", "-q", "-m", "merge side"]);
        let v = check_push(&work, "origin", &update(&work, &base), &deny).unwrap();
        assert_eq!(
            v,
            vec![Violation {
                pattern_line: 3,
                text: "resolved forbidden-3".into(),
            }]
        );
    }

    #[test]
    fn a_commit_message_fails_the_push() {
        let (_tmp, work, deny) = setup();
        let base = git(&work, &["rev-parse", "HEAD"]);
        commit(&work, "a", "fine\n", "mention forbidden-1");
        let v = check_push(&work, "origin", &update(&work, &base), &deny).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].text, "mention forbidden-1");
    }

    #[test]
    fn removing_a_term_and_a_clean_push_pass() {
        let (_tmp, work, deny) = setup();
        let base = git(&work, &["rev-parse", "HEAD"]);
        commit(&work, "a", "forbidden-9\n", "clean");
        git(&work, &["push", "-q", "origin", "HEAD:refs/heads/b"]);
        let pushed = git(&work, &["rev-parse", "HEAD"]);
        commit(&work, "a", "gone\n", "clean again");
        assert!(
            check_push(&work, "origin", &update(&work, &pushed), &deny)
                .unwrap()
                .is_empty()
        );
        // The same range would fail if it started before the term was added.
        assert_eq!(
            check_push(&work, "origin", &update(&work, &base), &deny)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn deletions_and_an_absent_deny_list_pass() {
        let (tmp, work, deny) = setup();
        commit(&work, "a", "forbidden-2\n", "clean");
        let head = git(&work, &["rev-parse", "HEAD"]);
        let delete = format!("(delete) {ZERO_OID} refs/heads/b {head}\n");
        assert!(
            check_push(&work, "origin", &delete, &deny)
                .unwrap()
                .is_empty()
        );
        let absent = tmp.0.join("no-such-file");
        assert!(
            check_push(&work, "origin", &update(&work, ZERO_OID), &absent)
                .unwrap()
                .is_empty()
        );
        assert!(check_push(&work, "origin", "garbage\n", &deny).is_err());
    }

    #[test]
    fn install_writes_an_executable_hook_and_refuses_a_foreign_one() {
        let tmp = TempDir::new();
        let work = clone_with_origin(&tmp.0);
        let hooks = tmp.0.join("hooks");
        git(
            &work,
            &["config", "core.hooksPath", hooks.to_str().unwrap()],
        );

        let hook = install(&work).unwrap();
        assert_eq!(hook, hooks.join("pre-push"));
        let text = std::fs::read_to_string(&hook).unwrap();
        assert!(text.starts_with("#!/bin/sh\n") && text.contains("just hook-pre-push"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&hook).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111);
        }
        // Reinstalling over its own hook is fine; another hook is left alone.
        install(&work).unwrap();
        std::fs::write(&hook, "#!/bin/sh\nexit 0\n").unwrap();
        assert!(install(&work).is_err());
        assert_eq!(
            std::fs::read_to_string(&hook).unwrap(),
            "#!/bin/sh\nexit 0\n"
        );
    }
}
