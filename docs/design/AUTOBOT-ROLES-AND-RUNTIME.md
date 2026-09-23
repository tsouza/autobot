# AutoBot — roles and runtime

Authority for: the swarm roles and what each may request; the scope capsule and its canonicalization rules; reviewer independence; the agent-runtime contract; human intervention.
Depends on: `AUTOBOT-THESIS.md` (I-5, I-6, I-8, I-10), `AUTOBOT-TRUST-MODEL.md`, `AUTOBOT-KERNEL.md` (§3, §5, §6, §9, §10).

## 1. Roles

A role is a field of an `AgentRun`, not a kind of its own. Every role is an untrusted session (TRUST); what distinguishes roles is what each may *request*, and every request is a command or a claim that deterministic code decides on.

| Role | Purpose | May request | May never |
|---|---|---|---|
| **Manager** | Semantic authority for one accepted plan: assign, prioritize ready work, resolve ambiguity, decompose further and propose replanning through revisions. Its authority is the register entry `manager_authority[plan]` (KERNEL §4), not the process. One logical Manager per plan; no immortal process, no unbounded context. | Plan revisions, task admission, assignments, decisions — each a command under a current reservation with a fixed target revision. | Merge, waive evidence, widen a capsule, spend outside budget, write status. |
| **Worker** | Implement one bounded assignment in one workspace. | Brokered tool calls inside its capsule; checkpoints; a candidate change; findings; "blocked". | Fix, refactor or clean up anything outside the capsule; retry an effect blind; approve anything. |
| **Reviewer** | Assess the pinned candidate independently and report evidence-backed findings against the acceptance contract. | A review attestation on the exact candidate digest; findings. | Modify the candidate; audit the repository beyond the review policy's scope; be the worker (§3). |
| **Tester** | Design verification and arrange its execution locally or in CI. | `VerificationRun`s against the exact candidate; findings for failures outside the target. | Rewrite acceptance criteria; delete required checks. |
| **Integrator** | Reserve and verify the `IntegrationBasis`; request the external operations that integrate accepted work; verify their effects independently. | A basis reservation or invalidation, submitted by the Integration controller (KERNEL §3.1); external operations, which only the broker sends. | Send an effect itself; merge without a current basis generation and provider-enforced protection. |
| **MicroManager** (optional, task-local) | Guardian for one `TaskRun`: classify the run's state from a bounded evidence bundle as `ON_TRACK`, `DRIFT_RISK`, `DRIFTED`, `BLOCKED`, `COMPROMISED` or `INSUFFICIENT_EVIDENCE` and act within policy. | Nudge (a return-to-task prompt plus a checkpoint request); pause; checkpoint; preserve; fence; a recovery request. | Redefine acceptance, merge, discard uncertain work, promote side work. Its absence changes no permission: the worker contract and deterministic admission continue without it. |

No role accepts a charter revision or entry, a proposed context configuration or a plan proposal, promotes a candidate entry or grants a waiver; those are human acts (ONBOARD §1, §7).

Onboarding is no role: the intake client that understands the brief, interviews the person and proposes the first plan runs outside AutoBot (ONBOARD §1, TRUST). The Manager's authority starts at the accepted plan (KERNEL §4).

The operator's observer and garbage-collection duties are reconcilers of the Custody and Context controllers, not agent roles. Every role outcome becomes a resource or a resource-linked artifact; routine timers, retries and receipts never need a model call.

Escalation on a drift assessment is graduated and never silently changes the contract: `DRIFT_RISK` nudges; `DRIFTED` pauses new actions and preserves the workspace; `BLOCKED` records a `Decision` or `Finding` for the Manager; `COMPROMISED` fences, preserves and revokes; `INSUFFICIENT_EVIDENCE` gathers a bounded observation or escalates — it never guesses. Recovery separates original-objective changes from the rest into a recovery attempt and linked findings; when the two cannot be separated with confidence, the whole workspace is quarantined for adjudication rather than trimmed.

## 2. The scope capsule — I-5

A `ScopeCapsule` is the immutable assignment contract of one `TaskRun`, issued by the TaskRun controller and enforced by the broker. It contains: the single objective and expected outcome; allowed repositories (by UID), canonical path globs, branches, tools and effect kinds; explicit non-goals; the acceptance evidence required; budget, deadline, attempt and repair limits; the plan, milestone and task revision that authorized the work; and the charter digest, with the entries of the charter in force relevant to the task (KERNEL §5, ONBOARD §7). A fresh capsule replaces it only as KERNEL §5 states. The broker refuses a call that violates a `mechanical` entry of the charter in force before effect, and a checkpoint that finds a violation makes the candidate ineligible for evidence. The prompt repeats it for context; the prompt is not the boundary.

