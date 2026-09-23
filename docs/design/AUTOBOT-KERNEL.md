# AutoBot — kernel

Authority for: the authority protocol — how state commits, how effects are admitted and sent, how the Manager is serialized, how plans activate and are superseded, how plans and attempts pin a charter revision, how attempts are identified and fenced, how work is kept and restored, how outcomes and costs are recorded, how sessions continue — and, in §10, every lifecycle state machine in AutoBot.
Depends on: `AUTOBOT-THESIS.md` (I-1 … I-10), `AUTOBOT-TRUST-MODEL.md`.

The kernel is a set of rules over Kubernetes custom resources. There is no cross-resource transaction, no lease token that authorizes anything, no controller-local mutex, and no journal that can reconstruct an aggregate. What follows is written so that each rule names the invariant it serves.

## 1. Aggregates and commit lanes — I-1

An **aggregate** is a custom resource whose `status` is the committed, canonical state of one workflow entity. Each aggregate kind has exactly one **owning controller**, which alone may commit its protected status and alone repairs receipts and audit events for commands that target it. Admission rejects any other writer. The aggregate kinds of the core are:

```text
WorkContext · Charter · Project · ProjectCharter · Repository · WorkBrief · Intake · PlanProposal
Plan · PlanSnapshot · ManagerLease · Milestone · Task · TaskRun · AgentRun · AgentCheckpoint
ScopeCapsule · ExecutionIdentity · CredentialGrant · FenceSession
Workspace · CustodyPolicy · CustodyCheckpoint · ArtifactCommit · Artifact · WorkspaceConflict · RestoreRequest
EffectIntent · ExternalOperation · ToolInvocation · EffectReceipt · AdmissionStamp
IntegrationBasis · VerificationRun · EvidenceBundle
Budget · BudgetReservation · UsageReceipt · OutcomeRecord · TelemetryGap
Finding · Decision · Intervention
```

`AutoBotCommand`, `CommandReceipt`, `AutoBotEvent`, `GraphActivationReceipt` and `RestoreWitnessReceipt` are command, receipt and audit records; they are not aggregates.

**Decisions — I-8.** A `Decision` is committed only by the Decision controller, only as `RECORDED`, and only when its `selected` is a member of the eligible set whose digest it records, that set having been computed before the question was asked.

**The commit primitive** is one optimistic-concurrency write on one resource with an expected revision as precondition (CAS). Every state change is a CAS by the owning controller. Every aggregate carries `state_revision`, `control_revision` and `commit_sequence`; the last is incremented by every commit of either lane below, so any two commits on one aggregate have one order. "Before" and "after" between commits on one aggregate always means `commit_sequence` order.

**Domain lane.** A domain commit is one CAS with `state_revision` as precondition that applies the domain transition, increments `state_revision` and `commit_sequence`, and installs the aggregate's single bounded **pending commit** slot holding the exact receipt, the full audit envelope and the effect intents (§3) of that commit. This CAS is the commit point; a new process rebuilds the receipt from the command's `PREPARED` receipt and the slot's commit fields, and the audit event from the slot's audit envelope alone. The **receipt barrier**: the next domain commit on that aggregate may not proceed until the slot's receipt and audit event have been written, read back and verified, after which a reconciliation-only CAS (below) clears the slot without changing `state_revision`. A slot is never cleared by elapsed time. A timeout at any write is `UNCERTAIN`, never rejection; it is resolved by reading the slot and the receipt.

**Control lane.** Safety controls must move while a domain slot is unresolved — a hold while the receipt store is down. Each aggregate that has a control lane declares a fixed, admission-enforced **field partition** of its status into domain fields and control fields; a CAS that touches both is rejected. A control commit is one CAS with `control_revision` as precondition that applies only control fields, increments `control_revision` and `commit_sequence`, preserves the pending slot and every domain field byte for byte, and appends a complete **control receipt** to a bounded ring embedded in the same status. Because the receipt is in the CAS, a control commit is durable and reconstructible with no second write; only its audit publication is asynchronous and drains the ring. A full ring refuses further control transitions rather than dropping a receipt; that refusal is a visible degraded condition. Domain repair verifies that the aggregate's current *domain* digest equals the slot's after-digest; control commits cannot disturb that relation.

**Reconciliation fields.** An aggregate may declare a third class of status field that records only the *progress* of an intent some earlier commit already made canonical: the pending slot's `CLEARED` mark, a control-receipt ring entry's `PUBLISHED` mark, and on `WorkContext` every `dispatch_ledger[]` entry in full — appended only by the `AcceptDispatch` domain commit that creates it and re-derivable from that commit's `COMMITTED` receipt, advanced and removed only by reconciliation-only CASes, never appended by one — and every `blocked_targets[]` pair, which records that a committed operation's outcome is not yet settled, added and removed only by reconciliation-only CASes and re-derivable from the operation aggregates. Reconciliation fields are outside both digests. Either lane's CAS may write them together with its own fields (an `AcceptDispatch` domain commit appends the ledger entry it creates); a **reconciliation-only CAS** writes nothing but reconciliation fields, increments no revision and installs no receipt, because losing such a write loses nothing that §3.3 recovery cannot re-derive from receipts and operation aggregates. A reconciliation-only CAS is never a bypass: it can neither change a domain or control field nor make anything canonical.

Field classes in the core: on every aggregate, `state_revision` is domain and `control_revision` is control; on `WorkContext`, control fields `hold_state`, `hold_generation`, `hold_causes`, `manager_authority[*].phase`, `dispatch_authority_generation`, and reconciliation fields `pending_commit.state`, `control_receipt_ring[*].state`, every `dispatch_ledger[]` entry in full and every `blocked_targets[]` entry in full; on `TaskRun` and `AgentRun`, control fields `fence_state`, `execution_epoch`, `revocation_generation`, and reconciliation fields `pending_commit.state`, `control_receipt_ring[*].state`; on every other aggregate, reconciliation fields `pending_commit.state` and, where a ring exists, `control_receipt_ring[*].state`. Everything else in every status is domain, except the structural fields: `commit_sequence`, `last_receipt_ref`, `observedGeneration`, `conditions`, the pending slot body and the control-receipt ring entries belong to no class and are outside both digests — the domain digest covers domain fields only, the control digest control fields only. Either lane's commit may write `commit_sequence`, `last_receipt_ref`, `observedGeneration` and `conditions`, so a control commit may set a condition, such as the ring-full degraded condition.

**Keyed entries.** Creating or removing an entry of a keyed status map (such as `manager_authority[plan_uid]`) is a domain change. An absent control field reads as its initial value (§10), and the control digest reads it so. A domain CAS may therefore create an entry by writing only its domain fields, and may remove an entry only while each of its control fields holds its initial value; neither changes the control digest.

**Audit.** After a commit, the owning controller publishes an immutable `AutoBotEvent` carrying `(aggregate_uid, commit_sequence, lane, state_revision, control_revision, event_digest, …)`; a missing event is repaired from the slot or ring. A projection applies events to a read model strictly in `(aggregate_uid, commit_sequence)` order, holds a later event in a bounded buffer while a gap is visible, and rejects and quarantines a same-identity event with a different digest. Audit and projections explain and repair; they never roll back, overwrite or reconstruct an aggregate.

## 2. Commands and receipts — I-1

An `AutoBotCommand` is an authenticated, immutable, single-target mutation request. The principal is the authenticated writer, bound at admission; an actor string in the payload is ignored. A command pins its target UID and expected revision (or, for a create, kind, parent and client request key), its input digest, its replay window and, where relevant, its policy and Manager pins. Multi-target work is a recoverable sequence of commands, never a claimed transaction.

The **replay identity** is `(idempotency_key, principal, input_digest)`. Within the replay window the same identity returns the original receipt and never a second commit; the same key with a different payload or principal is rejected; after the window a retry is `REPLAY_EXPIRED` and is never read as new intent. A retry is never rebased onto a newer revision.

A `CommandReceipt` has a deterministic name (it is the idempotency index), immutable prepared input, and a terminal result owned by the target kind's controller: `PREPARED` is not evidence of commitment; `COMMITTED` and `REJECTED` cannot be rewritten; `UNCERTAIN` is an observation to be reconciled to a terminal state. A `REJECTED` receipt records the **rejection proof** of its ground, and a ground without its proof is no rejection:

- **Passed revision.** The exact expected revision can no longer commit *and* no matching committed slot or receipt exists; the receipt records the revision and `commit_sequence` it read. A CAS conflict alone proves nothing about an earlier transmission of the same command.
- **Replay conflict.** The key is already bound to a different payload or principal; the receipt records the existing receipt and the digest bound to it.
- **Create conflict.** The create's name is held by another object; the receipt records the observed object, or its UID, at the `commit_sequence` of the read that observed it.
- **Guard refusal.** The owning controller refused a guard before any CAS; the receipt records the guard's identifier and the revision it read.

**Creates.** A create command is indexed by context, principal, kind, parent and client request key, with the input digest compared but not incorporated, so a changed payload conflicts instead of creating a second object. The receipt reserves a deterministic name; the atomic create writes immutable origin metadata `(create_receipt_uid, input_digest, context_uid)` beside the spec; the target controller initializes status only after validating it. A lost acknowledgement is resolved by GET of that name, never by a generated name. A target cannot be deleted or recreated before its create receipt is terminal, and a name's absence after an ambiguous delete or restore is never authorization to recreate it.

