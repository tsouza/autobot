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

The first compilable model represents these records with these fields. Each field is the same field as in the M0 schema (`AUTOBOT-M0-AND-GATES.md`); the refinement mapping is field by field, and a kernel field missing from the model is a model defect. Every `state` domain is exactly its KERNEL §10 machine. Kinds without a record are abstracted, each as what the model keeps instead: an `Intervention` enters only as the §3 interventions block states: as the register or adjudication action it becomes, an `EXCEPTION` as an `EvidenceBundle.exceptions` entry, and a `PAUSE` not at all; `Plan.phase` is outside the model, which admits against `plan_authority` and the verified `ACTIVATED` snapshot (F-13); a `Finding` enters only as opened by `AskJudgedQuestion` (F-42), as classified by `ClassifyFinding` and as linked by `LinkFindingHistorically`, which changes no other record (F-24); a `VerificationRun` enters only as the `EvidenceBundle` it records; a `RestoreRequest` is `RestoreLineage`; `PlanProposal`, `Project` and `WorkBrief` enter only as the `IntakeWrite` and `ProposalAcceptance` records that name them; the `Charter` and `ProjectCharter` kind states enter only as their `CharterRevision`s; a `CustodyPolicy` enters only as the cadence at which `CustodyCheckpoint`s occur; and an `Artifact` enters only as the digests its `CustodyCheckpoint` verifies.

