#!/usr/bin/env rust-script
//! Fails when the pull request title given as the only argument is not a Conventional Commits
//! header.
//!
//! ```cargo
//! [dependencies]
//! autobot-devtools = { path = "devtools" }
//! ```

use std::process::ExitCode;

fn main() -> Result<ExitCode, autobot_devtools::Error> {
    autobot_devtools::conventional::run(std::env::args().skip(1))
}
