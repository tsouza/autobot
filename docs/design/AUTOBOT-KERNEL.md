# AutoBot — kernel

Authority for: the authority protocol — how state commits, how effects are admitted and sent, how the Manager is serialized, how plans activate and are superseded, how attempts are identified and fenced, how work is kept and restored, how outcomes and costs are recorded, how sessions continue — and, in §10, every lifecycle state machine in AutoBot.
Depends on: `AUTOBOT-THESIS.md` (I-1 … I-9), `AUTOBOT-TRUST-MODEL.md`.

The kernel is a set of rules over Kubernetes custom resources. There is no cross-resource transaction, no lease token that authorizes anything, no controller-local mutex, and no journal that can reconstruct an aggregate. What follows is written so that each rule names the invariant it serves.

## 1. Aggregates and commit lanes — I-1

An **aggregate** is a custom resource whose `status` is the committed, canonical state of one workflow entity. Each aggregate kind has exactly one **owning controller**, which alone may commit its protected status and alone repairs receipts and audit events for commands that target it. Admission rejects any other writer. The aggregate kinds of the core are:

```text
WorkContext · Project · Repository · WorkBrief · Intake · PlanProposal
Plan · PlanSnapshot · ManagerLease · Milestone · Task · TaskRun · AgentRun · AgentCheckpoint
ScopeCapsule · ExecutionIdentity · CredentialGrant · FenceSession
Workspace · CustodyPolicy · CustodyCheckpoint · ArtifactCommit · Artifact · WorkspaceConflict · RestoreRequest
EffectIntent · ExternalOperation · ToolInvocation · EffectReceipt · AdmissionStamp
IntegrationBasis · VerificationRun · EvidenceBundle
Budget · BudgetReservation · UsageReceipt · OutcomeRecord · TelemetryGap
Finding · Decision · Intervention
```

`AutoBotCommand`, `CommandReceipt`, `AutoBotEvent`, `GraphActivationReceipt` and `RestoreWitnessReceipt` are command, receipt and audit records; they are not aggregates.

**The commit primitive** is one optimistic-concurrency write on one resource with an expected revision as precondition (CAS). Every state change is a CAS by the owning controller. Every aggregate carries `state_revision`, `control_revision` and `commit_sequence`; the last is incremented by every commit of either lane below, so any two commits on one aggregate have one order. "Before" and "after" between commits on one aggregate always means `commit_sequence` order.

**Domain lane.** A domain commit is one CAS with `state_revision` as precondition that applies the domain transition, increments `state_revision` and `commit_sequence`, and installs the aggregate's single bounded **pending commit** slot holding the exact receipt, audit envelope and effect intents (§3) of that commit. This CAS is the commit point; a new process can reconstruct the receipt and audit event from the slot. The **receipt barrier**: the next domain commit on that aggregate may not proceed until the slot's receipt and audit event have been written, read back and verified, after which a reconciliation-only CAS (below) clears the slot without changing `state_revision`. A slot is never cleared by elapsed time. A timeout at any write is `UNCERTAIN`, never rejection; it is resolved by reading the slot and the receipt.

**Control lane.** Safety controls must move while a domain slot is unresolved — a hold while the receipt store is down. Each aggregate that has a control lane declares a fixed, admission-enforced **field partition** of its status into domain fields and control fields; a CAS that touches both is rejected. A control commit is one CAS with `control_revision` as precondition that applies only control fields, increments `control_revision` and `commit_sequence`, preserves the pending slot and every domain field byte for byte, and appends a complete **control receipt** to a bounded ring embedded in the same status. Because the receipt is in the CAS, a control commit is durable and reconstructible with no second write; only its audit publication is asynchronous and drains the ring. A full ring refuses further control transitions rather than dropping a receipt; that refusal is a visible degraded condition. Domain repair verifies that the aggregate's current *domain* digest equals the slot's after-digest; control commits cannot disturb that relation.

**Reconciliation fields.** An aggregate may declare a third class of status field that records only the *progress* of an intent some earlier commit already made canonical: the pending slot's `CLEARED` mark, a control-receipt ring entry's `PUBLISHED` mark, and on `WorkContext` every `dispatch_ledger[]` entry in full — appended only by the `AcceptDispatch` domain commit that creates it and re-derivable from that commit's `COMMITTED` receipt, advanced and removed only by reconciliation-only CASes, never appended by one. Reconciliation fields are outside both digests. Either lane's CAS may write them together with its own fields (an `AcceptDispatch` domain commit appends the ledger entry it creates); a **reconciliation-only CAS** writes nothing but reconciliation fields, increments no revision and installs no receipt, because losing such a write loses nothing that §3.3 recovery cannot re-derive from receipts and operation aggregates. A reconciliation-only CAS is never a bypass: it can neither change a domain or control field nor make anything canonical.