```text
\* commit, receipt, audit                                         KERNEL §1–§2
Aggregate            = [uid, kind, context_uid, domain_digest, control_digest,
                        state_revision, control_revision, commit_sequence,
                        pending_commit, control_receipt_ring]
CommandReceipt       = [uid, command_uid, idempotency_key, target_kind, target_uid, parent_context_uid,
                        principal, expected_revision, proposed_revision, commit_sequence,
                        input_digest, origin_metadata, issue_time, expiry_time,
                        rejection_proof,         \* NONE unless state is REJECTED
                        state_digest, policy_digest, scope_digest, schema_version, reducer_version,
                        replay_identity, state]
RejectionProof       = [ground ∈ {passed_revision, replay_conflict, create_conflict, guard_refusal},
                        observed_revision,                            \* passed revision
                        observed_commit_sequence,                     \* passed revision, create conflict
                        existing_receipt_uid, bound_digest,           \* replay conflict
                        observed_uid,                                 \* create conflict
                        guard_id, read_revision]                      \* guard refusal
AuditEnvelope        = [aggregate_uid, commit_sequence, lane ∈ {DOMAIN, CONTROL}, state_revision,
                        control_revision, source_uid, event_type, state_digest,
                        actor, causation_id, correlation_id, schema_version]
EffectIntentRecord   = [operation_key,          \* derived when the EffectIntent is built (KERNEL §3.3); a slot's record holds installation_lineage in its place
                        effect_index, payload_digest,
                        provider_binding,        \* [provider, operation]: the key of its ProviderCapability
                        desired_outcome, target_identity, contract_revision]
PendingCommit        = [command_uid, receipt_uid, commit_sequence, before_digest, after_digest,
                        expected_revision, proposed_revision, control_revision_at_commit,
                        audit_envelope,          \* AuditEnvelope: rebuilds the event with no other read
                        effect_intents,          \* ordered, bounded list of EffectIntentRecord
                        state ∈ {OCCUPIED, REPAIRING, CLEARED}]
ControlReceipt       = [control_uid, control_revision, commit_sequence, before_control_digest,
                        after_control_digest, audit_envelope, principal,   \* audit_envelope: AuditEnvelope
                        state ∈ {UNPUBLISHED, PUBLISHED}]         \* ring, bounded
CreateIdentity       = [receipt_uid, context_uid, target_kind, parent_uid, target_name, input_digest]
AutoBotEvent         = AuditEnvelope ⊕ [event_digest]
ProjectionState      = [aggregate_uid, applied_commit_sequence, late_event_buffer,
                        gap ∈ {NONE, OPEN, PERMANENT}, integrity ∈ {OK, DIGEST_CONFLICT}, stalled]

\* dispatch registers                                             KERNEL §3–§4
WorkContextRegisters = [hold_state, hold_generation,
                        hold_causes,             \* set of Intervention uid (HOLD or KILL_SWITCH)
                        admission_sequence,
                        manager_authority,       \* plan_uid → ManagerAuthority
                        plan_authority,          \* plan_uid → PlanAuthority
                        integration_authority,   \* basis_uid → [basis_generation, state]
                        dispatch_authority_generation, dispatch_ledger,
                        blocked_targets,         \* list of [target_identity, operation_uid] in order added, one per unsettled operation
                        active_manager_transaction]
ManagerAuthority     = [lease_uid, epoch, holder, deadline, phase ∈ {ACTIVE, DRAINING}]
PlanAuthority        = [active_revision, snapshot_digest, activation_receipt_uid,
                        revision_phase ∈ {ACTIVE, QUIESCING}, plan_generation]   \* absent: no active revision
ManagerLease         = [uid, plan_uid, holder, epoch, state]   \* uid: the entry's lease_uid; an acknowledged copy of it (F-12)
ManagerReservation   = [plan_uid, target_uid, expected_revision, command_uid,
                        phase ∈ {RESERVED, APPLYING, RESOLVED},
                        target_receipt_uid, cancellation_receipt_uid,
                        terminal_state ∈ {COMMITTED, CANCELLED, REJECTED, NONE}]
AdmissionStamp       = [uid, work_context_uid, operation_uid, hold_generation, plan_uid, plan_revision, plan_generation,
                        lease_uid, epoch, routing_pin, provider_binding, basis_uid, basis_generation,
                        dispatch_authority_generation, budget_reservation_uid, credential_grant_uid,
                        scope_digest, effect_digest, nonce, expires_at,
                        dispatch_acceptance_sequence, dispatch_acceptance_generation,   \* copies of the ledger entry's values
                        state]
DispatchLedgerEntry  = [operation_uid, operation_key, permit_uid, acceptance_sequence,
                        acceptance_generation, accepted_at,
                        send_state ∈ {ACCEPTED_NOT_SENT, SEND_ATTEMPTED, ACKNOWLEDGED}]

\* effects                                                        KERNEL §3.3
EffectIntent         = EffectIntentRecord ⊕ [uid, installation_lineage, aggregate_uid,
                        committed_revision, state]
ExternalOperation    = [uid, operation_key, attempt_index, provider, remote_identity, source_head,
                        base_head, capability_digest, permit_uid,
                        send_attempt,            \* NONE | [attempt_index, started_at, acceptance_sequence] of the current attempt
                        prior_send_attempts,     \* append-only list of earlier attempts' send_attempt values, oldest first; ProveNonApplication moves send_attempt here
                        acceptance_sequence, acceptance_generation,
                        adjudication,            \* NONE | [decision_uid, outcome ∈ {CONFIRMED, FAILED, COMPENSATED}]; written once, by a human adjudication of an UNRESOLVED operation
                        state]
ToolInvocation       = ExternalOperation ⊕ [agent_run_uid, session_sequence, request_digest]
EffectReceipt        = [operation_uid, attempt_index, outcome, remote_identity]
ProviderCapability   = [provider, operation, supports_idempotency, supports_lookup,
                        supports_remote_marker, requires_head_base, supports_dry_run,
                        rate_limit_authoritative,   \* a rate-limit answer proves non-application (KERNEL §3.3)
                        reconciliation_method, qualified]

\* plans, evidence, integration                                   KERNEL §5
Plan                 = [uid, work_context_uid, source_proposal_uid, accepted_revision, snapshot_uid,
                        active_revision, plan_generation, manager_epoch]   \* the last three: acknowledged copies of the registers
Milestone            = [uid, plan_uid, plan_revision, members, final_basis_uid, accepted_bundles, state]
Task                 = [uid, plan_uid, plan_revision, milestone_uid, obligation, repositories,
                        acceptance_evidence, non_goals, consequence_class, dependencies,
                        accepted_bundles, state]  \* accepted_bundles: (uid, digest) of every EvidenceBundle its acceptance adjudication references
PlanSnapshot         = [uid, plan_uid, plan_revision, brief_digest, members, edges,
                        acceptance_policy, budget_policy, charter_revisions, graph_digest, state]
GraphActivationReceipt = [uid, plan_uid, plan_revision, snapshot_digest, member_set_digest,
                        work_context_commit_sequence]
PlanRevisionState    = [plan_uid, revision, state]
EvidenceBundle       = [uid, candidate_digest, base_head, head, plan_revision, scope_digest,
                        charter_digest, criteria_digest, environment_digest,
                        provider_runs, ci_attestations, review_attestations, reviewer_identity,
                        correlated,              \* the reviewer ran on the worker's model (ROLES §3); set by the recording controller from the reviewer configuration and the TaskRun's routing pin, never from a claim
                        exceptions,              \* uid of every APPLIED EXCEPTION Intervention the bundle relies on
                        remote_generation, expiry, state]
IntegrationBasis     = [uid, plan_uid, basis_generation, source_heads, base_head, overlap_set,
                        merge_order, integrated_candidate, verification_uid, state]

\* scope, identity, fencing, continuation                         KERNEL §6, §9; ROLES §2
ScopeCapsule         = [uid, task_run_uid, objective, expected_outcome, repository_uids, path_globs,
                        branches, tools, effect_kinds, non_goals, acceptance_evidence,
                        consequence_class, budget_limit, deadline, attempt_limit, repair_limit,
                        plan_uid, plan_revision, milestone_revision, task_revision,
                        charter_digest, charter_entries,
                        digest,                  \* over every field above: the capsule digest
                        state]
ScopeCheck           = [capsule_uid, requested_path, canonical_path, inode, link_target,
                        verdict ∈ {ALLOW, DENY, DETECTED_AT_CHECKPOINT}]
ExecutionIdentity    = [uid, task_run_uid, workspace_uid, agent_run_uid, execution_epoch,
                        installation_id, audience, state]
CredentialGrant      = [uid, identity_uid, audience, repositories, paths, operations,
                        installation_lineage, execution_epoch, expires_at, revocation_generation, state]
FenceSession         = [uid, task_run_uid, execution_epoch, process_fenced, workspace_fenced,
                        broker_fenced,
                        fence_settled,           \* no ledger entry names an operation of the TaskRun and none of them is OUTCOME_UNKNOWN or RECONCILING
                        revocation_latency, state]
AgentCheckpoint      = [agent_run_uid, session_sequence, execution_epoch, context_digest,
                        scope_digest, budget_consumed, open_tool_invocations, progress_digest,
                        digest,                  \* over every field above: what a CustodyCheckpoint records
                        state]
ContinuationSession  = [agent_run_uid, from_session, to_session, checkpoint_uid]
TaskRun              = [uid, task_uid, task_revision,
                        planning_subject,        \* NONE for a task's run; the Plan or Intake uid a planning TaskRun serves, its task_uid and task_revision then NONE
                        source_basis, execution_profile, routing_pin,
                        consequence_class, floor, capsule_digest, budget_reservation_uid,
                        fence_state, execution_epoch, revocation_generation, state]
AgentRun             = [uid, task_run_uid, session_sequence, identity_uid, credential_grant_uid,
                        capsule_digest, budget_reservation_uid, continuation_deadline,
                        fence_state, execution_epoch,   \* acknowledged copies of its TaskRun's (KERNEL §10)
                        revocation_generation, state]
CumulativeCounters   = [task_run_uid, attempts, repairs, spend]

\* custody and restore                                            KERNEL §7
Workspace            = [uid, task_run_uid,           \* the TaskRun holding it; NONE while none does
                        repository_uid, custody_policy_uid, artifact_commit_uid,
                        conflict_uid,            \* NONE unless a WorkspaceConflict holds it
                        retire_only,             \* set on entering QUARANTINED or CONFLICT; never cleared: it never returns to use
                        state]
ArtifactCommit       = [uid, custody_checkpoint_uid, state]
CustodyCheckpoint    = [uid, workspace_uid,
                        sequence,                \* +1 per checkpoint of its workspace
                        agent_checkpoint_digests,   \* the digest of every CREATED AgentCheckpoint of a session holding the workspace, recorded when it begins
                        inventory_digest, outbox_digest, artifact_digest,
                        restore_receipt, state]
WorkspaceConflict    = [workspace_uid, owners, attribution_digest, quarantine_owner,
                        restore_mapping, state]
RestoreLineage       = [installation_id, restore_generation, witness_generation, old_grant_expiry,
                        revocation_generation,
                        ambiguous_operation_set, \* every operation the restored state holds non-terminal (KERNEL §7)
                        state]                   \* state: the RestoreRequest machine
RestoreWitnessReceipt = [installation_id, restore_generation, witness_generation,
                        old_installation_fence_evidence,
                        ambiguous_operation_set_digest,   \* over RestoreLineage.ambiguous_operation_set; the witness signs the digest only
                        identity_mapping_digest, old_grant_expiry, signature]

\* budget and canonical records                                   KERNEL §8
Budget               = [uid, ceiling, allocated, state]
BudgetReservation    = [uid, budget_uid, task_run_uid, amount,
                        purpose ∈ {ATTEMPT, EFFECT},   \* ATTEMPT: the model spend of every session of task_run_uid, a planning TaskRun's included; EFFECT: one operation of it, kept by a later attempt that inherits the operation
                        expires_at, state]
ExpectedRecords      = [task_run_uid,
                        outcome,                 \* ExpectedEntry
                        usage]                   \* producer → ExpectedEntry, added PENDING before the producer spends; a producer is
                                                 \* [agent_run_uid, session_sequence] of a session of the TaskRun, or broker for its operations' effects
ExpectedEntry        = [state ∈ {PENDING, RECORDED, GAP},
                        gap_uid,                 \* NONE unless GAP: the TelemetryGap of GAP(uid)
                        record_deadline]         \* NONE until the TaskRun is terminal; then the record_deadline bound after that commit, or after the entry's creation if later
OutcomeRecord        = [task_run_uid, candidate_digest, acceptance_revision, outcome, state]
UsageReceipt         = [task_run_uid, producer,   \* the producer of ExpectedRecords.usage whose entry it records
                        replaces,                \* NONE for the producer's own spend; else the producer of the one refused entry this replacing receipt records, part of its name and create key (KERNEL §8 step 2)
                        provider, usage_digest, amount, censored_bound, state]
TelemetryGap         = [uid, gap_kind ∈ {OUTCOME_MISSING, USAGE_MISSING}, task_run_uid,
                        producer,                \* NONE for OUTCOME_MISSING; for USAGE_MISSING the producer of the entry it is for
                        interval,
                        closing_record_uid,      \* NONE unless CLOSED; the late OutcomeRecord or UsageReceipt CloseGap links it to
                        state]

\* charter                                                        KERNEL §5
CharterRevision      = [uid, charter_uid, charter_kind ∈ {Charter, ProjectCharter}, owner_uid,
                        inherited_charter_uid, revision, entries, digest, accepted_by, state]
CharterEntry         = [entry_id, section, mode ∈ {mechanical, review, judged, advisory},
                        statement, threshold, provenance]           \* provenance: a list of Provenance, non-empty for a proposed entry

\* intake                                                         ONBOARD §1; TRUST
Intake               = [uid, context_uid, revision, proposal_set_digest, repository_uids,
                        budget_uid,              \* the Budget of every reservation of a planning TaskRun that serves it (KERNEL §6)
                        verified_brief_digests, revision_principals, state]
Repository           = [uid, intake_uid, forge_ref, external_id, forge_verified, state]   \* forge_verified: the ForgeVerified condition
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
RequestHold                 control; applies a HOLD or KILL_SWITCH: adds it to hold_causes; from RUNNING also RUNNING → FREEZE_PENDING (the cut), else hold_state unchanged
PropagateHold               control; FREEZE_PENDING → PROPAGATING; after every ISSUED permit of the context that no ledger entry and no COMMITTED AcceptDispatch receipt names is INVALIDATED
EnforceHold                 control; PROPAGATING → ENFORCED; precondition dispatch_ledger empty
ReleaseHold                 control; applies a RESUME in ENFORCED or RELEASING: removes only the cause it answers; ENFORCED → RELEASING when hold_causes becomes empty
CompleteHoldRelease         control; RELEASING → RUNNING; precondition hold_causes empty; after every permit ISSUED under an earlier hold_generation that no ledger entry and no COMMITTED AcceptDispatch receipt names is INVALIDATED
InstallManagerAuthority     domain; creates manager_authority[plan] with epoch 1 and deadline now + lease duration, phase unwritten (reads ACTIVE, KERNEL §1); precondition no entry for the plan ∧ fewer entries than the profile's plans
RenewManagerAuthority       domain; deadline := now + lease duration; precondition holder, lease_uid, epoch match ∧ phase = ACTIVE ∧ now < deadline
DrainManager                control; phase := DRAINING; precondition deadline passed ∨ takeover requested
ReserveManagerTransaction   domain; slot := RESERVED for the command; precondition holder, lease_uid, epoch match ∧ phase = ACTIVE ∧ now < deadline ∧ slot empty (absent or RESOLVED)
ClaimManagerTransaction     domain; RESERVED → APPLYING, submitted by the target's owning controller before it commits or rejects the command; precondition the slot holds the command ∧ manager_authority[plan].phase = ACTIVE ∧ epoch = the command's epoch
ResolveReservedCommand · CancelReservedCommand      (CAS on the reserved command's target by its owning controller, only while the slot is APPLYING for that command; the cancel consumes the reserved expected revision)
ReleaseManagerTransaction   domain; → RESOLVED with a non-empty terminal_state and the receipt that proves it: from APPLYING as COMMITTED or REJECTED with the target's receipt, or as CANCELLED with the cancel's receipt; from RESERVED only as CANCELLED during a takeover, with no target write, the DrainManager control receipt as proof
AdvanceManagerEpoch         domain; sets holder, lease_uid, epoch := e+1, deadline; precondition phase = DRAINING ∧ slot empty
ResumeManager               control; precondition epoch = e+1
ActivatePlanRevision        domain; first activation: creates plan_authority[plan] = (R, snapshot, receipt, ACTIVE, 1); precondition no entry ∧ manager_authority[plan] present
QuiescePlan                 domain; revision_phase := QUIESCING; precondition ACTIVE
ResumePlanRevision          domain; revision_phase := ACTIVE, same revision; precondition QUIESCING
SupersedePlanRevision       domain; plan_authority[plan] := (R2, snapshot, receipt, ACTIVE, plan_generation+1); precondition QUIESCING at R1 ∧ R2 ≠ R1
                            (ActivatePlanRevision, QuiescePlan, ResumePlanRevision, SupersedePlanRevision: plan_generation+1 each)
InvalidatePlanPermits       after QuiescePlan: every ISSUED permit pinning the plan at an earlier plan_generation that no ledger entry and no COMMITTED AcceptDispatch receipt names → INVALIDATED
RetirePlanAuthority         domain; removes plan_authority[plan] and manager_authority[plan]; precondition the Plan terminal ∧ manager_authority[plan].phase = ACTIVE ∧ no RESERVED or APPLYING slot for the plan
ReserveIntegrationBasis     domain; basis_generation+1, state := RESERVED; refuses a new basis when the entries equal the profile's bases, and a basis with a source or base head at a target a blocked_targets pair names
InvalidateIntegrationBasis  domain; basis_generation+1, state := INVALIDATED
RetireIntegrationBasis      domain; removes integration_authority[basis]; precondition the IntegrationBasis terminal ∧ state = INVALIDATED
AdvanceDispatchAuthorityGeneration   control; dispatch_authority_generation := the witness_generation of a recorded RestoreWitnessReceipt
IssueAdmissionStamp         create on the broker's command, client request key (operation_uid, operation state_revision), for an operation REQUESTED with no ledger entry and no permit that is ISSUED or BROKER_ACCEPTED; precondition hold_state = RUNNING ∧ plan revision_phase = ACTIVE ∧ manager phase = ACTIVE ∧ (provider_binding names a merge ⇒ basis_uid ≠ ∅); every register pin from one WorkContext read; expires_at ≤ creation + the profile's replay window, the window every AcceptDispatch pins
AcceptDispatch              domain; the broker's command, submitted only once PermitOperation recorded the permit on its operation, idempotency key permit_uid; permit.state = ISSUED read before the CAS; preconditions = registers ∧ now < expires_at ∧ no entry with the permit's uid or operation_uid ∧ (basis_uid = ∅ ⇒ provider_binding names no merge) ∧ |dispatch_ledger| < capacity ∧ (|dispatch_ledger| + |blocked_targets| < capacity ∨ the operation holds a pair); admission_sequence+1; ledger append recording acceptance sequence and generation
RejectStalePermit           a failed AcceptDispatch precondition → guard refusal; then InvalidateAdmissionStamp and ReturnOperationToRequested
RecordPermitAccepted        domain, the broker's command; ISSUED → BROKER_ACCEPTED; precondition a COMMITTED AcceptDispatch receipt for the permit; copies its acceptance sequence and generation
RecordPermitConsumed        domain, the broker's command; BROKER_ACCEPTED → CONSUMED; precondition the permit's operation carries a send_attempt under it
InvalidateAdmissionStamp    domain; ISSUED → INVALIDATED after a change to a register the permit pins or a REJECTED AcceptDispatch for it, and only while no ledger entry and no COMMITTED AcceptDispatch receipt names it; BROKER_ACCEPTED → INVALIDATED on the broker's command, only while no ledger entry names it ∧ its operation, still naming it, REQUESTED ∧ send_attempt = NONE
ExpireAdmissionStamp        domain; ISSUED → EXPIRED; precondition now ≥ expires_at ∧ no ledger entry and no COMMITTED AcceptDispatch receipt names the permit
AcknowledgeDispatch         reconciliation-only; removes the entry only after acceptance ∧ ((a) terminal ∨ OUTCOME_UNKNOWN ∨ (b) REQUESTED ∧ send_attempt = NONE after a currency or register re-validation failure, the entry ACCEPTED_NOT_SENT or SEND_ATTEMPTED) recorded on the operation

\* TaskRun admission (TaskRun controller)
AdmitTask                   pins routing_pin, consequence_class, floor; refuses a pin whose tier is below the floor; sets ExpectedRecords (outcome PENDING, usage empty, no record_deadline)
                            planning TaskRun: planning_subject a Plan or Intake, task_uid NONE, no workspace, a capsule with no path; no consequence class or floor to compare; for an Intake, its reservations from the Intake's budget_uid, and for one of the intake namespace the Intake in the WorkContext's place and no effect
                            later attempt: precondition the predecessor terminal ∧ its fence_state ∈ {ACTIVE, FENCED}; inherits its CumulativeCounters and its non-terminal operations, which become its own (KERNEL §9)

\* effects
Unsent(op)                  ≜ op.state = REQUESTED ∧ op.send_attempt = NONE, which is the current attempt's by construction (ProveNonApplication); the one definition of unsent (KERNEL §3.3)
RequestToolInvocation       AgentRun controller, on the runtime adapter's command; a domain commit on the AgentRun carrying the intent of one call with an effect outside the workspace
MaterializeEffectIntent     from a committed receipt of any carrier, the AgentRun for a ToolInvocation
QuarantineEffectIntentCollision   EffectIntent MATERIALIZED → QUARANTINED; precondition its name holds an intent with a different payload digest, target identity or contract revision; never dispatched
PermitOperation             Broker; REQUESTED → PERMITTED, recording permit_uid; precondition the permit's COMMITTED create receipt
ReturnOperationToRequested  Broker; PERMITTED → REQUESTED; precondition the permit INVALIDATED or EXPIRED with no COMMITTED AcceptDispatch, or a currency failure or refused 2a before any send_attempt, or a register or currency re-validation failure at recovery row 1
ValidateCurrency            grant, reservation, capsule, before RecordSendAttempt; mismatch → REQUESTED, no send
RecordSendAttempt           2a ledger SEND_ATTEMPTED, precondition no blocked_targets pair on the target ∨ the first pair on it is the operation's own, a refusal handled as a currency failure; then 2b operation DISPATCHING + send_attempt
RemoteSend                  Broker; precondition the operation DISPATCHING with a durable send_attempt ∧ its entry SEND_ATTEMPTED
ObserveExternalOutcome      Broker; DISPATCHING → CONFIRMED | REJECTED | FAILED on a provider answer for the attempt; a rate-limit answer or timeout goes to MarkOutcomeUnknown
AddBlockedTarget            reconciliation-only; appends [target_identity, operation_uid] to blocked_targets; precondition the operation's entry SEND_ATTEMPTED, or recovery row 6, or Unsent(op) ∧ op in the ambiguous_operation_set of its installation's RestoreLineage, that lineage FENCED, RECONCILED or MAPPED, on a capability offering neither idempotency nor lookup (the precondition of EscalateUnresolvedOperation's REQUESTED case, less its pair); no-op when present
MarkOutcomeUnknown          Broker; DISPATCHING → OUTCOME_UNKNOWN; precondition the operation's pair in blocked_targets; OUTCOME_UNKNOWN → RECONCILING only after AcknowledgeDispatch removed its entry
RecoverAcceptedDispatch     restart scan; the six-row table, row 4 writing send_attempt on a PERMITTED operation before acting as row 2; brings every permit level; removes the pair of every terminal operation
ReleaseBlockedTarget        reconciliation-only; removes [target_identity, operation_uid] from blocked_targets; precondition the operation terminal ∧ no entry names it; no-op when absent
ReconcileAmbiguousEffect    Broker; OUTCOME_UNKNOWN → RECONCILING after AcknowledgeDispatch removed its entry; RECONCILING → CONFIRMED | FAILED on a provider lookup or deduplication answer
EscalateUnresolvedOperation Broker; RECONCILING → UNRESOLVED; precondition the capability offers neither idempotency nor lookup ∧ provider_reconcile_bound passed; or REQUESTED → UNRESOLVED; precondition the capability offers neither idempotency nor lookup ∧ Unsent(op) ∧ op in the ambiguous_operation_set of its installation's RestoreLineage, that lineage FENCED, RECONCILED or MAPPED ∧ its pair in blocked_targets; either way creates the adjudication Intervention
AdjudicateUnresolvedOperation   Broker, domain, on a human adjudication; one CAS writing adjudication := [decision_uid, outcome] and UNRESOLVED → outcome, outcome ∈ {CONFIRMED, FAILED, COMPENSATED}; precondition state = UNRESOLVED ∧ adjudication = NONE; COMPENSATED only with the adjudicator's evidence of compensation outside AutoBot; then ReleaseBlockedTarget
ProveNonApplication         RECONCILING → REQUESTED, same operation_key, attempt_index+1, in one CAS appending send_attempt to prior_send_attempts and setting send_attempt := NONE; precondition provider lookup, deduplication or a rate-limit answer under rate_limit_authoritative proves non-application ∧ the operation's permit CONSUMED ∧ (after a rate-limit answer) the provider's back-off passed
ReleaseUnsentOperation      Broker; REQUESTED → RELEASED; precondition Unsent(op) ∧ no ledger entry and no ISSUED or BROKER_ACCEPTED permit names it ∧ it is an operation of a TaskRun whose Task, or whose planning_subject, is terminal ∧ (it is in the ambiguous_operation_set of its installation's RestoreLineage ⇒ a provider lookup under its operation_key, taken with that lineage FENCED, RECONCILED or MAPPED, proves non-application); then its EFFECT BudgetReservation, if any, RELEASED, and ReleaseBlockedTarget
ConfirmUnsentOperation      Broker; REQUESTED → CONFIRMED; precondition the broker's usage entry of the TaskRun holding it committed (ExpectUsage) ∧ Unsent(op) ∧ no ledger entry and no ISSUED or BROKER_ACCEPTED permit names it ∧ it is in the ambiguous_operation_set of its installation's RestoreLineage ∧ a provider lookup under its operation_key, taken with that lineage FENCED, RECONCILED or MAPPED, finds it applied; its EffectReceipt records the remote_identity the lookup returned; its EFFECT BudgetReservation, if any, UNKNOWN until its usage settles, then COMMITTED, or EXPIRED by explicit conservative expiry, and the broker's UsageReceipt includes the effect; then ReleaseBlockedTarget
BlockUnsupportedOperation   Broker; REQUESTED | PERMITTED → BLOCKED_UNSUPPORTED; precondition the capability lacks a required semantic ∧ send_attempt = NONE

\* plans and evidence
VerifyPlanSnapshot · VerifyGraphMembers · RecordGraphActivationReceipt · AdmitGraphMember · InvalidateEvidence
FailGraphActivation         precondition no activation command submitted, or its receipt REJECTED; never while it is UNCERTAIN
RecordEvidenceBundle        refuses a reviewer below reviewTier of the class, a reviewer in the worker's session, a correlated review as the required review of a class other than REVERSIBLE, and a required SECURITY_OR_DATA_INTEGRITY review from a configuration outside securityReviewers; a missing required check is covered only by an APPLIED, unexpired EXCEPTION answered by the no-test approver for that check and candidate, listed in exceptions
AcceptPlanRevision          Plan controller; a revision proposed on the Plan (RevisionPending), pinned to that revision and its snapshot digest; refuses every principal but WorkContext.spec.revisionAuthority
RecordAcceptanceAdjudication   Task controller; precondition every bundle it references RECORDED, unexpired and at the current remote generation; records the uid and digest of each

\* charter
AcceptCharterRevision       human principal only, every entry accepted; refuses a law marked advisory and a project entry that relaxes an inherited one
PinCharterRevision          at plan acceptance; refuses acceptance without an accepted charter revision
ComputeEffectiveCharter     union of the context and project entries; a conflict resolves toward the stricter entry
DenyCharterViolation        mechanical entry: broker refusal before effect, or the candidate ineligible at the checkpoint
ApplyTightenedLaw           new or tightened law: in force at once for the broker and for evidence; fresh capsule at the next continuation, the old one revoked
AskJudgedQuestion           violates above the threshold → candidate blocked, Finding opened; any other answer or none → the review backstop alone

\* intake (admission and the Intake controller)
AdmitIntakeWrite            intake-submitter identity: intake kinds in proposal state only; refuses an attribute without provenance, a Charter entry with any provenance entry not INTERVIEW_ANSWER, a proposal other than the configuration that references an Intake in the intake namespace, and every accept
VerifyRepositoryBinding     condition ForgeVerified on a PROPOSED Repository only on an authenticated forge-adapter answer matching it
VerifyBriefDigest           over the stored content, or the forge adapter's answer for a path at a commit
ProposeIntake               Intake CAPTURED → PROPOSED only when every held proposal is valid, every named repository ForgeVerified and every brief digest verified; any new or revised held proposal returns it to CAPTURED
AcceptProposal              from PROPOSED only, pinned to the Intake revision and proposal-set digest: by the principal ONBOARD §1 step 6 names for it: a context configuration creates the WorkContext, ends the intake-namespace Intake ACCEPTED and creates its bound successor in the context namespace; a plan proposal creates the Plan and moves its projects and repositories to ADOPTED
RejectIntake                by the principal who may accept that Intake (ONBOARD §1 step 6) only; rejects every proposal it holds in proposal state

\* scope, identity, fencing, continuation
IssueScopeCapsule · CanonicalizeScopeCheck · DenyOutOfScopeAction · DetectOutOfScopeAtCheckpoint
QuarantineOutOfScopeWorkspace · LinkFindingHistorically
IssueExecutionIdentity      one per AgentRun, bound to its TaskRun, workspace (NONE if it holds none) and the TaskRun's execution_epoch
IssueCredentialGrant        one per ExecutionIdentity, carrying only its role's requests (ROLES §4); refused while a RestoreLineage of the installation is not DISPATCH_ENABLED
RevokeCredentialGrant
BeginFence                  TaskRun control; fence_state ACTIVE → FENCE_PENDING and execution_epoch+1 in one CAS; the FenceSession at the new epoch created PENDING
ApplyWorkspaceWriteFence
ConfirmFence                FenceSession PENDING | UNCERTAIN → CONFIRMED at the TaskRun's execution_epoch; precondition process, workspace and broker fenced ∧ every grant of an AgentRun of the TaskRun REVOKED ∧ fence_settled; the TaskRun's fence_state FENCE_PENDING | FENCED_UNCERTAIN → FENCED acknowledges it
MarkFenceUncertain          FenceSession PENDING → UNCERTAIN at the TaskRun's execution_epoch, when a fence check cannot be confirmed; fence_state FENCE_PENDING → FENCED_UNCERTAIN acknowledges it
CreateCheckpoint
VerifyCheckpoint            AgentCheckpoint CREATED → VERIFIED; precondition its execution_epoch = the TaskRun's ∧ its scope_digest = its AgentRun's capsule_digest (KERNEL §9) ∧ a VERIFIED CustodyCheckpoint whose agent_checkpoint_digests holds its digest, for a session with a workspace (KERNEL §7)
RequestContinuation
StartContinuation           session_sequence+1 on the same AgentRun; precondition an AgentCheckpoint of it VERIFIED (so not STALE) at the TaskRun's execution_epoch ∧ fence_state = ACTIVE
ResumeOpenInvocation        under the invocation's original operation identity only, by a continuation of its AgentRun or by a later attempt that inherited it, with that attempt's grant and the operation's original EFFECT reservation; the send's cost on the sending TaskRun's broker UsageReceipt (KERNEL §9)
FenceOnUnverifiableCheckpoint   BeginFence on the TaskRun; precondition continuation_deadline passed ∧ no AgentCheckpoint of the AgentRun VERIFIED at the current execution_epoch

\* custody and restore
BeginCustodyCheckpoint      Workspace IN_USE → PRESERVING at the CustodyPolicy cadence or run end, once its write fence is confirmed; the CustodyCheckpoint records agent_checkpoint_digests
InventoryWorkspace · UploadCheckpoint · VerifyIndependentRestore
RecordArtifactCommit        PENDING → VERIFIED; precondition its CustodyCheckpoint VERIFIED with the completion marker written
PreserveWorkspace           PRESERVING → PRESERVED; precondition an ArtifactCommit VERIFIED for a CustodyCheckpoint begun in this PRESERVING
ResumeWorkspace             PRESERVED → IN_USE, lifting the write fence; precondition the TaskRun holding it non-terminal ∧ its fence_state = ACTIVE ∧ retire_only unset
RetireWorkspace             PRESERVED → RETIRED; precondition state = PRESERVED (write fence held since that checkpoint) ∧ no unadjudicated WorkspaceConflict
QuarantineWorkspaceConflict · AdjudicateConflict
RestoreState                RestoreLineage REQUESTED → RESTORING, then RESTORING → READ_ONLY once restored; every restored WorkContext's dispatch_authority_generation := NONE, which matches no pin; records ambiguous_operation_set
VerifyRestoreWitness · RecordRestoreWitnessReceipt · FenceOldInstallation
ExpireOldGrant · MapRestoredIdentity
EnableRestoreDispatch       MAPPED → DISPATCH_ENABLED on the COMMITTED receipt of AdvanceDispatchAuthorityGeneration, which the Custody controller submits only from MAPPED; precondition a recorded RestoreWitnessReceipt for this installation_id and restore_generation whose digest is that of ambiguous_operation_set ∧ every operation in it terminal, adjudicated, or with no send_attempt and proven not applied by lookup or deduplication under its operation_key ∧ every old grant revoked or max_old_grant_ttl + broker_revocation_bound passed

\* canonical records
ExpectUsage                 TaskRun domain; usage[producer] := PENDING, with record_deadline := now + the record_deadline bound if the TaskRun is terminal; the producer's first model call or accepted effect follows its COMMITTED receipt
StartRecordDeadlines        TaskRun domain, in the commit that moves the TaskRun to a terminal phase; record_deadline := now + the record_deadline bound for the outcome and every usage entry
WriteOutbox · DrainOutbox
RecordCanonicalRecord       entry PENDING → RECORDED on its own record's COMMITTED create receipt (outcome: the OutcomeRecord; usage[p]: the UsageReceipt with producer = p ∧ replaces = NONE, or one with replaces = p)
CreateGapForMissingRecord   precondition the entry PENDING ∧ its record_deadline ≠ NONE ∧ now ≥ it; creates a TelemetryGap linked to the TaskRun, naming the entry's producer for USAGE_MISSING, entry := GAP(uid), its gap_uid := uid
CloseGap                    TelemetryGap OPEN → CLOSED, closing_record_uid := the entry's record committed after it; the entry stays GAP
CensorUsage · SettleUsage

\* judgment
ComputeEligibleSet · AbstainDecision
RecordDecision              Decision controller only; commits RECORDED only; precondition selected ∈ the eligible set whose digest it records, computed before the question
ClassifyFinding             a RecordDecision of the finding-severity question class (ROLES §2) over the eligible set computed from the finding's fields; an absent judge selects the highest eligible severity; the Finding's CLASSIFIED and the Manager's disposition are outside the model (§2)

\* interventions: no actions of their own (§2); each becomes the kernel action listed
HOLD, KILL_SWITCH           RequestHold
RESUME of a hold            ReleaseHold, then CompleteHoldRelease once hold_causes is empty
RESUME of a QUIESCE or SUPERSEDE   ResumePlanRevision, only while Plan.phase is QUIESCING and no budget-exhausted pause is in force (ROLES §5)
QUIESCE                     QuiescePlan
SUPERSEDE                   QuiescePlan, then SupersedePlanRevision; under a budget-exhausted pause only for a replacement whose budget policy raises the ceiling
QUIESCE, FAIL under a budget-exhausted pause   none: REJECTED (ROLES §5)
FAIL, CANCEL                QuiescePlan if the register holds the revision ACTIVE, then RetirePlanAuthority once the Plan is terminal, if it holds entries
ADJUDICATE_OPERATION        AdjudicateUnresolvedOperation
ADJUDICATE_CONFLICT         AdjudicateConflict
EXCEPTION                   none of its own: an APPLIED one enters only as an EvidenceBundle.exceptions entry (RecordEvidenceBundle)
PAUSE, RESUME of a PAUSE    none: they change only Plan.phase, which gates TaskRun admission, revokes no accepted effect and weakens no hold (KERNEL §5), so no invariant of §4 depends on them
                            (the role a principal needs for each action, ROLES §5, is admission's and the owning controller's check, assumed by KERNEL §11 and not modelled)

\* environment
DetectFault · CrashProcess · PartitionAPI · ProviderTimeout · DuplicateDelivery · ReorderDelivery · Tick
```

