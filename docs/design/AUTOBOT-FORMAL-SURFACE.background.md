# AutoBot — formal surface: background

Rationale for `AUTOBOT-FORMAL-SURFACE.md`, by the same section numbers.

## Status

The document is written ahead of the stack it describes. Treating a missing model, check or refinement test as a design gap would make every design review fail on schedule rather than on substance, so the absence is classified as a planned artifact (§9) and the review looks instead at whether the states, actions and guards are precise enough to be modelled.

## 1. Shape

Liveness is stated as conditional because the kernel depends on parties it does not control — the API server, providers, CI, humans. An unconditional progress claim would be false the first time any of them stays down; a conditional one with a named degraded branch is both true and testable.

The model deliberately stops at bounded abstract values. Source trees, logs, tokens and CI output are unbounded and carry no protocol decision; modelling them would blow up the state space without adding a single guard to check.

Exactly two transitions commit canonical state, one per lane, so that every canonical change has one receipt and one `commit_sequence` position. Progress of an intent that is already canonical — clearing a pending slot, publishing a ring entry, advancing or removing a dispatch-ledger entry — needs a write too, and neither lane fits it:

- As a domain commit, each ledger `SEND_ATTEMPTED` write would occupy the pending slot and put a receipt barrier in front of every send.
- As a control commit, effect progress would travel on the safety lane that holds, fences and receipt repair depend on.

The reconciliation-only CAS is the third class: it touches only reconciliation fields, which sit outside both digests, so it can make nothing canonical and cannot disturb a domain repair that re-verifies the domain digest.

Audit events, effect materialization and projections are downstream of the commit. Letting them roll back or reconstruct an aggregate would create a second source of truth; restricting them to explaining or repairing a missing record keeps the aggregate the only authority.

Extensions sit outside the model so that the kernel's safety claims do not silently widen to cover learning, policy, mirroring, observability or the CLI, none of which may carry authority the kernel does not grant.

## 2. Typed correspondence

The field-by-field correspondence with the M0 schema makes the refinement mapping mechanical: a Rust field either has a model counterpart or is visibly outside the model. A kernel field the model lacks would be a place where the implementation can hold state the checker never reasoned about, which is why it counts as a model defect rather than an abstraction.

Every `state` domain is taken from KERNEL §10 rather than restated here, so that a lifecycle has one printed machine and no second copy can drift.

## 3. Actions

A bare action name says that a transition exists, not when it is allowed; the guard is what a checker exercises and what a negative variant removes. A list of unguarded names would look like coverage while constraining nothing.

`ResolveReservedCommand` and `CancelReservedCommand` are CASes on the reserved command's target by its owning controller, with the slot released afterwards through a Context command. Placing them on the target keeps each kind written only by its owner; the Context controller never writes another controller's status.

`AdmitTask` stands under its own heading because admission belongs to the TaskRun controller, not to the Context controller's register CASes. `AdmitTask` and `RecordEvidenceBundle` print their floor and review-tier refusals on the action line so that F-37 visibly maps to a guard.

`AcknowledgeDispatch` releases a ledger entry in three situations, including the return of an operation to `REQUESTED` after a currency or register re-validation failure with no `send_attempt`. Without that third case such an entry would have no release rule and would hold ledger capacity for ever; setting the permit `INVALIDATED` in the same reconciliation keeps the permit from being presented again.

`ValidateCurrency` includes the check for an `UNRESOLVED` operation with the same target identity. This is the point where a dependent of an unresolved operation is refused before any send, matching the other two refusal points KERNEL §3.3 names.

## 4. Safety invariants

Grouping each F-n under the thesis invariant it makes precise gives traceability in both directions: every I-n has at least one F-n, and every F-n is exercised by exactly one fixture group in `AUTOBOT-M0-AND-GATES.md`.

F-9 is worded as a register-equality invariant — a permit is accepted only if the register holds exactly `(R, ACTIVE, g)` — with a separate clause forbidding issuance while the phase is not `ACTIVE`. A wording based on "a CAS that advanced the generation beyond g" does not catch a permit issued during `QUIESCING` and presented after a resume that forgot to advance the generation; the equality form does.

