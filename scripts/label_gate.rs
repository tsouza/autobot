#!/usr/bin/env rust-script
//! Fails when a pull request changes a human-lane path without the labels that path requires,
//! or when its title is not a Conventional Commits header;
//! the only argument is the pull request number.
//!
//! ```cargo
//! [dependencies]
//! autobot-devtools = { path = "devtools", features = ["github"] }
//! ```

use std::process::ExitCode;

fn main() -> Result<ExitCode, autobot_devtools::Error> {
    autobot_devtools::github::label_gate::run(std::env::args().skip(1))
}