## 4. Safety invariants

Each is a property of the bounded model and maps to a guard in §3 and to a fixture group in `AUTOBOT-M0-AND-GATES.md`. They are grouped under the thesis invariant they make precise.

**I-1 Commit**
- F-1 *Idempotency.* One replay identity commits at most one payload to at most one aggregate revision; a differing payload or principal under the same key is rejected.
- F-2 *Receipt durability.* Every committed command has exactly one `COMMITTED` receipt whose identity no later revision can erase; a `PREPARED` receipt never implies commitment.
- F-3 *Receipt barrier.* No domain commit lands on an aggregate whose pending slot is `OCCUPIED` or `REPAIRING`; a slot is cleared only after its receipt and event are verified.
- F-4 *Lane separation.* A domain commit changes no control field; a control commit changes only control fields, `control_revision` among them, preserves the slot and every domain field, and appends its receipt in the same CAS; either lane may also write the structural fields `commit_sequence`, `last_receipt_ref`, `observedGeneration` and `conditions`, which are in neither digest; a reconciliation-only CAS changes only reconciliation fields and no revision; `commit_sequence` is strictly increasing across both lanes; a full ring refuses rather than drops.
- F-5 *Create identity.* A create resolves to one object with origin metadata equal to its receipt; a lost acknowledgement never produces a second object.
- F-6 *Projection order.* A projection applies event k only after every event < k; a same-identity different-digest event is rejected and quarantined; a gap is visible until repaired or declared permanent.

