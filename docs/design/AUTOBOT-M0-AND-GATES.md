# AutoBot — M0 and gates

Authority for: the first qualification slice (kinds, owners, fixtures, proposed limits); the sole milestone and gate table; the DEFERRED list.
Depends on: `AUTOBOT-THESIS.md`, `AUTOBOT-KERNEL.md`, `AUTOBOT-ROLES-AND-RUNTIME.md`, `AUTOBOT-ONBOARDING-AND-CONVERGENCE.md`, `AUTOBOT-FORMAL-SURFACE.md`.

M0 means **M0-Q**, a non-production qualification slice. It proves one complete path on an existing, non-Rust repository — onboarding through the intake client → human acceptance through the CLI → accepted plan → activated graph → task → attempt → workspace → verification → evidence → canonical records — through Kubernetes resources, surviving operator restart, inspectable with both the CLI and `kubectl`. It enables no live merge, no deployment, no destructive external effect, no adaptive routing and no production custody claim. It uses fixed routing, a fake forge and CI, a deterministic fake semantic judge with the same typed-question and abstention contract, and fault-injected fake provider adapters. No real provider capability is asserted by M0.

## 1. Kinds and owners

Every kind below is a namespaced custom resource with a structural `spec`, a status subresource owned by exactly one controller, `observedGeneration`, conditions, `state_revision`, `commit_sequence` and `last_receipt_ref`; aggregates with a control lane also carry `control_revision`, the control-receipt ring and the control fields of KERNEL §1. References carry namespace, name and UID. Every status `state` enum is exactly its KERNEL §10 machine; a difference is a schema defect. **This table is the closure: a kind without a row is not in M0.** Each controller also owns receipt and audit repair for commands targeting its kinds; an audit reconciler detects gaps and never writes receipts.

| Owner | Kinds |
|---|---|
| **Context** | `WorkContext` (registers, ledger, reservation slot), `ManagerLease`, `AdmissionStamp` |
| **Intake** | `WorkBrief`, `Intake`, `Project`, `Repository`, `PlanProposal`, `Charter`, `ProjectCharter` |
| **Plan** | `Plan`, `PlanSnapshot`, `GraphActivationReceipt` |
| **Task** | `Milestone`, `Task` |
| **TaskRun** | `TaskRun`, `ScopeCapsule` |
| **AgentRun** | `AgentRun`, `AgentCheckpoint` |
| **Broker** | `ExecutionIdentity`, `CredentialGrant`, `FenceSession`, `EffectIntent`, `ExternalOperation`, `ToolInvocation`, `EffectReceipt` |
| **Custody** | `Workspace`, `CustodyCheckpoint`, `ArtifactCommit`, `Artifact`, `WorkspaceConflict`, `RestoreRequest`, `CustodyPolicy` |
| **Verification** | `VerificationRun`, `EvidenceBundle` |
| **Integration** | `IntegrationBasis` |
| **Decision** | `Decision`, `Finding` |
| **Budget** | `Budget`, `BudgetReservation` |
| **Settlement** | `UsageReceipt` |
| **Outcome** | `OutcomeRecord`, `TelemetryGap` |
| **Intervention** | `Intervention` |
| target kind's controller | `AutoBotCommand` (status points only at its receipt), `CommandReceipt`, `AutoBotEvent` |
| external witness | `RestoreWitnessReceipt` (signed; recorded by Custody, never issued by AutoBot) |

Besides `kubectl`, two AutoBot clients submit commands, and neither owns a kind: the CLI and the intake client (ONBOARD §1). The operator configuration deployed with the installation names its intake namespace and its bootstrap administrators; each namespace has its own intake-submitter identity.

