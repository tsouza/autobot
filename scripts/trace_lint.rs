#!/usr/bin/env rust-script
//! Checks traceability from the THESIS invariants to the fixture tests and the Quint model;
//! the arguments are those of `autobot_devtools::design::trace::run`.
//!
//! ```cargo
//! [dependencies]
//! autobot-devtools = { path = "devtools" }
//! ```

use std::process::ExitCode;

fn main() -> Result<ExitCode, autobot_devtools::Error> {
    autobot_devtools::design::trace::run(std::env::args().skip(1))
}