**I-2 Authority**
- F-7 *Register linearization.* For every accepted permit p and cut c on one context, `commit_sequence(AcceptDispatch(p)) < commit_sequence(c)` or p is never accepted; and at acceptance every pinned value equals its register.
- F-8 *Hold.* Only permits accepted before `RequestHold` complete; every permit issued under a previous `RUNNING` generation fails after `ReleaseHold` and `CompleteHoldRelease`.
- F-9 *Plan generation.* No permit pinning `(revision R, generation g)` is accepted unless the register holds `(R, ACTIVE, g)`; every `ActivatePlanRevision`, `QuiescePlan`, `ResumePlanRevision` and `SupersedePlanRevision` changes the generation, so no permit issued before any of them is accepted after it — including a permit issued before a quiesce and presented after a resume of the same revision; and no permit is issued while the register phase is not `ACTIVE`.
- F-10 *Basis.* A merge permit pinning generation g is not accepted after `InvalidateIntegrationBasis` advanced it.
- F-11 *Manager epoch.* A target commits a Manager command only against the reservation held for that command, taken and claimed while `phase = ACTIVE` at the epoch the command pins, not yet resolved or cancelled, and only while that epoch is still the register's epoch; a reservation is taken and claimed only while `phase = ACTIVE`; no permit is accepted between `AdvanceManagerEpoch` and `ResumeManager`; after `ResumeManager` no permit pinning the old epoch is accepted; a reservation is released only with a recorded terminal receipt: the target's receipt, the cancel's receipt, or, for a `RESERVED` reservation cancelled during a takeover, the control receipt of the `DrainManager` that drained its entry.
- F-12 *Register acknowledgement.* No `Plan`, `IntegrationBasis` or `ManagerLease` status claims a revision, generation or epoch the register does not hold; only the Context controller writes `WorkContext` status.
- F-13 *Graph activation.* No member is ready or admitted before a verified `ACTIVATED` snapshot and matching register.
- F-14 *Routing pin.* A `TaskRun`'s routing pin, consequence class and floor never change; a permit's pin equals its `TaskRun`'s.
- F-15 *Restore barrier.* A restored installation dispatches nothing before a signed witness receipt, old-grant expiry or revocation, identity mapping, and reconciliation or adjudication of every operation in the ambiguous set.