A **tombstone** is the terminal `CommandReceipt` of a delete, retained together with the target's create receipt and origin metadata. It proves that the name existed and was deleted, so a replayed create of that name returns the original create receipt and never recreates the object. The target kind's controller owns it, like every receipt for that kind.

Receipts, slots, rings and tombstones are retained through the replay window and for as long as any effect of the command is pending, ambiguous or unresolved.

## 3. Dispatch: registers, acceptance, effects and the outbox — I-2, I-4

### 3.1 The dispatch registers

`WorkContext.status` holds the **dispatch registers**, the only authority any dispatch acceptance is checked against:

```text
hold_state                        RUNNING | FREEZE_PENDING | PROPAGATING | ENFORCED | RELEASING
hold_generation                   +1 on every hold_state transition
hold_causes                       the HOLD and KILL_SWITCH Interventions in force, by UID
admission_sequence                +1 on every AcceptDispatch
manager_authority[plan_uid]       (lease_uid, epoch, holder, deadline, phase ∈ {ACTIVE, DRAINING})
                                  from InstallManagerAuthority until RetirePlanAuthority (§4)
plan_authority[plan_uid]          (active_revision, snapshot_digest, activation_receipt_uid,
                                   revision_phase ∈ {ACTIVE, QUIESCING}, plan_generation)
                                  from the first activation until RetirePlanAuthority; absent
                                  means no active revision (§5)
integration_authority[basis_uid]  (basis_generation, state ∈ {RESERVED, INVALIDATED})
                                  from ReserveIntegrationBasis until RetireIntegrationBasis
dispatch_authority_generation     witness generation of this installation (§7)
dispatch_ledger[]                 bounded accepted-dispatch outbox (§3.3)
blocked_targets[]                 (target_identity, operation_uid) of every unsettled operation, in order added (§3.3)
active_manager_transaction        one bounded reservation slot (§4)
```

Routing is deliberately **not** a register. A TaskRun's **routing pin** — the identifier of the worker-strategy configuration it was admitted with — is written into its spec at `AdmitTask` and never changes for the life of the TaskRun; a permit carries that pin and acceptance checks only that the two are equal. Changing what future admissions pin needs no linearization with dispatch.

An **authority cut** is any register change. Every cut and every acceptance is a CAS on `WorkContext` applied by the **Context controller**, the sole owner of `WorkContext` status. A controller that needs a cut — the broker for `AcceptDispatch`, the Plan controller for `InstallManagerAuthority` / `ActivatePlanRevision` / `QuiescePlan` / `ResumePlanRevision` / `SupersedePlanRevision` / `RetirePlanAuthority`, the Manager holder for `RenewManagerAuthority`, a Manager command's target controller for `ClaimManagerTransaction`, the Integration controller for `ReserveIntegrationBasis` / `InvalidateIntegrationBasis` / `RetireIntegrationBasis`, the Custody controller for `AdvanceDispatchAuthorityGeneration` — submits a command targeting the `WorkContext` under its own principal and proceeds only on the `COMMITTED` receipt. Wherever this document says "the broker's `AcceptDispatch`", "the ledger entry to `SEND_ATTEMPTED`" or `AcknowledgeDispatch` it means that command path: the broker submits `AcceptDispatch` as a command, and the ledger updates and the `blocked_targets` updates as **reconciliation requests**, which are not `AutoBotCommand`s. A reconciliation request pins no revision and produces no `CommandReceipt`. A ledger update, or the addition of a pair, which is also a no-op when the pair is present, names the entry by `(operation_uid, permit_uid)` and the `send_state` it expects to find, and is a no-op when the entry is absent or already past that `send_state`, so a delayed request can neither advance a later entry nor remove one; the step-5(b) removal expects either `send_state`, and recovery row 6 adds a pair with no entry, a no-op when the pair is present (§3.3). The removal of a pair names the pair and is a no-op when it is absent (§3.3). The Context controller applies `AcceptDispatch` as a domain commit and every reconciliation request as a reconciliation-only CAS, and the broker proceeds only after its request has landed — the `COMMITTED` receipt for `AcceptDispatch`, a linearizable read of the updated entry or pair for the others. Only the Context controller writes the ledger, and only the Context controller writes `AdmissionStamp` status, so the `BROKER_ACCEPTED` acknowledgement is likewise a broker command it applies.

| Register change | Action | Lane |
|---|---|---|
| Hold | `RequestHold` (adds a cause; from `RUNNING` also `RUNNING → FREEZE_PENDING`, the cut), `PropagateHold`, `EnforceHold`, `ReleaseHold`, `CompleteHoldRelease` | control |
| Manager authority | `InstallManagerAuthority` (creates the entry), `RenewManagerAuthority` (deadline) | domain (§4) |
| Manager takeover | `DrainManager` (phase := `DRAINING`), then `AdvanceManagerEpoch` (holder, lease, epoch), then `ResumeManager` (phase := `ACTIVE`) | control, domain, control — in that order (§4) |
| Manager reservation | `ReserveManagerTransaction`, `ClaimManagerTransaction` (`RESERVED → APPLYING`), `ReleaseManagerTransaction` (`→ RESOLVED`) | domain (§4) |
| Plan | `ActivatePlanRevision`, `QuiescePlan`, `ResumePlanRevision`, `SupersedePlanRevision` — each increments `plan_generation`; `RetirePlanAuthority` | domain (§5) |
| Basis | `ReserveIntegrationBasis`, `InvalidateIntegrationBasis` — each increments `basis_generation`; `RetireIntegrationBasis` | domain |
| Restore | `AdvanceDispatchAuthorityGeneration` | control |
| Acceptance | `AcceptDispatch` | domain |

Because every one of these is a CAS on the same aggregate, any two are totally ordered by `commit_sequence`. "Accepted before the cut" has exactly one meaning: `commit_sequence(AcceptDispatch) < commit_sequence(cut)`. `Plan`, `IntegrationBasis` and `ManagerLease` keep their own detail and status, but their dispatch-relevant fact is *acknowledged from* the register, never the reverse: a `Plan` whose status says revision R2 while the register says R1 is stale and admits nothing.

**Holds.** `hold_causes` records every `HOLD` and `KILL_SWITCH` `Intervention` in force. Each hold action is a control CAS the Context controller applies:

- `RequestHold` applies a `HOLD` or `KILL_SWITCH`: it adds that `Intervention` to `hold_causes`. From `RUNNING` it is also the cut `RUNNING → FREEZE_PENDING`; in any other state it changes only the set, so a `HOLD` during a hold adds a cause.
- `PropagateHold` (`FREEZE_PENDING → PROPAGATING`) follows the Context controller's setting to `INVALIDATED` of every `ISSUED` permit of the context that no ledger entry and no `COMMITTED` `AcceptDispatch` receipt names (§3.2); a permit it misses pins an earlier `hold_generation` and fails at acceptance (§3.2).
- `EnforceHold` (`PROPAGATING → ENFORCED`) has the precondition that `dispatch_ledger` is empty: every acceptance before the cut is settled, `OUTCOME_UNKNOWN` or released (§3.3), and nothing accepted is still in flight. `ENFORCED` therefore arrives only once every in-flight send has an outcome or has timed out, and a quarantined `DispatchLedgerConflict` entry keeps it waiting until its adjudication.
- `ReleaseHold` applies a `RESUME` in `ENFORCED` or `RELEASING`: it removes from `hold_causes` only the cause that `RESUME` answers, so answering one hold never lifts another. When the set becomes empty in `ENFORCED`, it also moves `ENFORCED → RELEASING`. A `RESUME` that arrives before `ENFORCED` waits for it.
- `CompleteHoldRelease` (`RELEASING → RUNNING`) has the precondition that `hold_causes` is empty, and follows the stale-permit sweep: the Context controller sets to `INVALIDATED` every permit still `ISSUED` under an earlier `hold_generation` that no ledger entry and no `COMMITTED` `AcceptDispatch` receipt names (§3.2). Permits are issued again only after it.

**Register capacity and retirement.** `manager_authority` and `plan_authority` hold at most the profile's number of plans per context, and `integration_authority` at most its number of integration bases. `InstallManagerAuthority` refuses once the context holds that many plans, and `ReserveIntegrationBasis` refuses a new basis once it holds that many bases; either refusal is a visible degraded condition, like a full ledger. `ActivatePlanRevision` requires the plan's `manager_authority` entry, so `plan_authority` never holds a plan that `manager_authority` does not. An entry leaves only by retirement, a domain CAS:

- `RetirePlanAuthority(plan)` removes `plan_authority[plan]` and `manager_authority[plan]`. The Plan controller submits it once the `Plan` is `COMPLETED`, `FAILED` or `CANCELLED`. Its preconditions are `manager_authority[plan].phase = ACTIVE`, which is the §1 condition for removing the entry, and that `active_manager_transaction` holds no unresolved reservation for the plan (§4). An entry still `DRAINING` when the plan ends is first taken over by the Plan controller as holder — `AdvanceManagerEpoch`, then `ResumeManager` (§4) — and then retired; the plan is terminal, so the Plan controller submits no Manager command under that epoch.
- `RetireIntegrationBasis(basis)` removes `integration_authority[basis]`. Once the `IntegrationBasis` is terminal, the Integration controller submits `InvalidateIntegrationBasis` if the register still holds it `RESERVED`, and then `RetireIntegrationBasis`. Its precondition is `state = INVALIDATED`, so no merge permit pinning the basis can still be accepted.

