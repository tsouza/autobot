# AutoBot — formal surface

Authority for: the typed records, actions, safety invariants, negative variants and conditional-liveness assumptions of the **kernel only**; the planned Quint → Apalache → TLC → Rust-refinement stack; the review classification.
Depends on: `AUTOBOT-THESIS.md` (I-1 … I-10), `AUTOBOT-KERNEL.md` (every section; §10 is the state-domain authority for every `state` field below).

Absence of a compiled model, a check, or a refinement test is expected in the design phase and is never a design gap.

## 1. Shape

```tla
Spec == Init /\ [][Next]_vars /\ Liveness
```

`Init` is a finite valid kernel state. `Next` is the disjunction of the actions in §3, of external observations, timers, faults and recovery. Safety properties are invariants (§4). Liveness is conditional on the named assumptions of §6 and is never promised unconditionally.

The model represents bounded abstract values, hashes and finite sets; it does not model source trees, unbounded logs, tokens or CI output. `CommitAggregateCAS` and `CommitControlCAS` are the only transitions that commit canonical state; `ReconciliationCAS` records progress of already-committed intents and never changes a digest. Audit events, effect materialization and projections are modelled so that they can explain or repair a missing record and never roll back, overwrite or reconstruct a newer aggregate. Extensions (learning, decision policy, mirroring, observability, CLI, the onboarding MCP server) are outside this model; a claim about them is not a claim of this surface.

## 2. Typed correspondence

The first compilable model represents these records with these fields. Each field is the same field as in the M0 schema (`AUTOBOT-M0-AND-GATES.md`); the refinement mapping is field by field, and a kernel field missing from the model is a model defect. Every `state` domain is exactly its KERNEL §10 machine.

