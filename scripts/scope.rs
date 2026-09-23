#!/usr/bin/env rust-script
//! Fails when a pull request changes a path outside the allowed paths of the task issue it
//! closes, its scope extensions and the paths every task may change; the only argument is the
//! pull request number.
//!
//! ```cargo
//! [dependencies]
//! autobot-devtools = { path = "devtools", features = ["github"] }
//! ```

use std::process::ExitCode;

fn main() -> Result<ExitCode, autobot_devtools::Error> {
    autobot_devtools::scope::run(std::env::args().skip(1))
}
