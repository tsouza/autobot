# AutoBot — onboarding and convergence

Authority for: how a brief or an existing repository becomes an accepted plan through the intake client, its interview, provenance and human acceptance; what "milestone done" means; the stop conditions; the consequence class and the worker floor; degraded modes and what each forbids; the charter — its levels, content, enforcement modes, waivers, relation to operational policy and bootstrap.
Depends on: `AUTOBOT-THESIS.md` (goals 1, 2, 6; I-3, I-4, I-5, I-6, I-7, I-8, I-9, I-10), `AUTOBOT-TRUST-MODEL.md`, `AUTOBOT-KERNEL.md` (§2, §5, §7, §8, §9, §10), `AUTOBOT-ROLES-AND-RUNTIME.md`.

## 1. From a brief to an accepted plan

The engineer's document is context, not an executable plan. Three concepts stay distinct throughout: a **WorkContext** (scope, policy, credentials, participating repositories and projects), a **Project** (a logical product or initiative that may span repositories and may predate AutoBot), and a **Repository** (a forge location with its own branch, CI and toolchain). A repository is not a project; a project is not a plan.

Onboarding goes through an **intake client**: a process a person runs, outside AutoBot, in the working directory of the project to onboard (TRUST). For a new project that directory holds only design documents; for an existing one it is a work-in-progress checkout with a forge repository, issues and pull requests. Both cases take the one path below and differ only in what the directory holds. The client analyses the directory, interviews the person and submits the result under the **intake-submitter identity** of the namespace it submits to (TRUST). A client proposes; it never accepts, activates or grants anything.

**The attributes are the schema.** Every attribute an onboarding settles is a field of the spec of an intake kind — `WorkBrief`, `Intake`, `Project`, `Repository`, `Charter`, `ProjectCharter`, `PlanProposal` (M0 §1) — and a proposal is complete exactly when it validates against the schemas of those specs. Every attribute is a **sourced value**: the value with its **provenance**, one or more entries, each naming a source — a path or URL with a content digest, or an interview question with the digest of the question and its answer — and a **trust label**: `UNTRUSTED_REPOSITORY_CONTENT` for content of the directory or of a repository, `UNTRUSTED_ISSUE_OR_PR_TEXT` for issue and pull-request text, `INTERVIEW_ANSWER` for the person's answer as the client relays it. Every label is untrusted content: nothing the client read or was told becomes authority; it becomes a proposal field with provenance, and only an accept (step 6) makes it canonical.

**Proposal state** is an `Intake` in `CAPTURED` or `PROPOSED`, a `Project` or `Repository` in `PROPOSED`, a `Charter` or `ProjectCharter` revision in `PROPOSED`, a `PlanProposal` in `DRAFT` or `REVIEW`, and a new `WorkBrief`. The intake-submitter identity writes intake kinds only in proposal state, and admission refuses its write of an attribute without provenance, of a `Charter` entry any of whose provenance entries is not `INTERVIEW_ANSWER`, and of any accept. Every proposal an `Intake` holds references it; the Intake controller creates the `Charter` of a context when it creates the bound successor `Intake` at the context accept (step 6), and the `ProjectCharter` of each proposed project, so the client only proposes their revisions. A new or revised proposal the `Intake` holds, or a revision of the `Intake`, returns an `Intake` in `PROPOSED` to `CAPTURED`, and a revision returns a `PlanProposal` in `REVIEW` to `DRAFT`.

The intake is staged. The first three stages run in the client and leave nothing in AutoBot; the rest are resources, so what AutoBot holds is visible, resumable and auditable:

1. **Understand.** The client reads the directory and, for an existing project, the forge's issues and pull requests, and extracts objective, non-goals, constraints, acceptance signals, risks, dependencies, and candidate projects and repositories, each with its provenance.
2. **Interview.** The client asks the person, one question at a time, for every attribute a required field needs that the directory does not settle, until none is left. Ambiguity is a question, never an assumption: the client never hides it inside a prompt or manufactures precise tasks from an under-specified brief. The person names the documents that form the brief and confirms each.
3. **Resolve scope.** The client matches repositories and projects by their forge identity, never by names in prose, and settles the adoption: a new project or an existing one to attach, and the repositories to bind. A project is never created because a name did not match.
4. **Submit.** The client's first submission is the `Intake`. An `Intake` for a project of an existing context lives in that context's namespace. An `Intake` that proposes a new context lives in the installation's intake namespace and carries the proposed `WorkContext` configuration, which is accepted (step 6) before anything else is submitted. That `Intake` holds only the configuration: admission refuses any other proposal that references it. The client then submits the adoption as `Project` and `Repository` proposals, each brief document as an immutable `WorkBrief` — its content, which the Intake controller stores as the brief's artifact, or, for a committed file, its path at a pushed commit of a proposed repository, with the content digest either way; input beyond the object limits of the profile is refused (M0 §2) — the charter revisions (§7), and the `PlanProposal`: candidate outcomes, milestones, task obligations with the outcome each advances, dependencies, non-goals, acceptance evidence, repository scope, estimated cost, and the consequence class of each task (§5). A changed document is a new brief revision; the original is never rewritten. Nothing runs and nothing external is created because a proposal was submitted.
5. **Verify.** The Intake controller checks what the client claimed and records the result as a condition. A proposed `Repository` is `ForgeVerified` only when the forge adapter's authenticated answer matches it (§2); a brief's digest is verified over the stored content, or through the forge adapter for a path at a commit, and only the latest revision of each brief counts. It moves a `PlanProposal` to `REVIEW` once it validates and every reference in it resolves, and the `Intake` to `PROPOSED` once every proposal it holds is valid, every repository it names is `ForgeVerified` and every brief digest is verified. The `Intake` status then records the revision and digest of every proposal it holds. The client revises any of them until the person is satisfied.
6. **Accept.** Only a named human accepts, never an intake client. Admission enforces the principal: it refuses every accept from the intake-submitter identity, admits each accept only from the principal named for it, and requires it to pin the revision and digest of what it accepts; the CLI is the product path for these accepts (`kubectl` stays a full client under the same rule). A context that does not exist yet has no reviser, so a proposed `WorkContext` configuration is accepted only by a bootstrap administrator the installation names when AutoBot is installed; that administrator creates the target namespace and its intake-submitter identity. Accepting the configuration creates the `WorkContext` from it and binds the onboarding to it: the `Intake` in the intake namespace, whose only proposal was the configuration, ends `ACCEPTED`, and the Intake controller creates its bound successor `Intake` in the context's namespace, carrying the digest of the accepted configuration, which is immutable from then on, and the condition `ContextAccepted`. The cross-context rule (TRUST) applies to the successor from its creation; only the intake-submitter identity of the context's namespace revises it, and every later proposal of the onboarding references it. The configuration names the context's human roles (ROLES §5), and from then on the reviser it names, `WorkContext.spec.revisionAuthority`, accepts the revisions of the context's `Charter` and of each project's `ProjectCharter`, every plan proposal, and every plan revision, whoever proposed it (KERNEL §5). No agent identity, the Manager's included, accepts a plan or a revision: the owning controller of what an accept commits — the Intake controller for a `PlanProposal`, the Plan controller for a revision proposed on the `Plan` — rejects an accept from any principal but the reviser. An accepted revision of the context's `Charter` and of the project's `ProjectCharter` is a precondition of plan acceptance (§7). A plan accept targets the `Intake` in `PROPOSED`, pinned to its revision and to the digest of the proposals it records, so any proposal the client submits or revises after the reviser read them fails it. It freezes the plan revision — brief digest, bindings, pinned charter revisions, resolved graph, criteria, non-goals, policy, budget, execution profiles — creates the `Plan` in `ACCEPTED` and its `PlanSnapshot`, moves the proposed projects and repositories it binds to `ADOPTED`, the `PlanProposal` and the `Intake` to `ACCEPTED`. This is the boundary between planning and execution; activation is KERNEL §5. An `intake reject` rejects every proposal the `Intake` still holds in proposal state, and admission enforces it like an accept: it is admitted only from the principal who may accept that `Intake`, and never from the intake-submitter identity.

**Later briefs.** A later brief or document change for an onboarded project takes the same path: a new `Intake` naming the project, whose status shows the project's plan and its active revision; a new brief revision; and a `PlanProposal` that names that plan and revision and proposes a delta, never a re-decomposition. Its accept (§1 step 6) accepts that revision, which replaces the active one by supersession (KERNEL §5); the accept fails when the plan's active revision is no longer the one the proposal names.

Project prerequisites — branch protection, CI registration, toolchain configuration — are themselves a bounded plan verified through external operations; a project is not ready because files were generated. Creating a forge repository is a live forge write, disabled until its gate (M0 §5): until then the person creates the repository outside AutoBot, and onboarding adopts it.

## 2. Adopting an existing repository

Adoption starts from observation, not intent. The intake client proposes the `Project` and `Repository` bindings; AutoBot verifies each proposed `Repository` through its forge adapter, binds it only at the plan accept, and then records authenticated observations of branches, open issues and pull requests, CI definitions, recent commits, existing AutoBot resources and unfinished work. The client's own reading of issues and pull requests is a proposal input labelled `UNTRUSTED_ISSUE_OR_PR_TEXT`, never an observation. Observing an issue does not make it a task; existing remote work is linked only after identity and ownership are established, and the mechanism for that link is an extension. The observations and the proposals feed the same acceptance as §1 and the charter candidates of §7, and an adoption preserves remote identities rather than starting over.

