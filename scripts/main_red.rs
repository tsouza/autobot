#!/usr/bin/env rust-script
//! Opens, or comments on, the `urgent` issue for a gate-lane workflow run on main, from a
//! push or the nightly schedule, that failed; `--dry-run <run-url>` prints what it would do for
//! that run instead.
//!
//! ```cargo
//! [dependencies]
//! autobot-devtools = { path = "devtools", features = ["github"] }
//! ```

fn main() -> Result<(), autobot_devtools::Error> {
    autobot_devtools::github::main_red::main(std::env::args().skip(1))
}