```text
\* commit, receipt, audit                                         KERNEL §1–§2
Aggregate            = [uid, kind, context_uid, domain_digest, control_digest,
                        state_revision, control_revision, commit_sequence,
                        pending_commit, control_receipt_ring]
CommandReceipt       = [uid, command_uid, idempotency_key, target_kind, target_uid, parent_context_uid,
                        principal, expected_revision, proposed_revision, commit_sequence,
                        input_digest, origin_metadata, issue_time, expiry_time, terminal_result,
                        state_digest, policy_digest, scope_digest, schema_version, reducer_version,
                        replay_identity, state]
PendingCommit        = [command_uid, receipt_uid, commit_sequence, before_digest, after_digest,
                        expected_revision, proposed_revision, control_revision_at_commit,
                        audit_digest, effect_intents, state ∈ {OCCUPIED, REPAIRING, CLEARED}]
ControlReceipt       = [control_uid, control_revision, commit_sequence, before_control_digest,
                        after_control_digest, audit_envelope, principal,
                        state ∈ {UNPUBLISHED, PUBLISHED}]         \* ring, bounded
CreateIdentity       = [receipt_uid, context_uid, target_kind, parent_uid, target_name, input_digest]
AutoBotEvent         = [aggregate_uid, commit_sequence, lane ∈ {DOMAIN, CONTROL}, state_revision,
                        control_revision, source_uid, event_type, event_digest, state_digest,
                        actor, causation_id, correlation_id, schema_version]
ProjectionState      = [aggregate_uid, applied_commit_sequence, late_event_buffer,
                        gap ∈ {NONE, OPEN, PERMANENT}, integrity ∈ {OK, DIGEST_CONFLICT}, stalled]

\* dispatch registers                                             KERNEL §3–§4
WorkContextRegisters = [hold_state, hold_generation, admission_sequence,
                        manager_authority,       \* plan_uid → ManagerAuthority
                        plan_authority,          \* plan_uid → PlanAuthority
                        integration_authority,   \* basis_uid → [basis_generation, state]
                        dispatch_authority_generation, dispatch_ledger, active_manager_transaction]
ManagerAuthority     = [lease_uid, epoch, holder, deadline, phase ∈ {ACTIVE, DRAINING}]
PlanAuthority        = [active_revision, snapshot_digest, activation_receipt_uid,
                        revision_phase ∈ {ACTIVE, QUIESCING, NONE}, plan_generation]
ManagerReservation   = [target_uid, expected_revision, command_uid,
                        phase ∈ {RESERVED, APPLYING, RESOLVED},
                        target_receipt_uid, cancellation_receipt_uid,
                        terminal_state ∈ {COMMITTED, CANCELLED, REJECTED, NONE}]
AdmissionStamp       = [uid, work_context_uid, hold_generation, plan_uid, plan_revision, plan_generation,
                        lease_uid, epoch, routing_pin, basis_uid, basis_generation,
                        dispatch_authority_generation, budget_reservation_uid, credential_grant_uid,
                        scope_digest, effect_digest, nonce, expires_at,
                        dispatch_acceptance_sequence, dispatch_acceptance_generation, state]
DispatchLedgerEntry  = [operation_uid, operation_key, permit_uid, acceptance_sequence,
                        acceptance_generation, accepted_at,
                        send_state ∈ {ACCEPTED_NOT_SENT, SEND_ATTEMPTED, ACKNOWLEDGED}]

\* effects                                                        KERNEL §3.3
EffectIntent         = [uid, operation_key, installation_lineage, aggregate_uid, committed_revision,
                        effect_index, payload_digest, provider_binding, desired_outcome,
                        target_identity, contract_revision, state]
ExternalOperation    = [uid, operation_key, attempt_index, provider, remote_identity, source_head,
                        base_head, capability_digest, permit_uid,
                        send_attempt,            \* NONE | [attempt_index, started_at, acceptance_sequence]
                        acceptance_sequence, acceptance_generation, state]
ToolInvocation       = ExternalOperation ⊕ [agent_run_uid, session_sequence, request_digest]
EffectReceipt        = [operation_uid, attempt_index, outcome, remote_identity]
ProviderCapability   = [provider, operation, supports_idempotency, supports_lookup,
                        supports_remote_marker, requires_head_base, supports_dry_run,
                        reconciliation_method, qualified]

\* plans, evidence, integration                                   KERNEL §5
PlanSnapshot         = [uid, plan_uid, plan_revision, brief_digest, members, edges,
                        acceptance_policy, budget_policy, charter_revisions, graph_digest, state]
GraphActivationReceipt = [uid, plan_uid, plan_revision, snapshot_digest, member_set_digest,
                        work_context_commit_sequence]
PlanRevisionState    = [plan_uid, revision, state]
EvidenceBundle       = [uid, candidate_digest, base_head, head, plan_revision, scope_digest,
                        charter_digest, criteria_digest, environment_digest,
                        provider_runs, ci_attestations, review_attestations, reviewer_identity,
                        remote_generation, expiry, state]
IntegrationBasis     = [uid, plan_uid, basis_generation, source_heads, base_head, overlap_set,
                        merge_order, integrated_candidate, verification_uid, state]

\* scope, identity, fencing, continuation                         KERNEL §6, §9; ROLES §2
ScopeCapsule         = [uid, task_run_uid, repository_uids, path_globs, tools, effect_kinds,
                        non_goals, consequence_class, charter_digest, charter_entries,
                        digest, state]
ScopeCheck           = [capsule_uid, requested_path, canonical_path, inode, link_target,
                        verdict ∈ {ALLOW, DENY, DETECTED_AT_CHECKPOINT}]
ExecutionIdentity    = [uid, task_run_uid, workspace_uid, agent_run_uid, execution_epoch,
                        installation_id, audience, state]
CredentialGrant      = [uid, identity_uid, audience, repositories, paths, operations,
                        installation_lineage, execution_epoch, expires_at, revocation_generation, state]
FenceSession         = [uid, task_run_uid, execution_epoch, process_fenced, workspace_fenced,
                        broker_fenced, ledger_reconciled, revocation_latency, state]
AgentCheckpoint      = [agent_run_uid, session_sequence, execution_epoch, context_digest,
                        scope_digest, budget_consumed, open_tool_invocations, progress_digest, state]
ContinuationSession  = [agent_run_uid, from_session, to_session, checkpoint_uid]
CumulativeCounters   = [task_run_uid, attempts, repairs, spend]

\* custody and restore                                            KERNEL §7
CustodyCheckpoint    = [workspace_uid, inventory_digest, outbox_digest, artifact_digest,
                        restore_receipt, state]
WorkspaceConflict    = [workspace_uid, owners, attribution_digest, quarantine_owner,
                        restore_mapping, state]
RestoreLineage       = [installation_id, restore_generation, witness_generation, old_grant_expiry,
                        revocation_generation, state]
RestoreWitnessReceipt = [installation_id, restore_generation, witness_generation,
                        old_installation_fence_evidence, ambiguous_operation_set,
                        ambiguous_operation_set_digest, identity_mapping_digest,
                        old_grant_expiry, signature]

\* budget and canonical records                                   KERNEL §8
Budget               = [uid, ceiling, allocated, state]
BudgetReservation    = [uid, budget_uid, task_run_uid, amount, purpose, expires_at, state]
ExpectedRecords      = [task_run_uid, outcome ∈ {PENDING, RECORDED, GAP},
                        usage ∈ {PENDING, RECORDED, GAP}, record_deadline]
OutcomeRecord        = [task_run_uid, candidate_digest, acceptance_revision, outcome, state]
UsageReceipt         = [task_run_uid, provider, usage_digest, amount, censored_bound, state]
TelemetryGap         = [uid, gap_kind ∈ {OUTCOME_MISSING, USAGE_MISSING}, task_run_uid, interval, state]

\* charter                                                        KERNEL §5
CharterRevision      = [uid, charter_uid, charter_kind ∈ {Charter, ProjectCharter}, owner_uid,
                        inherited_charter_uid, revision, entries, digest, accepted_by, state]
CharterEntry         = [entry_id, section, mode ∈ {mechanical, review, judged, advisory},
                        statement, threshold, provenance]           \* provenance: a list of Provenance, non-empty for a proposed entry

\* intake                                                         ONBOARD §1; TRUST
Provenance           = [source, content_digest,
                        trust_label ∈ {UNTRUSTED_REPOSITORY_CONTENT, UNTRUSTED_ISSUE_OR_PR_TEXT, INTERVIEW_ANSWER}]
SourcedValue         = [value_digest, provenance]                     \* provenance: a non-empty list of Provenance
IntakeWrite          = [command_uid, principal, client, target_kind, target_uid, expected_revision,
                        attributes]                                  \* attributes: field → SourcedValue
ProposalAcceptance   = [intake_uid, intake_revision, proposal_set_digest, principal,
                        accepts ∈ {context_configuration, plan_proposal}]   \* a charter revision: AcceptCharterRevision

\* judgment                                                       THESIS I-8
Decision             = [uid, question_class, evidence_digest, eligible_set_digest, selected,
                        policy_revision, state]
```

