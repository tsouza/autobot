#!/usr/bin/env rust-script
//! Verifies the record of the gate named by the first argument: every listed artifact exists
//! with its digest, and an SSH-signed tag on the record commit passes `git verify-tag`.
//!
//! ```cargo
//! [dependencies]
//! autobot-devtools = { path = "devtools" }
//! ```

use std::process::ExitCode;

fn main() -> Result<ExitCode, autobot_devtools::Error> {
    let Some(gate) = std::env::args().nth(1) else {
        eprintln!("usage: gate_evidence.rs <gate>");
        return Ok(ExitCode::FAILURE);
    };
    autobot_devtools::gates::run_evidence(std::path::Path::new("."), &gate)
}
