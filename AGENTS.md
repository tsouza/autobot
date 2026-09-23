# Agent instructions

Read `CONTRIBUTING.md` first; everything in it applies to agents. The rules below are the ones agents most often get wrong.

## Scope

- You work on exactly one task issue. Its capsule (Objective, Design refs, Allowed paths, Non-goals, Acceptance evidence) is your contract.
- Do not touch a file outside the allowed paths and the inherited paths listed in `CONTRIBUTING.md`.
- Do not fix, refactor or clean up anything unrelated. File it as a finding (see `CONTRIBUTING.md`).
- `docs/design/` is the contract. If it is wrong, file a `design` finding; never edit it from a task that is not labelled `design-change`.
- Never pick up a `human-lane` task.

## Workflow

- One worktree per pull request under `.worktrees/<issue#>-<slug>`, on branch `<issue#>-<slug>`.
- A local test run is for iteration. Acceptance is the pull request's checks plus a PASS review verdict for its head SHA.
- Never push to `main`, never merge, never enable auto-merge for your own pull request before a PASS verdict names its current head SHA, and never change `.github/workflows/` or the ruleset.
- Pull request description: `Closes #N`, then only the resolution: what changed and how, decisions, deviations and why, evidence per acceptance item. Never restate the issue.

## Reviewer checklist

A reviewer is a fresh session that did not write the change. It tries to refute the change, not to confirm it. Before posting a verdict it checks:

1. Only allowed and inherited paths changed.
2. Every acceptance item of the task issue is met by evidence in the diff or the checks.
3. The change matches the design text it cites; any drift is a defect or a `design` finding.
4. Tests assert behaviour and would fail if the behaviour were wrong.
5. Unrelated fixes, deferrals without a tracked reason, and inconsistencies between code, comments, docs and tests are defects.
6. The pull request description does not copy, paraphrase or mirror its issue. If it does, the verdict is FAIL.
7. If `~/.config/autobot/deny-terms` exists, no line of the diff, commit messages or pull request text matches it. Never commit that list or quote its entries.

The verdict is one comment whose first line is `Review verdict: PASS @ <full-head-sha>` or `Review verdict: FAIL @ <full-head-sha>`, followed by what was checked and, for a FAIL, each defect with file and line.
