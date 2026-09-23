#!/usr/bin/env rust-script
//! Applies `.github/rulesets/main.json` and `.github/settings.json` to the GitHub repository,
//! printing the diff first; `--dry-run` prints the diff only.
//!
//! ```cargo
//! [dependencies]
//! autobot-devtools = { path = "devtools", features = ["github"] }
//! ```

fn main() -> Result<(), autobot_devtools::Error> {
    autobot_devtools::github::settings::main(std::env::args().skip(1))
}