## 3. Actions

Every action below is a guarded transition of `Next`; a name without a guard is not coverage.

```text
\* commit lanes and audit
ReconciliationCAS           writes only reconciliation fields; no revision, no receipt
ReceiveCommand · AuthenticateCommand · RejectCommandReplay · PrepareCommandReceipt
CommitAggregateCAS          domain lane: state_revision+1, commit_sequence+1, slot installed
RepairPendingCommit         reconstruct receipt/event from the slot; verify domain digest
ClearPendingCommit          reconciliation-only, after receipt and event verified
CommitControlCAS            control lane: control_revision+1, commit_sequence+1, ring append, slot preserved
RefuseControlBacklog        ring full → no transition
PublishAuditEvent · RebuildAuditEvent · ApplyProjectionEvent · BufferLateEvent
DetectAuditGap · DeclarePermanentGap · RejectDigestConflict
ReserveCreateIdentity · AtomicCreate · RecoverCreateByName

\* WorkContext registers and permits (each a CAS by the Context controller, on WorkContext or AdmissionStamp)
RequestHold · PropagateHold · EnforceHold · ReleaseHold
DrainManager · ReserveManagerTransaction
ResolveReservedCommand · CancelReservedCommand      (CAS on the reserved command's target by its owning controller; the slot is then released through a Context command)
AdvanceManagerEpoch         domain; precondition phase = DRAINING ∧ slot empty
ResumeManager               control; precondition epoch = e+1
ActivatePlanRevision · QuiescePlan · ResumePlanRevision · SupersedePlanRevision   (plan_generation+1 each)
InvalidatePlanPermits
ReserveIntegrationBasis · InvalidateIntegrationBasis                                (basis_generation+1)
AdvanceDispatchAuthorityGeneration
IssueAdmissionStamp · InvalidateAdmissionStamp
AcceptDispatch              preconditions = registers; admission_sequence+1; ledger append
RejectStalePermit
AcknowledgeDispatch         only after acceptance ∧ (terminal ∨ OUTCOME_UNKNOWN ∨ (REQUESTED after a currency or register re-validation failure ∧ send_attempt = NONE)) recorded on the operation; in the last case the same reconciliation sets the permit INVALIDATED

\* TaskRun admission (TaskRun controller)
AdmitTask                   pins routing_pin, consequence_class, floor; refuses a pin whose tier is below the floor; sets ExpectedRecords

\* effects
MaterializeEffectIntent · QuarantineEffectIntentCollision
ValidateCurrency            grant, reservation, capsule, and no UNRESOLVED operation with the same target_identity, before RecordSendAttempt; mismatch → REQUESTED, no send
RecordSendAttempt           2a ledger SEND_ATTEMPTED, then 2b operation DISPATCHING + send_attempt
RemoteSend · ObserveExternalOutcome · MarkOutcomeUnknown
RecoverAcceptedDispatch     restart scan; the six-row table
ReconcileAmbiguousEffect · ProveNonApplication · EscalateUnresolvedOperation · AdjudicateUnresolvedOperation
BlockUnsupportedOperation

\* plans and evidence
VerifyPlanSnapshot · VerifyGraphMembers · RecordGraphActivationReceipt · FailGraphActivation
AdmitGraphMember · RecordEvidenceBundle (refuses a reviewer below the review tier of the class) · InvalidateEvidence · RecordAcceptanceAdjudication

\* charter
AcceptCharterRevision       human principal only, every entry accepted; refuses a law marked advisory and a project entry that relaxes an inherited one
PinCharterRevision          at plan acceptance; refuses acceptance without an accepted charter revision
ComputeEffectiveCharter     union of the context and project entries; a conflict resolves toward the stricter entry
DenyCharterViolation        mechanical entry: broker refusal before effect, or the candidate ineligible at the checkpoint
ApplyTightenedLaw           new or tightened law: in force at once for the broker and for evidence; fresh capsule at the next continuation, the old one revoked
AskJudgedQuestion           violates above the threshold → candidate blocked, Finding opened; any other answer or none → the review backstop alone

\* intake (admission and the Intake controller)
AdmitIntakeWrite            intake-submitter identity: intake kinds in proposal state only; refuses an attribute without provenance, a Charter entry with any provenance entry not INTERVIEW_ANSWER, and every accept
VerifyRepositoryBinding     condition ForgeVerified on a PROPOSED Repository only on an authenticated forge-adapter answer matching it
VerifyBriefDigest           over the stored content, or the forge adapter's answer for a path at a commit
ProposeIntake               Intake CAPTURED → PROPOSED only when every held proposal is valid, every named repository ForgeVerified and every brief digest verified; any new or revised held proposal returns it to CAPTURED
AcceptProposal              pinned to the Intake revision and proposal-set digest: a context configuration by a bootstrap administrator creates the WorkContext, ends the intake-namespace Intake ACCEPTED and creates its bound successor in the context namespace; a plan proposal by the reviser, from PROPOSED only, creates the Plan and moves its projects and repositories to ADOPTED

\* scope, identity, fencing, continuation
IssueScopeCapsule · CanonicalizeScopeCheck · DenyOutOfScopeAction · DetectOutOfScopeAtCheckpoint
QuarantineOutOfScopeWorkspace · LinkFindingHistorically
IssueExecutionIdentity · IssueCredentialGrant · RevokeCredentialGrant
BeginFence · ApplyWorkspaceWriteFence · ConfirmFence · MarkFenceUncertain
CreateCheckpoint · VerifyCheckpoint · RequestContinuation · StartContinuation · ResumeOpenInvocation
FenceOnUnverifiableCheckpoint

\* custody and restore
InventoryWorkspace · UploadCheckpoint · VerifyIndependentRestore · RecordArtifactCommit
PreserveWorkspace · RetireWorkspace · QuarantineWorkspaceConflict · AdjudicateConflict
RestoreState · VerifyRestoreWitness · RecordRestoreWitnessReceipt · FenceOldInstallation
ExpireOldGrant · MapRestoredIdentity · EnableRestoreDispatch

\* canonical records
WriteOutbox · DrainOutbox · RecordCanonicalRecord · CreateGapForMissingRecord · CensorUsage · SettleUsage

\* judgment
ComputeEligibleSet · RecordDecision · AbstainDecision

\* environment
DetectFault · CrashProcess · PartitionAPI · ProviderTimeout · DuplicateDelivery · ReorderDelivery · Tick
```