Field classes in the core: on `WorkContext`, control fields `hold_state`, `hold_generation`, `manager_authority[*].phase`, `dispatch_authority_generation`, and reconciliation fields `pending_commit.state`, `control_receipt_ring[*].state` and every `dispatch_ledger[]` entry in full; on `TaskRun` and `AgentRun`, control fields `fence_state`, `execution_epoch`, `revocation_generation`, and reconciliation fields `pending_commit.state`, `control_receipt_ring[*].state`; on every other aggregate, reconciliation fields `pending_commit.state` and, where a ring exists, `control_receipt_ring[*].state`. Everything else in every status is domain. The pending slot body and the control-receipt ring entries are structural: they belong to no class and are outside both digests — the domain digest covers domain fields only, the control digest control fields only.

**Audit.** After a commit, the owning controller publishes an immutable `AutoBotEvent` carrying `(aggregate_uid, commit_sequence, lane, state_revision, control_revision, event_digest, …)`; a missing event is repaired from the slot or ring. A projection applies events to a read model strictly in `(aggregate_uid, commit_sequence)` order, holds a later event in a bounded buffer while a gap is visible, and rejects and quarantines a same-identity event with a different digest. Audit and projections explain and repair; they never roll back, overwrite or reconstruct an aggregate.

## 2. Commands and receipts — I-1

An `AutoBotCommand` is an authenticated, immutable, single-target mutation request. The principal is the authenticated writer, bound at admission; an actor string in the payload is ignored. A command pins its target UID and expected revision (or, for a create, kind, parent and client request key), its input digest, its replay window and, where relevant, its policy and Manager pins. Multi-target work is a recoverable sequence of commands, never a claimed transaction.

The **replay identity** is `(idempotency_key, principal, input_digest)`. Within the replay window the same identity returns the original receipt and never a second commit; the same key with a different payload or principal is rejected; after the window a retry is `REPLAY_EXPIRED` and is never read as new intent. A retry is never rebased onto a newer revision.

A `CommandReceipt` has a deterministic name (it is the idempotency index), immutable prepared input, and a terminal result owned by the target kind's controller: `PREPARED` is not evidence of commitment; `COMMITTED` and `REJECTED` cannot be rewritten; `UNCERTAIN` is an observation to be reconciled to a terminal state. Rejection requires proof that the exact expected revision can no longer commit *and* that no matching committed slot or receipt exists; a CAS conflict alone proves nothing about an earlier transmission of the same command.

**Creates.** A create command is indexed by context, principal, kind, parent and client request key, with the input digest compared but not incorporated, so a changed payload conflicts instead of creating a second object. The receipt reserves a deterministic name; the atomic create writes immutable origin metadata `(create_receipt_uid, input_digest, context_uid)` beside the spec; the target controller initializes status only after validating it. A lost acknowledgement is resolved by GET of that name, never by a generated name. A target cannot be deleted or recreated before its create receipt is terminal, and a name's absence after an ambiguous delete or restore is never authorization to recreate it.

Receipts, slots, rings and tombstones are retained through the replay window and for as long as any effect of the command is pending, ambiguous or unresolved.

## 3. Dispatch: registers, acceptance, effects and the outbox — I-2, I-4

### 3.1 The dispatch registers

`WorkContext.status` holds the **dispatch registers**, the only authority any dispatch acceptance is checked against:

```text
hold_state                        RUNNING | FREEZE_PENDING | PROPAGATING | ENFORCED | RELEASING
hold_generation                   +1 on every hold_state transition
admission_sequence                +1 on every AcceptDispatch
manager_authority[plan_uid]       (lease_uid, epoch, holder, deadline, phase ∈ {ACTIVE, DRAINING})
plan_authority[plan_uid]          (active_revision, snapshot_digest, activation_receipt_uid,
                                   revision_phase ∈ {ACTIVE, QUIESCING}, plan_generation)
integration_authority[basis_uid]  (basis_generation, state ∈ {RESERVED, INVALIDATED})
dispatch_authority_generation     witness generation of this installation (§7)
dispatch_ledger[]                 bounded accepted-dispatch outbox (§3.3)
active_manager_transaction        one bounded reservation slot (§4)
```

Routing is deliberately **not** a register. A TaskRun's **routing pin** — the identifier of the worker-strategy configuration it was admitted with — is written into its spec at `AdmitTask` and never changes for the life of the TaskRun; a permit carries that pin and acceptance checks only that the two are equal. Changing what future admissions pin needs no linearization with dispatch.

An **authority cut** is any register change. Every cut and every acceptance is a CAS on `WorkContext` applied by the **Context controller**, the sole owner of `WorkContext` status. A controller that needs a cut — the broker for `AcceptDispatch`, the Plan controller for `ActivatePlanRevision` / `QuiescePlan` / `ResumePlanRevision` / `SupersedePlanRevision`, the Integration controller for `ReserveIntegrationBasis` / `InvalidateIntegrationBasis`, the Custody controller for `AdvanceDispatchAuthorityGeneration` — submits a command targeting the `WorkContext` under its own principal and proceeds only on the `COMMITTED` receipt. Wherever this document says "the broker's `AcceptDispatch`", "the ledger entry to `SEND_ATTEMPTED`" or `AcknowledgeDispatch` it means that command path: the broker submits `AcceptDispatch` as a command, and the two ledger updates as **reconciliation requests**, which are not `AutoBotCommand`s: each names the entry by `(operation_uid, permit_uid)` and the `send_state` it expects to find, pins no revision and produces no `CommandReceipt`, and is a no-op when the entry is absent or already past that `send_state`, so a delayed request can neither advance a later entry nor remove one; the Context controller applies `AcceptDispatch` as a domain commit and the two ledger updates as reconciliation-only CASes; and the broker proceeds only after its request has landed — the `COMMITTED` receipt for `AcceptDispatch`, a linearizable read of the updated ledger entry for the other two. Only the Context controller writes the ledger, and only the Context controller writes `AdmissionStamp` status, so the `BROKER_ACCEPTED` acknowledgement is likewise a broker command it applies.

