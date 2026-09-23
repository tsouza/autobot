# AutoBot — trust model

Authority for: who and what AutoBot trusts, what it does not, what a compromised agent can and cannot reach, and what is outside the threat model.
Depends on: `AUTOBOT-THESIS.md` (I-2, I-5, I-6, I-7, I-8). `AUTOBOT-KERNEL.md` and `AUTOBOT-ROLES-AND-RUNTIME.md` implement the boundaries named here.

## Trusted

Trust is granted to exactly these, and each is trusted for exactly the stated thing.

| Trusted party | Trusted for | Not trusted for |
|---|---|---|
| **Kubernetes API server** | Single-resource atomic writes with optimistic concurrency (`resourceVersion`); the durability of committed status. | Cross-resource transactions (there are none); watch delivery (a watch is a trigger, relist is the truth). |
| **Admission and RBAC** | Rejecting direct writes to protected status; rejecting a CAS that touches both commit lanes (KERNEL §1); rejecting cross-context references; enforcing the intake rules of ONBOARD §1: the intake-submitter identity writes intake kinds in proposal state only, and each accept and each `intake reject` is admitted only from the principal ONBOARD §1 step 6 names for it. | Side effects of any kind. Admission validates, it never acts. |
| **Owning controllers** | Committing the protected status of exactly the kinds they own, and repairing receipts and audit events for commands that target those kinds (KERNEL §1). | Writing any other kind's status. The Context controller is the sole writer of `WorkContext` status; every other controller's register change is a command it applies. |
| **EffectBroker** | Being the only path for privileged filesystem, forge, CI, deployment and credential effects; validating grant, scope and capability before every such call. | Deciding what is accepted — acceptance is the Context controller's CAS (KERNEL §3). |
| **Authority witness** | Holding dispatch authority in a separate failure domain and signing restore receipts. AutoBot cannot self-assert that an old installation is fenced. | Anything else; it is an external control plane (KERNEL §7). |
| **Artifact store** | Independent, versioned, restorable custody outside the worker-node loss domain. | Being the source of truth for anything but the bytes it stores. |
| **Authenticated human principals** | Requesting holds, pauses, plan revisions, plan failure and cancellation, adjudications and kill switches through commands and Interventions, under the roles `WorkContext.spec` names (ROLES §5); accepting a plan or a plan revision, only as the reviser `WorkContext.spec.revisionAuthority`; accepting and rejecting proposals, each by the principal ONBOARD §1 step 6 names for it, through the CLI. | Editing protected status. A human acts through the same command path as everything else. |

## Not trusted

Every input below is data. It can inform a decision; it can never carry authority.

- **Agents, in every role.** Worker, reviewer, tester, integrator, micro-manager and Manager sessions are untrusted processes. The Manager's *authority* is a bounded register entry on the WorkContext (KERNEL §4), not the process that holds it. An agent-supplied actor string is never the principal; the principal is the authenticated writer.
- **The intake client.** The process a person runs to onboard a project (ONBOARD §1) is an untrusted proposer, whatever agent drives it. It authenticates as the **intake-submitter identity** of one namespace — the installation's intake namespace, or a context's — a dedicated principal that may create and revise intake kinds in proposal state in that namespace and read them, and nothing else. Every `Intake` in the intake namespace is open to that namespace's identity until its configuration is accepted, when it ends and its bound successor in the context's namespace is open only to that namespace's identity (ONBOARD §1); what either identity writes grants nothing. Every submission records its authenticated principal and the client name it claims. What it relays — repository content, issue and pull-request text, the person's interview answers — is untrusted content with provenance, never authority, and a person's answer relayed by it is no act of that person.
- **Repository content**, including build scripts, configuration and anything an agent may have written.
- **Forge text**: issue bodies, PR descriptions, comments, labels written by humans or bots. A remote object is a projection (I-7); its text cannot change an accepted contract, and human commands in it are validated against actor permissions before they become commands.
- **CI output.** A CI result is an observation bound to head, workflow and environment digests; it becomes evidence only inside an evidence bundle (I-6).
- **Model and typed-question outputs.** A judgment selects from an eligible set computed before it was asked (I-8); confidence summarizes a distribution and establishes nothing.
- **Telemetry, projections and read models.** Rebuildable, explanatory, never authoritative (I-9 concerns the canonical ledger, which is not telemetry).
- **Kubernetes Leases and heartbeats.** They carry liveness, never authority.

