//! AutoBot kernel, specified by `docs/design/AUTOBOT-KERNEL.md`.
//!
//! This crate has no Kubernetes, async runtime or I/O dependency: controllers drive it
//! from `autobot-controllers`.
#![warn(missing_docs)]

pub mod error;
pub mod fields;
pub mod profile;
pub mod reducer;
pub mod status;
pub mod store;
pub mod types;