An absent entry fails every acceptance precondition that names it (§3.2), so retirement fails closed. Keys are Kubernetes UIDs, and a retired key is never created again: no install, activation or reservation is submitted for a retired plan or basis.

### 3.2 Permits and `AcceptDispatch`

An `AdmissionStamp` (**permit**) is a capability to *request* acceptance, nothing more. It names one operation and pins every register value it will be checked against, plus the identities of its budget reservation, credential grant and scope capsule digest, its routing pin, its `provider_binding`, its effect digest, a nonce and an expiry.

**Issue.** For an operation in `REQUESTED` with no ledger entry, and whose previous permit, if any, is `CONSUMED`, `INVALIDATED` or `EXPIRED`, the broker submits the create of its permit under the §2 create protocol, with client request key `(operation_uid, operation state_revision)`: a lost acknowledgement resolves to the same permit, and each return to `REQUESTED` yields a new key. The Context controller validates the create against one read of the registers and refuses it unless `hold_state = RUNNING`, `plan_authority[plan].revision_phase = ACTIVE` and `manager_authority[plan].phase = ACTIVE`. It pins every register value from that same read, and the reservation, grant, capsule digest, routing pin, `provider_binding` and effect digest the broker names. A **merge** is an effect whose provider operation integrates source heads into a base branch; a permit whose `provider_binding` names a merge always pins an integration basis; a merge create without one is refused. A permit's `expires_at` is at most its creation time plus the profile's replay window, and every `AcceptDispatch` pins that replay window (§2), so a replay of an acceptance is never `REPLAY_EXPIRED` while its permit can still be accepted. On the permit's `COMMITTED` create receipt the broker CASes the operation `REQUESTED → PERMITTED`, recording `permit_uid`.

`AcceptDispatch(permit)` is one `WorkContext` domain commit, submitted by the broker with idempotency key `permit_uid`, whose preconditions are these:

```text
permit.state = ISSUED                                            (read before the CAS)
now < permit.expires_at
no dispatch_ledger entry names permit.uid or permit.operation_uid
hold_state = RUNNING ∧ permit.hold_generation = hold_generation
manager_authority[permit.plan_uid] = (permit.lease_uid, permit.epoch, _, _, ACTIVE)
plan_authority[permit.plan_uid] = (permit.plan_revision, _, _, ACTIVE, permit.plan_generation)
(permit.basis_uid = ∅ ∧ permit.provider_binding names no merge)
    ∨ integration_authority[permit.basis_uid] = (permit.basis_generation, RESERVED)
dispatch_authority_generation = permit.dispatch_authority_generation
permit.routing_pin = the pin in the permit's TaskRun spec        (not a register; a pin)
|dispatch_ledger| < capacity
|dispatch_ledger| + |blocked_targets| < capacity ∨ blocked_targets holds a pair of the operation
```

Every `permit.*` value except `permit.state`, and the routing pin in the TaskRun spec, is an immutable pin, so reading it before the CAS is exact; every other value is a register read in that same CAS. The last line reserves the pair a send may need at §3.3 step 4: pairs and entries share the ledger's capacity, and only an operation that already holds a pair is exempt, since it adds none. `permit.state` may be stale, and that is safe: a permit leaves `ISSUED` before acceptance only after a change to a register it pins, after its expiry or after a `REJECTED` `AcceptDispatch` for it, and each of those fails this CAS or its replay; after acceptance, the idempotency key returns the original receipt, and the ledger line refuses a second entry for the permit or its operation under any other replay identity.

Effect: `admission_sequence += 1`; a ledger entry is appended that records the acceptance sequence and generation. The ledger entry, created by that commit and carried in its `COMMITTED` receipt, is the authority. A failed precondition is a guard refusal (§2); the permit is then invalidated and the operation returned to `REQUESTED` to seek a new permit under the new registers.

**Acknowledgements.** After `ISSUED`, a permit's status acknowledges the ledger and its operation and is never read as authority. Each transition is its own domain commit on the `AdmissionStamp`, which the Context controller applies on the broker's command once the fact it acknowledges is durable:

- `ISSUED → BROKER_ACCEPTED` on the `COMMITTED` `AcceptDispatch` receipt, copying the acceptance sequence and generation; the `ExternalOperation`, still `PERMITTED`, records them too.
- `BROKER_ACCEPTED → CONSUMED` once the operation carries a `send_attempt` under the permit (§3.3 step 2b). The send does not wait for it.
- `BROKER_ACCEPTED → INVALIDATED` once the ledger entry is removed and the operation, still naming this permit, is `REQUESTED` with no `send_attempt` (§3.3 step 5(b)). An operation leaves `RECONCILING` for a re-request only once its permit is `CONSUMED` (§3.3), so a permit that was sent is never invalidated.

`RecoverAcceptedDispatch` (§3.3) submits the same commands for every permit it finds behind its facts, so a lost acknowledgement is brought level after a crash. The Context controller moves a permit `ISSUED → INVALIDATED` or `ISSUED → EXPIRED` only after a change to a register it pins, after its expiry or after a `REJECTED` `AcceptDispatch` for it, and never while a ledger entry or a `COMMITTED` `AcceptDispatch` receipt names it; every sweep of `ISSUED` permits (§3.1, §5) is bound by this. On a permit found `INVALIDATED` or `EXPIRED` before acceptance, the broker CASes its operation `PERMITTED → REQUESTED`.

The consequences that follow from ordering alone:

- **Hold.** A permit accepted at `s` and `RequestHold` at `c`: `s < c` means pre-cut and may complete; `s > c` fails on `hold_generation`. There is no third case. `ReleaseHold`'s move to `RELEASING` and `CompleteHoldRelease`'s return to `RUNNING` each increment `hold_generation` again, so no permit issued under a previous `RUNNING` survives a hold.
- **Plan.** No permit pinning R1 is accepted after `SupersedePlanRevision(R2)` or `QuiescePlan(R1)`; and because `QuiescePlan` and `ResumePlanRevision` each increment `plan_generation`, a permit issued before a quiesce fails after a resume *of the same revision* even if its own `INVALIDATED` write was never applied.
- **Basis.** A merge permit pinning generation g is not accepted after `InvalidateIntegrationBasis` advanced it.
- **Manager.** Acceptance under a drained or advanced epoch fails at the register.

Grant, reservation and capsule **currency** is not a register and is not read by the acceptance CAS. The broker enforces it before `RecordSendAttempt` (§3.3) and on every privileged call: revocation generation, execution epoch and expiry of the grant, `RESERVED` state of the reservation, and the capsule digest. A mismatch returns the operation to `REQUESTED` without a send, releases the ledger entry (§3.3 step 5(b)) and then invalidates the permit (`BROKER_ACCEPTED → INVALIDATED`); a register re-validation failure at `RecoverAcceptedDispatch` row 1 ends the same way. A fence that lands between acceptance and send is therefore caught at the send.

### 3.3 Effect intents, the dispatch ledger and the send-attempt outbox

A domain commit's pending slot carries an ordered, bounded set of immutable **effect intents**, each `(operation_key, effect_index, payload_digest, provider_binding, desired_outcome, target_identity, contract_revision)`, where `provider_binding` is the `[provider, operation]` pair that keys the provider's capability, with

```text
operation_key = hash(installation_lineage, aggregate_uid, committed_revision, effect_index, payload_digest)
```

— the logical identity of one intended effect across retries and restores. The slot holds each intent with `installation_lineage` in place of `operation_key`: `aggregate_uid` and `committed_revision` are those of the slot's own commit, so the key is derived when the `EffectIntent` is built. The committed receipt carries the intents before the slot can clear; `EffectIntent` and `ExternalOperation` resources are **materialized** at deterministic names from committed receipts by a dispatcher that scans slots and retained unacknowledged receipts, not only watches. A name collision with a different payload digest, target identity or contract revision is `EffectIntentCollision`: quarantined, never dispatched. Dispatch requires a verified committed receipt, the exact intent digest, current acceptance (§3.2) and a provider capability the adapter has qualified for that operation; a prepared receipt, an existing child resource or a missing parent never authorizes it.

Every agent tool call with an effect outside its workspace — forge, CI, deployment or credential — is a `ToolInvocation`. Its effect intent is carried in the pending slot of an `AgentRun` domain commit, which the AgentRun controller applies on the runtime adapter's command, so its `operation_key` takes the `AgentRun` UID and that commit's revision. It then follows this same protocol with its own permit, acceptance, ledger entry and `send_attempt`, and its `EffectReceipt` records the outcome or `OUTCOME_UNKNOWN`. Stream closure never proves a tool failed. A filesystem call inside the workspace is brokered for scope (ROLES §2) and preserved by custody (§7); it is not a `ToolInvocation`. Read-only calls need none of this.

The **dispatch ledger** entry is `(operation_uid, operation_key, permit_uid, acceptance_sequence, acceptance_generation, accepted_at, send_state ∈ {ACCEPTED_NOT_SENT, SEND_ATTEMPTED, ACKNOWLEDGED})`. `ACKNOWLEDGED` is written in the same CAS that removes the entry; it is never retained, never observable and never counts toward capacity. The broker's send sequence is:

1. `AcceptDispatch` → entry `ACCEPTED_NOT_SENT`.
2. `RecordSendAttempt`. Before it, the broker validates currency (§3.2); a failure here returns the operation to `REQUESTED` with no ledger change past `ACCEPTED_NOT_SENT`. Then two CASes in this fixed order: (2a) the ledger entry to `SEND_ATTEMPTED` — a reconciliation-only `WorkContext` CAS applied by the Context controller on the broker's reconciliation request (§3.1), which also names the operation's `target_identity`, and which leaves the entry `ACCEPTED_NOT_SENT` when the blocked-target guard below refuses it; the broker handles a refused 2a exactly as a currency failure; then (2b) the operation to `DISPATCHING` with `send_attempt = (attempt_index, started_at, acceptance_sequence)`. **Both precede opening the remote connection.** A present `send_attempt` therefore always implies a `SEND_ATTEMPTED` entry. Once 2b is durable, the broker submits the command that records the permit `CONSUMED` (§3.2).
3. Send.
4. Observe; CAS the operation to `CONFIRMED` / `REJECTED` / `FAILED`. A rate-limit answer is no outcome: it is handled like a timeout, and it proves non-application at reconciliation only when the provider's declared capability marks its rate-limit answers authoritative (below). On timeout or disconnect, the broker's reconciliation request first adds the operation's pair to `blocked_targets` (below), and only once it has landed does the broker CAS the operation to `OUTCOME_UNKNOWN`, so no `OUTCOME_UNKNOWN` operation exists without its pair. The request names the entry and expects it `SEND_ATTEMPTED`, like the other ledger requests, so it is a no-op once the entry is gone.
5. `AcknowledgeDispatch`: reconciliation-only CAS removing the entry, only after the operation has durably recorded its acceptance and one of two cases:
   - (a) a terminal or `OUTCOME_UNKNOWN` state. The broker moves an `OUTCOME_UNKNOWN` operation to `RECONCILING` only after this acknowledgement has landed.
   - (b) `REQUESTED` with no `send_attempt` present, after a currency failure at step 2 or a register or currency re-validation failure at `RecoverAcceptedDispatch` row 1. The entry is removed whether it reads `ACCEPTED_NOT_SENT` or, when a 2a the broker read as refused landed late, `SEND_ATTEMPTED`: with no `send_attempt` on the operation, nothing was sent under the entry. After the removal, the broker submits the command that records the permit `INVALIDATED` (§3.2).

   An entry is never removed while `send_attempt` is present and no outcome is recorded.

`RecoverAcceptedDispatch` runs on broker restart and joins the ledger with the operations:

| Ledger `send_state` | Operation `send_attempt` | Conclusion | Action |
|---|---|---|---|
| `ACCEPTED_NOT_SENT` | absent | never sent | re-validate registers and currency; if still current continue at step 2, else invalidate and return to `REQUESTED` |
| `SEND_ATTEMPTED` | present, no outcome | sent or unknown | the pair, then `OUTCOME_UNKNOWN`, acknowledge, then `RECONCILING`; no resend |
| any | terminal outcome | done | acknowledge |
| `SEND_ATTEMPTED` | absent (2b uncertain) | unknown, or not sent if `REQUESTED` | an operation still `PERMITTED` gets `send_attempt` written by the broker (`PERMITTED → DISPATCHING`), which consumes the revision a delayed 2b expected; then as row 2, with no send. An operation in `REQUESTED` is acknowledged under step 5(b) |
| `ACCEPTED_NOT_SENT` | present | impossible under 2a-before-2b | `DispatchLedgerConflict`: quarantine, no send, human adjudication |
| no entry | present, no outcome | acknowledged early or lost | as row 2, the pair added without an entry |

The same scan brings every permit level with its ledger entry and operation (§3.2), and removes the pair of every terminal operation.

**Blocked targets.** An operation is **unsettled** from its `OUTCOME_UNKNOWN` until it reaches a terminal state: while it is `OUTCOME_UNKNOWN`, `RECONCILING` or `UNRESOLVED`, and through a re-request after proven non-application. The `blocked_targets` register is the list, in the order the pairs were added, of one pair `(target_identity, operation_uid)` per unsettled operation. A pair is added at step 4 and removed by a reconciliation request from the broker once the operation's terminal state is durable and its entry is gone; a request naming an absent pair is a no-op. Capacity is reserved at acceptance: pairs and ledger entries share the ledger's capacity (§3.2), so adding a pair never waits; only recovery row 6, adding the pair of an operation whose entry was lost, can take the list past the bound, and acceptance is refused until it drops back.

The order of dependents is the order of CASes on the one `WorkContext` resource, reconciliation-only CASes included: every CAS on it is linearized by its expected-revision precondition, whether or not it increments `commit_sequence`. An effect intent is a **dependent** of an unsettled operation when it has the same `target_identity` and its 2a would follow the CAS that added that operation's pair. A sibling whose 2a preceded that CAS is concurrent, not a dependent, and is settled on its own if its outcome is unknown. The 2a guard is: no pair is on the operation's target, or the first pair on it is the operation's own. So a fresh intent is refused while any pair is on its target; among unsettled operations on one target, only the earliest one may send again, by re-request, and the next follows once it is terminal. The broker requests no permit for an operation whose 2a this guard would refuse; the guard stays the authority. The block is by target, not by attempt: an intent of a replacement attempt is a dependent like any other.

The other dependents of an unsettled operation are every `IntegrationBasis` whose source or base heads the operation could have changed and every acceptance evaluation of the milestone that owns the originating command. Each is refused at the check that would otherwise admit it: `ReserveIntegrationBasis`, itself a `WorkContext` CAS, refuses a basis with a source or base head at a blocked target; once a pair is added, the Integration controller submits `InvalidateIntegrationBasis` for every reserved basis with a head at that target; `RecordAcceptanceAdjudication` refuses the owning milestone.

**Reconciliation.** An operation in `RECONCILING` may be re-requested (same `operation_key`, `attempt_index + 1`) only when the adapter proves definitive non-application, and only once its permit is `CONSUMED`. The proofs are a provider lookup, provider deduplication, and a rate-limit answer from a provider whose declared capability marks its rate-limit answers authoritative; after such an answer the re-request is sent no earlier than the provider's back-off allows. A negative search proves nothing, and neither does a rate-limit answer from any other provider. When the provider's declared capability offers neither idempotency nor lookup, the operation becomes `UNRESOLVED` after `provider_reconcile_bound`: retention pinned, its dependents still blocked, plan completion blocked, an `Intervention` for human adjudication created. `UNRESOLVED` is never garbage-collected, never re-requested and never counted as success or failure. A provider whose declared capability lacks a required semantic yields `BLOCKED_UNSUPPORTED` before any send.

**Adjudication.** An `UNRESOLVED` operation ends only through one human adjudication, recorded in its `adjudication` field, a domain field of the `ExternalOperation`: the `Decision` it references and the outcome chosen, `CONFIRMED`, `FAILED` or `COMPENSATED`. The broker writes it once, by a domain CAS that also moves the operation from `UNRESOLVED` to the chosen outcome; its preconditions are `state = UNRESOLVED` and the field absent. A second adjudication, or a delayed copy of the first, can therefore never reopen or replace it, and no crash can leave an adjudication recorded on an operation that has not yet ended. Once that CAS is durable, the broker releases the operation's pair. `COMPENSATED` records the adjudicator's evidence that the effect was compensated outside AutoBot; AutoBot sends no compensating effect, and automatic compensation is DEFERRED to G-FORGE. A `COMPENSATED` operation counts as a failure wherever outcomes are counted.

## 4. Manager serialization — I-2

`manager_authority[plan_uid]` on the `WorkContext` is the Manager's authority: `(lease_uid, epoch, holder, deadline, phase)`. `ManagerLease` and `Plan` status expose acknowledged copies and authorize nothing.

Before a `Plan` exists there is no Manager authority: the intake kinds are ordinary aggregates, written by the intake client and the Intake controller (ONBOARD §1) under §1 with a fixed expected revision and no Manager reservation. After the `Plan` create that accepting a `PlanProposal` produces (§5), the Plan controller submits `InstallManagerAuthority(plan_uid, holder, lease_uid)`, which the Context controller applies as a domain CAS creating the first `manager_authority[plan]` entry with `epoch = 1` and `deadline` set by the lease rule below. The entry starts `ACTIVE`, the §10 initial state of its phase, which the CAS leaves unwritten (§1, keyed entries). Its preconditions are that the plan has no entry and that the context holds fewer plans than the profile allows (§3.1); a refused install leaves the `Plan` in `ACCEPTED`, and the Plan controller submits it again once an entry is retired. The plan-scoped rules of this section start at its `COMMITTED` receipt.

**Lease.** `InstallManagerAuthority`, `AdvanceManagerEpoch` and `RenewManagerAuthority` each set `deadline` to the time of their CAS plus the Manager lease duration (M0 §2). The holder renews with `RenewManagerAuthority`, a domain CAS whose preconditions are that holder, lease UID and epoch match the entry, `phase = ACTIVE` and `deadline` has not passed. Once `deadline` has passed, the Context controller applies `DrainManager`.

