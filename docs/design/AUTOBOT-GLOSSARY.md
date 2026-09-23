# AutoBot — glossary

Authority for: the meaning of every term used in the core documents, and the layer each term belongs to.
Depends on: `AUTOBOT-THESIS.md`. Definitions summarize the document named beside them and never override it.

## Layers and the dependency rule

| Layer | Meaning | Dependency rule |
|---|---|---|
| **CORE** | Terms without which an invariant in `AUTOBOT-THESIS.md` cannot be stated, or which M0 must implement. Removing one makes AutoBot unsafe, not merely less capable. | A CORE definition references only CORE terms. It never names an EXTENSION or EXTERNAL term, not even in passing. |
| **EXTENSION** | Capabilities built on the core that can be disabled, replaced or absent while every core invariant still holds: learning and routing, TypeSafe decision policy, forge mirroring, observability, the CLI, the onboarding MCP server. | An EXTENSION definition references CORE and EXTENSION terms. An extension never creates authority the core does not grant. |
| **EXTERNAL** | Systems, actors and tools AutoBot runs on, observes or adapts to but does not own. Their facts enter AutoBot only through authenticated observation and a core commit. | An EXTERNAL thing is never authoritative for AutoBot state. |

The check that closes this glossary: every term used in a core document is defined here under CORE, or appears on the DEFERRED list in `AUTOBOT-M0-AND-GATES.md`. Every term used in an extension file is defined here under CORE or EXTENSION. Where a core rule needs a party AutoBot does not own (a provider, the witness), the *role* is a CORE term and the *service that fills it* is EXTERNAL.

## Admission rule for entries

An entry enters CORE only if (a) a thesis invariant depends on it, (b) M0 must implement it, or (c) removing it would leave a term used but undefined in a core document. An entry that fails all three is EXTENSION if some extension file uses it, and is otherwise omitted.

One term, one meaning. A term dropped from the core is dropped everywhere. The retired forms are listed at the end so nobody reaches for them.

Every entry ends with its authority: **THESIS**, **TRUST**, **KERNEL §n**, **ROLES §n**, **ONBOARD §n**, **FORMAL §n**, **M0 §n**, or `extensions/<name>.md`.

## CORE

### Commit and receipt

- **Aggregate** — A custom resource whose `status` is the committed, canonical state of one workflow entity; only its owning controller commits that status. *(KERNEL §1)*
- **Owning controller** — The single controller that may commit an aggregate kind's protected status and that repairs receipts and audit events for commands targeting it. The kind-to-owner table is M0 §1. *(KERNEL §1; M0 §1)*
- **CAS** — One optimistic-concurrency write on one resource with an expected revision as precondition; the only commit primitive. *(KERNEL §1)*
- **Domain lane** — The ordinary commit path: precondition `state_revision`, domain fields only, installs the pending commit. *(KERNEL §1)*
- **Pending commit (slot)** — The single bounded slot a domain CAS installs, holding the exact receipt, audit envelope and effect intents; the next domain commit waits on it. *(KERNEL §1)*
- **Receipt barrier** — The next domain commit on an aggregate waits until the slot's receipt and audit event are written and verified; timeout is `UNCERTAIN`; no slot is cleared by elapsed time. *(KERNEL §1)*
- **Control lane** — The second, disjoint commit path for safety controls: precondition `control_revision`, control fields only, receipt embedded in the same CAS, domain slot and fields preserved. *(KERNEL §1)*
- **Field partition** — The fixed, admission-enforced split of a status into domain and control fields; a CAS touching both is rejected. *(KERNEL §1)*
- **Control receipt / ring** — The receipt of a control commit, appended in the CAS to a bounded ring in status; a full ring refuses the transition. *(KERNEL §1)*
- **Reconciliation field / reconciliation-only CAS** — A declared status field outside both digests that records only the progress of an already-committed intent (slot clearing, ring publication, and the dispatch ledger entries, which a reconciliation-only CAS may advance or remove but never append); a reconciliation-only CAS writes nothing else, increments no revision and installs no receipt, and can make nothing canonical. *(KERNEL §1)*
- **`state_revision` / `control_revision` / `commit_sequence`** — Per-lane preconditions, and the counter both lanes increment that totally orders every commit on one aggregate. *(KERNEL §1)*
- **`AutoBotCommand`** — An authenticated, immutable, single-target mutation request; the only way canonical state changes. *(KERNEL §2)*
- **Reconciliation request** — The broker's request for a ledger update: names the entry by `(operation_uid, permit_uid)` and the expected `send_state`, pins no revision, produces no receipt, and is a no-op when the entry is absent or already past that state. Not a command. *(KERNEL §3.1)*
- **Replay identity** — `(idempotency_key, principal, input_digest)`; the same identity within the replay window returns the original receipt; a changed payload or principal is rejected; after the window, `REPLAY_EXPIRED`. *(KERNEL §2)*
- **`CommandReceipt`** — The deterministically named, durable per-command record with immutable prepared input and a controller-owned terminal result. *(KERNEL §2, §10)*
- **Create identity / origin metadata** — A create's deterministic reserved name and the immutable `(create_receipt_uid, input_digest, context_uid)` written with the object; lost acknowledgement is resolved by GET, never by a generated name. *(KERNEL §2)*
- **`AutoBotEvent`** — The immutable audit event published after a commit and repairable from the slot or ring; never an authority. *(KERNEL §1)*
- **Projection** — A rebuildable read model applied in `(aggregate_uid, commit_sequence)` order with visible gaps and quarantined digest conflicts. *(KERNEL §1)*