Spec contracts M0 fixes: every intake-kind spec the intake client writes carries `client` (the name and version the producing client claims) and states each attribute as a sourced value — the value and a non-empty provenance list, each entry a source (a path at a commit, a URL, or an interview question), a content digest and a trust label in `UNTRUSTED_REPOSITORY_CONTENT`, `UNTRUSTED_ISSUE_OR_PR_TEXT`, `INTERVIEW_ANSWER` (ONBOARD §1) — and its status records the authenticated principal of every revision; `Intake.spec` (for a later brief, the project it continues; for a new context, the proposed `WorkContext.spec` configuration with its target namespace and human roles; objective, non-goals, constraints, acceptance signals, risks, dependencies); `WorkBrief.spec` (its `Intake`, source kind, author, the brief revision it replaces, the person's confirmation as relayed by the client, and either the artifact ref the Intake controller writes for submitted content or a path at a pushed commit of a proposed `Repository`, with the revision digest); `Project.spec` (its `Intake`, or the existing project attached; name, purpose); `PlanProposal.spec` (its `Intake`; for a later brief, the plan and the revision it revises; candidate outcomes, milestones, tasks with obligation, outcome advanced, dependencies, non-goals, acceptance evidence, repository scope and proposed consequence class; estimated cost); `Repository.spec` (its `Intake`, forge ref, external id, clone URL, default branch, credential-grant policy, one or more named execution profiles with image, checkout, setup, build and verify commands); `Charter.spec` and `ProjectCharter.spec` (the owning `WorkContext` or `Project`, for a `ProjectCharter` the inherited `Charter`, and revisions, each with its entries under the six sections of ONBOARD §7 — id, section, mode, statement, the threshold of a `judged` entry, the provenance of a proposed entry — its digest and its accepting human principal); `Plan.spec` (source proposal, accepted revision, contract snapshot with brief, policy, charter and graph digests and member UIDs with revisions); `Task.spec` (one obligation, milestone, repositories, acceptance evidence, non-goals, consequence class, dependencies); `TaskRun.spec` (task UID and revision, or for a planning TaskRun the `Plan` or `Intake` it serves in their place (KERNEL §6), source basis, execution profile, routing pin, consequence class and floor, the digest of its current capsule (replaced only by a fresh capsule, KERNEL §5), budget reservation); `ScopeCapsule` as ROLES §2; `Budget.spec` (ceiling, unit policy, child allocation, unknown-cost policy); `BudgetReservation.spec.purpose ∈ {ATTEMPT, EFFECT}` (`ATTEMPT` for the model spend of every session of its TaskRun, a planning TaskRun's included; `EFFECT` for one operation); `Intervention.spec` as ROLES §5; `WorkContext.spec` names the tier order and the tier of each worker and reviewer configuration the routing pin may name, and, when created from an `Intake`, that `Intake` and the digest of the accepted configuration. Admission webhooks enforce structural references, context binding, immutable snapshot fields, the field partition, allowed transitions and the intake rules of ONBOARD §1 (proposal state, provenance, the M0-Q `ProjectCharter` rule below, an `Intake` in the intake namespace holding only its configuration, and the principal step 6 names for each accept and each `intake reject`); controllers enforce everything semantic.

The charter in M0 is its minimum: the `Charter` and `ProjectCharter` kinds, revision pinning and digests, the charter in the capsule, `mechanical` term and path laws, and the charter precondition of plan acceptance. At M0-Q admission refuses a `ProjectCharter` entry from the intake client without an `INTERVIEW_ANSWER` provenance entry, as it refuses a `Charter` entry with any other; candidate extraction from repository content, the repository projection and waivers, each an `Intervention` answered by a named human, are DEFERRED (§5).

## 2. M0 profile — proposed limits, not design facts

These values are stated once, here. Any other document that needs one refers to "the profile". Each is a limit to test, not a measured capacity.

| Area | Proposed value |
|---|---|
| Replay window | 30 days, plus 7 days clock and transport margin |
| Control-receipt ring | 8 unpublished entries, ≤ 4 KiB each |
| Dispatch ledger | 64 entries |
| Registers | 8 plans, 8 integration bases per context; blocked-target pairs share the dispatch ledger's capacity |
| Scale | 1 context, 4 repositories, 100 tasks, 1 active TaskRun; two simulated Managers and two simulated installation identities to exercise races |
| Objects | status ≤ 256 KiB; pending slot ≤ 32 KiB; ≤ 8 effect intents per command; late-event buffer 64 per projected aggregate; oversize input rejected or referenced by a verified artifact |
| API budget | 10 requests/s, burst 20, per operator process; queue of 500 keys, FIFO within priority, reserved control capacity for hold, fence and receipt repair; a full queue stops admission and drops nothing accepted |
| Checkpoint cadence | after each completed tool boundary and at most 60 s of active work; an indivisible write capped at 60 s; new writes stop when checkpoint age exceeds the bound |
| RPO / RTO | at most the uncheckpointed 60 s plus one bounded in-flight operation, with `last_verified_checkpoint_age` and `at_risk_interval` exposed; restore of a 100 MiB fixture within 10 minutes with healthy dependencies — measured before claimed |
| Artifact fixture | independent S3-compatible store outside the worker-node loss domain, versioned, encrypted, separate restore process; ≤ 1 GiB per workspace; no automatic deletion of an original during M0 |
| Evidence | TTL 24 h, shortened by repository policy; no-test exception expiry 7 days; defect maturity 14 days (30 for `SECURITY_OR_DATA_INTEGRITY` and `COMPATIBILITY_RISK`) |
| Liveness bounds | the Manager lease duration, `provider_reconcile_bound`, `continuation_deadline`, `record_deadline`, `usage_settlement_deadline`, `human_decision_deadline` and the rest of FORMAL §6 are fixture constants chosen per fixture |
| Sandbox | Linux, pinned OCI image, default-deny network, broker-only egress, one writable mount, no host mount, privileged container or device; optional live model API only through a metered proxy that cannot reach production endpoints |