A Manager command pins plan UID and revision, holder, lease UID and epoch, exact target UID and revision, and input digest. `ReserveManagerTransaction` is the Context controller's domain CAS on the `WorkContext` that checks holder, lease UID, epoch, `phase = ACTIVE` and that `deadline` has not passed, and reserves the single `active_manager_transaction` slot: `(plan_uid, target_uid, expected_revision, command_uid, phase ∈ {RESERVED, APPLYING, RESOLVED}, target_receipt_uid, cancellation_receipt_uid, terminal_state)`. The slot is **empty** when it is absent or `RESOLVED`; a reservation needs an empty slot and overwrites a `RESOLVED` one. The slot serializes short control mutations, not reasoning, builds or agent execution; it is released only by `ReleaseManagerTransaction`, which the target's owning controller submits with the target's receipt, or the controller driving a takeover submits in step 2 below: a domain CAS that records a non-empty `terminal_state` together with the receipt that proves it and sets `phase := RESOLVED`. Only the target's owning controller applies the reserved command, under §1 and with the command's **fixed expected revision**, which is never refreshed to force an old command through. It claims before it decides: before it either commits or rejects the command, it submits `ClaimManagerTransaction(command_uid)`, which the Context controller applies as a domain CAS `RESERVED → APPLYING` whose preconditions are that the slot holds that command and that `manager_authority[plan].phase` is `ACTIVE` at the epoch the command pins. The owning controller writes the target, or rejects the command, only on the claim's `COMMITTED` receipt. A claimed command that it rejects is released by `ReleaseManagerTransaction` with `terminal_state = REJECTED` and the `REJECTED` receipt as proof, so a rejection never leaves the slot `RESERVED`.

**Takeover drains before the epoch advances**, in three steps that are never merged:

1. `DrainManager` — control CAS, `phase := DRAINING`, applied by the Context controller once `deadline` has passed or when a takeover is requested. New reservations and claims under the old epoch fail; the existing reservation is preserved. Holds may still be requested.
2. Release the slot. A `RESERVED` slot is released as `CANCELLED`, with no target write and with the control receipt of the `DrainManager` that drained the entry as proof: a claim needs `phase = ACTIVE`, so none can commit once the entry is `DRAINING`, and the target's owning controller, whose claim is refused, rejects the unclaimed command as a guard refusal (§2). Outside a takeover a `RESERVED` slot is never released: its command is claimed first and then resolved. An `APPLYING` slot is released once its command is resolved to a durable terminal receipt, or cancelled with a CAS on the target that consumes the reserved expected revision and retains a cancellation receipt, so a delayed old CAS fails. An uncertain result cannot release the slot; takeover waits. Uncertain creates are drained to a receipt or a retained inert target, never abandoned.
3. `AdvanceManagerEpoch` — domain CAS with preconditions `phase = DRAINING` and slot empty; sets holder, lease, `epoch := e+1` and `deadline` while phase stays `DRAINING`. Then `ResumeManager` — control CAS with precondition `epoch = e+1`; sets `phase := ACTIVE`. Between the two no permit is accepted (`phase ≠ ACTIVE`); after the second every permit pinning epoch e fails. A single CAS touching both phase and epoch is rejected by the field partition and is never issued.

Lease expiry begins draining; it is never permission to skip it. Draining is per plan and independent of the context hold: a context may be `RUNNING` with one plan `DRAINING`, or `ENFORCED` with all plans `ACTIVE`; acceptance checks both.

## 5. Plan activation and supersession — I-2, I-6

Accepting a `PlanProposal` creates the `Plan` in `ACCEPTED`: the contract exists, nothing can run. A **plan revision** is one immutable contract version; its graph is an immutable `PlanSnapshot` — members with UID and revision, edges, acceptance and budget policy, digests. Activation is owned by the Plan controller: verify the snapshot digest, create or verify every member against it (`MEMBERS_VERIFIED`), then submit `ActivatePlanRevision`, which the Context controller applies as the register CAS `plan_authority[plan] := (revision, snapshot_digest, activation_receipt_uid, ACTIVE, plan_generation + 1)` — the **activation cut** — and on its receipt CAS `PlanSnapshot` to `ACTIVATED` and `Plan` to `ACTIVE` as acknowledgements, recording the `GraphActivationReceipt`. `ActivatePlanRevision` is a plan's first activation only: it creates `plan_authority[plan]`, and its preconditions are that the entry is absent, which counts as `plan_generation = 0`, and that `manager_authority[plan]` is present (§3.1). A partial graph cannot produce ready work: every readiness and admission decision verifies the active revision, snapshot digest and activation receipt in the same decision, and every acceptance verifies the register.

Only an authenticated Manager holder, a human named by `WorkContext.spec.revisionAuthority`, or an intake client through a `PlanProposal` that names the plan (ONBOARD §1) may propose a revision; the proposal is the Plan condition `RevisionPending`, not a phase, which an intake client's proposal raises only when it is accepted (ONBOARD §1 step 6). **Supersession** is: `QuiescePlan` (register `revision_phase := QUIESCING`, `plan_generation + 1`; every new acceptance for the plan fails, every issued permit is invalidated), wait for active attempts to reach a terminal, fenced or unknown/unresolved boundary, record unresolved operations, invalidate acceptance evidence, then `SupersedePlanRevision(R2)` on the same register. `SupersedePlanRevision(R2)` is the replacement's activation cut: once R2's snapshot is `MEMBERS_VERIFIED`, the Plan controller submits it, and the Context controller applies the register CAS `plan_authority[plan] := (R2, snapshot_digest, activation_receipt_uid, ACTIVE, plan_generation + 1)` with the precondition `revision_phase = QUIESCING` at R1. On its receipt R1 is acknowledged `SUPERSEDED`, and R2 is acknowledged as for `ActivatePlanRevision`. That no R1 effect can be accepted after R2 activation is an ordering fact, not a policy. Resuming the same revision is `ResumePlanRevision` (`revision_phase := ACTIVE`, `plan_generation + 1`). `PAUSED` is a plan-level block on new TaskRun admission requested through an `Intervention`; it revokes no accepted effect and never weakens a context hold.

An active attempt keeps the revision and capsule it started with until it is terminal or fenced, except that its capsule is replaced by a fresh capsule at its next continuation after a new or tightened law; the broker and acceptance apply that law to the attempt at once, without waiting for the replacement (below). There is no in-place revision of a task, a capsule or an acceptance contract; any change to what a task must do is a new revision through this section or a new task.

**Charter pin — I-5, I-8.** A `Charter` holds the standing entries of a `WorkContext`, a `ProjectCharter` those of one `Project`; each is a resource with immutable, digested revisions, and a project is bound by its `ProjectCharter` together with the `Charter` it inherits. An entry is a **law**, which is never waived, or a **rule**, which may be. Acceptance of a plan revision pins the accepted revision of both, with their digests, in the `PlanSnapshot`; a plan revision cannot be accepted without an accepted charter revision. The **charter in force** for a plan revision's work is its pinned revisions strengthened by every law a later accepted revision adds or tightens; its digest is the **charter digest**. Every capsule carries the charter digest at its issue and the entries relevant to its task. A charter change reaches pinned work by kind of change:

- A new or tightened law applies immediately to all work not yet accepted. It enters the charter in force at once: the broker refuses effects that violate it, candidates are judged under it, and an `EvidenceBundle` bound to an earlier charter digest satisfies no acceptance. Every capsule issued from then on carries it, and an active attempt receives a fresh capsule carrying it at its next continuation (§9).
- Relaxing a law, or changing a rule, waits for the next plan revision; a plan revision pinned before the change keeps the entry as it was pinned.

A **fresh capsule** is a new `ScopeCapsule` that differs from the one it replaces only in its charter digest and entries. The TaskRun controller issues it, revokes the one it replaces and moves the `TaskRun`'s capsule reference to it; a permit pinning the old capsule digest fails the broker's currency check (§3.2). A `TaskRun` holds one capsule at a time.

**Evidence and integration basis.** Every acceptance decision uses an `EvidenceBundle` binding candidate, base and head digests, plan and scope revisions, the charter digest, criteria, environment and toolchain digests, provider runs, CI and review attestations, remote generation and expiry; any change to any of these invalidates it. Every milestone's change sets are serialized through an `IntegrationBasis` — base head, source heads, overlap set, merge order, integrated candidate — whose `basis_generation` lives in the register, so a merge permit is invalidated at the same linearization point as holds when any reserved head, base, protection digest or earlier merge changes. A milestone's **final basis** is its basis at the current `basis_generation` whose merge order contains every member change set. Milestone acceptance is evaluated against the final basis's integrated candidate, never against individually passing branches.

## 6. Identity, grants and fencing — I-2, I-5

Every AgentRun receives its own `ExecutionIdentity` `(task_run, workspace, agent_run, execution_epoch, installation_id, audience)` and its own short-lived `CredentialGrant` bound to that identity, its audience, repositories, paths, operations, installation lineage, execution epoch, expiry and revocation generation. Two AgentRuns never share an identity or a grant (the sessions of one AgentRun, its continuations, keep its own, §9), and a grant carries only what its session's role may request (ROLES §4); `workspace` is empty for a session that holds no workspace. Every session runs in a `TaskRun`: a task's sessions in that task's TaskRun, and the Manager's, the Integrator's and those that assess a plan's integrated candidate in a **planning TaskRun**, whose spec names the `Plan` or `Intake` it serves in place of a task and which holds no workspace (ROLES §4). Agents never hold a reusable credential. The broker validates namespace, WorkContext UID, target UID, audience, repository, path, operation, lineage, epoch, revocation generation and expiry on every privileged call; a grant that fails any of these, or references another context, is refused.