### Admission and dispatch

- **`WorkContext`** — The namespace-level aggregate that is the serialized admission authority for everything under it; holds the dispatch registers. *(KERNEL §3.1)*
- **Context controller** — The owning controller of `WorkContext`; the only writer of its status; applies every authority cut and acceptance on other controllers' commands. *(KERNEL §3.1)*
- **Dispatch registers** — The `WorkContext` status fields every acceptance is checked against in one CAS: `hold_state`, `hold_generation`, `admission_sequence`, `manager_authority[plan]`, `plan_authority[plan]`, `integration_authority[basis]`, `dispatch_authority_generation`, `dispatch_ledger[]`, `active_manager_transaction`. *(KERNEL §3.1)*
- **Authority cut** — Any register change: hold transition, Manager drain, epoch advance and resume, plan activation, quiescing, resume and supersession, basis reservation and invalidation, dispatch-authority advance. All are `WorkContext` CASes and therefore totally ordered with acceptances. *(KERNEL §3.1)*
- **Hold** — The `hold_state` control field of a `WorkContext` (transitions in KERNEL §10); `RequestHold` is the cut; `hold_generation` increments on every transition; permits are issued only in `RUNNING`. *(KERNEL §3.2, §10)*
- **`AdmissionStamp` (permit)** — A capability to *request* acceptance, pinning every register value plus reservation, grant and capsule identities, effect digest, nonce and expiry. Never itself authority to send. *(KERNEL §3.2)*
- **`AcceptDispatch`** — The `WorkContext` domain commit that accepts a permit when every pinned value equals its register; increments `admission_sequence`; appends a ledger entry. The single linearization point. *(KERNEL §3.2)*
- **Currency** — The state of a grant, reservation and capsule at send time, enforced by the broker before `RecordSendAttempt` and on every privileged call; not a register. *(KERNEL §3.2)*
- **Dispatch ledger / entry** — The bounded accepted-dispatch outbox on `WorkContext`: `send_state ∈ {ACCEPTED_NOT_SENT, SEND_ATTEMPTED, ACKNOWLEDGED}`; `ACKNOWLEDGED` is written and removed in one CAS. *(KERNEL §3.3)*
- **`send_attempt`** — The write-ahead marker on an operation that precedes opening the remote connection; present after a crash means "sent or unknown", absent means "never sent". *(KERNEL §3.3)*
- **`RecordSendAttempt`** — Ledger entry to `SEND_ATTEMPTED`, then operation to `DISPATCHING` with `send_attempt`, in that order, both before the send. *(KERNEL §3.3)*
- **`RecoverAcceptedDispatch`** — The restart scan joining ledger and operations by the six-row table. *(KERNEL §3.3)*
- **EffectBroker (broker)** — The only path for privileged filesystem, forge, CI, deployment and credential effects; validates grant, scope and capability; submits `AcceptDispatch`; writes `send_attempt`; sends; records the outcome. *(TRUST; KERNEL §3)*
- **Routing pin** — The identifier of the worker-strategy configuration a `TaskRun` was admitted with, written into its spec at `AdmitTask` and never changed; not a register. *(KERNEL §3.1)*
- **Effect intent / `EffectIntent`** — The immutable descriptor of one intended effect, committed in the slot and copied to the receipt; materialized as a resource at a deterministic name. *(KERNEL §3.3)*
- **`operation_key`** — `hash(installation_lineage, aggregate_uid, committed_revision, effect_index, payload_digest)`: the logical identity of one effect across retries and restores. *(KERNEL §3.3)*
- **Materialization** — Deterministic creation of `EffectIntent` and `ExternalOperation` resources from committed receipts by a dispatcher that scans, not only watches; a collision with a different payload is quarantined. *(KERNEL §3.3)*
- **`EffectIntentCollision` / `DispatchLedgerConflict`** — Quarantine conditions: a materialization name reused with a different payload; a ledger entry `ACCEPTED_NOT_SENT` beside a present `send_attempt`. Neither is ever dispatched; both need human adjudication. *(KERNEL §3.3)*
- **`ExternalOperation`** — The aggregate for one effect against an external system; lifecycle in KERNEL §10. *(KERNEL §3.3)*
- **`ToolInvocation` / `EffectReceipt`** — The same protocol applied to an effectful agent tool call, and the record of its outcome or `OUTCOME_UNKNOWN`. *(KERNEL §3.3)*
- **`OUTCOME_UNKNOWN` / `RECONCILING`** — A timeout or disconnect is never failure; the operation is reconciled by provider lookup or idempotency, never retried blind. *(KERNEL §3.3)*
- **`UNRESOLVED`** — The retained state of an operation whose provider can neither deduplicate nor look it up; never garbage-collected, exits only by human adjudication. *(KERNEL §3.3)*
- **Dependents (of an `UNRESOLVED` operation)** — Every later effect intent with the same `target_identity`, every `IntegrationBasis` whose heads the operation could have changed, and every acceptance evaluation of the owning milestone; all blocked until adjudication. *(KERNEL §3.3)*
- **Provider capability** — A per-adapter, per-operation declaration (idempotency, lookup, remote marker, head/base requirement, dry run, reconciliation method) with qualification status; an unqualified required semantic yields `BLOCKED_UNSUPPORTED`. *(KERNEL §3.3)*

