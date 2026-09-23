#!/usr/bin/env rust-script
//! Fails when a pull request's diff, commit messages or text match a term of the list in
//! `SENSITIVE_TERMS`, printing only where; the only argument is the pull request number.
//!
//! ```cargo
//! [dependencies]
//! autobot-devtools = { path = "devtools", features = ["github", "hooks"] }
//! ```

use std::process::ExitCode;

fn main() -> Result<ExitCode, autobot_devtools::Error> {
    autobot_devtools::sensitive::run(std::env::args().skip(1))
}