`execution_epoch` is a per-TaskRun counter advanced on fencing and part of every identity, grant and checkpoint. A logical epoch alone is not fencing. The **fence protocol** — `fence_state: ACTIVE → FENCE_PENDING → FENCED | FENCED_UNCERTAIN`, on the control lane, independent of the run's phase — is: increment the epoch; revoke the grant of every AgentRun of the TaskRun; revoke broker capability; stop or isolate the process; remove or quarantine workspace write access; verify broker and workspace fences; record `FenceConfirmed`. The **operations of a TaskRun** are the `ToolInvocation`s of its AgentRuns (§3.3) and every operation it inherited from a predecessor (§9); a ledger entry belongs to the TaskRun whose operation it names. `FenceConfirmed` additionally requires that no ledger entry names an operation of the fenced TaskRun, each having been removed under §3.3 step 5 after its send was refused or its outcome recorded, and that no operation of the fenced TaskRun is `OUTCOME_UNKNOWN` or `RECONCILING`: each has reached a terminal state or `UNRESOLVED`, was proven not applied and re-requested, or was never sent. The fence therefore waits out every send the fenced run had in flight, and every operation it leaves unsettled is `UNRESOLVED` or proven not applied, with its pair on its target (§3.3). If confirmation cannot be obtained the state is `FENCED_UNCERTAIN`: replacement, cleanup and privileged effects are blocked until a late confirmation arrives. A mounted volume or a changed epoch alone is never evidence of fencing.

## 7. Custody and restore — I-3, I-2

A `Workspace` is custody of a checkout and outlives sessions and attempts. It is never retired on the strength of a clean git status or a merged PR. A **custody checkpoint** is: confirm the workspace write fence; inventory tracked, staged, untracked, ignored and unpushed content, local refs, stashes and the record outbox (§8) as `(canonical_path, inode, digest)`; write a manifest; upload; verify the digest *and an independent restore into a fresh location*; write a completion marker; record `ArtifactCommit(VERIFIED)`; only then mark the workspace `PRESERVED`. A lost upload acknowledgement stays uncertain until verified by lookup or restore. Unknown custody is quarantine.

Custody checkpoints recur. While a run uses its workspace, one is taken at the checkpoint cadence its `CustodyPolicy` declares (the profile's, M0 §2) and one at run end. Each moves the workspace `IN_USE → PRESERVING`, which is when the write fence is confirmed, and then `PRESERVED`; a run that goes on working lifts the fence with `PRESERVED → IN_USE`. The write fence holds for as long as the workspace is `PRESERVED`, so a `PRESERVED` workspace holds nothing its last verified checkpoint lacks, and retirement, which requires `PRESERVED`, never loses a write made after that checkpoint. An `AgentCheckpoint` is **covered** by the first `VERIFIED` custody checkpoint of its session's workspace begun after it, and is `VERIFIED` only once it is covered, so a failed custody checkpoint only defers it to the next; the checkpoint of a session that holds no workspace is verified without one. "The next checkpoint" of I-5 is the next custody checkpoint of the workspace, and so is every scope diff "at every custody checkpoint" (ROLES §2).

Inventory that shows content from more than one TaskRun, project or unowned local work creates a `WorkspaceConflict` with owner candidates, a per-file attribution manifest, preservation digests and a quarantine owner. Nothing under an unresolved conflict is deleted or assigned to a task until a human adjudicates it; attribution uncertainty survives restore.

**Restore.** A restored installation has a new `installation_id` and `restore_generation` and stays read-only until four conditions hold: `OldInstallationFenced`, `OutstandingOperationsReconciled`, `RestoreIdentityMapped`, `RestoreDispatchEnabled`. Dispatch authority is held by the external **authority witness**; the installation cannot self-assert that its predecessor is fenced. The **ambiguous operation set** is every operation the restored state holds in a non-terminal state, since the old installation may have moved any of them after the state was taken; the `RestoreRequest` records the set, and the witness signs only its digest. Re-enabling dispatch requires a witness-signed `RestoreWitnessReceipt` `(installation_id, restore_generation, witness_generation, old_installation_fence_evidence, ambiguous_operation_set_digest, identity_mapping_digest, old_grant_expiry, signature)` whose digest matches the recorded set, reconciliation or `UNRESOLVED` adjudication of every operation in the ambiguous set, and either proof of active revocation of every old grant or a wait through `max_old_grant_ttl + broker_revocation_bound`. The receipt's `witness_generation` becomes `dispatch_authority_generation` through `AdvanceDispatchAuthorityGeneration`, so restore participates in the same linearization as holds.

**The read-only guard.** A restore writes every restored `WorkContext` with `dispatch_authority_generation` holding no value, and only `AdvanceDispatchAuthorityGeneration` gives it one; the Custody controller submits it only from `MAPPED`, with the receipt above, and moves the `RestoreRequest` to `DISPATCH_ENABLED` on its `COMMITTED` receipt. A register with no value equals no pin. Until then three checks refuse: `AcceptDispatch` fails its `dispatch_authority_generation` precondition for every permit (§3.2); `RecoverAcceptedDispatch` row 1 fails its register re-validation, so a restored `ACCEPTED_NOT_SENT` entry is released instead of sent (§3.3); and the broker issues no `CredentialGrant`, and honours none on a privileged call, for an installation whose `RestoreRequest` is not `DISPATCH_ENABLED`. A restored operation of the ambiguous set that has no `send_attempt` is sent after the barrier only once a provider lookup or deduplication answer under its `operation_key` proves it was not applied; a provider that offers neither leaves it for human adjudication, and the broker requests no permit for it before then. Old external-operation identities (`operation_key`) are preserved across restore; old pending actions are never replayed merely because a recovered resource lists them. Witness or broker uncertainty fails closed.

## 8. The canonical-record obligation — I-9

`OutcomeRecord`, `UsageReceipt` and `TelemetryGap` are aggregates created through the create protocol of §2, not telemetry. Their durability under API unavailability is an obligation on the `TaskRun`, a planning TaskRun included. Its entries are one `outcome` entry and one `usage` entry per **usage producer**: each session of each of its AgentRuns, keyed `(agent_run, session_sequence)`, and the broker, for the effects of its operations.

1. At admission, `TaskRun.status.expected_records = {outcome: PENDING, usage: {}}` and a `record_deadline` are set. Before a usage producer spends, its entry `expected_records.usage[producer] := PENDING` commits on the `TaskRun`: a session's before its first model call, which its runtime adapter makes only after that commit, and the broker's before it accepts the first effect of the TaskRun's operations. A producer with no entry never spends, so no spend is outside the obligation.
2. The producing process — each session's runtime adapter, the broker, or for the outcome the verifier (the TaskRun controller for a planning TaskRun) — writes its record first to its **durable local outbox** (inside the checkpointed workspace area, or the broker's persistent queue, which a session with no workspace uses) with its deterministic name, input digest and create key, *before* reporting completion. A usage producer writes one `UsageReceipt` for its spend under the TaskRun, naming itself.
3. The outbox drains through create commands with retry state on the entry; retries after a lost acknowledgement resolve to the same record.
4. An entry becomes `RECORDED` only on its own record's `COMMITTED` create receipt: `outcome` on the `OutcomeRecord`'s, `usage[producer]` on that producer's `UsageReceipt`'s.
5. A TaskRun, terminal or not, with an entry still `PENDING` at `record_deadline` receives, for that entry, a `TelemetryGap` (`gap_kind ∈ {OUTCOME_MISSING, USAGE_MISSING}`) linked to it, and the entry becomes `GAP(uid)`. A gap is a canonical record: whatever counts outcomes or costs counts it, as unknown outcome or censored cost, never as absent. A record that commits after its gap closes the gap the entry names (`CLOSED`, linked to the record), and every count uses the record from then on, as an append-only correction; the entry stays `GAP` (§10).
6. Outbox entries are inventoried and preserved by custody checkpoints; a workspace lost before its outbox drained yields a gap, never a silently missing record.

A `UsageReceipt` not settled by its settlement deadline becomes `CENSORED` at `min(reservation ceiling, rate-card bound)`, counts at that bound for cost, and is corrected only by append-only later settlement. Censoring is a cost rule only: an unsettled cost is never read as an outcome. A `BudgetReservation` in `UNKNOWN` stays held until settlement or an explicit conservative expiry.

## 9. Continuation — I-5, I-9