### Manager serialization

- **Manager** — The semantic planning role for one plan; its authority is the register entry `manager_authority[plan]`, not a process. *(ROLES §1; KERNEL §4)*
- **`manager_authority[plan]`** — `(lease_uid, epoch, holder, deadline, phase ∈ {ACTIVE, DRAINING})`; `ManagerLease` is its acknowledgement and authorizes nothing. *(KERNEL §4)*
- **`active_manager_transaction` (reservation)** — The one-slot reservation of a short control mutation, released only with a recorded terminal receipt. *(KERNEL §4)*
- **Drain before epoch advance** — `DrainManager` (control) → resolve or cancel the reservation → `AdvanceManagerEpoch` (domain) → `ResumeManager` (control); never merged, never reordered. *(KERNEL §4)*
- **Fixed expected revision** — A Manager command's target revision is never refreshed to force an old command through. *(KERNEL §4)*

### Plans and graph

- **`WorkBrief` / `Intake` / `PlanProposal`** — The immutable record of one brief document, its stored artifact or a path at a pushed commit, with its digest; the record of one onboarding submission, which carries a new context's proposed configuration; the candidate graph, revised by the intake client, whose acceptance creates the `Plan` or, when it names a plan, accepts a revision of it. *(ONBOARD §1)*
- **`Project` / `Repository`** — A logical product or initiative that may span repositories; a forge location with its own branch, CI and toolchain. Neither is a plan. A proposed repository is bound only by a plan accept, after an authenticated forge answer verified it. *(ONBOARD §1)*
- **`Charter` / `ProjectCharter`** — The human-authored statement of what must be true of all work in a `WorkContext`, and in one `Project`, whatever the plan: Identity, Constitution, Rules, Conventions, Vocabulary and the project's Non-goals, in immutable, digested revisions. Every project inherits its context's `Charter`; a `ProjectCharter` may only tighten it. *(ONBOARD §7; KERNEL §5, §10)*
- **Effective charter** — The union of a project's `ProjectCharter` and the `Charter` it inherits; a conflict between two entries resolves toward the stricter one. *(ONBOARD §7)*
- **Law / Constitution** — An absolute guardrail of a charter, and the set of them; a law changes only through a new charter revision authored by a human, is never waived and is never `advisory`. *(ONBOARD §7)*
- **Rule / waiver** — A waivable guardrail of a charter; an exception to one is a waiver `Decision` (DEFERRED, M0 §5), which no agent grants. *(ONBOARD §7)*
- **Enforcement mode** — What each charter entry declares, chosen by its human author: `mechanical` (checked by code in the broker or at checkpoints), `review` (a violation is a blocking finding), `judged` (a block-only typed question with a `review` backstop) or `advisory` (context only). *(ONBOARD §7)*
- **Charter pin / charter in force / charter digest** — The accepted charter revisions a plan revision pins at acceptance; those revisions strengthened by every law a later accepted revision adds or tightens; and the digest of that set, which every capsule and `EvidenceBundle` carries. Relaxing a law or changing a rule waits for the next plan revision. *(KERNEL §5)*
- **Execution profile** — A named entry of `Repository.spec`: image, checkout, setup, build and verify commands, and the sandbox constraints (single writable mount, no additional mounts or devices, broker-only egress) a `TaskRun` pins at admission; a departure from it is a profile violation that fences the run. *(ROLES §2; ONBOARD §2; M0 §1)*
- **`Plan` / plan revision / `PlanSnapshot`** — The accepted immutable execution contract; one immutable version of it; the immutable graph bundle of one revision. *(KERNEL §5, §10)*
- **Activation cut** — `ActivatePlanRevision`, the register CAS `plan_authority[plan] := (revision, snapshot_digest, activation_receipt_uid, ACTIVE, plan_generation + 1)`; `Plan` and `PlanSnapshot` acknowledge it; `GraphActivationReceipt` records it. *(KERNEL §5)*
- **`plan_generation`** — Incremented by every activation, quiesce, supersession and resume of a plan; pinned by every permit, so a permit issued before a quiesce fails after a same-revision resume. *(KERNEL §3.2, §5)*
- **Quiescing / supersession / `ResumePlanRevision`** — Fail new acceptances and invalidate permits; settle attempts, record unresolved operations, invalidate evidence, activate the replacement; or resume the same revision with a new generation. *(KERNEL §5)*
- **`RevisionPending` / `PAUSED`** — A Plan *condition* noting an authenticated revision proposal; a plan-level block on new admission requested by an `Intervention` that never weakens a context hold. *(KERNEL §5)*
- **`Milestone` / `Task` / `TaskRun` / `AgentRun`** — A versioned outcome; one bounded obligation across attempts; one attempt with pinned inputs and cumulative counters; one agent session within an attempt. *(KERNEL §10; ROLES §4)*