| Register change | Action | Lane |
|---|---|---|
| Hold | `RequestHold` (`RUNNING → FREEZE_PENDING`, the cut), `PropagateHold`, `EnforceHold`, `ReleaseHold` | control |
| Manager | `DrainManager` (phase := `DRAINING`), then `AdvanceManagerEpoch` (holder, lease, epoch), then `ResumeManager` (phase := `ACTIVE`) | control, domain, control — in that order (§4) |
| Plan | `ActivatePlanRevision`, `QuiescePlan`, `ResumePlanRevision`, `SupersedePlanRevision` — each increments `plan_generation` | domain |
| Basis | `ReserveIntegrationBasis`, `InvalidateIntegrationBasis` — each increments `basis_generation` | domain |
| Restore | `AdvanceDispatchAuthorityGeneration` | control |
| Acceptance | `AcceptDispatch` | domain |

Because every one of these is a CAS on the same aggregate, any two are totally ordered by `commit_sequence`. "Accepted before the cut" has exactly one meaning: `commit_sequence(AcceptDispatch) < commit_sequence(cut)`. `Plan`, `IntegrationBasis` and `ManagerLease` keep their own detail and status, but their dispatch-relevant fact is *acknowledged from* the register, never the reverse: a `Plan` whose status says revision R2 while the register says R1 is stale and admits nothing.

### 3.2 Permits and `AcceptDispatch`

An `AdmissionStamp` (**permit**) is a capability to *request* acceptance, nothing more. It is issued only while `hold_state = RUNNING` and `plan_authority[plan].revision_phase = ACTIVE`, and pins every register value it will be checked against, plus the identities of its budget reservation, credential grant and scope capsule digest, its effect digest, a nonce and an expiry.

`AcceptDispatch(permit)` is one `WorkContext` domain commit whose preconditions are all evaluated against the registers in that same CAS:

```text
permit.state = ISSUED ∧ now < permit.expires_at
hold_state = RUNNING ∧ permit.hold_generation = hold_generation
manager_authority[permit.plan_uid] = (permit.lease_uid, permit.epoch, _, _, ACTIVE)
plan_authority[permit.plan_uid] = (permit.plan_revision, _, _, ACTIVE, permit.plan_generation)
permit.basis_uid = ∅ ∨ integration_authority[permit.basis_uid] = (permit.basis_generation, RESERVED)
dispatch_authority_generation = permit.dispatch_authority_generation
permit.routing_pin = the pin in the permit's TaskRun spec        (not a register; a pin)
|dispatch_ledger| < capacity
```

Effect: `admission_sequence += 1`; the permit's acceptance sequence and generation are stamped; a ledger entry is appended. A failed precondition invalidates the permit and returns the operation to `REQUESTED` to seek a new permit under the new registers. Afterwards, as acknowledgements, the `AdmissionStamp` records `BROKER_ACCEPTED` and the `ExternalOperation` — still `PERMITTED` — records the acceptance sequence and generation; the ledger entry, created by that commit and carried in its `COMMITTED` receipt, is the authority.

The consequences that follow from ordering alone:

- **Hold.** A permit accepted at `s` and `RequestHold` at `c`: `s < c` means pre-cut and may complete; `s > c` fails on `hold_generation`. There is no third case. `ReleaseHold` and the return to `RUNNING` each increment `hold_generation` again, so no permit issued under a previous `RUNNING` survives a hold.
- **Plan.** No permit pinning R1 is accepted after `ActivatePlanRevision(R2)` or `QuiescePlan(R1)`; and because `QuiescePlan` and `ResumePlanRevision` each increment `plan_generation`, a permit issued before a quiesce fails after a resume *of the same revision* even if its own `INVALIDATED` write was never applied.
- **Basis.** A merge permit pinning generation g is not accepted after `InvalidateIntegrationBasis` advanced it.
- **Manager.** Acceptance under a drained or advanced epoch fails at the register.

Grant, reservation and capsule **currency** is not a register and is not read by the acceptance CAS. The broker enforces it before `RecordSendAttempt` (§3.3) and on every privileged call: revocation generation, execution epoch and expiry of the grant, `RESERVED` state of the reservation, and the capsule digest. A mismatch invalidates the permit (`BROKER_ACCEPTED → INVALIDATED`), returns the operation to `REQUESTED` without a send, and releases the ledger entry (§3.3 step 5); a register re-validation failure at `RecoverAcceptedDispatch` row 1 ends the same way. A fence that lands between acceptance and send is therefore caught at the send.

