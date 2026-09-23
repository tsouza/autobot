#!/usr/bin/env rust-script
//! Local build setup checks: `doctor`, or `doctor --measure` for the sccache hit split.
//!
//! ```cargo
//! [dependencies]
//! autobot-devtools = { path = "devtools", features = ["doctor"] }
//! ```

fn main() -> Result<(), autobot_devtools::Error> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    autobot_devtools::doctor::cli(&args)
}
