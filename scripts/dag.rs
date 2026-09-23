#!/usr/bin/env rust-script
//! Renders the blocked-by graph as Mermaid, or with `--critical` prints the longest open
//! chain (read-only).
//!
//! ```cargo
//! [dependencies]
//! autobot-devtools = { path = "devtools", features = ["github"] }
//! ```

fn main() -> std::process::ExitCode {
    autobot_devtools::github::graph::dag_main(std::env::args().skip(1))
}