Loss of both workspace and store, zone-wide loss and hostile cluster administrators are outside the M0 custody profile (TRUST).

## 3. Fixture groups

Passing a group is necessary for M0-Q and is never evidence that a production gate passed. Each group names the formal invariants it exercises.

- **G-COMMIT** (F-1 … F-6): C1 crash then C2 repair; duplicate, different-payload and expired commands; create with lost acknowledgement; deletion or recreation before a terminal create receipt; hold on the control lane beside a pending domain commit with digest-verified repair; ring full refuses the ninth transition; audit delivered out of order and with a conflicting digest.
- **G-MANAGER** (F-11, F-12): reservation versus draining; target conflict and cancellation; uncertain target write holds takeover; stale controller after epoch advance; a permit between `AdvanceManagerEpoch` and `ResumeManager`; release only with a recorded receipt.
- **G-DISPATCH** (F-7 … F-10, F-13, F-14): `AcceptDispatch` interleaved at every CAS position with a hold cut, a plan activation, a quiesce-then-resume of the same revision with the permit's invalidation lost, a basis invalidation and a dispatch-authority advance; ledger full refuses; register acknowledgement lag admits nothing.
- **G-EFFECT** (F-18 … F-22): crash before materialization; missed watch and relist; duplicate child; `EffectIntentCollision`; broker crash before and after the send-attempt marker; `DispatchLedgerConflict`; a provider without lookup or idempotency reaching `UNRESOLVED` and blocking dependents until adjudication; proven non-application re-requesting with `attempt_index + 1`; tool stream disconnect with no blind retry; unqualified capability blocked before send.
- **G-SCOPE** (F-23 … F-26, F-38 … F-40): symlink, hardlink, rename, ignored-path and subprocess writes outside the capsule denied before effect or detected at the next checkpoint with quarantine and no accepted evidence; a finding linked to an active task leaves its capsule digest unchanged; fence confirmation denies a broker call and a workspace write and refuses stale evidence; a continuation keeps identity, grant lineage, open invocations and counters; a plan revision without an accepted charter revision is not accepted, and a capsule carries the charter digest of the charter in force at its issue and its relevant entries; a project entry relaxing an inherited law is refused, and a conflict resolves toward the stricter entry; a `mechanical` term or path law violation is refused by the broker before effect or blocks the candidate at the checkpoint; a relaxed law does not affect a plan revision pinned before it; a tightened law binds the next continuation through a fresh capsule; a law tightened after a plan revision was pinned is refused by the broker at once and invalidates evidence bound to the earlier charter digest.
- **G-CUSTODY-M0** (F-16, F-17): untracked, ignored and unpushed inventory including the record outbox; interrupted upload; independent restore; storage outage; age-bound pause; no deletion under uncertainty; mixed-ownership conflict.
- **G-RESTORE** (F-15): isolated restore with witness present, absent and late; old-grant expiry versus active revocation; ambiguous-set reconciliation before dispatch.
- **G-EVIDENCE** (F-27 … F-29, F-37): a result for another candidate, base, environment, policy or remote generation satisfies nothing; reviewer equal to worker session is refused; a reviewer below the fixed review tier is refused; a routing pin below the floor is not admitted; integration basis invalidation invalidates a merge permit.
- **G-RECORD** (F-30, F-31, F-32, F-33, F-34, F-35, F-36, F-41, F-42, F-43, F-44): a fake-forge observation with no controller commit changes no aggregate, and forge text under an unauthorized actor becomes no command; a grant or reference to another context is refused; duplicated, delayed and reordered deliveries violate no invariant above; every recorded field is in its §10 domain; worker crash after a local outcome and before the API write ends `RECORDED` from the restored outbox or `GAP`; a censored receipt counts at `min(reservation, rate-card bound)`; the fake judge, unavailable, takes the conservative branch and every recorded selection is in its eligible set; an agent principal cannot accept a charter revision or entry; a `judged` "complies" from the fake judge grants nothing, and an unavailable or abstaining fake judge leaves the `review` backstop as the only check; an accept of a context configuration, a charter revision or a plan proposal from the intake-submitter identity is refused, as is its write to any kind but an intake kind or to an intake kind outside proposal state, and so is any accept from a principal other than the one ONBOARD §1 step 6 names for it; a plan accept pinned to an `Intake` revision that a later submission or revision has moved fails; an intake-kind write with an attribute lacking provenance is refused, and so are a `Charter` entry with any provenance entry not `INTERVIEW_ANSWER` and, at M0-Q, a `ProjectCharter` entry without one; a plan accept is refused while the context has no accepted `Charter` revision; a revision of a bound successor `Intake` by the intake namespace's identity is refused; a proposal other than the configuration that references an `Intake` in the intake namespace is refused; an `intake reject` from anyone but the principal who may accept that `Intake` (ONBOARD §1 step 6) is refused; a proposed repository the fake forge does not verify never becomes `ForgeVerified`, and its `Intake` never reaches `PROPOSED`.

