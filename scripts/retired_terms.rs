#!/usr/bin/env rust-script
//! Fails when a GLOSSARY §Retired terms identifier is used in the repository whose root is the
//! first argument (default `.`).
//!
//! ```cargo
//! [dependencies]
//! autobot-devtools = { path = "devtools" }
//! ```

use std::process::ExitCode;

fn main() -> Result<ExitCode, autobot_devtools::Error> {
    let root = std::env::args().nth(1).unwrap_or_else(|| ".".to_owned());
    autobot_devtools::design::retired::run(std::path::Path::new(&root))
}