Session expiry, context exhaustion, stream disconnect or provider outage produce a **continuation**, never a fresh identity. The `AgentRun` keeps its UID, execution identity, grant lineage, capsule digest, budget reservation and execution epoch, except that a law added or tightened since its capsule was issued gives the continuation a fresh capsule that differs from the old one only in its charter digest and entries (§5). The continuation still starts from a checkpoint whose `scope_digest` names the capsule it replaces, verified against that capsule, and its first checkpoint carries the fresh capsule's digest; the continuation is `session_sequence + 1` on the same `AgentRun` and starts only from a `VERIFIED` `AgentCheckpoint` `(session_sequence, execution_epoch, context_digest, scope_digest, budget_consumed, open_tool_invocations, progress_digest)` of the same epoch. Attempt, repair and spend counters are cumulative on the `TaskRun` and never reset. Open tool invocations listed in the checkpoint are resumed under their original identities; a continuation cannot issue a new invocation with the same request digest while the original is non-terminal. If no checkpoint can be verified before `continuation_deadline`, the `AgentRun` enters `FENCE_PENDING` and §6 applies before any replacement. A later `TaskRun` attempt of the same task (for a planning TaskRun, of the same `Plan` or `Intake`) is admitted only once its predecessor is terminal and its `fence_state` is `ACTIVE` or `FENCED`, never `FENCE_PENDING` or `FENCED_UNCERTAIN`; after a fence it is a **replacement**. Every later attempt inherits the counters and the predecessor's non-terminal operations, which keep their operation identities and become its operations (§6). It issues no invocation with the request digest of an inherited one while that one is non-terminal; an unsettled one's pair also refuses every intent on its target at 2a (§3.3). An inherited operation is sent again only under its original `operation_key`, as a §3.3 re-request or, if it was never sent, as its first send, under the new attempt's grant and an `EFFECT` reservation of the new attempt.

## 10. Lifecycles — the only place they are printed

