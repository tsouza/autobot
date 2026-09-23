//! AutoBot adapter traits, including `RuntimeAdapter`, and their contract suites.
//!
//! An adapter is the code between AutoBot and a party it does not own. Each module holds one
//! trait, the values it exchanges, a harness trait through which a contract suite drives a
//! double of the adapter, and the suite itself:
//!
//! | Module | Trait | Design |
//! |---|---|---|
//! | [`provider`] | [`ProviderAdapter`](provider::ProviderAdapter) | KERNEL §3.3; FORMAL §2 `ProviderCapability` |
//! | [`runtime`] | [`RuntimeAdapter`](runtime::RuntimeAdapter) | ROLES §4; KERNEL §8 step 2, §9 |
//! | [`observation`] | [`ObservationSource`](observation::ObservationSource) | TRUST §Not trusted; forge mirroring |
//! | [`artifact`] | [`ArtifactStore`](artifact::ArtifactStore) | TRUST §Trusted; KERNEL §7 |
//! | [`witness`] | [`AuthorityWitness`](witness::AuthorityWitness) | KERNEL §7 |
//! | [`judge`] | [`SemanticJudge`](judge::SemanticJudge) | TRUST §Not trusted; TypeSafe decision policy, Decision 1 |
//!
//! A suite is a function `run(&mut harness)` that returns every rule the adapter broke as a
//! [`Violation`](contract::Violation), or `Ok(())`. A harness gives the suite fresh adapters
//! and the ground truth a test needs, such as how many times a provider applied an operation,
//! and injects the faults the suite asks for. A fake or a real adapter runs its suite to show it
//! keeps the contract; each suite is also run here against a minimal double that passes and
//! against broken doubles that each break one rule.
//!
//! The traits are synchronous and take `&mut self`: this crate depends on the kernel only, and
//! a controller that needs concurrency wraps an adapter in its own runtime.
//!
//! Choices this crate makes where the design is open:
//!
//! - Every input to an adapter carries one of the three trust classes of TRUST
//!   ([`TrustClass`](trust::TrustClass)). The finer trust-label enum of the decision-policy
//!   extension arrives at G-INTAKE (M0 §5) and is not modelled.
//! - Faults are injected through the harness, not through the adapter traits, so no production
//!   adapter carries a fault hook.
//! - Identifiers adapters exchange that the kernel has no type for (provider and operation
//!   names, remote identities, heads, event identifiers) are opaque, non-empty text: the
//!   provider assigns them and AutoBot only compares them.
//! - A time an adapter exchanges is whole seconds since the Unix epoch, as a `u64`.
#![warn(missing_docs)]

pub mod artifact;
pub mod contract;
pub mod judge;
pub mod observation;
pub mod provider;
pub mod runtime;
pub mod text;
pub mod trust;
pub mod witness;