## 4. Safety invariants

Each is a property of the bounded model and maps to a guard in §3 and to a fixture group in `AUTOBOT-M0-AND-GATES.md`. They are grouped under the thesis invariant they make precise.

**I-1 Commit**
- F-1 *Idempotency.* One replay identity commits at most one payload to at most one aggregate revision; a differing payload or principal under the same key is rejected.
- F-2 *Receipt durability.* Every committed command has exactly one `COMMITTED` receipt whose identity no later revision can erase; a `PREPARED` receipt never implies commitment.
- F-3 *Receipt barrier.* No domain commit lands on an aggregate whose pending slot is `OCCUPIED` or `REPAIRING`; a slot is cleared only after its receipt and event are verified.
- F-4 *Lane separation.* A control commit changes only control fields and `control_revision`, preserves the slot and every domain field, appends its receipt in the same CAS; a reconciliation-only CAS changes only reconciliation fields and no revision; `commit_sequence` is strictly increasing across both lanes; a full ring refuses rather than drops.
- F-5 *Create identity.* A create resolves to one object with origin metadata equal to its receipt; a lost acknowledgement never produces a second object.
- F-6 *Projection order.* A projection applies event k only after every event < k; a same-identity different-digest event is rejected and quarantined; a gap is visible until repaired or declared permanent.

