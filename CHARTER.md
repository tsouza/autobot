# Charter

The standing description of this project: what it is, the laws every change obeys, the rules that allow recorded exceptions, and the conventions and vocabulary it is written in. It does not describe any milestone; milestones and their issues do that.

Every entry has a stable id and an enforcement mode:

| Mode | Meaning |
|---|---|
| `mechanical` | Checked by code; a violation fails a required check. |
| `review` | Checked by the reviewer; a violation is a blocking finding. |
| `judged` | Checked by a typed-question judgment that can only block or escalate; a `review` backstop always applies. |
| `advisory` | Context for contributors; never blocks. |

Laws are absolute: a law changes only through a new revision of this charter, and it is never waived. Rules may have an exception, recorded in the pull request with its reason.

## Identity

- **Name:** AutoBot.
- **Purpose:** a durable multi-agent software delivery engine that converges on an accepted milestone with role-specialized agents, spending as little frontier-model money as the quality bar allows, never losing work and never acting on stale authority.
- **Design:** `docs/design/`, starting at [the thesis](docs/design/AUTOBOT-THESIS.md).

## Constitution

| Id | Law | Mode |
|---|---|---|
| L-1 | The project is never presented in comparison with another project. Another project may be cited only as a reference for a specific decision, never as a competitor. | `review` |
| L-2 | Committed content — code, comments, tests, fixtures, documentation, commit messages and pull request text — names no maintainer account and no local tooling. | `review` |
| L-3 | Committed content names no internal review process. | `review` |
| L-4 | `main` changes only through pull requests that pass every required check, including a review verdict bound to the pull request's head. | `mechanical` |
| L-5 | No required check depends on a model, an API key or a secret. | `review` |
| L-6 | Design text under `docs/design/` changes only through pull requests labelled `design-change`; implementation work that finds it wrong files a finding instead. | `mechanical` |

## Rules

| Id | Rule | Mode |
|---|---|---|
| R-1 | A pull request changes only its task's allowed paths and the paths CONTRIBUTING lets every task change. | `review` |
| R-2 | A pull request delivers exactly one task. | `review` |
| R-3 | Every workflow `run:` step is a single `just` recipe. | `mechanical` |
| R-4 | Logic behind a recipe lives once, in `autobot-devtools`; scripts are thin entry points. | `review` |
| R-5 | Tests assert behaviour and fail when the behaviour is wrong. | `review` |

## Conventions

| Id | Convention | Mode |
|---|---|---|
| C-1 | Work items, findings, worktrees, branches and the pull request title and description follow [CONTRIBUTING](CONTRIBUTING.md). | `review` |
| C-2 | Reviews follow the checklist in [AGENTS.md](AGENTS.md): only blocking findings fail a verdict. | `review` |
| C-3 | A document describes what the thing is; its rationale goes to a companion `XXX.background.md` when that reaches both floors in CONTRIBUTING, and stays inline otherwise. | `review` |
| C-4 | The Rust toolchain is pinned in `rust-toolchain.toml` and kept at the latest stable release. | `advisory` |

## Vocabulary

| Id | Entry | Mode |
|---|---|---|
| V-1 | Design terms mean what [the glossary](docs/design/AUTOBOT-GLOSSARY.md) defines; a new term enters the glossary before it is used in the design. | `review` |
| V-2 | Retired design terms are not used; `just check-retired-terms` reads them from the glossary. | `mechanical` |

## Non-goals

| Id | The project never | Mode |
|---|---|---|
| N-1 | makes a forge, a CI provider or a model provider the source of truth. | `review` |
| N-2 | lets a model output grant authority, scope, budget or acceptance. | `review` |
| N-3 | trades the quality floor for cost. | `review` |