Onboarding never takes custody of the person's working directory. Uncommitted or unpushed work the client finds in it is an interview question — commit and push it, or leave it out — and is never uploaded; only the documents the person names as the brief are captured (§1 step 4). AutoBot's custody (KERNEL §7) covers only the workspaces it provisions.

The target project's toolchain is configuration: image, checkout, setup, build and verify commands come from the repository's execution profile. AutoBot infers no language or build system and no rule anywhere assumes one.

## 3. What "milestone done" means

A milestone is accepted when all of the following hold at one plan revision, and the acceptance is a recorded acceptance adjudication committed by the Task controller under deterministic acceptance policy — no agent and no judgment decides it:

- every member task is `ACCEPTED` or `SUPERSEDED`, and no member is `BLOCKED` on an unanswered `Intervention` or `Finding`;
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
| **Budget exhausted** — the `Budget` is `EXHAUSTED` or no reservation can be made for the next admissible attempt | No new `TaskRun` is admitted; running attempts stop at their next checkpoint and end `CANCELLED` with their custody kept; their tasks are `BLOCKED`; the Plan controller raises a `PAUSE` for the reviser, and the plan stays `PAUSED` until a revision whose budget policy raises the ceiling supersedes it or the reviser cancels it; a `RESUME` of this pause is `REJECTED`. A budget ceiling is raised only by a plan revision. |
| **Human deadline** — an `Intervention` a member depends on passes its deadline unanswered | The member stays `BLOCKED`; the request escalates; the plan continues elsewhere or, if nothing is eligible, becomes the next row. |
| **Blocked with no eligible action** — no task is `READY`, every remaining task is `BLOCKED` on an unanswered `Intervention` or `Finding`, a dependency, capability or evidence | The Plan controller records the blocking set as a condition and raises an `Intervention` of kind `PAUSE`; the plan is `PAUSED` awaiting the reviser's `RESUME`, a revision, `FAIL` or `CANCEL` (ROLES §5); it is never marked `FAILED` for lack of options. |
| **Unresolved effect** — an operation the milestone depends on is `UNRESOLVED` | Dependents block; the milestone cannot be accepted until a human adjudicates the operation. |
| **Custody uncertain** — a workspace the candidate depends on is `QUARANTINED` or in `CONFLICT` | The candidate is ineligible for evidence until adjudication; the task stays `VERIFYING` or `BLOCKED`. |

A plan reaches `FAILED` only by the reviser's `FAIL` and `CANCELLED` only by the reviser's `CANCEL` (ROLES §5), never by timeout. An `ACTIVE` plan is `COMPLETED` when every milestone of its active revision is `ACCEPTED` or `SUPERSEDED` and nothing blocks plan completion; the Plan controller commits it.

## 5. Consequence class and the worker floor — I-10

Every task carries one **consequence class**, assigned at proposal time and recorded in the plan revision: the proposer of the revision — the intake client or the Manager — names a class, and the charter's deterministic path and consequence rules (§7) raise it wherever they require a higher one:

| Class | Meaning | Worker floor | Review |
|---|---|---|---|
| `REVERSIBLE` | A local change that can be reverted with no external consequence: contained code, tests, documentation. | `workerFloor[REVERSIBLE]` or higher. | A reviewer at `reviewTier[REVERSIBLE]` or higher, in a session other than the worker's; a correlated review satisfies it (ROLES §3). |
| `COMPATIBILITY_RISK` | Public interfaces, schemas, wire formats, dependencies, build and CI configuration — anything a consumer could observe. | `workerFloor[COMPATIBILITY_RISK]` or higher. | A reviewer at `reviewTier[COMPATIBILITY_RISK]` or higher, on a model other than the worker's; compatibility checks required in evidence. |
| `SECURITY_OR_DATA_INTEGRITY` | Authentication, authorization, secrets, cryptography, migrations, destructive data operations, deployment. | `workerFloor[SECURITY_OR_DATA_INTEGRITY]`, the highest tier of the `WorkContext.spec` tier order. | A reviewer configuration named in `securityReviewers`, at `reviewTier[SECURITY_OR_DATA_INTEGRITY]` or higher, on a model other than the worker's; security checks required in evidence. |