F-11 ties a Manager command to the specific reservation taken for it while `phase = ACTIVE` at the pinned epoch, not to the phase at commit time. Takeover drains first and then resolves the already-reserved command while the phase is `DRAINING`; a rule requiring `phase = ACTIVE` at commit would either fail on every such resolution or refuse it and deadlock takeover. New reservations still require `ACTIVE`, so the drain admits no further old-holder commands.

F-19 names the currency and re-validation release paths explicitly. The ledger's conservation rule has to account for every way an entry leaves; an unlisted path would either be an invariant violation in a correct system or a leak the invariant tolerates.

F-32 is conditional on `record_deadline` having passed. A terminal `TaskRun` with `PENDING` records is a legal state until the deadline — the outbox writes records after the run ends — so an unconditional form would fail on correct traces and make its negative variant meaningless.

F-37 exists so that I-10 has a formal counterpart. Floor and independence are otherwise only implied by F-14 and F-28, neither of which says an admitted worker is at or above the floor or that a reviewer meets the review tier.

## 5. Negative variants

A check that passes is only informative if the same check fails when the protected guard is removed; otherwise the invariant may be true for reasons unrelated to the guard, or trivially true. Each row therefore names the one invariant its removal must break.

Quiesce and resume both advance `plan_generation`, and issuance is refused outside `ACTIVE`. Removing the advance on resume alone produces no reachable counterexample, since the quiesce has already changed the generation. The variant removes the advance on quiesce and resume as one guard: a permit issued at `(R, ACTIVE, g)` survives `QuiescePlan` and `ResumePlanRevision` with the register back at `(R, ACTIVE, g)`, and F-9 fails.

## 6. Conditional liveness

Each bound is named, finite and paired with a terminal branch for the case where the dependency never recovers. The branch is what keeps a stuck dependency visible — `RECOVERING`, read-only, quarantine, `UNRESOLVED`, `FENCE_PENDING`, escalation, a gap, a censored usage — instead of leaving work in an unnamed limbo. The bound names match the kernel's so that the refinement mapping can carry them across unchanged. Values live only in the M0 profile.

External users, providers and CI are allowed to stay unavailable for ever because nothing in the system can compel them; the guarantee is that the kernel's own state stays explained, not that the outside world answers.

## 7. Bounded model plan

Two of each actor that can race — repositories, milestones, workers, concurrent attempts, Managers, installation identities — is the smallest configuration in which ordering, takeover, cross-repository integration and restore-under-old-identity interleavings exist at all. The fault list names the specific interleavings the dispatch, commit, custody and restore protocols are built to survive, so that each is reached by the checker rather than assumed.

## 8. Stack and phases

Quint gives a typed, executable specification that can be simulated and produce readable counterexample traces; Apalache checks it symbolically over the bounded configuration; TLC, a separate explicit-state engine over TLA+, cross-checks a small state space so that a result does not rest on a single tool. PlusCal may make procedural sections readable but is never allowed to become a second source of the protocol. Dafny or Lean suit the pure sequential pieces — digests, identity derivation, manifest completeness — and stay out of the runtime so the operator carries no proof-tool dependency.

Authority runs one way. The model may abstract Kubernetes, providers and storage, since those are trusted or observed rather than decided; the implementation may not add authority the model lacks, because an unmodelled authority is exactly what the checks cannot see.

The G-FORMAL evidence is invalidated on any change to protocol, reducer or model because a check result is only meaningful for the exact artifacts it ran against.

## 9. Review classification

Only `DESIGN_DEFECT` and `DESIGN_GAP` block design closure because they are the two classes that no amount of implementation can fix: a contradictory or missing contract must be settled in the design. The other classes describe scheduled work, implementation bugs or declared deferrals. Reporting design readiness and implementation readiness separately keeps a finished design from being read as a finished system, and a missing artifact from being read as a broken design.