**I-3 Custody**
- F-16 *Custody commit.* A workspace is `PRESERVED` only after an `ArtifactCommit` `VERIFIED` by independent restore of a custody checkpoint taken under its write fence, and the fence holds while it is `PRESERVED`, so no write is made after the checkpoint it rests on; retirement requires `PRESERVED`, so a workspace that returned to `IN_USE` is retired only after a later checkpoint; uncertain custody is `QUARANTINED`.
- F-17 *Conflict.* Mixed ownership or incomplete attribution forces a `WorkspaceConflict` and prevents retirement and assignment until adjudicated.

**I-4 Effects**
- F-18 *Send-attempt write-ahead.* No remote send without a durable `send_attempt`; after a crash, an operation with no send attempt is sent at most once under its current `attempt_index` and one with a send attempt and no outcome is never resent; a re-request moves the earlier `send_attempt` to `prior_send_attempts`, never deletes it.
- F-19 *Ledger conservation.* Every accepted permit — one a `COMMITTED` `AcceptDispatch` receipt names — has exactly one ledger entry until its operation has durably recorded acceptance and a terminal or `OUTCOME_UNKNOWN` state, or has durably returned to `REQUESTED` on a currency failure, or on a register or currency re-validation failure at recovery row 1, with no `send_attempt` present, in which case the permit is, or is recovered to, `INVALIDATED`; no ledger holds two entries for one permit or one operation; a `DISPATCHING` operation always has an entry; no reachable state has an `ACCEPTED_NOT_SENT` entry beside a present `send_attempt`, and one observed after a storage or restore fault outside the model is quarantined; acceptance is refused once ledger entries and blocked-target pairs together fill the ledger's capacity, except for an operation that already holds a pair; no operation is `OUTCOME_UNKNOWN` without its pair.
- F-20 *Unresolved retention.* An `UNRESOLVED` operation is never deleted or automatically retried, blocks its dependents (as KERNEL §3.3 defines them) and plan completion, and leaves only by human adjudication.
- F-21 *Effect key.* `operation_key` is a function of its five inputs; equal keys with differing payload, target or contract quarantine and never dispatch; every committed intent materializes to exactly one operation, recoverably.
- F-22 *Tool identity.* A tool invocation is retried only under its original identity; stream closure never becomes `FAILED`.