Every state referenced in a core document appears here. Another document may name a state; it may not print a machine. An extension kind's machine is printed at the extension's gate, never in a core document. The first value listed is the initial state. A state set that actions move is a machine here even when its record is not a kind (a gate, a projection's read model); an enumerated value that no transition moves, such as a lane, a verdict, a mode, a trust label or a consequence class, is a value, not a state, and is not printed here.

```text
CommandReceipt       PREPARED → COMMITTED | REJECTED | CANCELLED | REPLAY_EXPIRED
                     PREPARED → UNCERTAIN → COMMITTED | REJECTED | CANCELLED
                     (REPLAY_EXPIRED: the command was received after its own pinned replay window; it is never evaluated,
                      never read as new intent and never rewrites a terminal receipt, and it is not reached from UNCERTAIN,
                      which may already have committed)
                     (CANCELLED: a reserved Manager command whose slot was APPLYING when its target's owning controller
                      consumed its fixed expected revision with the §4 cancel CAS, so it can never commit; the retained
                      cancellation receipt is the proof)

AdmissionStamp       ISSUED → BROKER_ACCEPTED → CONSUMED
                     ISSUED | BROKER_ACCEPTED → INVALIDATED ; ISSUED → EXPIRED
                     (BROKER_ACCEPTED → INVALIDATED only on a currency or register re-validation failure, or a blocked-target refusal at 2a, before any send_attempt)

ExternalOperation,   REQUESTED → PERMITTED → DISPATCHING → CONFIRMED | REJECTED | FAILED
ToolInvocation       PERMITTED → REQUESTED                          (permit invalidated: before acceptance, or on a currency or register re-validation failure or a blocked-target refusal at 2a, before any send)
                     DISPATCHING → OUTCOME_UNKNOWN → RECONCILING → CONFIRMED | FAILED
                     RECONCILING → REQUESTED                        (non-application proven; same key, attempt_index+1)
                     RECONCILING → UNRESOLVED → CONFIRMED | COMPENSATED | FAILED   (human adjudication only)
                     REQUESTED | PERMITTED → BLOCKED_UNSUPPORTED
                     ("accepted, not sent" is PERMITTED with a ledger entry ACCEPTED_NOT_SENT;
                      DISPATCHING always carries send_attempt)

EffectIntent         MATERIALIZED → ACKNOWLEDGED ; MATERIALIZED → QUARANTINED
                     (the Broker writes ACKNOWLEDGED once the intent's operation is terminal; an operation OUTCOME_UNKNOWN,
                      RECONCILING or UNRESOLVED keeps it MATERIALIZED; the source receipt is retained, §2, while any of its
                      intents is not ACKNOWLEDGED)
EffectReceipt        RECORDED  (immutable, one per attempt)

pending commit slot  CLEARED → OCCUPIED → CLEARED ; OCCUPIED → REPAIRING → CLEARED   (per aggregate; REPAIRING while a new process reconstructs the receipt and event)
control receipt      UNPUBLISHED → PUBLISHED   (ring entry; drained by audit publication)
reservation phase    RESERVED → APPLYING → RESOLVED   (active_manager_transaction; RESOLVED only with a non-empty terminal_state and its receipt)
                     RESERVED → RESOLVED (released as CANCELLED before any claim, §4)
                     RESOLVED → RESERVED (a new reservation overwrites a RESOLVED slot)
                     terminal_state:  NONE → COMMITTED | CANCELLED | REJECTED   (set by ReleaseManagerTransaction with its proving receipt)
                     terminal_state:  COMMITTED | CANCELLED | REJECTED → NONE (a new reservation overwrites the RESOLVED slot)
ledger entry         ACCEPTED_NOT_SENT → SEND_ATTEMPTED → ACKNOWLEDGED   (send_state; ACKNOWLEDGED is written in the CAS that removes the entry)
                     ACCEPTED_NOT_SENT → ACKNOWLEDGED                    (currency failure or blocked-target refusal at step 2, or currency or register failure at recovery row 1; no send_attempt)
expected record      PENDING → RECORDED | GAP   (TaskRun.status.expected_records.<kind>; GAP is final: a record committed after
                      the gap closes the TelemetryGap and leaves the entry GAP)

WorkContext          hold_state:  RUNNING → FREEZE_PENDING → PROPAGATING → ENFORCED → RELEASING → RUNNING
                     manager_authority[plan].phase:  ACTIVE → DRAINING → ACTIVE (new epoch)
                     plan_authority[plan].revision_phase:  ACTIVE → QUIESCING → ACTIVE (the same revision, or the replacement)
                     integration_authority[basis].state:  RESERVED → INVALIDATED
                     (hold_state and each manager_authority[plan].phase are independent of each other; AcceptDispatch checks every field)
                     (a hold requested in RELEASING keeps it there until that hold's own RESUME, because CompleteHoldRelease
                      requires hold_causes empty)
                     (a keyed entry exists from the action that creates it until its retirement, §3.1, and its absence is
                      no state: plan_authority[plan] is created ACTIVE by the first ActivatePlanRevision, and absent means no
                      active revision; integration_authority[basis] is created RESERVED by ReserveIntegrationBasis, and
                      INVALIDATED is final for that key)

Plan.phase           ACCEPTED → ACTIVATING → ACTIVE
                     ACTIVATING → ACTIVATION_FAILED → ACTIVATING (same snapshot) | CANCELLED
                     ACTIVATION_FAILED → QUIESCING (replacement revision only; the register still holds the previous revision QUIESCING)
                     ACTIVE → PAUSED → ACTIVE
                     ACTIVE | PAUSED → QUIESCING → ACTIVE (same revision) | ACTIVATING (replacement revision)
                     ACTIVE → COMPLETED
                     ACTIVE | PAUSED | QUIESCING → FAILED
                     ACCEPTED | ACTIVATING | ACTIVE | PAUSED | QUIESCING → CANCELLED
                     (RevisionPending is a condition, not a phase)
                     (ACTIVATING runs from the Plan controller's start of snapshot verification through MEMBERS_VERIFIED and
                      the submission of ActivatePlanRevision, or of SupersedePlanRevision for a replacement; ACTIVE follows
                      only its COMMITTED receipt; ACTIVATION_FAILED acknowledges PlanSnapshot ACTIVATION_FAILED and is reached
                      only before that submission or on a REJECTED receipt, never while the receipt is UNCERTAIN)
                     (FAILED and CANCELLED of a plan that holds a plan_authority entry follow its QuiescePlan and the §5 wait
                      for active attempts, during which the phase stays where it was and then moves directly to FAILED or
                      CANCELLED; no plan is CANCELLED while an activation receipt is UNCERTAIN; a plan that ended is retired
                      from the registers, §3.1)

Plan.status.         PROPOSED → VERIFIED → ACTIVE → QUIESCING → SUPERSEDED
revisions[rev]       PROPOSED | VERIFIED → ABANDONED
                     QUIESCING → ACTIVE                              (explicit resume of the same revision)

PlanSnapshot         PROPOSED → SNAPSHOT_VERIFIED → MEMBERS_VERIFIED → ACTIVATED
                     PROPOSED | SNAPSHOT_VERIFIED | MEMBERS_VERIFIED → ACTIVATION_FAILED
                     ACTIVATION_FAILED → PROPOSED (retry of the same snapshot: verification starts again)

PlanProposal         DRAFT → REVIEW → ACCEPTED | REJECTED ; DRAFT → REJECTED
                     REVIEW → DRAFT                                  (revised by the intake client)
Intake               CAPTURED → PROPOSED → ACCEPTED | REJECTED ; CAPTURED → REJECTED
                     PROPOSED → CAPTURED                             (a revision of the Intake or of a proposal it holds)
WorkBrief            RECORDED  (immutable)
Project, Repository  PROPOSED → ADOPTED → ACTIVE → RETIRED ; PROPOSED → REJECTED
Charter, ProjectCharter  ACTIVE → RETIRED
                     revisions[rev]:  PROPOSED → ACCEPTED → SUPERSEDED
                     revisions[rev]:  PROPOSED → REJECTED
                     (both kinds carry revisions[rev]; an ACCEPTED revision is immutable and digested; a SUPERSEDED revision stays pinned by every plan revision that pinned it)
ManagerLease         ACKNOWLEDGED → EXPIRED   (acknowledgement of manager_authority; never authority)
                     (the Context controller writes EXPIRED on the DrainManager control commit that drained this lease_uid,
                      whatever caused the drain, or on the COMMITTED receipt of the RetirePlanAuthority that removed its entry; an EXPIRED
                      lease never returns: AdvanceManagerEpoch installs a new lease)

Task, Milestone      PROPOSED → READY → RUNNING → VERIFYING → ACCEPTED | BLOCKED | FAILED | CANCELLED | SUPERSEDED
                     READY | RUNNING | VERIFYING → BLOCKED → READY            (blocking decision, dependency, budget, capability or evidence resolved)
                     READY | RUNNING | VERIFYING | BLOCKED → SUPERSEDED | CANCELLED
                     (VERIFYING → ACCEPTED is the acceptance adjudication the Task controller commits; it lists the UID and
                      digest of every EvidenceBundle it relies on)

TaskRun              PENDING → ADMITTED → PREPARING → EXECUTING → VERIFYING → SUCCEEDED | FAILED
                     EXECUTING | VERIFYING → RECOVERING → EXECUTING | FAILED
                     PREPARING → FAILED (setup failed, or fenced)
                     PENDING | ADMITTED | EXECUTING → FAILED (fenced)
                     any non-terminal → CANCELLED (a cancel fences first: only once fence_state is FENCED or FENCED_UNCERTAIN)
                     fence_state (control lane, from any non-terminal phase, phase unchanged):
                       ACTIVE → FENCE_PENDING → FENCED | FENCED_UNCERTAIN ; FENCED_UNCERTAIN → FENCED
                     (the TaskRun holds the fence request and the epoch: ACTIVE → FENCE_PENDING is one TaskRun control CAS that also
                      increments execution_epoch; FENCED and FENCED_UNCERTAIN acknowledge the FenceSession of this TaskRun at
                      that epoch reaching CONFIRMED or UNCERTAIN, never the reverse)
                     (once fence_state leaves ACTIVE the phase never moves to EXECUTING or SUCCEEDED; it moves to FAILED, or
                      to CANCELLED when a cancel caused the fence, and only once fence_state is FENCED or FENCED_UNCERTAIN;
                      FENCED_UNCERTAIN → FENCED may land after the phase is terminal; custody keeps the candidate, and a
                      replacement TaskRun verifies it again)

AgentRun             STARTING → RUNNING → COMPLETED | FAILED | CANCELLED
                     STARTING → FAILED | CANCELLED
                     RUNNING → HEARTBEAT_LOST → RUNNING (continuation) | fence_state := FENCE_PENDING (continuation_deadline passed; the copy acknowledging the fence requested on the TaskRun)
                     HEARTBEAT_LOST → FAILED | CANCELLED   (only once fence_state is FENCED or FENCED_UNCERTAIN)
                     (CANCELLED from any phase only once fence_state is FENCED or FENCED_UNCERTAIN: a cancel fences the TaskRun first)
                     fence_state: as TaskRun
                     (fence_state and execution_epoch are acknowledged copies of its TaskRun's and authorize nothing; a fence
                      of an AgentRun is requested on its TaskRun)

AgentCheckpoint      CREATED → VERIFIED | STALE | QUARANTINED
                     VERIFIED → STALE
                     (STALE: its execution_epoch is below its TaskRun's; QUARANTINED: out-of-scope content was detected at
                      the checkpoint, ROLES §2)
ScopeCapsule         ISSUED → REVOKED
ExecutionIdentity    ISSUED → REVOKED
CredentialGrant      ISSUED → EXPIRED | REVOKED
FenceSession         PENDING → CONFIRMED | UNCERTAIN ; UNCERTAIN → CONFIRMED
                     (the Broker's evidence of one fence of one TaskRun at one execution_epoch, created for the TaskRun's
                      FENCE_PENDING commit; CONFIRMED records FenceConfirmed, §6)

Workspace            REQUESTED → PROVISIONING → READY → IN_USE → PRESERVING → PRESERVED → RETIRED
                     PRESERVED → IN_USE (write fence lifted after a custody checkpoint; retirement needs a new PRESERVED)
                     any non-terminal → QUARANTINED | CONFLICT
                     QUARANTINED | CONFLICT → PRESERVING
                     (QUARANTINED for uncertain custody leaves once a later custody checkpoint of it is VERIFIED; QUARANTINED
                      for a scope escape or unattributed work, and CONFLICT, leave only once their WorkspaceConflict is
                      ADJUDICATED; a workspace that was ever QUARANTINED or in CONFLICT never returns to READY or IN_USE, and
                      later work restores its preserved artifact into a new Workspace)
CustodyPolicy        ACTIVE → RETIRED
CustodyCheckpoint    INVENTORIED → UPLOADING → UPLOADED → VERIFIED ; any non-terminal → FAILED
                     (UPLOADED: every Artifact is VERIFIED by digest; VERIFIED: an independent restore into a fresh location
                      is verified and restore_receipt set, and the completion marker follows)
ArtifactCommit       PENDING → VERIFIED | FAILED
                     (VERIFIED only for a CustodyCheckpoint VERIFIED with its completion marker written; the Workspace's
                      PRESERVED follows it, §7)
Artifact             PENDING → VERIFIED | EXPIRED
WorkspaceConflict    DETECTED → PRESERVING → QUARANTINED → ADJUDICATED ; PRESERVING → PRESERVED → ADJUDICATED
RestoreRequest       REQUESTED → RESTORING → READ_ONLY → FENCED → RECONCILED → MAPPED → DISPATCH_ENABLED ; any non-terminal → FAILED

IntegrationBasis     RESERVED → INTEGRATING → VERIFIED ; RESERVED | INTEGRATING | VERIFIED → STALE | BLOCKED
                     VERIFIED → RELEASED
                     (STALE and RELEASED acknowledge the register's INVALIDATED and differ by cause; the register is the authority)
                     (RELEASED: every merge operation of its merge order is terminal and InvalidateIntegrationBasis has
                      advanced the register; RetireIntegrationBasis then removes the entry, §3.1)
VerificationRun      PENDING → RUNNING → PASSED | FAILED | INCONCLUSIVE
EvidenceBundle       RECORDED → INVALIDATED | EXPIRED
                     (a bundle has no accepted state: an accepted bundle is one a committed acceptance adjudication references)

Budget               OPEN → EXHAUSTED | CLOSED
BudgetReservation    RESERVED → COMMITTED | RELEASED | UNKNOWN | EXPIRED
                     UNKNOWN → COMMITTED | RELEASED | EXPIRED         (settlement or explicit conservative expiry)
                     (UNKNOWN: its consumer, the TaskRun for ATTEMPT or the operation for EFFECT, is terminal, fenced or
                      OUTCOME_UNKNOWN and its usage is not SETTLED; COMMITTED on settlement; RELEASED on proven zero use;
                      EXPIRED only by an explicit conservative expiry, counted at the reservation ceiling)
UsageReceipt         PENDING → PARTIAL → SETTLED | DISPUTED
                     PENDING | PARTIAL → UNKNOWN → CENSORED ; CENSORED → SETTLED (append-only correction)
OutcomeRecord        PROVISIONAL → MATURE | DEFECT_CONFIRMED ; MATURE | DEFECT_CONFIRMED → REVISED
TelemetryGap         OPEN → CLOSED ; OPEN → PERMANENT
                     (CLOSED: a record of the gap's kind for its TaskRun committed after the gap, linked from it; the gap is
                      never removed, and every count uses the linked record from then on as an append-only correction, as
                      for a CENSORED receipt later SETTLED; PERMANENT: the outbox that held the record is lost, §8)

Finding              RAISED → CLASSIFIED → LINKED | PROMOTED | DEFERRED | REJECTED
Decision             RECORDED  (immutable)
Intervention         REQUESTED → ACKNOWLEDGED → APPLIED | REJECTED | EXPIRED

gate                  NOT_RUN → RUNNING → PASSED | FAILED ; PASSED → INVALIDATED
                     FAILED | INVALIDATED → RUNNING   (a new run of the gate)
                     (a gate of M0 §4, not a kind: its evidence is a signed manifest, M0 §5, and NOT_RUN is a gate with no manifest)
ProjectionState      gap:  NONE → OPEN → NONE | PERMANENT
                     integrity:  OK → DIGEST_CONFLICT
                     (a projection's read model of one aggregate, not a kind: DetectAuditGap opens a gap, the repaired
                      event closes it and DeclarePermanentGap makes it PERMANENT; RejectDigestConflict records DIGEST_CONFLICT)
```

`Dispatched`, `ReceiptObserved`, `Ambiguous`, `Reconciled`, `Unresolved` and every other name of the form *Verb-ed* are event types, never states. `RECOVERING`, `INCONCLUSIVE`, `UNRESOLVED` and `QUARANTINED` are explicit states, never generic failures. A state set printed in a schema that differs from this section is a schema defect.

## 11. Scope of guarantee

This is CRD-authoritative CQRS with durable audit, not journal-authoritative event sourcing: canonical state needs its resources, slots, rings and receipts. The barriers above cost write availability per aggregate; that cost is measured (M0) before control-plane concurrency is raised. Assumed: single-resource atomicity, trusted controllers and admission, authenticated principals, and storage within the declared custody profile. Everything outside the trust model is contained and recovered, not promised.
