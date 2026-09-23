#!/usr/bin/env rust-script
//! Checks the crate dependency rules of the workspace containing the first argument
//! (default `.`); the rules are those of `autobot_devtools::layering`.
//!
//! ```cargo
//! [dependencies]
//! autobot-devtools = { path = "devtools" }
//! ```

use std::process::ExitCode;

fn main() -> Result<ExitCode, autobot_devtools::Error> {
    let dir = std::env::args().nth(1).unwrap_or_else(|| ".".to_owned());
    autobot_devtools::layering::run(dir)
}