**I-2 Authority**
- F-7 *Register linearization.* For every accepted permit p and cut c on one context, `commit_sequence(AcceptDispatch(p)) < commit_sequence(c)` or p is never accepted; and at acceptance every pinned value equals its register.
- F-8 *Hold.* Only permits accepted before `RequestHold` complete; every permit issued under a previous `RUNNING` generation fails after `ReleaseHold`.
- F-9 *Plan generation.* No permit pinning `(revision R, generation g)` is accepted unless the register holds `(R, ACTIVE, g)`; every `ActivatePlanRevision`, `QuiescePlan`, `ResumePlanRevision` and `SupersedePlanRevision` changes the generation, so no permit issued before any of them is accepted after it — including a permit issued before a quiesce and presented after a resume of the same revision; and no permit is issued while the register phase is not `ACTIVE`.
- F-10 *Basis.* A merge permit pinning generation g is not accepted after `InvalidateIntegrationBasis` advanced it.
- F-11 *Manager epoch.* A target commits a Manager command only against the reservation held for that command, taken while `phase = ACTIVE` at the epoch the command pins, and not yet resolved or cancelled; a reservation is taken only while `phase = ACTIVE`; no permit is accepted between `AdvanceManagerEpoch` and `ResumeManager`; after `ResumeManager` no permit pinning the old epoch is accepted; a reservation is released only with a recorded terminal receipt.
- F-12 *Register acknowledgement.* No `Plan`, `IntegrationBasis` or `ManagerLease` status claims a revision, generation or epoch the register does not hold; only the Context controller writes `WorkContext` status.
- F-13 *Graph activation.* No member is ready or admitted before a verified `ACTIVATED` snapshot and matching register.
- F-14 *Routing pin.* A `TaskRun`'s routing pin, consequence class and floor never change; a permit's pin equals its `TaskRun`'s.
- F-15 *Restore barrier.* A restored installation dispatches nothing before a signed witness receipt, old-grant expiry or revocation, identity mapping, and reconciliation or adjudication of every operation in the ambiguous set.

