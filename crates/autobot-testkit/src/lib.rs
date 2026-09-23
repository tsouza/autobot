//! AutoBot fixture harness, interleaving explorer and registry, for the fixture groups of
//! `docs/design/AUTOBOT-M0-AND-GATES.md` §3 and the injected faults of
//! `docs/design/AUTOBOT-FORMAL-SURFACE.md` §7.
//!
//! - [`harness`]: the store-agnostic [`harness::Driver`] a scenario runs against, faults
//!   positioned at any write, lossy deliveries and each fixture's liveness bounds.
//! - [`interleave`]: two actors run under every position or every interleaving of their
//!   steps, each schedule checked.
//! - [`registry`]: the kernel pieces a scenario resolves at run time, so that a fixture test
//!   compiles and fails with a named missing implementation before that implementation exists.
//!
//! A fixture group's tests live under `crates/*/tests/g_<group>/`, and the test that owns a
//! FORMAL §4 fixture F-n is named `f<n>_<slug>`, such as `f3_receipt_barrier`: stage 2 of
//! `just trace-lint` reads that name as ownership, requires exactly one owning test per F-n
//! of the group, and requires every ignored test to carry exactly
//! `#[ignore = "awaiting #N"]`, naming the implementation task that removes it.
#![warn(missing_docs)]

pub mod harness;
pub mod interleave;
pub mod registry;