### Onboarding

- **Intake client** — The untrusted proposer a person runs, outside AutoBot, in the working directory of the project to onboard: it understands the directory, interviews the person and submits proposals under the intake-submitter identity; it never accepts, activates or grants anything. *(ONBOARD §1; TRUST)*
- **Intake-submitter identity** — The dedicated principal of one namespace, the intake namespace or a context's, that an intake client authenticates as: it may create and revise intake kinds in proposal state there and read them, nothing else; admission refuses every accept from it. *(TRUST; ONBOARD §1)*
- **Intake kinds / proposal state** — The kinds the Intake controller owns: `WorkBrief`, `Intake`, `Project`, `Repository`, `Charter`, `ProjectCharter`, `PlanProposal`; and the states in which the intake client may write them: an `Intake` `CAPTURED` or `PROPOSED`, a `Project` or `Repository` `PROPOSED`, a charter revision `PROPOSED`, a `PlanProposal` `DRAFT` or `REVIEW`, a new `WorkBrief`. *(ONBOARD §1; M0 §1)*
- **Interview** — The intake client's questioning of the person, one question at a time, for every attribute a required field needs that the directory does not settle, until none is left; ambiguity is a question, never an assumption. *(ONBOARD §1)*
- **Sourced value / provenance** — An attribute of an intake-kind spec: the value with one or more provenance entries, each a source (a path or URL with a content digest, or an interview question with the digest of question and answer) and a trust label; admission refuses an attribute without one. *(ONBOARD §1; M0 §1)*
- **Trust label** — What kind of untrusted content a proposal field came from: `UNTRUSTED_REPOSITORY_CONTENT`, `UNTRUSTED_ISSUE_OR_PR_TEXT` or `INTERVIEW_ANSWER`; no label makes a field authority. *(ONBOARD §1; TRUST)*
- **Proposal acceptance** — The human act, through the command path, that makes a proposal canonical, pinned to what the human was shown: a new context's configuration by a bootstrap administrator, which creates the `WorkContext` and a bound successor of its `Intake` in the context's namespace; a charter revision by the context's reviser; a plan proposal by the reviser, as an accept of the `PROPOSED` `Intake` at its revision and proposal-set digest, which also binds its projects and repositories. *(ONBOARD §1)*
- **Intake namespace / bootstrap administrator** — The installation's namespace for an `Intake` that proposes a new context, and the human principals the installation names, when AutoBot is installed, to accept a proposed `WorkContext` configuration. *(ONBOARD §1; ROLES §5)*
- **`ForgeVerified` / `ContextAccepted`** — Conditions the Intake controller records: on a proposed `Repository`, that the forge adapter's authenticated answer matched it, which a plan accept requires before binding it; on an `Intake`, that it is the bound successor of an accepted `WorkContext` configuration. *(ONBOARD §1)*

### Scope, identity and fencing