### 3.3 Effect intents, the dispatch ledger and the send-attempt outbox

A domain commit's pending slot carries an ordered, bounded set of immutable **effect intents**, each `(operation_key, effect_index, payload_digest, provider_binding, desired_outcome, target_identity, contract_revision)`, with

```text
operation_key = hash(installation_lineage, aggregate_uid, committed_revision, effect_index, payload_digest)
```

— the logical identity of one intended effect across retries and restores. The committed receipt carries the intents before the slot can clear; `EffectIntent` and `ExternalOperation` resources are **materialized** at deterministic names from committed receipts by a dispatcher that scans slots and retained unacknowledged receipts, not only watches. A name collision with a different payload digest, target identity or contract revision is `EffectIntentCollision`: quarantined, never dispatched. Dispatch requires a verified committed receipt, the exact intent digest, current acceptance (§3.2) and a provider capability the adapter has qualified for that operation; a prepared receipt, an existing child resource or a missing parent never authorizes it.

Every effectful agent tool call is a `ToolInvocation`: it first obtains a durable identity and request digest, then follows this same protocol; its `EffectReceipt` records the outcome or `OUTCOME_UNKNOWN`. Stream closure never proves a tool failed. Read-only calls need none of this.

The **dispatch ledger** entry is `(operation_uid, operation_key, permit_uid, acceptance_sequence, acceptance_generation, accepted_at, send_state ∈ {ACCEPTED_NOT_SENT, SEND_ATTEMPTED, ACKNOWLEDGED})`. `ACKNOWLEDGED` is written in the same CAS that removes the entry; it is never retained, never observable and never counts toward capacity. The broker's send sequence is:

