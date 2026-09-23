# Contributing

## Work items

- Every change delivers exactly one task issue. A task issue is a scope capsule with these sections: **Objective**, **Design refs**, **Allowed paths**, **Non-goals**, **Acceptance evidence**, and optionally **Owns**, which names the design elements (kinds, F-n fixtures, formal modules) the task is the single owner of.
- **Allowed paths** holds paths and globs only, separated by commas or line breaks: `**` spans any number of directories, `*` any run of characters within one, and a trailing `/` everything under a directory. A glob that starts with `**` is refused. A qualifier on a path (a section, an entry, "only deleting ...") goes in **Non-goals**; an entry with one allows nothing.
- A comment on the task issue that starts with `Scope extension`, written by the repository owner, a member or a collaborator, adds to the task's allowed paths each backticked span that holds a `/`, a `.` or a `*` and is a path or glob.
- A task is a sub-issue of one epic and belongs to that epic's milestone. Ordering between issues is expressed only as native "blocked by" dependencies: minimal, acyclic, and never pointing at an issue in a later milestone.
- Work order: the earliest open milestone first, then issues with no open blockers, then issues on the critical path of the blocked-by graph, which `just dag --critical` computes. The `urgent` label is the only override. There are no priority labels.
- Epics carry `type:epic`, tasks `type:task`.

## Findings

Anything outside a task's scope is reported, never fixed in passing. A finding is an issue in the capsule format labelled `finding` plus one of `design`, `bug` or `debt`, filed as a sub-issue of the epic that owns its scope and placed in that epic's milestone. A `design` finding also carries `needs-decision`. There is no backlog milestone.

## Worktrees and branches

Development happens in one git worktree per pull request, under `.worktrees/`, managed with `just`:

```sh
just wt new <issue#>   # branch <issue#>-<slug> from origin/main, worktree .worktrees/<issue#>-<slug>
just wt list           # worktrees on issue branches
just wt rm <issue#>    # remove the worktree and its branch
```

- The slug is derived from the issue title. Each worktree builds into its own `target/`.
- `.worktrees` may be a symlink to another volume; `just wt new` refuses when its target is missing. When the machine-local `~/.config/autobot/local.toml` sets `require_mount_uuid`, it also refuses unless the resolved directory is on the filesystem with that UUID. That file is never committed.
- `just wt rm` refuses while the branch has commits that no remote-tracking branch contains, unless the branch tip is the head of a merged pull request (the remote branch may already be deleted and pruned). Remove the worktree once its pull request is merged. To discard unpushed work instead, run `git worktree remove <path>` and `git branch -D <branch>`.
- `just hooks` installs a pre-push hook that rejects a push whose commit messages or added lines (including the lines a merge commit adds, such as a conflict resolution) match an expression in the machine-local `~/.config/autobot/deny-terms` (one case-insensitive regular expression per line). Without that file every push passes. It is never committed.

## Build cache

Local builds share one sccache server and one cache directory across every worktree. The repository configures no compiler wrapper and exports no `CARGO_*` variable; each machine sets `[build] rustc-wrapper = "sccache"` in its home Cargo configuration (`~/.cargo/config.toml`). Each worktree keeps its own `target/`, and incremental compilation stays on.

sccache's key for a Rust compilation includes its working directory, its `CARGO_*` environment variables and the compiler version, and `SCCACHE_BASEDIRS` does not apply to Rust. Dependency crates build from the shared registry, so a second worktree at the same commit gets them from the cache, apart from crates whose compilation depends on build-script output under the worktree's own `target/`, which miss once per worktree. Workspace crates build from the worktree path with incremental compilation, which sccache does not cache, so every worktree compiles them itself.

```sh
just doctor            # tool versions, the wrapper, the server, volumes, Justfile exports
just doctor --measure  # also the sccache hit split: dependencies, then workspace
```

`just doctor` checks that rust-script, just, cargo-nextest, cargo-deny, cargo-machete and sccache run; that the effective rustc wrapper is sccache from the home Cargo configuration; that the sccache server is running; that its cache directory and `.worktrees`, followed through symlinks, are on the volume `require_mount_uuid` names in `~/.config/autobot/local.toml`, when it names one; and that the Justfile exports no `CARGO_*` variable. `--measure` builds into `target/doctor`, emptied first: it zeroes the sccache counters, builds the non-workspace packages of the resolve and prints the counters as the dependency split, then zeroes them, builds the workspace and prints the workspace split. The counters are the shared server's, so builds running elsewhere at the same time are included.

