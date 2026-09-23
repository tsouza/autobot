# Contributing

## Work items

- Every change delivers exactly one task issue. A task issue is a scope capsule with these sections: **Objective**, **Design refs**, **Allowed paths**, **Non-goals**, **Acceptance evidence**, and optionally **Owns**, which names the design elements (kinds, F-n fixtures, formal modules) the task is the single owner of.
- A task is a sub-issue of one epic and belongs to that epic's milestone. Ordering between issues is expressed only as native "blocked by" dependencies: minimal, acyclic, and never pointing at an issue in a later milestone.
- Work order: the earliest open milestone first, then issues with no open blockers, then issues on the critical path of the blocked-by graph. The `urgent` label is the only override. There are no priority labels.
- Epics carry `type:epic`, tasks `type:task`.

## Findings

Anything outside a task's scope is reported, never fixed in passing. A finding is an issue labelled `finding` plus one of `design`, `bug` or `debt`, filed as a sub-issue of the epic that owns its scope and placed in that epic's milestone. A `design` finding also carries `needs-decision`. There is no backlog milestone.

## Worktrees and branches

Development happens in one git worktree per pull request, under `.worktrees/`:

```sh
git fetch origin
git worktree add -b <issue#>-<slug> .worktrees/<issue#>-<slug> origin/main
```

The branch name is `<issue#>-<slug>`. Remove the worktree once its pull request is merged.

## Pull requests

- **Title:** Conventional Commits (`feat(kernel): …`, `fix(docs): …`, `chore: …`). Merges are squash-only, so the title becomes the commit on `main`.
- **Description:** starts with `Closes #N` and then describes only the resolution: what changed and how, decisions taken while implementing, deviations from the capsule and why, and the evidence for each acceptance item. It never copies, paraphrases or mirrors the issue; the issue is already linked.
- **Paths:** a pull request changes only its task's allowed paths. Every task may also edit its crate's `Cargo.toml`, `Cargo.lock`, the `[workspace.dependencies]` entries it needs, the `mod` line in the parent module of a file it adds, and its own entry in `autobot_controllers::all_controllers()`. Under `crates/*/tests/g_*/` an implementation task may only delete the `#[ignore = "awaiting #N"]` lines that name its own issue.

## Review and merge

- A fresh reviewer session, never the author's, reviews every pull request against its task issue and posts one comment whose first line is exactly:

  ```text
  Review verdict: PASS @ <full-head-sha>
  ```

  or `Review verdict: FAIL @ <full-head-sha>`, followed by what was checked and, for a FAIL, each defect with file and line.
- The verdict binds to that head SHA only. Any new push needs a new verdict.
- `main` merges only through pull requests with squash. Auto-merge is enabled on a pull request only after a PASS verdict names its current head SHA; GitHub performs the merge.

## Lanes

- Tasks that touch `.github/workflows/**`, the repository ruleset or settings, or `docs/design/**` carry `human-lane`. They are done in owner-supervised sessions; autonomous loops skip them.
- `docs/design/` changes only through pull requests labelled `design-change`. Implementation work that finds the design wrong files a `design` finding instead of editing it.

## Tests

Fixture groups are test-first. A group's fixture task lands its F-n tests as `#[ignore = "awaiting #N"]` tests against the public API before any implementation task of the group. Each implementation task removes the ignore markers that name its own issue. A group is complete when none of its F-n tests is ignored.

## Decisions

There are no decision-record documents. An implementation choice the design leaves open is documented in the module rustdoc of the crate that makes it; a design fact goes into `docs/design/` through a `design-change` pull request.

## Documentation

Every document describes what the thing is. Rationale goes in a companion `XXX.background.md` when it reaches at least 50 lines and at least 5% of the main document's length; below either floor it stays inline, kept brief. A main document with a companion ends with one pointer line to it.