- **`ScopeCapsule`** — The immutable assignment contract of one `TaskRun`: objective, repositories, path globs, tools, effect kinds, non-goals, limits, authorizing revisions, the charter digest with the relevant entries of the charter in force, digest. *(ROLES §2)*
- **Scope canonicalization** — A verdict is computed on `realpath` and inode on the single writable mount, links checked at both ends, renames as delete plus create; preventive before effect, detective at every checkpoint before evidence. *(ROLES §2)*
- **`Finding`** — A scoped discovery outside the capsule, linked *historically* to a task; never changes an active capsule. *(ROLES §2)*
- **Consequence class / worker floor** — `REVERSIBLE`, `COMPATIBILITY_RISK`, `SECURITY_OR_DATA_INTEGRITY`; the class sets the minimum worker tier and the review requirement; a charter entry may raise it, and a judgment may only raise it. *(THESIS; ONBOARD §5)*
- **Tier** — One of the totally ordered capability levels that `WorkContext.spec` policy assigns to every worker-strategy configuration and reviewer configuration; the floor and the fixed review tier are tiers in that order, and admission compares the routing pin's tier against the floor deterministically. *(ONBOARD §5; ROLES §3)*
- **`ExecutionIdentity` / `CredentialGrant`** — The TaskRun-scoped identity and the short-lived capability bound to it, its audience, repositories, paths, operations, lineage, epoch, expiry and revocation generation. *(KERNEL §6)*
- **`execution_epoch`** — A per-TaskRun counter advanced on fencing; part of every identity, grant and checkpoint. *(KERNEL §6)*
- **Fence / `FenceSession`** — Epoch increment, grant and broker revocation, process stop, workspace write fence, verification, `FenceConfirmed`; tracked by the `fence_state` control field (transitions in KERNEL §10). *(KERNEL §6)*
- **`FENCED_UNCERTAIN`** — Fence not confirmable; replacement, cleanup and privileged effects blocked. *(KERNEL §6)*
- **Continuation / `AgentCheckpoint`** — Resumption of the same `AgentRun` (`session_sequence + 1`) from a `VERIFIED` checkpoint with the same identity, grant lineage, capsule and reservation (a fresh capsule only for a law added or tightened since), cumulative counters, open invocations resumed. *(KERNEL §9)*
- **Drift label** — The vocabulary of a task-local assessment: `ON_TRACK`, `DRIFT_RISK`, `DRIFTED`, `BLOCKED`, `COMPROMISED`, `INSUFFICIENT_EVIDENCE`; each has a fixed escalation. *(ROLES §1)*
- **MicroManager** — The optional task-local guardian role; classifies a run's state and may nudge, pause, checkpoint, preserve, fence or request recovery; its absence changes no permission. *(ROLES §1)*

### Custody and restore

- **`Workspace`** — Custody of a checkout; outlives sessions and attempts; retired only from `PRESERVED`. *(KERNEL §7)*
- **Custody checkpoint / `CustodyCheckpoint` / `ArtifactCommit` / `Artifact`** — Fence → inventory (including the record outbox) → manifest → upload → verified independent restore → marker → `ArtifactCommit(VERIFIED)` → `PRESERVED`. Unknown custody is quarantine. *(KERNEL §7)*
- **`CustodyPolicy`** — The declared loss domains, checkpoint cadence, RPO/RTO and quarantine rules a workspace is kept under. *(KERNEL §7; M0 §2)*
- **`RestoreRequest`** — The aggregate that drives one isolated restore through its barrier states. *(KERNEL §7, §10)*
- **`WorkspaceConflict`** — Mixed or unattributable content: attribution manifest, quarantine owner, restore mapping; nothing deleted or assigned until adjudicated. *(KERNEL §7)*
- **Installation / installation lineage** — The identity of one AutoBot deployment; a restore produces a new `installation_id` and `restore_generation`; part of `operation_key` and every grant. *(KERNEL §7)*
- **`RestoreWitnessReceipt` / restore barrier / `dispatch_authority_generation`** — The witness-signed record that, with old-grant expiry or revocation, identity mapping and ambiguous-set reconciliation, ends a restored installation's read-only state by advancing the register. *(KERNEL §7)*

### Evidence and integration

- **`EvidenceBundle`** — Everything an acceptance binds: candidate, base and head digests, plan and scope revisions, charter digest, criteria, environment, provider runs, attestations, reviewer identity, remote generation, expiry; invalidated on any change. *(KERNEL §5)*
- **Acceptance adjudication** — The recorded acceptance decision of a task or milestone, committed by the Task controller from `RECORDED` evidence bundles under deterministic acceptance policy, listing required, optional, missing, rejected and expired evidence; a no-test exception is visible as an exception and needs a human approver. *(ONBOARD §3)*
- **Human adjudication** — The human `Intervention` (`ADJUDICATE_OPERATION`, `ADJUDICATE_CONFLICT`) that is the only exit from `UNRESOLVED` or a `WorkspaceConflict`. *(KERNEL §3.3, §7; ROLES §5)*
- **`VerificationRun`** — Tool, CI or reviewer evidence for one pinned candidate. *(KERNEL §10)*
- **Reviewer independence** — A reviewer is never the worker's session and never below the fixed review tier; the same model in another session is correlated, not independent. *(ROLES §3)*
- **Integration controller** — The owning controller of `IntegrationBasis`; submits basis reservation and invalidation to the Context controller. The Integrator *role* requests through it. *(KERNEL §3.1; M0 §1)*
- **`IntegrationBasis` / `basis_generation`** — The serialized reservation of overlapping change sets, merge order, heads and the integrated candidate; its generation lives in the register and a merge permit pins it. *(KERNEL §5)*
- **Remote generation** — The provider-side version of an observed object; a stale generation satisfies no current acceptance. *(KERNEL §5)*
- **Integrated candidate** — The result of a milestone's final basis, the only thing milestone acceptance evaluates. *(ONBOARD §3)*

### Budget and canonical records

