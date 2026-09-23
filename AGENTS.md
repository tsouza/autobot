# Agent instructions

`CONTRIBUTING.md` is the rulebook and applies in full. This file applies it to agent sessions and carries the reviewer checklist; where the two differ, `CONTRIBUTING.md` wins.

## Before starting

- Read the task issue and every design section its **Design refs** name. The capsule is the contract; `CONTRIBUTING.md` lists the paths every task may additionally touch.
- Do not take a `human-lane` task unless the owner is supervising the session.

## While working

- If the design text looks wrong, stop and file a `design` finding; do not work around it in code.
- Never run CI locally: `just ci` and the recipes it is made of run only in CI, on the self-hosted runners or on GitHub for what cannot run there. Locally, run only the narrow command you are iterating on, such as one crate's `cargo check` or one test, then push and read the pull request's checks. Acceptance is those checks plus a PASS verdict for the head SHA.

## Reviewer checklist

A reviewer is a fresh session that did not write the change. It tries to refute the change, not to confirm it. It checks:

1. Only the task's allowed paths and the inherited paths changed.
2. Every acceptance item of the task issue is met by evidence in the diff, the checks or the pull request description. The reviewer reads the checks and their logs and does not re-run the suite locally; it runs locally only what an acceptance item needs that CI does not cover.
3. The change matches the design text it cites; any drift is a defect or a `design` finding.
4. Tests assert behaviour and would fail if the behaviour were wrong.
5. Unrelated fixes, deferrals without a tracked reason, and inconsistencies between code, comments, docs and tests are defects.
6. The pull request description does not copy, paraphrase or mirror its issue. If it does, the verdict is FAIL.
7. If `~/.config/autobot/deny-terms` exists, no line of the diff, commit messages or pull request text matches it. Never commit that list or quote its entries.

Every finding is either **blocking** or **advisory**:

- **Blocking:** the change is wrong for inputs the task has to handle, an acceptance item is not met, a project rule is broken, or the pull request description states something false.
- **Advisory:** everything else, such as inputs the task's purpose does not cover (a guardrail against honest mistakes need not resist deliberate evasion unless its issue says so), style, and wording that is not false.

The verdict is FAIL only when there is a blocking finding. When the only blocking findings concern the pull request description, the fix is to the description alone, and the next review checks the description against the head that was already reviewed. When the same blocking problem comes back after a fix, the reviewer names the approach as the defect instead of the next symptom.

The verdict comment format is in `CONTRIBUTING.md` under Review and merge.
