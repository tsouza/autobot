# AutoBot — kernel: background

Why `AUTOBOT-KERNEL.md` is shaped the way it is. Sections follow its § numbers.

## Preamble

Kubernetes gives atomicity on one resource and nothing wider. Every rule is built from that primitive. A lease token checked outside the committing write, a controller-local mutex, or a journal replayed into state would each put authority somewhere a crash or a stale process can leave it wrong. Each section names the invariants it serves so the thesis admission rule stays checkable: a mechanism with no invariant behind it does not enter the core.

## 1. Aggregates and commit lanes

- **Pending slot in the commit CAS.** If the receipt were a separate write only, a crash between the commit and that write would leave a committed change with no receipt, which breaks I-1. The slot puts the whole receipt, audit envelope and effect intents into the commit itself, so any new process can finish the job.
- **No clearing on elapsed time.** A slow receipt store is ordinary. Clearing a slot on a timer would drop the only copy of a committed receipt at exactly that moment. A timed-out write may still have landed, so a timeout counts as `UNCERTAIN` and not as rejection.
- **Why a control lane exists.** Safety controls have to move while a domain slot is unresolved. The typical case is a hold requested while the receipt store is down. If holds used the domain lane, the receipt barrier would block them just when they are needed most.
- **Control receipts in an embedded ring.** Because the receipt is inside the control CAS, a control commit needs no second write to be durable, and a hold does not depend on the audit store being up. A full ring refuses new transitions and never drops an entry, because dropping one would silently lose a committed receipt. Refusing is visible, and it is safe for a safety lane.
- **Field partition.** If one CAS could touch both lanes, a control commit could change the domain digest under an occupied slot. Domain repair would then fail its after-digest check on a healthy aggregate. Admission rejecting mixed CASes makes that impossible by construction.
- **Reconciliation fields, a third class.** The dispatch ledger's `send_state` and the entry's removal record progress on an intent that is already committed. Two other placements were considered:
  - As a domain commit, every send would occupy the pending slot and wait on a receipt barrier. That adds a receipt round-trip to every effect.
  - As a control commit, effect progress would sit on the safety lane and compete with holds for ring capacity.
  A class outside both digests keeps domain repair sound, because nothing in it can move the domain digest. It is safe to lose a write in this class, because every such write can be re-derived from receipts and operation aggregates.
- **Whole ledger entries are reconciliation fields.** If an entry's identity fields were domain while its `send_state` was reconciliation, removing the entry would change the domain digest. The whole entry is therefore one class. It is appended only by the `AcceptDispatch` domain commit that carries it in its receipt.
- **Structural slot body and ring.** If the slot body counted as domain, the after-digest would cover itself. If a ring append counted as domain, a control commit would write a domain field. Keeping both outside every class removes both contradictions.
- **Audit ordering.** Events can arrive late or twice. A per-aggregate `commit_sequence` gives projections one order to apply events in, a bounded buffer covers transient gaps, and a digest mismatch on the same identity points to corruption or forgery, which is quarantined. Audit never rolls back an aggregate, because the resources are authoritative and audit only describes them.

## 2. Commands and receipts

- The replay identity includes the principal and input digest. Reusing a key with a different payload or principal is therefore a conflict and can never pass as a replay.
- A retry after the window is `REPLAY_EXPIRED` and not new intent. A late duplicate of an old request must not commit a second time.
- A retry is never rebased. Rebasing would apply a decision made against one revision to a state its author never saw.
- A CAS conflict does not prove rejection. The conflict may have been caused by an earlier transmission of the same command that did land.
- Creates use deterministic names so a lost acknowledgement can be resolved with a GET. A generated name would turn every ambiguous create into a possible duplicate. A name's absence after an ambiguous delete or restore can be a transient view, so it never authorizes a recreate.

## 3. Dispatch

- **One aggregate for all registers.** I-2 needs every acceptance and every authority cut to be totally ordered. With all registers on `WorkContext` and every change a CAS there, that order is `commit_sequence`. No cross-resource protocol can give that guarantee. `Plan`, `IntegrationBasis` and `ManagerLease` only acknowledge register values, so there is never a second source to disagree with the first.
- **Routing is a pin, not a register.** A TaskRun's worker strategy is fixed at admission and never changes afterwards. Acceptance only has to compare two immutable values. Configuration changes affect future admissions only, so they never race with dispatch, and the register set stays small.
- **Ledger updates are reconciliation requests.** They carry no new intent. A request is keyed by the `send_state` it expects to find, so a delayed or duplicated request is a no-op. Revisions and receipts would add nothing to that.
- **A permit is only a capability to request.** Validity is decided in the acceptance CAS against the current registers. Any staleness between issuance and use is therefore caught at that point.
- **Generation on both quiesce and resume.** If resume did not increment `plan_generation`, a permit issued before a quiesce would match the register again after the same revision resumed. Quiesce and resume together are one guard; removing that guard produces the counterexample.
- **Currency is not a register.** Grants, reservations and capsules are per run and change often. Putting them in registers would serialize every run's churn through the context aggregate. The broker checks them before `RecordSendAttempt` and on every privileged call, which closes the window between acceptance and send.
- **Releasing the entry on a currency failure.** If an operation went back to `REQUESTED` and its entry stayed, the entry would never be removed. Each fence between acceptance and send would leak one entry until the full ledger refused every acceptance in the context.
- **2a before 2b.** Recording the entry's `SEND_ATTEMPTED` before the operation's `send_attempt` lets recovery tell every crash position apart:
  - before 2a: never sent;
  - between 2a and 2b: the entry says `SEND_ATTEMPTED` and no `send_attempt` exists, so the send is unknown and handled as row 2 of the §3.3 recovery table;
  - after 2b: sent or unknown.
  The reverse order would allow a present `send_attempt` next to an `ACCEPTED_NOT_SENT` entry, which recovery could not read.
