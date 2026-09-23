# AutoBot — onboarding and convergence

Authority for: how a brief or an existing repository becomes an accepted plan; what "milestone done" means; the stop conditions; the consequence class and the worker floor; degraded modes and what each forbids; the charter — its levels, content, enforcement modes, waivers, relation to operational policy and bootstrap.
Depends on: `AUTOBOT-THESIS.md` (goals 1, 2, 6; I-3, I-4, I-5, I-6, I-7, I-8, I-9, I-10), `AUTOBOT-KERNEL.md` (§5, §7, §8, §9, §10), `AUTOBOT-ROLES-AND-RUNTIME.md`.

## 1. From a brief to an accepted plan

The engineer's document is context, not an executable plan. Three concepts stay distinct throughout: a **WorkContext** (scope, policy, credentials, participating repositories and projects), a **Project** (a logical product or initiative that may span repositories and may predate AutoBot), and a **Repository** (a forge location with its own branch, CI and toolchain). A repository is not a project; a project is not a plan.

The intake is staged, and each stage is a resource so it is visible, resumable and auditable:

1. **Capture.** The document and its attachments become an immutable `WorkBrief` artifact with source, author and digest; an `Intake` owns the processing state. Nothing runs and nothing external is created because a document was uploaded. A changed document is a new brief revision; the original is never rewritten.
2. **Understand.** The Manager extracts objective, non-goals, constraints, acceptance signals, risks, dependencies and candidate projects and repositories; bounded typed questions classify ambiguity and prioritize questions; deterministic adapters inspect forge and CI metadata. Every extracted fact records the brief digest, its confidence and its provenance.
3. **Resolve scope.** Repositories and projects are matched by authenticated forge identity, never by names in prose. The result is an adoption proposal: create a project, attach an existing one, bind repositories, or leave a reference unresolved. An ambiguous match is a question or an `Intervention`; a project is never silently created because a name did not match.
4. **Propose.** The `PlanProposal` carries candidate outcomes, milestones, task obligations with the outcome each advances, dependencies, non-goals, acceptance evidence, repository scope, estimated cost, and the consequence class of each task (§5). It is mutable and reviewable.
5. **Resolve unknowns.** A question that materially affects scope, safety, cost or acceptance becomes a `Decision` or `Intervention`. Where policy allows, AutoBot proceeds on a recorded assumption with an expiry; it never hides ambiguity inside a prompt or manufactures precise tasks from an under-specified brief.
6. **Accept.** An accepted charter revision is a precondition of acceptance (§7). Acceptance freezes the plan revision — brief digest, bindings, pinned charter revisions, resolved graph, criteria, non-goals, policy, budget, execution profiles — and creates the `Plan` in `ACCEPTED` and its `PlanSnapshot`. This is the boundary between planning and execution. Activation is KERNEL §5; a later brief attaches to the project and proposes a delta as a new revision, never a re-decomposition.

Project prerequisites — repository creation, branch protection, CI registration, toolchain configuration — are themselves a bounded plan verified through external operations; a project is not ready because files were generated.

## 2. Adopting an existing repository

Adoption starts from observation, not intent. AutoBot binds the `Repository` and `Project`, then records authenticated observations of branches, open issues and pull requests, CI definitions, recent commits, existing AutoBot resources and unfinished work. Observing an issue does not make it a task; existing remote work is linked only after identity and ownership are established, and the mechanism for that link is an extension. Dirty local checkouts are `Workspace` custody from the moment they are seen (I-3): inventoried, preserved or quarantined, never adopted by deletion. The observations feed the same proposal and acceptance as §1 and the charter candidates of §7, and an adoption proposal preserves remote identities rather than starting over.

The target project's toolchain is configuration: image, checkout, setup, build and verify commands come from the repository's execution profile. AutoBot infers no language or build system and no rule anywhere assumes one.

## 3. What "milestone done" means

A milestone is accepted when all of the following hold at one plan revision, and the acceptance is a recorded acceptance adjudication committed by the Task controller under deterministic acceptance policy — no agent and no judgment decides it:

- every member task is `ACCEPTED` or `SUPERSEDED`, and no member is `BLOCKED` on a decision no one has taken;
- the **composed, integrated candidate** — the result of the milestone's final `IntegrationBasis` at its current generation — has passed the milestone's acceptance evidence; individually passing branches count for nothing (I-6);
- every `EvidenceBundle` the adjudication relies on is `RECORDED` and unexpired at the moment of acceptance, with its remote generation current;
- no external operation in the milestone's scope is `OUTCOME_UNKNOWN`, `RECONCILING` or `UNRESOLVED` (I-4);
- every member `TaskRun`'s expected records are `RECORDED` or `GAP` (I-9);
- the acceptance adjudication lists required, optional, missing, rejected and expired evidence, and any no-test exception carries its named no-test approver, rationale, compensating evidence, residual risk and expiry — visible as an exception, never as a passing test.