**Canonicalization.** "Inside the capsule" is decided on canonical identity on the workspace's single writable mount, never on the string a tool was given:

| Concern | Rule |
|---|---|
| Path | Resolve with `realpath` against the mount at check time; compare the result to the globs. A path that escapes the mount after resolution is denied whatever the glob says. |
| Symlinks | Link path and resolved target must both be in scope; creating a link whose target is out of scope is denied. |
| Hardlinks and inodes | Inventory records `(canonical_path, inode, digest)`. A write through a hardlink is attributed to every path sharing the inode; if any is out of scope the mutation is. Creating a link across the boundary is denied. |
| Renames and moves | A delete at the source and a create at the target; both must be in scope. |
| Mounts | The execution profile forbids additional mounts, bind mounts and devices; a mount change is a profile violation and fences the run. |
| Ignored paths | Inventoried like tracked content; `.gitignore` excludes nothing from a scope check. |
| Subprocesses | Inherit the sandbox — default-deny network, broker-only egress, the same single mount — so their filesystem effects are bounded to the workspace and caught by the checkpoint diff, and their external effects can only be brokered calls. |
| Tool-side effects | Forge, CI, deployment and credential effects exist only as brokered `ToolInvocation`s; `effect_kinds` and the repository set are checked at acceptance (KERNEL §3.2). |

**Enforcement is layered, and the timing is the contract.** *Preventive, before effect*: every brokered call is canonicalized and denied before it executes, recording requested path, canonical path, inode and capsule digest. *Detective, before evidence*: at every custody checkpoint and at run end, the inventory diff against the run's starting inventory is compared to the capsule under the rules above; any mutation outside it, by any route, makes the candidate ineligible for evidence, quarantines the workspace as a `WorkspaceConflict` with a per-file attribution manifest, and fences the run. No `VerificationRun` or `EvidenceBundle` is accepted for a candidate whose checkpoint diff was not scope-clean. *Attribution*: the out-of-scope set, its canonical paths and inodes, the producing tool or process where known, and the timing are recorded on the `Finding` and the conflict; the task is unchanged and new work needs a new capsule.

**Side issues.** A discovery outside the capsule is a `Finding` — observed fact, affected scope, severity, confidence, relation to the objective, reproducibility, suggested next action — linked to the exact task and source revision. Reporting is success; silently expanding the patch is scope failure. Linking a finding to a task is **historical only**: `(task_uid, task_revision_at_link)` in the task's evidence trail. It never changes an active run's capsule, acceptance contract or required evidence; promotion to work is a new task or a plan revision (KERNEL §5). The one exception is containment: a worker may stop immediately when continuing would create a security exposure, data loss, corruption or an invariant violation — it still records the evidence and repairs nothing unrelated.

## 3. Reviewer independence — I-6, I-10

Acceptance evidence is produced independently of the worker that made the candidate. A reviewer is never the worker's session; a reviewer using another session of the *same model* is recorded as correlated evidence, not independent judgment. Reviewers are never routed below the review tier fixed for the consequence class (ONBOARD §5), whatever the worker cost; `SECURITY_OR_DATA_INTEGRITY` work requires a separately configured reviewer and the relevant security checks. A reviewer checks the candidate against every `review` and `judged` entry of the charter in force (KERNEL §5); a violation is a blocking finding, and a `judged` answer never replaces this check. A worker cannot delete a required check. Any exception is a recorded decision of the no-test approver named in `WorkContext.spec`, taken within policy, that stays visible in the outcome as an exception, never as a pass.

## 4. The agent-runtime contract

An `AgentRun` is one session of one role through a runtime adapter, inside a `TaskRun`. Every AgentRun holds its own `ExecutionIdentity` and `CredentialGrant` (KERNEL §6); its model spend draws on the `ATTEMPT` reservation of its TaskRun, and each brokered effect on an `EFFECT` reservation of that TaskRun; and each of its sessions (its continuations, KERNEL §9) is a usage producer with its own entry in that TaskRun's expected records (KERNEL §8). Where each role's sessions run, and what their grant allows:

