#!/usr/bin/env rust-script
//! The M0 artifact-store fixture on the kind cluster: `artifact-store up`, `down`, `check`.
//!
//! ```cargo
//! [dependencies]
//! autobot-devtools = { path = "devtools" }
//! ```

fn main() -> Result<(), autobot_devtools::Error> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    autobot_devtools::artifact_store::cli(&args)
}