**I-3 Custody**
- F-16 *Custody commit.* A workspace is `PRESERVED` only after an `ArtifactCommit` `VERIFIED` by independent restore; retirement requires `PRESERVED`; uncertain custody is `QUARANTINED`.
- F-17 *Conflict.* Mixed ownership or incomplete attribution forces a `WorkspaceConflict` and prevents retirement and assignment until adjudicated.

**I-4 Effects**
- F-18 *Send-attempt write-ahead.* No remote send without a durable `send_attempt`; after a crash, an operation with no send attempt is sent at most once and one with a send attempt and no outcome is never resent.
- F-19 *Ledger conservation.* Every `BROKER_ACCEPTED` permit has exactly one ledger entry until its operation has durably recorded acceptance and a terminal or `OUTCOME_UNKNOWN` state, or has durably returned to `REQUESTED` on a currency failure, or on a register or currency re-validation failure at recovery row 1, with no `send_attempt` present, in which case the permit is `INVALIDATED`; a `DISPATCHING` operation always has an entry; `ACCEPTED_NOT_SENT` with a present `send_attempt` is quarantined; a full ledger refuses acceptance.
- F-20 *Unresolved retention.* An `UNRESOLVED` operation is never deleted or automatically retried, blocks its dependents (as KERNEL §3.3 defines them) and plan completion, and leaves only by human adjudication.
- F-21 *Effect key.* `operation_key` is a function of its five inputs; equal keys with differing payload, target or contract quarantine and never dispatch; every committed intent materializes to exactly one operation, recoverably.
- F-22 *Tool identity.* A tool invocation is retried only under its original identity; stream closure never becomes `FAILED`.

**I-5 Scope**
- F-23 *Canonical scope.* A verdict is computed on canonical path and inode; a mutation outside the capsule by symlink, hardlink, rename, mount or subprocess is denied before effect or detected at the next checkpoint before any evidence, and the workspace is quarantined.
- F-24 *Finding isolation.* Linking a finding to a task changes no capsule, contract or evidence requirement.
- F-25 *Real fencing.* A fenced process cannot write its workspace, use a revoked grant, invoke a privileged tool or produce current evidence; `FenceConfirmed` requires every ledger entry of the fenced run refused or reconciled; uncertainty blocks replacement.
- F-26 *Continuation.* A continuation has the same `AgentRun`, identity, grant lineage, capsule digest and reservation, except that a law added or tightened since its capsule was issued gives it a fresh capsule differing only in charter digest and entries; cumulative counters are non-decreasing; an open invocation is resumed, never re-issued.
- F-38 *Charter pin.* No plan revision is accepted without an accepted charter revision; it pins the accepted `ProjectCharter` revision and the `Charter` revision it inherits, with their digests; an accepted revision never changes; every capsule carries the charter digest of the charter in force at its issue and the entries relevant to its task.
- F-39 *Tighten-only.* The effective charter is the union of the context and project entries, a conflict resolved toward the stricter entry; no project entry relaxes an inherited one; no law is `advisory`; a relaxed law or a changed rule reaches no plan revision pinned before the change.
- F-40 *Mechanical laws.* A brokered effect that violates a `mechanical` entry of the charter in force is refused before effect, and a checkpoint diff that violates one makes the candidate ineligible for evidence; a new or tightened law is in force for all work not yet accepted from its acceptance, so no `EvidenceBundle` bound to an earlier charter digest satisfies an acceptance, every later capsule carries it, and an active attempt receives it through a fresh capsule at its next continuation that differs from the old one only in charter digest and entries, the old one revoked.

**I-6 Evidence**
- F-27 *Fresh evidence.* Acceptance binds candidate, base and head, contract, environment, provider run, reviewer identity and current remote generation; any change invalidates; a delayed result for an old basis satisfies nothing.
- F-28 *Independence.* The reviewer identity of an accepted bundle differs from the worker's session; `WorkerFinished` never implies `TaskAccepted`.
- F-29 *Integration.* Overlapping changes have one current basis and merge order; milestone acceptance references the integrated candidate.