M0-Q completion evidence: the non-Rust (Python) fixture repository onboarded from its working directory through the intake client against the fake forge — interview, proposals with provenance, forge verification — and accepted by a human principal through the CLI, its plan graph activated atomically, executed with a fixed worker and an independent reviewer, verified, custody-preserved and inspected through both the CLI and `kubectl`, with operator restart, receipt recovery, stale-result rejection, provider failure, effect-intent recreation, fence confirmation, independent restore and fake forge freshness exercised; cost, first-pass outcome and eventual outcome in canonical receipts; unknown cost censored, not dropped.

## 4. Milestones and gates — the only table

Each gate is `NOT_RUN | RUNNING | PASSED | FAILED | INVALIDATED` with immutable evidence bound to installation, software, policy and profile digests; a change to any of those invalidates it. Deployment capability is the intersection of gate evidence, action policy, current health and holds; a feature flag cannot bypass a gate. All gates are `NOT_RUN`.

| Gate | Depends on | Evidence | Enables |
|---|---|---|---|
| **M0-Q / G-QUAL** | frozen design | §3 groups; FORMAL §7 model written, checked and traced | non-production qualification only |
| **M1 / G-INTAKE** | M0-Q | installation and recovery; intake and adoption on real forge metadata; decision policy with per-class fallback and the trust labels beyond those of ONBOARD §1; narrow scope and finding routing; charter extraction, repository projection and waivers | broader non-production orchestration, fixed routing |
| **M2 / G-FENCE-CUSTODY** | M0-Q | real broker; qualified isolated runtime; process, storage and network fencing measured; replicated artifacts; RPO/RTO exercise; witness and split-brain exercise | qualified runtime and storage combinations |
| **M3 / G-FORGE** | M1, M2 | per-provider, per-operation conformance in disposable repositories; lost acknowledgement; stale CI; force-push; overlapping candidates; hold/dispatch race; mirror projection | only tested adapters and actions |
| **M4 / G-OBS** | M0-Q | telemetry pipeline coverage; fail-closed redaction; boot-epoch identity; backfill; retention; tenant access; independent alerts | qualified observability |
| **M5 / G-EVALUATION** | M1, M4 | canonical ledger at scale; cohorts with one primary id; support bounds; grouped splits; settled or censored cost; mature outcomes; disjoint attribution; calibration | offline experiments; no promotion |
| **M6 / G-ADAPT** | M3, M5 | qualified routing revisions; exploration escrow and expected-loss limit; canary, kill switch, rollback; in-flight pinning; bounded repair | adaptive routing (production use also needs G-PROD) |
| **M7 / G-PORTABILITY** | M3 | mixed-forge context and a second runtime adapter pass the same suite | those combinations only |
| **M8 / G-OPS** | M2, M3, M4 | measured scale limits; backup, upgrade, load, backpressure; recovery drills; runbooks; health and mode vocabulary | operational qualification |
| **G-FORMAL** | alongside M0 | non-vacuous checks with negative variants; counterexamples published; Rust refinement and fault injection; digests retained | required before G-PROD |
| **G-PROD** | G-QUAL, G-INTAKE, G-FENCE-CUSTODY, G-FORGE, G-OBS, G-OPS, G-FORMAL | signed, operator-acknowledged evidence | autonomous production effects within qualified scope; fixed routing needs no G-ADAPT |