## Pull requests

- **Title:** Conventional Commits (`feat(kernel): …`, `fix(docs): …`, `chore: …`). Merges are squash-only, so the title becomes the commit on `main`.
- **Description:** starts with `Closes #N` and then describes only the resolution: what changed and how, decisions taken while implementing, deviations from the capsule and why, and the evidence for each acceptance item. It never copies, paraphrases or mirrors the issue; the issue is already linked.
- **Paths:** a pull request changes only its task's allowed paths. Every task may also edit its crate's `Cargo.toml`, `Cargo.lock`, the `[workspace.dependencies]` entries it needs, the `mod` line in the parent module of a file it adds, and its own entry in `autobot_controllers::all_controllers()`. Under `crates/*/tests/g_*/` an implementation task may only delete the `#[ignore = "awaiting #N"]` lines that name its own issue.
- **Scope:** the `scope` check fails a pull request that closes no task issue, or that changes a path (or renames a file from one) that none of the task's allowed paths, its scope extensions and the paths above allows, and names each such path. It reads the parent-module and gate-test inherited paths by line. A parent module may only gain `mod <name>;` lines (optionally `pub` or `pub(crate)`) for files the pull request adds under it, with adjacent `///` doc comments and blank lines, and lose nothing. A gate test group may only lose `#[ignore = "awaiting #N"]` lines naming the task. A scope extension posted after the check ran counts from its next run; re-run the check.

## Review and merge

- A fresh reviewer session, never the author's, reviews every pull request against its task issue (checklist in `AGENTS.md`) and posts one comment whose first line is exactly:

  ```text
  Review verdict: PASS @ <full-head-sha>
  ```

  or `Review verdict: FAIL @ <full-head-sha>`, followed by what was checked, each blocking finding with file and line, and then an "Advisory (not blocking)" section. Only a blocking finding makes the verdict FAIL; the checklist in `AGENTS.md` defines both kinds.
- The verdict binds to that head SHA only. Any new push needs a new verdict. The `review-gate` check reads this first line.
- The `judge` check asks, for every `judged` entry of `CHARTER.md`, whether the pull request's diff and text violate it. It fails only on a "violates" answer at or above the entry's threshold, naming the entry and the confidence; a "complies" or "unsure" answer, a truncated input or an unavailable service passes it and leaves the entry to the review. It is never a required check and never replaces the review verdict.
- The `sensitive-terms` check matches the diff's added lines and paths, the commit messages and the pull request's title and body against the term list in the `SENSITIVE_TERMS` repository secret, in the format of the deny-terms file. It fails on a match and prints only where each match is, never the term or the matched text. Without the secret it passes and says nothing was screened. It is never a required check.
- `main` merges only through pull requests with squash, and GitHub performs every merge through auto-merge. Until `review-gate` is a required check, auto-merge is enabled on a pull request only after a PASS verdict names its current head SHA: with no required checks, enabling auto-merge merges immediately.

## Lanes

- Tasks that touch `.github/workflows/**`, the repository ruleset or `docs/design/**` carry `human-lane`. They are done only in sessions the owner supervises; unsupervised agents and autonomous loops skip them.
- `docs/design/` changes only through pull requests labelled `design-change`. Implementation work that finds the design wrong files a `design` finding instead of editing it.

## Tests

Fixture groups are test-first. A group's fixture task lands its F-n tests as `#[ignore = "awaiting #N"]` tests against the public API before any implementation task of the group. Each implementation task removes the ignore markers that name its own issue. A group is complete when none of its F-n tests is ignored.

## Decisions

There are no decision-record documents. An implementation choice the design leaves open is documented in the module rustdoc of the crate that makes it; a design fact goes into `docs/design/` through a `design-change` pull request.

## Documentation

Every document describes what the thing is. Rationale goes in a companion `XXX.background.md` when it reaches at least 50 lines and at least 5% of the main document's length; below either floor it stays inline, kept brief. A main document with a companion ends with one pointer line to it.
