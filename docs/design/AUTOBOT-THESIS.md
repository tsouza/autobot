# AutoBot — thesis

Authority for: what AutoBot is, its ranked goals, the cost/quality principle, and the invariant list every other document serves.
Depends on: nothing. Every other document depends on this one.

## What it is

AutoBot is a durable multi-agent software delivery engine. It takes an accepted milestone and converges on it with a swarm of role-specialized agents — manager, worker, reviewer, tester, integrator, and an optional task-local micro-manager — spending as little frontier-model money as the quality bar allows, and never losing work or acting on stale authority while doing so.

Kubernetes custom resources are the source of truth. A Rust operator reconciles them. Everything else — forges, CI, model providers, the typed-question service, telemetry — is observed, adapted to, or projected into; none of it is authoritative.

## Ranked goals, highest first

1. **Convergence to one milestone target.** This is the product.
2. **Never lose work; never act on stale authority; degrade gracefully under outage.** This is the precondition for running unattended, and it is what the formal methods are for.
3. **Minimize expected total cost per accepted milestone.** Cheap-model routing is the thesis. It is a policy over the kernel, disabled (fixed routing) until the ledger it depends on is trustworthy.
4. **Semantic judgments go to a cheap typed-question service (TypeSafe).** Every such judgment is a typed question over a deterministically pre-computed eligible set: the service picks, deterministic code decides. A model output never grants authority, scope, budget or acceptance.
5. **Forges mirror the state of work.** GitHub, Gitea, Bitbucket and the like are projections of canonical records, never the source of truth.
6. **Onboarding from either a design document or a whole work-in-progress repository.**
7. **Observability is crucial and is a side channel.** It can never authorize anything and can never be the only copy of a learning record.

## The one real tension, and its resolution

Cost pulls toward weak models; safety pulls toward strong ones. The principle: **quality is a constraint, cost is the objective.**

- A task's **consequence class** — `REVERSIBLE` → `COMPATIBILITY_RISK` → `SECURITY_OR_DATA_INTEGRITY` — sets a **floor on the worker tier**. A charter entry can raise the floor; a typed-question judgment can only raise it, never lower it.
- **Reviewers are never routed below the fixed review tier** and are never the worker's model or session. Review is always present, so minimizing worker cost is minimizing total cost.
- The router minimizes expected total cost **above the floor**, and every review, repair and escalation cost is attributed to the strategy that caused it. A strategy that is cheap up front and expensive to repair is an expensive strategy.

## Invariants the core exists to make true

Every core document states which of these it serves. A mechanism with no invariant behind it does not enter the core.

| # | Invariant |
|---|---|
| **I-1 Commit** | One idempotency key commits at most one payload to at most one aggregate revision; every committed command has one durable committed receipt; a control commit preserves the pending domain commit and every domain field. |
| **I-2 Authority** | No effect is sent under stale authority. Acceptance of a dispatch is one CAS on the WorkContext whose preconditions are its registers (hold generation, plan revision and generation, Manager epoch and phase, integration-basis generation, dispatch-authority generation), and every authority cut is a CAS on the same aggregate, so acceptance and cut are totally ordered. |
| **I-3 Custody** | No work is lost. A workspace is never retired without an independently verified, restorable custody checkpoint; uncertain custody means quarantine. |
| **I-4 Effects** | No ambiguous effect is retried blind. A durable send-attempt marker precedes every remote send; an effect whose provider cannot disambiguate it is retained as `UNRESOLVED` for human adjudication and blocks its dependents. |
| **I-5 Scope** | Every mutation happens inside an immutable scope capsule, judged on canonical filesystem identity, or is detected at the next checkpoint before any evidence is accepted, and the workspace is quarantined. |
| **I-6 Evidence** | Acceptance evidence binds the exact candidate, base, contract, environment and remote generation, and is produced independently of the worker that made the candidate. |
| **I-7 Projection** | The forge is never the source of truth; a remote object is a projection of a canonical record, and remote text cannot change an accepted contract. |
| **I-8 Judgment** | A model output never grants authority, scope, budget or acceptance; a semantic judgment selects only from a deterministically computed eligible set, and its absence widens no permission. |
| **I-9 Ledger** | Every outcome and every cost lands in the canonical ledger or becomes a visible, linked gap; nothing silently leaves a denominator. |
| **I-10 Economics** | Quality is a constraint — consequence-class worker floor, fixed-tier independent review — and cost is the objective. |

Liveness is never promised unconditionally. Every "eventually" in the design is conditional on a named bound with an explicit degraded terminal state.

## Document set

`AUTOBOT-THESIS.md` (this) → `AUTOBOT-GLOSSARY.md` → `AUTOBOT-TRUST-MODEL.md` → `AUTOBOT-KERNEL.md` → `AUTOBOT-ROLES-AND-RUNTIME.md` → `AUTOBOT-ONBOARDING-AND-CONVERGENCE.md` → `AUTOBOT-FORMAL-SURFACE.md` → `AUTOBOT-M0-AND-GATES.md` → `extensions/*.md`. A new engineer reads them in that order in one afternoon. No core document depends on an extension.

Nothing enters a core document unless a stated invariant depends on it, M0 must implement it, or removing it would leave a term used but undefined. Everything else is a one-line DEFERRED entry with a gate, a decision-only line in an extension file, or omitted.