Until their gates pass, these stay disabled: live forge writes, automated merge, deployments, destructive external effects, unqualified provider actions, adaptive routing, exploration, routing promotion, production custody guarantees. Economic superiority over frontier-only execution is a separately measured product hypothesis (G-EVALUATION, G-ADAPT), never implied by G-PROD.

## 5. DEFERRED

One line per idea not in the core, with the gate that first needs it. A term on this list may appear in a core document only as a name; its contract is written when its gate is reached.

- `MirrorBinding`, `ExternalObservation`, `ChangeSet`, `ProjectSetup`, remote-issue and PR projection, webhook and poll dedup — G-FORGE
- Provider capability matrix per real forge and CI; merge preconditions with provider-enforced protection — G-FORGE
- Forge repository creation by AutoBot, a live forge write; until then the person creates the repository outside AutoBot and onboarding adopts it — G-FORGE
- `RepositoryObservation`, `ReviewRequirement`, `Approval` as kinds — G-INTAKE
- `DecisionPolicy`, fallback kinds (`ABSTAIN`, `DETERMINISTIC_RULE`, `STRONGER_REASONER`), the trust labels beyond those of ONBOARD §1, `DriftAssessment`, judge calibration — G-INTAKE
- Charter import and candidate extraction from a work-in-progress repository (`ProjectCharter` entries with a repository-content provenance, submitted by the intake client), the first charter imported being AutoBot's own repository charter in the six-section format with laws worded generically; the read-only repository projection of the charter; waivers, each an `Intervention` answered by a named human approver, as for a no-test exception (ROLES §5), scoped to one plan revision or one task, recording a reason and an expiry, and visible in the evidence bundle — G-INTAKE
- The `judged` enforcement mode on the real judge, arriving with the decision policy; M0-Q uses the deterministic fake judge — G-INTAKE
- `CostRecord`, cost allocation policy and residual, `OutcomeAttribution`, `EvaluationAssignment`, `CohortWatermark`, `EvaluationRun`, `censored_fraction` promotion rule, milestone `superseded_by` — G-EVALUATION
- `RoutingPolicyRevision` lifecycle, `ExplorationPolicy`, canary scope, kill switch, rollback target, `ProblemTaxonomy`, `ProblemClass`, `WorkerCapability`, strategy segments — G-ADAPT
- `TelemetryGap` `BACKFILLING` state and backfill semantics, redaction, boot-epoch telemetry identity, `ObservabilityPolicy`, `TelemetryProfile`, `SLO`, `AlertRule`, `TelemetryIncident`, tail sampling — G-OBS
- `CleanupRequest`, `Backup`, real witness control plane (`DispatchAuthority`), Agent Sandbox and Kata qualification — G-FENCE-CUSTODY
- `ExecutionProfile` attestation, `ScaleProfile`, health and operating-mode vocabularies — G-OPS
- Second forge and second runtime adapter — G-PORTABILITY
- Runtime consumption of gate evidence (deployment capability as the intersection of gate evidence, action policy, current health and holds); gate records are signed manifests under `docs/gates/**`, and there is no Gate CRD in M0 — G-QUAL
- Micro-manager quality metrics (false alarms, missed drift, unnecessary takeovers) — G-EVALUATION
- CLI beyond the M0 commands (context accept, charter propose and accept, plan accept, intake reject, task start, watch, explain) — G-INTAKE
- Temporal, PostgreSQL as optional subordinate services — none; adopted only on a measured need and never as authority
- Bend — optional research; never on the gate path
