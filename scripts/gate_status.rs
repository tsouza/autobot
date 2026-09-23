#!/usr/bin/env rust-script
//! Prints the current binding digests and the state of every gate from its record under
//! `docs/gates`, reporting a PASSED record whose digests differ as INVALIDATED. Edits nothing.
//! The first argument is the repository root (default `.`).
//!
//! ```cargo
//! [dependencies]
//! autobot-devtools = { path = "devtools", features = ["gates"] }
//! ```

use std::process::ExitCode;

fn main() -> Result<ExitCode, autobot_devtools::Error> {
    let root = std::env::args().nth(1).unwrap_or_else(|| ".".to_owned());
    autobot_devtools::gates::run_status(std::path::Path::new(&root))
}