**I-5 Scope**
- F-23 *Canonical scope.* A verdict is computed on canonical path and inode; a mutation outside the capsule by symlink, hardlink, rename, mount or subprocess is denied before effect or detected at the next checkpoint before any evidence, and the workspace is quarantined.
- F-24 *Finding isolation.* Linking a finding to a task changes no capsule, contract or evidence requirement.
- F-25 *Real fencing.* A fenced process cannot write its workspace, use a revoked grant, invoke a privileged tool or produce current evidence; `FenceConfirmed` requires that no ledger entry names an operation of the fenced run and that none of its operations, inherited ones included, is `OUTCOME_UNKNOWN` or `RECONCILING`; uncertainty blocks replacement.
- F-26 *Continuation.* A continuation has the same `AgentRun`, identity, grant lineage, capsule digest and reservation, except that a law added or tightened since its capsule was issued gives it a fresh capsule differing only in charter digest and entries; cumulative counters are non-decreasing; an open invocation is resumed, never re-issued.
- F-38 *Charter pin.* No plan revision is accepted without an accepted charter revision; it pins the accepted `ProjectCharter` revision and the `Charter` revision it inherits, with their digests; an accepted revision never changes; every capsule carries the charter digest of the charter in force at its issue and the entries relevant to its task.
- F-39 *Tighten-only.* The effective charter is the union of the context and project entries, a conflict resolved toward the stricter entry; no project entry relaxes an inherited one; no law is `advisory`; a relaxed law or a changed rule reaches no plan revision pinned before the change.
- F-40 *Mechanical laws.* A brokered effect that violates a `mechanical` entry of the charter in force is refused before effect, and a checkpoint diff that violates one makes the candidate ineligible for evidence; a new or tightened law is in force for all work not yet accepted from its acceptance, so no `EvidenceBundle` bound to an earlier charter digest satisfies an acceptance, every later capsule carries it, and an active attempt receives it through a fresh capsule at its next continuation that differs from the old one only in charter digest and entries, the old one revoked.

