# AutoBot — glossary: background

Why `AUTOBOT-GLOSSARY.md` is shaped as it is, organized by its sections.

## Layers and the dependency rule

CORE is sized by safety. A CORE term is one whose removal makes AutoBot unsafe, not merely less capable. Capability that can be absent while every invariant still holds belongs in an extension, which keeps the core small enough to be stated precisely, modelled, and implemented first in M0.

A CORE definition never names an EXTENSION or EXTERNAL term, not even in passing. A definition that mentions an extension makes the core depend on it, and the reading order places extensions last with no core document depending on one. A passing mention is the usual way such a dependency enters unnoticed, so the rule covers mentions as well as dependencies.

An extension never creates authority the core does not grant. Extensions can be disabled, replaced or absent; any authority held only by an extension would disappear with it, and the core invariants would stop describing the running system.

An EXTERNAL thing is never authoritative for AutoBot state. AutoBot does not control an external system's consistency, availability or history, so its facts become state only through authenticated observation followed by a core commit. Every fact AutoBot acts on then has a committed, ordered, auditable origin.

Role and service are split. A core rule sometimes needs a party AutoBot does not own: the forge, a CI provider, a model provider, the semantic judge, the authority witness. Naming the service in the core would import an EXTERNAL term into CORE; leaving the party unnamed would leave a core rule undefined. Defining the role in CORE and the service that fills it under EXTERNAL satisfies both: the core states what the party must do, and the service can change without a core edit. This is why Forge, CI provider and Model provider each appear once as a CORE role and once as an EXTERNAL service.

The closing check turns an undefined term into a mechanical finding: every term used in a core document is either defined in CORE or named on the DEFERRED list with the gate that will define it. Nothing is left to a reader's inference.

## Admission rule for entries

The three admission criteria are the thesis rule for what enters a core document at all: an invariant depends on it, M0 implements it, or its absence leaves a term used but undefined. Using the same test keeps the glossary and the core documents from drifting apart in scope.

An entry that fails all three and is used by no extension is omitted rather than kept for later. Defining unused terms invites them back into the design without an invariant behind them.

One term, one meaning. When one word carries two meanings, readers of different documents implement different contracts under the same name. The remedy is to split the word into two defined terms, never to rely on context; "adjudication" is the standing case (see Evidence and integration).

Retired forms are listed because a familiar name reintroduced tends to bring its old semantics with it. Listing each with its replacement lets a reader who reaches for the old name find the current one.

Every entry names its authority so a disagreement between a summary and its source is settled in favour of the source, and so a reader can check the summary against the full rule.

## CORE

### Commit and receipt

`AutoBotCommand` is "the only way *canonical* state changes". Reconciliation-only CASes also write status — ledger progress, slot clearing, ring publication — but can make nothing canonical, and the qualifier keeps the sentence true of them.

A reconciliation request is not a command. A command pins an expected revision and yields a `CommandReceipt`, but reconciliation writes increment no revision: two successive requests would pin the same revision, so the pin could not detect a stale one, and a write that commits nothing has no terminal receipt result to record. Naming the entry by `(operation_uid, permit_uid)` and its expected `send_state` makes a delayed request from a crashed broker a no-op against a newer entry for the same operation, so an effect that was never sent cannot be recovered as "sent or unknown". Giving reconciliation requests a receipt of their own contradicts the rule that a reconciliation-only CAS installs no receipt.

Dispatch ledger entries are reconciliation fields appended only by `AcceptDispatch`. Each entry is re-derivable from the `COMMITTED` receipt that created it, so its later progress needs no receipt. Making the acknowledgement a domain commit with its own slot would put a receipt barrier on every ledger step.

### Admission and dispatch

Currency and the routing pin are stated as "not a register" because neither belongs to the `AcceptDispatch` precondition set. Currency lives on the grant, reservation and capsule and is re-checked by the broker before `RecordSendAttempt` and on every privileged call. The routing pin is fixed per `TaskRun` at admission, so a routing change never invalidates work already in flight.

Dependents of an `UNRESOLVED` operation are defined once, in the kernel, so the formal property that blocks them and the fixtures that exercise it refer to the same set.

### Scope, identity and fencing

Execution profile is CORE because M0 must implement it (`Repository.spec`, `TaskRun.spec`) and a fence decision depends on it. The EXTERNAL sandbox runtime is only the service that enforces it.

Tier is CORE because the consequence-class floor and the fixed review tier cannot be compared without a defined order. The order is declared per `WorkContext` rather than fixed to named levels, so the entry defines only the ordering and where it is recorded.

Drift labels are CORE because the roles document attaches fixed escalations to them. The judgment that produces a `DriftAssessment` is an extension; the labels a core escalation rule reads are not.

### Evidence and integration

Acceptance adjudication and human adjudication were once a single word. Acceptance is a deterministic decision by the Task controller from recorded evidence, which lets delivery run unattended; human adjudication is the only exit from states the system cannot resolve by itself. Separate terms name who decides each.

Reviewer independence treats another session of the same model as correlated, not independent: a shared model shares blind spots however many sessions run it.

### Judgment and humans

Semantic judge is a CORE role so the thesis rule that a typed-question judgment can only raise a floor is stated in core terms. The chosen service is named once, in the thesis goals, and is EXTERNAL.

An `Intervention` is applied by a command to the owning controller of what it changes. The two adjudication actions touch no register, so application "through the registers" would be false for them. A controller may raise an `Intervention` under its own principal to summon a human, and the human answer is a further `Intervention`, so every human decision has the same authenticated form. `RESUME` gives holds and pauses an exit with a named requester.

Human roles are never held by an agent identity: each is the point where the system stops relying on its own automation.

### External parties the kernel addresses

The authority witness sits in a separate failure domain because an installation cannot prove its own fencing; the installation that would assert it may be the stale one.

## EXTERNAL

Kubernetes Leases carry liveness, never authority: a lapsed Lease says a holder may be gone, not that its authority has ended. Authority ends only by a register cut.

Temporal and PostgreSQL are adopted only on a measured need and never as authority; a second store holding task state would be a second source of truth beside the aggregates.

## Retired terms

Each retired name was replaced by an existing mechanism or moved out of the core. Core replacements: a reservation plus the target's receipt, a pin on the `TaskRun`, the `WorkContext` registers, the broker as sole sender, the write-ahead `send_attempt`, conditions and per-plan phases, the `TaskRun`'s expected records, and reconciler duties of existing controllers. Names that left the core went to an extension term, an extension file or the DEFERRED list, or were dropped without a replacement. Keeping a retired name alongside its replacement would give one concept two names.