**I-7 Projection** — F-30 No external observation changes an aggregate without a controller CAS; forge text never becomes a command without actor validation.

**I-8 Judgment**
- F-31 Every `Decision.selected` is a member of the eligible set computed before the question; no decision grants credential, scope, budget, acceptance or merge; an absent judge takes the conservative branch and widens nothing.
- F-41 *Charter authority.* Only a human principal accepts a charter revision or any of its entries; a model output never promotes a candidate entry; no law is waived, and no agent grants a waiver.
- F-42 *Block-only judgment.* A `judged` answer can only block: "violates" above the entry's threshold blocks the candidate and opens a `Finding`; "complies" satisfies nothing the `review` backstop has not; an outage or abstention leaves the backstop as the only check, never a pass.
- F-43 *Proposal authority.* An accept of a context configuration commits only from a bootstrap administrator, and of a plan proposal only from the context's reviser, never from the intake-submitter identity; each commits only at the `Intake` revision and proposal-set digest it pins, and a plan accept only from `PROPOSED`; the intake-submitter identity commits nothing but intake kinds in proposal state, and nothing it writes is canonical before an accept.
- F-44 *Provenance and binding.* Every attribute the intake-submitter identity writes carries at least one provenance entry with a source, a content digest and a trust label, and every provenance entry of a `Charter` entry it writes is `INTERVIEW_ANSWER`; an `Intake` is `PROPOSED` only while every repository it names is `ForgeVerified` and every brief digest is verified; a `Repository` is `ADOPTED` only by a plan accept of an `Intake` that was `PROPOSED` with it `ForgeVerified`.

**I-9 Ledger** — F-32 A terminal `TaskRun` whose `record_deadline` has passed has `expected_records.outcome` and `.usage` each `RECORDED` or `GAP`, and no `TaskRun` is counted by any outcome or cost computation while either is `PENDING`; a `CENSORED` receipt carries `min(reservation ceiling, rate-card bound)` and counts at it; an `UNKNOWN` reservation stays held.

**I-10 Economics** — F-37 *Floor.* No `TaskRun` is admitted whose routing pin names a worker tier below its pinned floor; no accepted `EvidenceBundle` carries a reviewer below the review tier fixed for the consequence class; a judgment or policy change raises a floor and never lowers it.

**Cross-cutting** — F-33 *Type correctness* (every field in its declared domain). F-34 *Event robustness* (duplicates, delays, replays and reordering violate none of the above). F-35 *Degradation* (an unavailable capability is never used; degraded mode grants nothing). F-36 *Reference isolation* (cross-context references and grants are rejected unless every identity binding matches).

## 5. Negative variants

A check is vacuous unless removing a guard produces a counterexample. Each variant must fail its named invariant:

| Guard removed | Must violate |
|---|---|
| acceptance reads registers then records in another object | F-7, F-8, F-9, F-10 |
| `QuiescePlan` and `ResumePlanRevision` without `plan_generation + 1` (one guard: generation advance on quiesce and resume) | F-9 |
| takeover as one CAS, or `ResumeManager` before `AdvanceManagerEpoch` | F-4 or F-11 |
| send without a `send_attempt` marker | F-18 |
| `RecordSendAttempt` 2b before 2a | F-19 |
| control commit touching a domain field | F-4 |
| clearing a pending slot on elapsed time | F-3 |
| projection applying a later event first | F-6 |
| continuation with a fresh identity | F-26 |
| retire a workspace on a completion marker without restore verification | F-16 |
| dispatch on a restored installation without a witness receipt | F-15 |
| lexical glob check on the requested string | F-23 |
| no gap created at `record_deadline` | F-32 |
| reviewer identity equal to worker session | F-28 |
| admission without the floor comparison | F-37 |
| plan acceptance without a pinned charter revision | F-38 |
| a project entry that weakens an inherited law | F-39 |
| continuation keeping its capsule after a law was tightened | F-40 |
| charter revision accepted by an agent principal | F-41 |
| a `judged` "complies" satisfying the `review` backstop | F-42 |
| an accept admitted from the intake-submitter identity | F-43 |
| an `Intake` reaching `PROPOSED` with a repository lacking a forge-adapter answer | F-44 |