A task is accepted the same way, by the Task controller: its required evidence `RECORDED` and unexpired for the exact candidate, that candidate scope-clean at its last checkpoint, and every required `VerificationRun` `PASSED` or covered by a recorded exception. A milestone accepted this way is *operationally* accepted; its cost may still be pending or censored (KERNEL §8), and defects remain open through the maturity window the profile sets, because absence of a report is not proof of quality. Operational acceptance and economic settlement are separate states and neither is inferred from the other.

## 4. Stop conditions

Convergence stops only in a named state. Each condition below ends in states printed in KERNEL §10, with the affected scope visible and nothing proceeding silently:

| Condition | Where it lands |
|---|---|
| **Budget exhausted** — the `Budget` is `EXHAUSTED` or no reservation can be made for the next admissible attempt | No new `TaskRun` is admitted; running attempts finish to their next checkpoint; the plan stays `ACTIVE` with tasks `BLOCKED` and an `Intervention` for more budget or a replan. |
| **Human deadline** — a `Decision` or `Intervention` a member depends on passes its deadline unanswered | The member stays `BLOCKED`; the request escalates; the plan continues elsewhere or, if nothing is eligible, becomes the next row. |
| **Blocked with no eligible action** — no task is `READY`, every remaining task is `BLOCKED` on a decision, dependency, capability or evidence | The Plan controller records the blocking set as a condition and raises an `Intervention` of kind `PAUSE`; the plan is `PAUSED` awaiting a human `RESUME` or a revision; it is never marked `FAILED` for lack of options. |
| **Unresolved effect** — an operation the milestone depends on is `UNRESOLVED` | Dependents block; the milestone cannot be accepted until a human adjudicates the operation. |
| **Custody uncertain** — a workspace the candidate depends on is `QUARANTINED` or in `CONFLICT` | The candidate is ineligible for evidence until adjudication; the task stays `VERIFYING` or `BLOCKED`. |

A plan reaches `FAILED` only by explicit decision (a revision authority or a human intervention), never by timeout.

## 5. Consequence class and the worker floor — I-10

Every task carries one **consequence class**, assigned at proposal time by the charter's deterministic path and consequence rules (§7) plus the Manager's judgment, and recorded in the plan revision:

| Class | Meaning | Worker floor | Review |
|---|---|---|---|
| `REVERSIBLE` | A local change that can be reverted with no external consequence: contained code, tests, documentation. | The tier `WorkContext.spec` names as the `REVERSIBLE` floor, or higher. | Independent reviewer at the fixed review tier. |
| `COMPATIBILITY_RISK` | Public interfaces, schemas, wire formats, dependencies, build and CI configuration — anything a consumer could observe. | The tier `WorkContext.spec` names as the `COMPATIBILITY_RISK` floor, or higher. | Independent reviewer at the fixed review tier; compatibility checks required in evidence. |
| `SECURITY_OR_DATA_INTEGRITY` | Authentication, authorization, secrets, cryptography, migrations, destructive data operations, deployment. | The highest tier of the `WorkContext.spec` tier order. | Separately configured reviewer; security checks required; no no-test exception without human approval. |

The floor is a constraint the router (an extension) minimizes above. A charter entry may raise a task's class or floor; a typed-question judgment may raise them and can never lower them (I-8). A `TaskRun` pins its class, its floor and its routing pin at admission, and none changes for the life of the attempt. Review, repair and escalation costs are attributed to the strategy that caused them, so a floor is never "saved" by choosing a cheaper worker and paying for it in review.

## 6. Degraded modes and what each forbids

Degradation is scoped to the unavailable dependency, never global, and never widens a permission (I-8). What each mode still allows is bounded local progress toward the next checkpoint; what it forbids is listed:

