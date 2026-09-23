#!/usr/bin/env rust-script
//! Judges a pull request against the `judged` entries of `CHARTER.md` through the
//! typed-question service, failing only on a confident "violates"; the only argument is the
//! pull request number.
//!
//! ```cargo
//! [dependencies]
//! autobot-devtools = { path = "devtools", features = ["judge"] }
//! ```

use std::process::ExitCode;

fn main() -> Result<ExitCode, autobot_devtools::Error> {
    autobot_devtools::judge::run(std::env::args().skip(1))
}