- **`Budget` / `BudgetReservation`** — Hard ceiling; recoverable escrow per purpose; `UNKNOWN` stays held until settlement or explicit conservative expiry. *(KERNEL §8)*
- **Canonical record / ledger** — `OutcomeRecord`, `UsageReceipt` and `TelemetryGap`: aggregates created through the create protocol, never a sampled or best-effort signal. *(KERNEL §8)*
- **Expected-record obligation** — `TaskRun.status.expected_records{outcome, usage} ∈ PENDING | RECORDED | GAP(uid)` with a `record_deadline`; a record still missing at the deadline becomes a linked gap that counts as unknown outcome and censored cost. *(KERNEL §8)*
- **Durable local outbox** — The process-side queue that holds a record until its create receipt is `COMMITTED`; inventoried by custody. *(KERNEL §8)*
- **Censoring** — An unsettled `UsageReceipt` becomes `CENSORED` at `min(reservation ceiling, rate-card bound)` and counts at that bound. *(KERNEL §8)*

### Judgment and humans

- **Typed question / eligible set / `Decision`** — A semantic judgment is a typed question over an eligible set computed deterministically beforehand; the service picks, code decides; the immutable `Decision` records question class, evidence digest, eligible-set digest, selection and policy revision. *(THESIS I-8; KERNEL §1)*
- **Conservative branch** — What a question class does when the judge is unavailable or abstains: hold, escalate to a human, or take the fixed baseline; never widen a permission. *(ONBOARD §6)*
- **`Intervention`** — An authenticated request — `HOLD`, `RESUME`, `PAUSE`, `REVIEW`, `QUIESCE`, `SUPERSEDE`, `KILL_SWITCH`, `ADJUDICATE_OPERATION`, `ADJUDICATE_CONFLICT` — submitted by a human principal, or raised by an owning controller under its own principal to summon one (the answer is then a further human `Intervention` referencing it; `RESUME` is the only exit from a hold or a pause). Applied only through commands to the owning controller: a register or `Plan` CAS for the first seven actions, a CAS on the adjudicated `ExternalOperation` or `WorkspaceConflict` for the two adjudications; never by editing status. *(ROLES §5)*
- **Human roles** — Reviser (`WorkContext.spec.revisionAuthority`), adjudicator, no-test approver, kill-switch operator, and the installation's bootstrap administrator; never held by an agent identity or the intake-submitter identity. *(ROLES §5)*
- **Stop condition** — Budget exhausted, human deadline, blocked with no eligible action, unresolved effect, custody uncertain; each lands in a named state. *(ONBOARD §4)*
- **Degraded mode** — The scoped, named set of actions forbidden while one dependency is unavailable; never widens. *(ONBOARD §6)*

### External parties the kernel addresses (roles; the services filling them are EXTERNAL)

- **Forge** — The role of the system that hosts repositories, issues, pull requests and branch protection; addressed only through brokered effects and authenticated observations; never the source of truth. *(THESIS I-7; TRUST)*
- **CI provider** — The role of the system that executes tests and checks; its results are observations bound to head, workflow and environment. *(TRUST; KERNEL §5)*
- **Model provider / harness** — The role of the system that runs an agent session; reached only through a runtime adapter. *(ROLES §4)*
- **Semantic judge** — The role of the service that answers typed questions over an eligible set; the thesis names the chosen service, which is EXTERNAL. *(THESIS goal 4; ONBOARD §6)*
- **Authority witness** — The independently operated party in a separate failure domain that holds dispatch authority and signs restore receipts; AutoBot cannot self-assert fencing. *(TRUST; KERNEL §7)*

### Formal surface and gates

- **Invariant** — A thesis invariant `I-n`, or a bounded-model safety property `F-n` that makes one precise and maps to a guard and a fixture group. *(THESIS; FORMAL §4)*
- **Negative variant** — A model with one guard removed that must produce a counterexample to its named invariant. *(FORMAL §5)*
- **Conditional liveness assumption** — A named finite bound with an explicit degraded terminal branch; liveness is never unconditional. *(FORMAL §6)*
- **Review classification** — `DESIGN_DEFECT`, `DESIGN_GAP`, `PLANNED_ARTIFACT`, `IMPLEMENTATION_FAIL`, `OUT_OF_SCOPE`; only the first two block design closure. *(FORMAL §9)*
- **Gate** — One row of the sole gate table, `NOT_RUN | RUNNING | PASSED | FAILED | INVALIDATED`, with digest-bound evidence; capability is the intersection of gate evidence, policy, health and holds. *(M0 §4)*
- **Fixture group** — A named counterexample set required for M0-Q: `G-COMMIT`, `G-MANAGER`, `G-DISPATCH`, `G-EFFECT`, `G-SCOPE`, `G-CUSTODY-M0`, `G-RESTORE`, `G-EVIDENCE`, `G-RECORD`. *(M0 §3)*
- **M0 profile** — The single home of every proposed numeric limit. *(M0 §2)*

