#!/usr/bin/env rust-script
//! Runs the formal toolchain; the arguments are those of `autobot_devtools::formal::run`.
//!
//! ```cargo
//! [dependencies]
//! autobot-devtools = { path = "devtools" }
//! ```

use std::process::ExitCode;

fn main() -> Result<ExitCode, autobot_devtools::Error> {
    autobot_devtools::formal::run(std::env::args().skip(1))
}
