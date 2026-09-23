#!/usr/bin/env rust-script
//! Checks the design set in the directory given as the first argument (default `docs/design`).
//!
//! ```cargo
//! [dependencies]
//! autobot-devtools = { path = "devtools" }
//! ```

use std::process::ExitCode;

fn main() -> Result<ExitCode, autobot_devtools::Error> {
    let dir = std::env::args().nth(1).unwrap_or_else(|| "docs/design".to_owned());
    autobot_devtools::design::check::run(std::path::Path::new(&dir))
}