Every fact entering a decision carries one of three trust classes: a **canonical fact** (a committed aggregate status or receipt), an **authenticated observation** (a fact from a forge, CI, user or infrastructure that was authenticated, deduplicated and persisted before acknowledgement), or **untrusted content** (everything else). Deterministic code decides which classes a given decision may consume. A trust label on a proposal field (ONBOARD §1) names which kind of untrusted content it is; no label raises it to another class.

## What a compromised agent can reach

Assume any agent session is hostile from its first token. It can:

- read and write the **single writable mount** of its own workspace, subject to detection at the next checkpoint (I-5);
- make **brokered tool calls** within its scope capsule and its credential grant, each checked before effect;
- submit **claims**: progress reports, checkpoints, findings, candidate changes. Claims are inputs to someone else's decision.

It cannot:

- write protected status of any kind, or create a command under any principal but its own execution identity;
- obtain a reusable credential, reach the network except through the broker, or mount anything;
- accept, verify, or merge its own work (I-6: review is independent and fixed-tier);
- change its capsule, its budget, its acceptance contract, or the plan (I-8);
- act after it is fenced: its grant is revoked, its execution epoch is stale, and the broker refuses it (KERNEL §6);
- reference or affect another WorkContext or namespace: cross-context references are deny-by-default and a grant is bound to namespace, WorkContext UID, target UID, audience and repository membership;
- hide an out-of-scope mutation: any route — symlink, hardlink, rename, subprocess, ignored path — is caught on canonical identity at the next checkpoint, before evidence, and quarantines the workspace (I-5).

A compromised **Manager session** additionally can propose decompositions, assignments and plan revisions within policy, but every command it issues needs a reservation under the current epoch and a fixed target revision (KERNEL §4), and it can neither merge nor waive evidence. Takeover drains it.

A compromised **intake client** can propose anything the intake schemas admit, under provenance it may have forged. It cannot accept, activate or grant anything; write any kind other than an intake kind, or an intake kind outside proposal state; bind a repository the forge adapter does not verify; or read anything but intake kinds. What it proposes reaches work only through the human acceptance of ONBOARD §1, which shows every attribute with its provenance.

## Failing closed

When a trusted party is unavailable, the affected capability stops; nothing widens:

| Unavailable | Consequence |
|---|---|
| API server | Nothing commits. Agents may continue local work to their next checkpoint; no effect is accepted. |
| Authority witness | No new grants; a restored installation stays read-only (KERNEL §7). |
| EffectBroker | No privileged effect; accepted-but-unsent dispatches are recovered from the ledger on restart (KERNEL §3). |
| Artifact store | No workspace is preserved or retired; custody stays uncertain, which is quarantine (I-3). |
| Semantic judge | The deterministic conservative branch for that question class; no permission widens (I-8). |
| A required human | The affected scope stays visible and escalates under a deadline; it never proceeds silently. |

## Out of the threat model

- Cluster-admin bypass of admission and RBAC, and compromised control-plane credentials. These require containment and recovery, not an unconditional safety promise.
- Loss beyond the declared custody profile: simultaneous loss of a workspace and the artifact store, zone-wide loss, or physical loss of storage that finalizers cannot prevent.
- Provider misreporting: a forge or CI that returns a false definitive answer. AutoBot trusts an authenticated provider's *definitive* answer; it never trusts a timeout or a negative search as one.
- Correctness of target code, quality of a model, or solvability of an arbitrary requirement. Tests do not define correctness; AutoBot enforces evidence discipline, not truth.

Each of these is either recovered from by procedure or declared as a limit; none is silently assumed away.