**I-6 Evidence**
- F-27 *Fresh evidence.* Acceptance binds candidate, base and head, contract, environment, provider run, reviewer identity and current remote generation; any change invalidates; a delayed result for an old basis satisfies nothing.
- F-28 *Independence.* The reviewer identity of an accepted bundle differs from the worker's session; a `correlated` review is required-review evidence only for `REVERSIBLE`; the required review of `SECURITY_OR_DATA_INTEGRITY` work comes only from a configuration in `securityReviewers`; `WorkerFinished` never implies `TaskAccepted`.
- F-29 *Integration.* Every milestone's change sets have one current basis and merge order; milestone acceptance references the integrated candidate of its final basis.

**I-7 Projection** — F-30 No external observation changes an aggregate without a controller CAS; forge text never becomes a command without actor validation.

**I-8 Judgment**
- F-31 Every `Decision.selected` is a member of the eligible set computed before the question; no decision grants credential, scope, budget, acceptance or merge; an absent judge takes the conservative branch and widens nothing.
- F-41 *Charter authority.* Only a human principal accepts a charter revision or any of its entries; a model output never promotes a candidate entry; no law is waived, and no agent grants a waiver.
- F-42 *Block-only judgment.* A `judged` answer can only block: "violates" above the entry's threshold blocks the candidate and opens a `Finding`; "complies" satisfies nothing the `review` backstop has not; an outage or abstention leaves the backstop as the only check, never a pass.
- F-43 *Proposal authority.* An accept of a context configuration or a plan proposal, and an `intake reject`, commits only from the principal ONBOARD §1 step 6 names for it, never from the intake-submitter identity; each commits only at the `Intake` revision and proposal-set digest it pins, and a plan accept only from `PROPOSED`; the intake-submitter identity commits nothing but intake kinds in proposal state, and nothing it writes is canonical before an accept.
- F-44 *Provenance and binding.* Every attribute the intake-submitter identity writes carries at least one provenance entry with a source, a content digest and a trust label, and every provenance entry of a `Charter` entry it writes is `INTERVIEW_ANSWER`; an `Intake` is `PROPOSED` only while every repository it names is `ForgeVerified` and every brief digest is verified; a `Repository` is `ADOPTED` only by a plan accept of an `Intake` that was `PROPOSED` with it `ForgeVerified`.

**I-9 Ledger** — F-32 Every entry of `expected_records` — the `outcome` entry and every `usage` entry — whose `record_deadline` has passed is `RECORDED` or `GAP`; no entry's `record_deadline` is set while its `TaskRun` is not terminal; and no `TaskRun` is counted by any outcome or cost computation while any of its entries is `PENDING`; a `CENSORED` receipt carries `min(reservation ceiling, rate-card bound)` and counts at it; an `UNKNOWN` reservation stays held.

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
| a `correlated` review accepted as the required review of `COMPATIBILITY_RISK` work | F-28 |
| a required `SECURITY_OR_DATA_INTEGRITY` review accepted from a configuration outside `securityReviewers` | F-28 |
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