## 6. Conditional liveness

The model asserts only conditional progress, each condition a named finite bound with an explicit degraded terminal branch when the dependency never recovers:

```text
api_recovery_bound             or permanent-unavailability branch (RECOVERING, preserved)
queue_fairness_bound           reserved control capacity for hold, fence and receipt repair
witness_quorum_bound           or read-only branch
broker_revocation_bound        revocation propagation
max_old_grant_ttl              maximum accepted grant lifetime
artifact_store_recovery_bound  or quarantine branch
provider_reconcile_bound       or UNRESOLVED branch
webhook_relist_bound           poll after watch loss
continuation_deadline          or FENCE_PENDING branch
human_decision_deadline        escalation, never silent progress
record_deadline                gap creation
usage_settlement_deadline      censoring
```

Under these: a pending internal event with a fair controller is eventually processed; an eligible task with an available permitted worker is eventually admitted or explicitly blocked; an ambiguous operation whose provider eventually answers definitively is eventually reconciled; with storage eventually available, cleanup eventually preserves or quarantines; a never-answered human request stays visible and escalates. External users, providers and CI may remain unavailable forever.

## 7. Bounded model plan

Proposed first configuration: one context, two repositories, one plan, two milestones, four tasks, two workers, one reviewer, one tester, one forge, one CI provider, two concurrent attempts, two simulated Managers, two simulated installation identities. Inject: duplicate and reordered deliveries, stale completions, provider timeouts and rate limits, context exhaustion and mid-stream disconnect, lost merge responses, controller restart, broker crash before and after the send-attempt marker, a hold cut interleaved at every position with an acceptance, plan activation interleaved with an old-revision acceptance, quiesce-then-resume with the permit's invalidation lost, a control commit beside a pending domain commit, out-of-order and digest-conflicting audit delivery, storage outage, symlink and rename scope escape, and garbage collection discovering unpushed work.

This configuration is deliberately wider than the M0 profile; the profile bounds the deployment, the model bounds the protocol.

## 8. Stack and phases

```text
Quint executable specification → typed simulation and counterexample traces
  → Apalache symbolic bounded checking → small-state TLC/TLA+ cross-check
  → Rust reducer and controller implementation → trace refinement and fault injection
```

Quint is the authoring surface; Apalache the primary bounded checker; TLC the independent cross-check; PlusCal is permitted for readable procedural sections and is never a second authority; Dafny or Lean are optional for pure sequential kernels (digests, identity derivation, manifest completeness) and never runtime dependencies. The Rust implementation exposes deterministic reducer decisions and durable transition receipts; a refinement test checks that every accepted Rust transition is an allowed model transition and that every model counterexample has a fixture or a documented abstraction boundary. Authority is one-way: the model may abstract Kubernetes, providers and storage; the implementation may not invent authority outside the model.

Design: records, actions, invariants and negative variants are specified here. M0: the bounded kernel is written, type-checked, simulated and checked on the §7 configuration; traces feed the M0 fixtures. G-FORMAL: non-vacuous checks with negative variants, published counterexamples and assumptions, Rust refinement and fault-injection evidence, model and toolchain digests retained, invalidated on any change to protocol, reducer or model.

## 9. Review classification

A design review classifies every finding as exactly one of:

```text
DESIGN_DEFECT        the protocol is contradictory or unimplementable
DESIGN_GAP           a required contract is not specified
PLANNED_ARTIFACT     the contract is specified; its implementation is scheduled to a gate
IMPLEMENTATION_FAIL  an artifact exists and violates the contract
OUT_OF_SCOPE         explicitly deferred and capability-gated
```

Only the first two block design closure. Absence of a model, a check or a refinement test is `PLANNED_ARTIFACT` mapped to M0 or G-FORMAL; a reviewer still rejects a design whose states, actions, guards or refinement boundary cannot be made precise under this stack. Design readiness and implementation readiness are reported separately.
