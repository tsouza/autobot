# Agent instructions

`CONTRIBUTING.md` is the rulebook and applies in full; this file does not restate it. It adds what is specific to agent sessions.

## Before starting

- Read the task issue and every design section its **Design refs** name. The capsule is the contract; `CONTRIBUTING.md` lists the paths every task may additionally touch.
- Do not take a `human-lane` task unless the owner is supervising the session.

## While working

- Anything outside the task goes into a finding, never into the diff.
- If the design text looks wrong, stop and file a `design` finding; do not work around it in code.
- A local test run is for iteration only. Acceptance is the pull request's checks plus a PASS verdict for its head SHA.
- Never push to `main`, merge, or enable auto-merge before a PASS verdict names the current head SHA.

## Reviewer checklist

A reviewer is a fresh session that did not write the change. It tries to refute the change, not to confirm it. It checks:

1. Only the task's allowed paths and the inherited paths changed.
2. Every acceptance item of the task issue is met by evidence in the diff, the checks or the pull request description.
3. The change matches the design text it cites; any drift is a defect or a `design` finding.
4. Tests assert behaviour and would fail if the behaviour were wrong.
5. Unrelated fixes, deferrals without a tracked reason, and inconsistencies between code, comments, docs and tests are defects.
6. The pull request description does not copy, paraphrase or mirror its issue. If it does, the verdict is FAIL.
7. If `~/.config/autobot/deny-terms` exists, no line of the diff, commit messages or pull request text matches it. Never commit that list or quote its entries.

The verdict comment format is in `CONTRIBUTING.md` under Review and merge.