`WorkContext.spec.workerFloor[class]` and `WorkContext.spec.reviewTier[class]` name the worker floor and the review tier of each class, and `WorkContext.spec.securityReviewers` names the reviewer configurations eligible for the required review of `SECURITY_OR_DATA_INTEGRITY` work. A no-test exception, in every class, is a decision of the human no-test approver (ROLES §3). The floor is a constraint the router (an extension) minimizes above. A charter entry may raise a task's class or floor; a typed-question judgment may raise them and can never lower them (I-8). A `TaskRun` pins its class, its floor and its routing pin at admission, and none changes for the life of the attempt. Review, repair and escalation costs are attributed to the strategy that caused them, so a floor is never "saved" by choosing a cheaper worker and paying for it in review.

## 6. Degraded modes and what each forbids

Degradation is scoped to the unavailable dependency, never global, and never widens a permission (I-8). What each mode still allows is bounded local progress toward the next checkpoint; what it forbids is listed:

| Unavailable | Forbidden until recovery | Ends in, if it never recovers |
|---|---|---|
| Kubernetes API | Every commit, acceptance, admission and effect. Agents run to their next checkpoint and stop. | Preserved workspaces; `TaskRun`s `RECOVERING`. |
| Authority witness | New grants; any dispatch on a restored installation. | `RestoreRequest` `READ_ONLY`. |
| EffectBroker | Every privileged effect and tool call. | Operations `PERMITTED` with ledger entries, recovered on restart. |
| Artifact store | Preservation, retirement, any evidence from a checkpoint that could not be verified. | Workspaces `QUARANTINED`. |
| Model provider or harness | New sessions on that provider; never a silent retry of an effectful call. Continuation resumes when it returns. | `AgentRun` `HEARTBEAT_LOST`, then fenced; `TaskRun` `RECOVERING`. |
| Semantic judge | Any decision in the affected question class beyond its deterministic conservative branch. | Questions escalated as `Intervention`s awaiting a human under their deadline, or as `Finding`s for the Manager. |
| Forge or CI | Acceptance that needs fresh evidence from it; any merge. Observations resume by relist. | Tasks `VERIFYING` with expired evidence; operations `OUTCOME_UNKNOWN`. |
| A required human | The affected scope. | `BLOCKED` with an escalated `Intervention`. |

Acceptance, scope expansion, merge, destructive effects and workspace deletion always require current authoritative state and healthy dependencies. Health and mode are condition vocabularies on components and capabilities, not lifecycle states of any aggregate.

## 7. The charter — I-5, I-7, I-8

A **charter** says what must be true of all work, whatever plan it serves: its Constitution laws, rules, conventions, vocabulary and purpose. It has two levels. The `Charter` of a `WorkContext` holds the guardrails and conventions that apply across the organization, and every project in the context inherits them. A project's `ProjectCharter` adds project-specific entries and may only tighten, never relax, what it inherits. The **effective charter** of a project is the union of both, and a conflict between two entries resolves toward the stricter one.

**Content.** A charter revision has six sections: Identity, Constitution, Rules, Conventions, Vocabulary and the project's Non-goals. Every entry has a stable id (L-n for a law, R-n for a rule, C-n for a convention), one enforcement mode and a short timeless statement. Rationale goes in a companion background section of the revision, never in an entry.

**Laws and rules.** The absolute guardrails form the project's **Constitution**; each is a **law**. A law changes only through a new charter revision authored by a human, and it is never waived. Every other guardrail is a **rule**. An exception to a rule is a **waiver** `Decision` (DEFERRED, M0 §5); no agent can grant a waiver.

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

**Where it lives.** The `Charter` or `ProjectCharter` resource, with its immutable revisions, is canonical, and every edit is a new revision: proposed through the CLI or by the intake client (§1), and accepted only by the principal named for it (§1 step 6), with the CLI as the product path. The charter is projected read-only into the repository as `CHARTER.md` at its root; the projection is a view of the resource, never a source (I-7).

**Pinning.** Plans pin a charter revision at acceptance; KERNEL §5 states how a later change reaches pinned work.

**Bootstrap.** Besides revisions a human proposes through the CLI, both levels come through the onboarding of §1, as revisions the intake client proposes with every entry a sourced value:

- The context's `Charter` is authored in the interview, and a revision of it is accepted before the context's first project: a plan accept is refused while the context has no accepted `Charter` revision. Its entries come only from the person's own answers: admission refuses a `Charter` entry from the intake client any of whose provenance entries is not `INTERVIEW_ANSWER`, so no entry is ever extracted.
- An accepted `ProjectCharter` revision is likewise a precondition of plan acceptance (§1 step 6). From a design document, its entries are drafted with the person in the interview; from a work-in-progress repository (§2), candidate entries are extracted from its content, each labelled with its source; at M0-Q admission refuses a `ProjectCharter` entry from the intake client without an `INTERVIEW_ANSWER` provenance entry (M0 §1).

Every entry of either level is accepted only by the principal §1 step 6 names for a charter revision, with the CLI as its product path. An agent never promotes a candidate.