- **Order of the release writes.** The entry is removed, then the permit is invalidated. These are two resources and there is no transaction between them, so the order is fixed and the intermediate state has a defined recovery reading.
- **Operation key.** It covers installation lineage, aggregate, revision, index and payload. The same logical effect gets the same identity across retries and restores, which is what provider-side deduplication keys on.
- **Materialization by scan.** Watches can miss events. Scanning slots and retained receipts means no committed intent is lost to a missed notification.
- **Negative search.** Provider search indexes can lag. Not finding an effect does not show it was never applied, so only a lookup or deduplication proof justifies a re-request.
- **`UNRESOLVED`.** An effect that cannot be disambiguated is kept, blocks its dependents, and is never counted as success or failure. Counting it either way would bias the ledger's denominators (I-9). "Later" is defined relative to the `UNRESOLVED` CAS because there is no order across aggregates.

## 4. Manager serialization

- The authority lives in the register. A process holding a lease document proves nothing, and the Manager process is untrusted.
- **Drain before advance.** If the epoch advanced while a reserved command was still in flight, the old command could commit after takeover. Draining first and resolving or cancelling the reservation removes that interleaving.
- The three steps stay separate CASes. The field partition forbids changing phase and epoch in one write, and no permit is accepted in the window between advance and resume.
- A command's expected revision is fixed. Refreshing it would force a decision made against an old state onto a new one.
- Lease expiry is based on the clock. A slow old holder may still be in the middle of a write when its lease expires, so expiry starts a drain and never skips one.

## 5. Plan activation and supersession

- Activation is a register cut. As a result, "no R1 effect after R2" follows from ordering and needs no policy check.
- Every readiness decision verifies the activation receipt, so a partially created graph cannot produce ready work.
- There is no in-place revision. Running attempts keep the contract and capsule they started with, and evidence binds that revision.
- Basis generation lives in the register so a merge permit is invalidated at the same point as a hold.
- Branches that pass on their own can fail once combined. For that reason, milestone acceptance is judged on the integrated candidate.

## 6. Identity, grants and fencing

- An agent that holds a reusable credential outlives its authority. Short-lived grants bound to identity and epoch expire with the attempt.
- An epoch number alone is not fencing, because a process that ignores the epoch can still write. Fencing needs revocation, isolation and verification.
- `FENCED_UNCERTAIN` blocks replacement. Otherwise two writers could act on one workspace at once.

## 7. Custody and restore

- A clean git status or a merged PR says nothing about ignored or untracked files, stashes, local refs or the record outbox. Custody inventories all of them.
- A matching upload digest does not prove the content can be restored. Only an independent restore into a fresh location does.
- A restored installation cannot tell whether its predecessor is still running. The fence evidence therefore comes from an external witness in a separate failure domain. When active revocation of old grants cannot be proven, waiting out their maximum lifetime gives the same guarantee.
- Operation keys are preserved across a restore so provider deduplication and reconciliation still match the original sends.

## 8. The canonical-record obligation

- Telemetry can drop data. If outcomes and costs lived there, lost records would quietly disappear from quality and cost denominators, which breaks I-9. As aggregates with an outbox, they either land or become a linked gap.
- A record is written to the outbox before completion is reported. An API outage at completion then delays the record and never loses it.
- An unsettled cost is censored at its upper bound. This is conservative for cost and counts as non-success for quality, so an unknown can never make a strategy look better.

## 9. Continuation

- Giving a continuation a fresh identity would reset attempt and spend counters and orphan open tool invocations. That would bypass budgets and could duplicate effects. Continuing on the same `AgentRun` from a verified checkpoint preserves all of these.

## 10. Lifecycles

- Printing every machine in one place makes stray states mechanically detectable. Any state name outside this section is a defect.
- Events describe transitions and states describe resting points. Keeping the two vocabularies apart stops an event name from being mistaken for a state.

## 11. Scope of guarantee

- The receipt barrier and the single-slot Manager reservation trade per-aggregate write availability for I-1 and I-2. The cost is real, so it is measured before concurrency is raised and is never assumed away.