## EXTENSION

### Learning and routing — `extensions/learning-and-routing.md`

- **Execution strategy** — The unit optimized: context preparation, worker model and harness, reasoning budget, verification, reviewer, repair rounds, escalation.
- **Problem class / taxonomy** — Learned, versioned, pre-dispatch classification of work by operation, reasoning demand, coupling, specification, verification, novelty, consequence and environment; a release pins every definition.
- **`WorkerCapability`** — Evidence-backed capability of one configuration within a class: `Unknown | Evaluating | Qualified | Restricted | Suspended`.
- **`RoutingPolicyRevision`** — The immutable routing rules a routing pin refers to, with one lifecycle (draft, qualified, canary, promoted, rolled back, retired) printed at its gate; M0 has one fixed promoted revision.
- **Canary / kill switch / rollback** — A second pin for admissions inside a scope; human or triggered suspension; return of future admissions to the pinned rollback target. In-flight attempts keep their pin.
- **`ExplorationPolicy`** — Escrowed expected-loss budget and exposure limits; suspended on unknown cost or support failure, resumed only with a new qualified cohort.
- **`EvaluationAssignment` / cohort / `CohortWatermark`** — One immutable record per routing decision with one primary cohort; the set compared together; complete only when every assignment is `RECORDED` or `GAP`, usage settled or censored, outcomes mature, adjudications done.
- **`CostRecord` / cost allocation policy / residual** — The attributed cost of an attempt under a versioned policy; the unreachable remainder recorded on the context; conservation per period.
- **`OutcomeAttribution`** — One disjoint primary cause from a versioned precedence table plus contributors.
- **Milestone denominator** — Milestones with a settled acceptance decision at one plan revision; `superseded_by` retains a pre-split identity.
- **Defect-maturity window** — The interval after verified integration before an outcome is `MATURE`; absence of a report is not proof of quality.

### TypeSafe decision policy — `extensions/typesafe-decision-policy.md`

- **TypeSafe (as used by AutoBot)** — The bounded semantic judge behind typed questions; selects only from the eligible set; grants nothing.
- **Question-input trust labels** — The core trust labels and `CANONICAL_AUTOBOT_FACT`, `AUTHENTICATED_PROVIDER_OBSERVATION`, `UNTRUSTED_CI_OUTPUT`, `DERIVED_STATISTIC`, `DERIVED_FALLBACK_DECISION`; each question class declares which it accepts.
- **`DecisionPolicy` / fallback kind** — The pinned revision of question classes, trust classes, eligibility rules and per-class outage fallback: `ABSTAIN` (default), `DETERMINISTIC_RULE`, `STRONGER_REASONER` (escrowed).
- **`DriftAssessment`** — The micro-manager's recorded judgment: `ON_TRACK | DRIFT_RISK | DRIFTED | BLOCKED | COMPROMISED | INSUFFICIENT_EVIDENCE`.
- **Calibration** — Per-class comparison of judge confidence with AutoBot outcomes; fallback decisions excluded.

### Forge mirroring — `extensions/forge-mirroring.md`

- **`MirrorBinding`** — Canonical UID to remote issue, PR, comment or label with the last projected digest; a remote object is a view.
- **`ExternalObservation`** — An authenticated, deduplicated, persisted fact from a forge, CI, user or infrastructure; state only after a controller commit.
- **`ChangeSet`** — A candidate change with source, base and head lineage; the unit integrated through a basis.
- **`ProjectSetup`** — The bounded plan for repository and forge prerequisites, verified through external operations.
- **Capability matrix / qualification** — Per-provider, per-operation status starting `UNQUALIFIED`; qualified only by recorded conformance.

### Observability — `extensions/observability.md`

- **Telemetry** — OpenTelemetry traces, metrics and logs; explains, never authorizes, never substitutes for a canonical record.
- **Redaction (fail-closed)** — In the SDK before any queue or WAL; `RedactionFailed` rejects or quarantines; gateway redaction is defense in depth.
- **Telemetry identity** — `(installation_id, boot_epoch, source_sequence)`; stale epochs are not current evidence.
- **Telemetry degradation states** — `TelemetryHealthy | TelemetryDegraded | CollectorUnavailable | ExportBacklogged | ClickHouseUnavailable | TelemetryPolicyBlocked | TelemetryRecovered`.
- **`ObservabilityPolicy` / `TelemetryProfile` / `SLO` / `AlertRule` / `TelemetryIncident`** — Policy and objective kinds for signals, redaction, sampling, retention, objectives and deterministic alerts.

### CLI — `extensions/cli.md`

- **`autobot` CLI** — A frontend over the Kubernetes API using kubeconfig and RBAC; every command creates or reads resources; a disconnected watch has no lifecycle effect; it mints nothing; it carries the human accepts of proposals.
- **Installation receipt** — A small resumable local record supporting install recovery; authorizes nothing.

