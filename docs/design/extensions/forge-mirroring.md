# Extension — forge mirroring

Intent: make issues, pull requests, labels and comments on GitHub, Gitea, Bitbucket and similar forges faithful, collaborative *views* of canonical work, and bring source, merge and CI facts back in as authenticated observations (THESIS goal 5, I-7).
Gate: fake forge and CI at **M0-Q**; real adapters, per-operation conformance and projection at **G-FORGE**; mixed-forge contexts and additional adapters at **G-PORTABILITY**.

Relies on (kernel): effect intents, `operation_key`, the dispatch ledger and the send-attempt marker (KERNEL §3.3, F-18 … F-21); provider capability qualification and `BLOCKED_UNSUPPORTED`; evidence freshness and remote generation (KERNEL §5, F-27); the integration basis register (F-10, F-29); the authenticated-observation trust class (TRUST).

Decisions:

- **A remote object is a projection.** A `MirrorBinding` maps a canonical resource UID to its remote issue, PR, comment or label with the last projected digest. Progress and decisions are recorded internally first and projected afterwards through committed effect intents, so a missing outgoing operation is regenerated from the intent after any interruption, never invented.
- **Ownership of remote fields is split.** AutoBot owns its namespaced status labels and designated sections; human content and ordinary labels are human-owned and never overwritten. Exact projection echoes are suppressed without discarding real merge or CI facts.
- **Incoming facts are `ExternalObservation`s**: authenticated, deduplicated by provider event id *and* semantic key (a provider may not expose a stable id), persisted before acknowledgement, and recovered by periodic poll and relist after watch loss. An observation records provider, remote object UID, delivery id, poll cursor, remote generation, source and base heads, branch-protection digest and observation time; it becomes state only through a controller commit.
- **Remote text carries no authority.** A human command written in a forge is validated against actor permissions before it becomes a command; free-form text never changes an accepted contract.
- **Every real action starts `UNQUALIFIED`** — issue and PR create, comment and label projection, push and ref update, merge, CI dispatch and cancel — per provider and per operation, and becomes usable only with recorded provider version, API evidence and conformance fixtures for identity, lookup and completion semantics. A marker is not idempotency.
- **Merge relies on provider-enforced protection**: a merge reconciles remote object identity, expected source and base heads, protection digest, required CI attestation and required review attestation, and revalidates provider-enforceable preconditions at final dispatch. An AutoBot-only branch lock cannot stop an external merge; where required preconditions cannot be enforced, automated merge is unsupported.
- **CI evidence is bound** to head, workflow definition, environment and provider run identity; a delayed result for an old basis satisfies nothing current.
- **Project setup is a bounded plan** — repository creation, protection, CI registration, labels — verified through external operations; a project is ready when its checks pass, not when files exist.
