# Extension — CLI

Intent: one cross-platform executable, `autobot`, that is an ergonomic client of the Kubernetes API for the whole lifecycle — install, configure, intake, plan, run, review, investigate, intervene, recover, upgrade, retire — while `kubectl` and GitOps remain full clients of the same resources.
Gate: the M0 commands at **M0-Q**; installation, recovery and the complete command families at **G-INTAKE**; operational commands at **G-OPS**.

Relies on (kernel): the command and receipt protocol (KERNEL §2, F-1, F-2) — every mutation the CLI makes is an `AutoBotCommand` with a client-persisted idempotency key; create identity (KERNEL §2, F-5); interventions as a kernel kind (ROLES §5); the trust model's rule that no client writes protected status.

Decisions:

- **The deployment is the product; the CLI is a frontend.** It uses kubeconfig, Kubernetes authentication, namespaces and RBAC. Work continues when the CLI exits; a disconnected watch has no lifecycle effect; watching never pauses or cancels anything.
- **Every command creates or reads resources.** Declarative commands apply typed resources with a distinct field manager; conflicts are surfaced, never forced. Imperative-looking commands create durable request resources — `Intervention`, `RestoreRequest`, and later `CleanupRequest`, `ReviewRequirement`, `Approval`. Read commands join status with referenced evidence and artifacts. The CLI owns no scheduler, no state machine and no cache of authority.
- **It cannot mint anything.** The CLI cannot write any credential, permit, grant or protected status. A retry after an unknown submission looks the key up and never resubmits under a new one.
- **The intake is exposed as stages**: capture, inspect, answer, scope, propose, accept, activate; there is no single `run brief.md` command. A convenience chain may sequence the stages under policy; each transition stays visible, resumable and auditable.
- **Explain, don't repair.** `explain <task>` shows the capsule, scope, non-goals, revision, side issues, dependencies, current attempt, latest durable progress, evidence blocking acceptance, remaining budget and next permitted actions. Routing explanations show recorded policy, features and statistics, never invented reasoning. A repair is a separately scoped, auditable request.
- **Install into an existing supported cluster first.** `install` deploys versioned CRDs, operator, RBAC and admission. Cluster-scoped installation privilege is separate from daily namespaced permission. A local installation receipt supports recovery and authorizes nothing. Provisioning a local cluster is a development convenience.
- **Shutdown and upgrade go through resources.** Scaling the operator down stops neither running agents nor dispatched effects. Upgrade order: quiesce and reconcile outstanding operations, preserve recoverable workspaces, then upgrade.
- **Telemetry views are projections** (`observe …`); the CLI never bases a decision on the user's behalf on them.