### Onboarding MCP server — `extensions/onboarding-mcp.md`

- **`autobot-mcp`** — The local MCP server a coding agent runs to fill the intake-client role: reads the working directory and never writes it, reaches the Kubernetes API only as the intake-submitter identity, and decides nothing.
- **Onboarding tools** — `inspect_directory` (read-only) and the submit tools for the intake kinds, whose input schemas are generated from the kinds' spec types; none accepts, rejects, activates, grants or deletes.
- **`onboard` prompt** — The interview as an MCP prompt: settle every required attribute from the directory or by asking the person, one question at a time, then submit and hand the person the CLI accept commands.

## EXTERNAL

- **Kubernetes API server / CRDs / `resourceVersion`** — Single-resource atomicity and optimistic concurrency, nothing more; trusted together with admission and RBAC. *(TRUST)*
- **Admission webhook / RBAC** — Validate protected status, the field partition and references; never perform side effects. *(TRUST)*
- **Native Kubernetes resources** — Pods, Jobs, Secrets, Leases, PVCs; used as-is; Leases carry liveness, never authority. *(TRUST)*
- **Sandbox runtime** — The isolation runtime behind the execution profile (candidate: Kubernetes SIG Apps Agent Sandbox with a qualified Kata runtime); verified, never assumed. *(M0 §5 DEFERRED, G-FENCE-CUSTODY)*
- **Forge** — GitHub, Gitea, Bitbucket and the like: repositories, issues, PRs, labels, merges, branch protection; every action `UNQUALIFIED` until conformance. *(`extensions/forge-mirroring.md`)*
- **CI provider** — External executor of tests and checks; results are observations bound to head, workflow and environment. *(`extensions/forge-mirroring.md`)*
- **Model provider / agent harness** — Sessions that do the work through runtime adapters; transport failure, rate limits, exhaustion and refusal are separate from model quality. *(ROLES §4)*
- **Model Context Protocol / coding agent** — The open protocol of tools, resources and prompts between an agent and a server, and the agent a person runs that speaks it; the agent is untrusted and fills no role but through `autobot-mcp`. *(`extensions/onboarding-mcp.md`)*
- **TypeSafe service** — The external typed-question API (shared weights; AutoBot owns its learned tables). *(`extensions/typesafe-decision-policy.md`)*
- **Artifact store** — Independent, versioned, encrypted object storage outside the worker-node loss domain. *(TRUST; M0 §2)*
- **Authority witness service** — The small independently operated control plane (its own replicated API, or an equivalent strongly consistent lease service) filling the CORE witness role. *(TRUST)*
- **OpenTelemetry Collector / ClickHouse** — Telemetry pipeline and analytics backend; rebuildable projections. *(`extensions/observability.md`)*
- **Formal toolchain** — Quint, Apalache, TLC/TLA+, PlusCal, optional Dafny or Lean. *(FORMAL §8)*
- **Design review** — A review of this design set; its findings use the review classification. *(FORMAL §9)*

## Retired terms

Names this set does not use and must not acquire, each with what replaces it. Do not reintroduce them.

- **`ManagerCommitReceipt`** — subsumed by the `active_manager_transaction` reservation and the target's `CommandReceipt`.
- **`routing_authority` register, `promoted_routing_revision`, `canary_routing_revision`** — routing is a pin on the `TaskRun`, not a register; canary and promotion live in the learning extension.
- **`AdmissionController`** — there is no separate admission kind or controller; admission is the `WorkContext` registers applied by the Context controller.
- **Integrator as effect sender** — the Integrator requests; only the broker sends.
- **`DISPATCHING` without `send_attempt`; `REMOTE_SENT` and other permit-level remote states** — an operation enters `DISPATCHING` only with `send_attempt`; remote outcome states live only on the operation.
- **`REVISION_PENDING` phase; `DRAINING` hold state** — a condition and a per-plan Manager phase respectively.
- **`EvaluationAssignment` and cohorts as core accounting** — the core obligation is stated on the `TaskRun`'s expected records; cohorts are an extension.
- **Health enumeration and operating modes (`Healthy … Fenced`, `Normal … EmergencyStop`) as states** — condition vocabularies, deferred to G-OPS; degraded modes are named by the unavailable dependency.
- **`SideIssue`** — a `Finding`.
- **`ANALYZING` and `NEEDS_INPUT` `Intake` states; the Manager's intake analysis** — the intake client understands and interviews before it submits; the Intake controller only validates and verifies.
- **Observer, Garbage Collector as roles** — reconciler duties of the Context and Custody controllers.
- **`intervention_refs` control field** — dropped; an `Intervention` is a kind applied by a command to the owning controller of what it changes.
- **`ModelScore`** — `WorkerCapability`.
- **The event taxonomy (hundreds of event names)** — event types are named by the kernel where a rule needs one; the list is not a document.
- **Temporal as a core, PostgreSQL as task-state authority** — optional subordinate services only, never authority.