1. `AcceptDispatch` → entry `ACCEPTED_NOT_SENT`.
2. `RecordSendAttempt`. Before it, the broker validates currency (§3.2) and the dependents rule (below); a failure here returns the operation to `REQUESTED` with no ledger change past `ACCEPTED_NOT_SENT`. Then two CASes in this fixed order: (2a) the ledger entry to `SEND_ATTEMPTED` (a reconciliation-only `WorkContext` CAS applied by the Context controller on the broker's reconciliation request, §3.1); then (2b) the operation to `DISPATCHING` with `send_attempt = (attempt_index, started_at, acceptance_sequence)`, which is also the moment the permit becomes `CONSUMED`. **Both precede opening the remote connection.** A present `send_attempt` therefore always implies a `SEND_ATTEMPTED` entry.
3. Send.
4. Observe; CAS the operation to `CONFIRMED` / `REJECTED` / `FAILED`, or to `OUTCOME_UNKNOWN` on timeout or disconnect.
5. `AcknowledgeDispatch`: reconciliation-only CAS removing the entry, only after the operation has durably recorded its acceptance and either a terminal or `OUTCOME_UNKNOWN` state, or `REQUESTED` following a currency failure at step 2, or a register or currency re-validation failure at `RecoverAcceptedDispatch` row 1, in either case with no `send_attempt` present; in that second case the same reconciliation removes the entry first and then sets the permit `INVALIDATED`, and a permit found `BROKER_ACCEPTED` with no entry and its operation `REQUESTED` is read as `INVALIDATED` on recovery. An entry is never removed while `send_attempt` is present and no outcome is recorded.

`RecoverAcceptedDispatch` runs on broker restart and joins the ledger with the operations:

| Ledger `send_state` | Operation `send_attempt` | Conclusion | Action |
|---|---|---|---|
| `ACCEPTED_NOT_SENT` | absent | never sent | re-validate registers and currency; if still current continue at step 2, else invalidate and return to `REQUESTED` |
| `SEND_ATTEMPTED` | present, no outcome | sent or unknown | `OUTCOME_UNKNOWN → RECONCILING`; no resend |
| any | terminal outcome | done | acknowledge |
| `SEND_ATTEMPTED` | absent (2b uncertain) | unknown | as row 2 |
| `ACCEPTED_NOT_SENT` | present | impossible under 2a-before-2b | `DispatchLedgerConflict`: quarantine, no send, human adjudication |
| no entry | present, no outcome | acknowledged early or lost | as row 2 |

**Reconciliation.** An operation in `RECONCILING` may be re-requested (same `operation_key`, `attempt_index + 1`) only when the adapter proves definitive non-application by provider lookup or provider deduplication. A negative search proves nothing. When the provider's declared capability offers neither idempotency nor lookup, the operation becomes `UNRESOLVED` after `provider_reconcile_bound`: retention pinned, its **dependents** blocked — every later effect intent with the same `target_identity`, every `IntegrationBasis` whose source or base heads the operation could have changed, and every acceptance evaluation of the milestone that owns the originating command — plan completion blocked, an `Intervention` for human adjudication created, and only that adjudication can move it to `CONFIRMED`, `COMPENSATED` or `FAILED`. A dependent is refused at the check that would otherwise admit it: the broker's check before `RecordSendAttempt` refuses an effect intent whose `target_identity` equals that of an `UNRESOLVED` operation and returns it to `REQUESTED` as a currency failure; the Integration controller refuses `ReserveIntegrationBasis` for a basis whose source or base heads the operation could have changed; `RecordAcceptanceAdjudication` refuses the owning milestone. "Later" means an intent not yet `SEND_ATTEMPTED` when the operation's `UNRESOLVED` CAS committed. `UNRESOLVED` is never garbage-collected and never counted as success or failure. A provider whose declared capability lacks a required semantic yields `BLOCKED_UNSUPPORTED` before any send.

## 4. Manager serialization — I-2

`manager_authority[plan_uid]` on the `WorkContext` is the Manager's authority: `(lease_uid, epoch, holder, deadline, phase)`. `ManagerLease` and `Plan` status expose acknowledged copies and authorize nothing.

A Manager command pins plan UID and revision, holder, lease UID and epoch, exact target UID and revision, and input digest. The Context controller CASes the `WorkContext` to check holder, epoch, `phase = ACTIVE` and deadline and to reserve the single `active_manager_transaction` slot: `(target_uid, expected_revision, command_uid, phase ∈ {RESERVED, APPLYING, RESOLVED}, target_receipt_uid, cancellation_receipt_uid, terminal_state)`. The slot serializes short control mutations, not reasoning, builds or agent execution; it is released only by a CAS that records a non-empty `terminal_state` together with the receipt that proves it. Only the target's owning controller applies the reserved command, under §1 and with the command's **fixed expected revision**, which is never refreshed to force an old command through.

**Takeover drains before the epoch advances**, in three steps that are never merged:

1. `DrainManager` — control CAS, `phase := DRAINING`. New reservations by the old holder fail; the existing one is preserved. Holds may still be requested.
2. Resolve the reserved command to a durable terminal receipt, or cancel it with a CAS on the target that consumes the reserved expected revision and retains a cancellation receipt, so a delayed old CAS fails. An uncertain result cannot release the slot; takeover waits. Uncertain creates are drained to a receipt or a retained inert target, never abandoned.
3. `AdvanceManagerEpoch` — domain CAS with preconditions `phase = DRAINING` and slot empty; sets holder, lease and `epoch := e+1` while phase stays `DRAINING`. Then `ResumeManager` — control CAS with precondition `epoch = e+1`; sets `phase := ACTIVE`. Between the two no permit is accepted (`phase ≠ ACTIVE`); after the second every permit pinning epoch e fails. A single CAS touching both phase and epoch is rejected by the field partition and is never issued.

Lease expiry begins draining; it is never permission to skip it. Draining is per plan and independent of the context hold: a context may be `RUNNING` with one plan `DRAINING`, or `ENFORCED` with all plans `ACTIVE`; acceptance checks both.

## 5. Plan activation and supersession — I-2, I-6

Accepting a `PlanProposal` creates the `Plan` in `ACCEPTED`: the contract exists, nothing can run. A **plan revision** is one immutable contract version; its graph is an immutable `PlanSnapshot` — members with UID and revision, edges, acceptance and budget policy, digests. Activation is owned by the Plan controller: verify the snapshot digest, create or verify every member against it (`MEMBERS_VERIFIED`), then submit `ActivatePlanRevision`, which the Context controller applies as the register CAS `plan_authority[plan] := (revision, snapshot_digest, activation_receipt_uid, ACTIVE, plan_generation + 1)` — the **activation cut** — and on its receipt CAS `PlanSnapshot` to `ACTIVATED` and `Plan` to `ACTIVE` as acknowledgements, recording the `GraphActivationReceipt`. A partial graph cannot produce ready work: every readiness and admission decision verifies the active revision, snapshot digest and activation receipt in the same decision, and every acceptance verifies the register.

Only an authenticated Manager holder or a human named by `WorkContext.spec.revisionAuthority` may propose a revision; the proposal is the Plan condition `RevisionPending`, not a phase. **Supersession** is: `QuiescePlan` (register `revision_phase := QUIESCING`, `plan_generation + 1`; every new acceptance for the plan fails, every issued permit is invalidated), wait for active attempts to reach a terminal, fenced or unknown/unresolved boundary, record unresolved operations, invalidate acceptance evidence, then `ActivatePlanRevision(R2)` on the same register. That no R1 effect can be accepted after R2 activation is an ordering fact, not a policy. Resuming the same revision is `ResumePlanRevision` (`revision_phase := ACTIVE`, `plan_generation + 1`). `PAUSED` is a plan-level block on new TaskRun admission requested through an `Intervention`; it revokes no accepted effect and never weakens a context hold.

An active attempt keeps the revision and capsule it started with until it is terminal or fenced. There is no in-place revision of a task, a capsule or an acceptance contract; any change to what a task must do is a new revision through this section or a new task.

**Evidence and integration basis.** Every acceptance decision uses an `EvidenceBundle` binding candidate, base and head digests, plan and scope revisions, criteria, environment and toolchain digests, provider runs, CI and review attestations, remote generation and expiry; any change to any of these invalidates it. Overlapping change sets are serialized through an `IntegrationBasis` — base head, source heads, overlap set, merge order, integrated candidate — whose `basis_generation` lives in the register, so a merge permit is invalidated at the same linearization point as holds when any reserved head, base, protection digest or earlier merge changes. Milestone acceptance is evaluated against the final integrated candidate, never against individually passing branches.

## 6. Identity, grants and fencing — I-2, I-5

Every TaskRun receives an `ExecutionIdentity` `(task_run, workspace, agent_run, execution_epoch, installation_id, audience)` and a short-lived `CredentialGrant` bound to that identity, its audience, repositories, paths, operations, installation lineage, execution epoch, expiry and revocation generation. Agents never hold a reusable credential. The broker validates namespace, WorkContext UID, target UID, audience, repository, path, operation, lineage, epoch, revocation generation and expiry on every privileged call; a grant that fails any of these, or references another context, is refused.

`execution_epoch` is a per-TaskRun counter advanced on fencing and part of every identity, grant and checkpoint. A logical epoch alone is not fencing. The **fence protocol** — `fence_state: ACTIVE → FENCE_PENDING → FENCED | FENCED_UNCERTAIN`, on the control lane, independent of the run's phase — is: increment the epoch; revoke the grant; revoke broker capability; stop or isolate the process; remove or quarantine workspace write access; verify broker and workspace fences; record `FenceConfirmed`, which additionally requires the broker to have refused or reconciled every ledger entry of the fenced TaskRun. If confirmation cannot be obtained the state is `FENCED_UNCERTAIN`: replacement, cleanup and privileged effects are blocked until a late confirmation arrives. A mounted volume or a changed epoch alone is never evidence of fencing.

## 7. Custody and restore — I-3, I-2

A `Workspace` is custody of a checkout and outlives sessions and attempts. It is never retired on the strength of a clean git status or a merged PR. A **custody checkpoint** is: confirm the workspace write fence; inventory tracked, staged, untracked, ignored and unpushed content, local refs, stashes and the record outbox (§8) as `(canonical_path, inode, digest)`; write a manifest; upload; verify the digest *and an independent restore into a fresh location*; write a completion marker; record `ArtifactCommit(VERIFIED)`; only then mark the workspace `PRESERVED`. A lost upload acknowledgement stays uncertain until verified by lookup or restore. Unknown custody is quarantine.

Inventory that shows content from more than one TaskRun, project or unowned local work creates a `WorkspaceConflict` with owner candidates, a per-file attribution manifest, preservation digests and a quarantine owner. Nothing under an unresolved conflict is deleted or assigned to a task until a human adjudicates it; attribution uncertainty survives restore.

**Restore.** A restored installation has a new `installation_id` and `restore_generation` and stays read-only until four conditions hold: `OldInstallationFenced`, `OutstandingOperationsReconciled`, `RestoreIdentityMapped`, `RestoreDispatchEnabled`. Dispatch authority is held by the external **authority witness**; the installation cannot self-assert that its predecessor is fenced. Re-enabling dispatch requires a witness-signed `RestoreWitnessReceipt` `(installation_id, restore_generation, witness_generation, old_installation_fence_evidence, ambiguous_operation_set_digest, identity_mapping_digest, old_grant_expiry, signature)`, reconciliation or `UNRESOLVED` adjudication of every operation in the ambiguous set, and either proof of active revocation of every old grant or a wait through `max_old_grant_ttl + broker_revocation_bound`. The receipt's `witness_generation` becomes `dispatch_authority_generation` through `AdvanceDispatchAuthorityGeneration`, so restore participates in the same linearization as holds. Old external-operation identities (`operation_key`) are preserved across restore; old pending actions are never replayed merely because a recovered resource lists them. Witness or broker uncertainty fails closed.

## 8. The canonical-record obligation — I-9

`OutcomeRecord`, `UsageReceipt` and `TelemetryGap` are aggregates created through the create protocol of §2, not telemetry. Their durability under API unavailability is an obligation on the `TaskRun`:

1. At admission, `TaskRun.status.expected_records = {outcome: PENDING, usage: PENDING}` and a `record_deadline` are set.
2. The producing process — worker runtime adapter, broker or verifier — writes each record first to its **durable local outbox** (inside the checkpointed workspace area, or the broker's persistent queue) with its deterministic name, input digest and create key, *before* reporting completion.
3. The outbox drains through create commands with retry state on the entry; retries after a lost acknowledgement resolve to the same record.
4. `expected_records.<kind> := RECORDED` only on that record's `COMMITTED` create receipt.
5. A TaskRun terminal with an entry still `PENDING` at `record_deadline` receives a `TelemetryGap` (`gap_kind ∈ {OUTCOME_MISSING, USAGE_MISSING}`) linked to it, and `expected_records.<kind> := GAP(uid)`. A gap is a canonical record: whatever counts outcomes or costs counts it, as unknown outcome and censored cost, never as absent.
6. Outbox entries are inventoried and preserved by custody checkpoints; a workspace lost before its outbox drained yields a gap, never a silently missing record.

A `UsageReceipt` not settled by its settlement deadline becomes `CENSORED` at `min(reservation ceiling, rate-card bound)`, counts at that bound for cost and as non-success for quality, and is corrected only by append-only later settlement. A `BudgetReservation` in `UNKNOWN` stays held until settlement or an explicit conservative expiry.

## 9. Continuation — I-5, I-9

Session expiry, context exhaustion, stream disconnect or provider outage produce a **continuation**, never a fresh identity. The `AgentRun` keeps its UID, execution identity, grant lineage, capsule digest, budget reservation and execution epoch; the continuation is `session_sequence + 1` on the same `AgentRun` and starts only from a `VERIFIED` `AgentCheckpoint` `(session_sequence, execution_epoch, context_digest, scope_digest, budget_consumed, open_tool_invocations, progress_digest)` of the same epoch. Attempt, repair and spend counters are cumulative on the `TaskRun` and never reset. Open tool invocations listed in the checkpoint are resumed under their original identities; a continuation cannot issue a new invocation with the same request digest while the original is non-terminal. If no checkpoint can be verified before `continuation_deadline`, the `AgentRun` enters `FENCE_PENDING` and §6 applies before any replacement; a replacement is a new `TaskRun` attempt that inherits the counters.

## 10. Lifecycles — the only place they are printed

Every state referenced in a core document appears here. Another document may name a state; it may not print a machine. An extension kind's machine is printed at the extension's gate, never in a core document. The first value listed is the initial state.

```text
CommandReceipt       PREPARED → COMMITTED | REJECTED | CANCELLED | REPLAY_EXPIRED
                     PREPARED → UNCERTAIN → COMMITTED | REJECTED | CANCELLED

AdmissionStamp       ISSUED → BROKER_ACCEPTED → CONSUMED
                     ISSUED | BROKER_ACCEPTED → INVALIDATED ; ISSUED → EXPIRED
                     (BROKER_ACCEPTED → INVALIDATED only on a currency or register re-validation failure before any send_attempt)

ExternalOperation,   REQUESTED → PERMITTED → DISPATCHING → CONFIRMED | REJECTED | FAILED
ToolInvocation       PERMITTED → REQUESTED                          (permit invalidated: before acceptance, or on a currency or register re-validation failure before any send)
                     DISPATCHING → OUTCOME_UNKNOWN → RECONCILING → CONFIRMED | COMPENSATED | FAILED
                     RECONCILING → REQUESTED                        (non-application proven; same key, attempt_index+1)
                     RECONCILING → UNRESOLVED → CONFIRMED | COMPENSATED | FAILED   (human adjudication only)
                     REQUESTED | PERMITTED → BLOCKED_UNSUPPORTED
                     ("accepted, not sent" is PERMITTED with a ledger entry ACCEPTED_NOT_SENT;
                      DISPATCHING always carries send_attempt)

EffectIntent         MATERIALIZED → ACKNOWLEDGED ; MATERIALIZED → QUARANTINED
EffectReceipt        RECORDED  (immutable, one per attempt)

pending commit slot  CLEARED → OCCUPIED → CLEARED ; OCCUPIED → REPAIRING → CLEARED   (per aggregate; REPAIRING while a new process reconstructs the receipt and event)
control receipt      UNPUBLISHED → PUBLISHED   (ring entry; drained by audit publication)
reservation phase    RESERVED → APPLYING → RESOLVED   (active_manager_transaction; RESOLVED only with a non-empty terminal_state and its receipt)
ledger entry         ACCEPTED_NOT_SENT → SEND_ATTEMPTED → ACKNOWLEDGED   (send_state; ACKNOWLEDGED is written in the CAS that removes the entry)
                     ACCEPTED_NOT_SENT → ACKNOWLEDGED                    (currency failure at step 2, or currency or register failure at recovery row 1; no send_attempt)
expected record      PENDING → RECORDED | GAP   (TaskRun.status.expected_records.<kind>)

WorkContext          hold_state:  RUNNING → FREEZE_PENDING → PROPAGATING → ENFORCED → RELEASING → RUNNING
                     manager_authority[plan].phase:  ACTIVE → DRAINING → ACTIVE (new epoch)
                     (the two fields are independent; both are checked by AcceptDispatch)

Plan.phase           ACCEPTED → ACTIVATING → ACTIVE
                     ACTIVATING → ACTIVATION_FAILED → ACTIVATING (same snapshot) | CANCELLED
                     ACTIVE → PAUSED → ACTIVE
                     ACTIVE | PAUSED → QUIESCING → ACTIVE (same revision) | ACTIVATING (replacement revision)
                     ACTIVE → COMPLETED | FAILED
                     ACCEPTED | ACTIVATING | ACTIVE | PAUSED | QUIESCING → CANCELLED
                     (RevisionPending is a condition, not a phase)

Plan.status.         PROPOSED → VERIFIED → ACTIVE → QUIESCING → SUPERSEDED
revisions[rev]       PROPOSED | VERIFIED → ABANDONED
                     QUIESCING → ACTIVE                              (explicit resume of the same revision)

PlanSnapshot         PROPOSED → SNAPSHOT_VERIFIED → MEMBERS_VERIFIED → ACTIVATED
                     PROPOSED | SNAPSHOT_VERIFIED | MEMBERS_VERIFIED → ACTIVATION_FAILED

PlanProposal         DRAFT → REVIEW → ACCEPTED | REJECTED
Intake               CAPTURED → ANALYZING → NEEDS_INPUT ↔ ANALYZING ; ANALYZING → PROPOSED → ACCEPTED | REJECTED
WorkBrief            RECORDED  (immutable)
Project, Repository  PROPOSED → ADOPTED → ACTIVE → RETIRED
ManagerLease         ACKNOWLEDGED → EXPIRED   (acknowledgement of manager_authority; never authority)

Task, Milestone      PROPOSED → READY → RUNNING → VERIFYING → ACCEPTED | BLOCKED | FAILED | CANCELLED | SUPERSEDED
                     READY | RUNNING | VERIFYING → BLOCKED → READY            (blocking decision, dependency, budget, capability or evidence resolved)
                     READY | RUNNING | VERIFYING | BLOCKED → SUPERSEDED | CANCELLED

TaskRun              PENDING → ADMITTED → PREPARING → EXECUTING → VERIFYING → SUCCEEDED | FAILED
                     EXECUTING | VERIFYING → RECOVERING → EXECUTING | FAILED
                     any non-terminal → CANCELLED
                     fence_state (control lane, from any non-terminal phase, phase unchanged):
                       ACTIVE → FENCE_PENDING → FENCED | FENCED_UNCERTAIN ; FENCED_UNCERTAIN → FENCED

AgentRun             STARTING → RUNNING → COMPLETED | FAILED | CANCELLED
                     RUNNING → HEARTBEAT_LOST → RUNNING (continuation) | fence_state := FENCE_PENDING
                     fence_state: as TaskRun

AgentCheckpoint      CREATED → VERIFIED | STALE | QUARANTINED
ScopeCapsule         ISSUED → REVOKED
ExecutionIdentity    ISSUED → REVOKED
CredentialGrant      ISSUED → EXPIRED | REVOKED
FenceSession         PENDING → CONFIRMED | UNCERTAIN ; UNCERTAIN → CONFIRMED

Workspace            REQUESTED → PROVISIONING → READY → IN_USE → PRESERVING → PRESERVED → RETIRED
                     any → QUARANTINED | CONFLICT
CustodyPolicy        ACTIVE → RETIRED
CustodyCheckpoint    INVENTORIED → UPLOADING → UPLOADED → VERIFIED ; any → FAILED
ArtifactCommit       PENDING → VERIFIED | FAILED
Artifact             PENDING → VERIFIED | EXPIRED
WorkspaceConflict    DETECTED → PRESERVING → QUARANTINED → ADJUDICATED ; PRESERVING → PRESERVED → ADJUDICATED
RestoreRequest       REQUESTED → RESTORING → READ_ONLY → FENCED → RECONCILED → MAPPED → DISPATCH_ENABLED ; any → FAILED

IntegrationBasis     RESERVED → INTEGRATING → VERIFIED ; RESERVED | INTEGRATING → STALE | BLOCKED
                     (STALE acknowledges the register's INVALIDATED; the register is the authority)
VerificationRun      PENDING → RUNNING → PASSED | FAILED | INCONCLUSIVE
EvidenceBundle       RECORDED → INVALIDATED | EXPIRED

Budget               OPEN → EXHAUSTED | CLOSED
BudgetReservation    RESERVED → COMMITTED | RELEASED | UNKNOWN | EXPIRED
                     UNKNOWN → COMMITTED | RELEASED | EXPIRED         (settlement or explicit conservative expiry)
UsageReceipt         PENDING → PARTIAL → SETTLED | DISPUTED
                     PENDING | PARTIAL → UNKNOWN → CENSORED ; CENSORED → SETTLED (append-only correction)
OutcomeRecord        PROVISIONAL → MATURE | DEFECT_CONFIRMED ; MATURE | DEFECT_CONFIRMED → REVISED
TelemetryGap         OPEN → CLOSED ; OPEN → PERMANENT

Finding              RAISED → CLASSIFIED → LINKED | PROMOTED | DEFERRED | REJECTED
Decision             RECORDED  (immutable)
Intervention         REQUESTED → ACKNOWLEDGED → APPLIED | REJECTED | EXPIRED
```

`Dispatched`, `ReceiptObserved`, `Ambiguous`, `Reconciled`, `Unresolved` and every other name of the form *Verb-ed* are event types, never states. `RECOVERING`, `INCONCLUSIVE`, `UNRESOLVED` and `QUARANTINED` are explicit states, never generic failures. A state set printed in a schema that differs from this section is a schema defect.

## 11. Scope of guarantee

This is CRD-authoritative CQRS with durable audit, not journal-authoritative event sourcing: canonical state needs its resources, slots, rings and receipts. The barriers above cost write availability per aggregate; that cost is measured (M0) before control-plane concurrency is raised. Assumed: single-resource atomicity, trusted controllers and admission, authenticated principals, and storage within the declared custody profile. Everything outside the trust model is contained and recovered, not promised.