| Unavailable | Forbidden until recovery | Ends in, if it never recovers |
|---|---|---|
| Kubernetes API | Every commit, acceptance, admission and effect. Agents run to their next checkpoint and stop. | Preserved workspaces; `TaskRun`s `RECOVERING`. |
| Authority witness | New grants; any dispatch on a restored installation. | `RestoreRequest` `READ_ONLY`. |
| EffectBroker | Every privileged effect and tool call. | Operations `PERMITTED` with ledger entries, recovered on restart. |
| Artifact store | Preservation, retirement, any evidence from a checkpoint that could not be verified. | Workspaces `QUARANTINED`. |
| Model provider or harness | New sessions on that provider; never a silent retry of an effectful call. Continuation resumes when it returns. | `AgentRun` `HEARTBEAT_LOST`, then fenced; `TaskRun` `RECOVERING`. |
| Semantic judge | Any decision in the affected question class beyond its deterministic conservative branch. | `Decision`s awaiting a human under their deadline. |
| Forge or CI | Acceptance that needs fresh evidence from it; any merge. Observations resume by relist. | Tasks `VERIFYING` with expired evidence; operations `OUTCOME_UNKNOWN`. |
| A required human | The affected scope. | `BLOCKED` with an escalated `Intervention`. |

Acceptance, scope expansion, merge, destructive effects and workspace deletion always require current authoritative state and healthy dependencies. Health and mode are condition vocabularies on components and capabilities, not lifecycle states of any aggregate.

## 7. The charter — I-5, I-7, I-8

A **charter** says what must be true of all work, whatever plan it serves: its Constitution laws, rules, conventions, vocabulary and purpose. It has two levels. The `Charter` of a `WorkContext` holds the guardrails and conventions that apply across the organization, and every project in the context inherits them. A project's `ProjectCharter` adds project-specific entries and may only tighten, never relax, what it inherits. The **effective charter** of a project is the union of both, and a conflict between two entries resolves toward the stricter one.

**Content.** A charter revision has six sections: Identity, Constitution, Rules, Conventions, Vocabulary and the project's Non-goals. Every entry has a stable id (L-n for a law, R-n for a rule, C-n for a convention), one enforcement mode and a short timeless statement. Rationale goes in a companion background section of the revision, never in an entry.

**Laws and rules.** The absolute guardrails form the project's **Constitution**; each is a **law**. A law changes only through a new charter revision authored by a human, and it is never waived. Every other guardrail is a **rule**. An exception to a rule is a **waiver**: a `Decision` scoped to one plan revision or one task, recording a named human approver, a reason and an expiry, and visible in the `EvidenceBundle` of the work it covers. No agent can grant a waiver.

**Enforcement modes.** Each entry declares exactly one mode, and the human author chooses it:

| Mode | Enforcement |
|---|---|
| `mechanical` | Checked by code in the broker or at custody checkpoints, such as a term law (a term that must never be written) or a path law (a path that must never be changed). The broker refuses a violating effect before it executes; a violation found at a checkpoint makes the candidate ineligible for evidence and is recorded on a `Finding`. |
| `review` | Checked by the reviewer against the candidate; a violation is a blocking finding (ROLES §3). |
| `judged` | Checked by a typed question at each checkpoint or candidate, block-only, and always backed by `review`. |
| `advisory` | Context only; never blocks. |

A law is never `advisory`.

**`judged`.** For an entry marked `judged`, each checkpoint or candidate gets a typed question over the diff and the entry's statement: a yes/no "violates L-n?", or a choice among complies, violates and unsure. The judgment is one-way. "Violates" above the entry's threshold blocks the candidate and opens a `Finding`. "Complies" grants nothing, and the entry's `review` backstop still applies (I-8). An outage or an abstention falls back to the backstop, never to a pass. Questions are batched per checkpoint, and their cost is recorded in the ledger (KERNEL §8).

**Relation to operational policy.** The two stay separate, and the charter is upstream. The charter says what must be true of the work: Constitution laws, rules, conventions, vocabulary and purpose. Operational policy — `WorkContext.spec` and `Repository.spec` — says how AutoBot runs: credentials, tiers, budgets and execution profiles. Path and consequence rules (§5) are charter rules. Operational policy may reference charter entries and never contradicts them.

**Where it lives.** The `Charter` or `ProjectCharter` resource, with its immutable revisions, is canonical, and edits go through the CLI as new revisions. The charter is projected read-only into the repository as `CHARTER.md` at its root; the projection is a view of the resource, never a source (I-7).

**Pinning.** Plans pin a charter revision at acceptance; KERNEL §5 states how a later change reaches pinned work.

**Bootstrap.** A human writes the context's `Charter` before the context's first project. A project charter revision is a precondition of plan acceptance (§1 step 6):

- from a design document, intake drafts it;
- from a work-in-progress repository (§2), intake extracts candidate entries, each labelled with its source.

A human accepts every entry; an agent never promotes a candidate.