| Role | Runs in | Grant allows |
|---|---|---|
| Manager | a planning TaskRun of its own, of its `Plan` or of the `Intake` it serves | once the `Plan` exists, Manager commands under `manager_authority[plan]` (KERNEL §4); no repository write |
| Integrator | a planning TaskRun of its own, of its `Plan` | basis requests to the Integration controller and the external operations of the merge order, which only the broker sends; no workspace |
| Worker | the TaskRun of its task | writes to that TaskRun's workspace inside the capsule, and brokered tool calls inside it |
| Reviewer, Tester | the TaskRun of the task whose candidate it assesses; for a milestone's integrated candidate, a planning TaskRun of its own, of the `Plan` | reads of the pinned candidate and its evidence, and its role's requests (§1); no write to the workspace |
| MicroManager | the TaskRun it guards | reads of the run's evidence, and its role's requests (§1); no write to the workspace |

A planning TaskRun holds no workspace and no task: its spec names the `Plan` or `Intake` in the task's place, and its capsule allows no path, so its sessions change a repository only through operations the broker sends. Its expected records are those of every TaskRun: one usage entry per session, one for the broker when it sends effects, and an `outcome` entry for the run. A reviewer's session is therefore never the worker's: it is another AgentRun with another identity and grant (§3).

The contract with the runtime, whatever the harness:

**Context in.** The capsule, with the charter entries it carries; the task obligation, acceptance evidence and non-goals; the pinned plan, milestone and task revision; the candidate basis; references to prior checkpoints and findings; the budget and limits. Nothing else is authoritative, and repository or forge text arrives labelled as untrusted content (TRUST).

**Progress and checkpoints out.** The runtime reports progress, artifacts and findings through resources, never through side channels. It creates an `AgentCheckpoint` at every completed tool boundary and at the checkpoint cadence of the profile (M0), and it identifies newly touched files and concerns at each one. A worker that waits to be rescued by a MicroManager is already a degraded outcome; self-monitoring against the capsule is part of the role.

**Tools only through the broker.** Every effectful call is a `ToolInvocation` under KERNEL §3.3; every privileged filesystem, forge, CI, deployment or credential action passes the broker's grant, scope and capability checks first. The sandbox gives the session no reusable credential, no egress except to the broker, no mount but the workspace, and a process boundary the fence can stop. Read-only calls may be retried within policy.

**Continuation.** Provider outage, rate limiting, context or session exhaustion, mid-stream disconnect, malformed output, refusal, a refused durable-outbox write (`OutboxRefused`, KERNEL §8 step 2) and process crash are separate categories from model quality and are handled by KERNEL §9: checkpoint, then continue the same `AgentRun` from a verified checkpoint; never truncate context silently; treat a refusal as a semantic outcome, not a transport retry; after an `OutboxRefused`, continue and write the records again from the checkpoint, under the producer key of the session that made them, never fence and preserve for it, and if the refusal lasts until `record_deadline` the gap rule of KERNEL §8 step 5 applies; after a crash, fence and preserve before any replacement.

## 5. Human intervention — a kernel kind

Humans act through the same command path as everything else, never by editing status. An `Intervention` is an authenticated request with a scope, an expiry and one action, submitted by a human principal or raised by an owning controller under its own principal to summon one — in which case the human's answer is a further `Intervention` that references it:

```text
HOLD | RESUME | PAUSE | REVIEW | QUIESCE | SUPERSEDE | KILL_SWITCH | ADJUDICATE_OPERATION | ADJUDICATE_CONFLICT
```

`HOLD` becomes `RequestHold` on the context registers; `RESUME` is the only way out of a `HOLD` (`ReleaseHold`), a `KILL_SWITCH` hold, or a `PAUSE` (`PAUSED → ACTIVE`), and is the human answer to a controller-raised `PAUSE`; `PAUSE` and `REVIEW` become the plan-level `PAUSED` block (a review pauses new admissions by default and may request a full hold under policy); `QUIESCE` and `SUPERSEDE` drive KERNEL §5 and require the `revisionAuthority` role; `KILL_SWITCH` is a context hold that no extension may lift or narrow; `ADJUDICATE_OPERATION` is the only exit from `UNRESOLVED` (KERNEL §3.3); `ADJUDICATE_CONFLICT` is the only exit from a `WorkspaceConflict` (KERNEL §7). Human roles — reviser, adjudicator, approver of a no-test exception, kill-switch operator — are named in `WorkContext.spec`; a bootstrap administrator, named by the installation when AutoBot is installed, holds the acts ONBOARD §1 step 6 gives it; an agent identity and the intake-submitter identity can hold none of them. An intervention that is never answered stays visible and escalates under its deadline; it never proceeds silently.
