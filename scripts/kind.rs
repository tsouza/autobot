#!/usr/bin/env rust-script
//! The kind cluster for integration tests: `kind up`, `kind down`, `kind load <image>`.
//!
//! ```cargo
//! [dependencies]
//! autobot-devtools = { path = "devtools" }
//! ```

fn main() -> Result<(), autobot_devtools::Error> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    autobot_devtools::kind::cli(&args)
}
