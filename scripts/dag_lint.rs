#!/usr/bin/env rust-script
//! Lints the repository's issue graph against the work-item rules (read-only).
//!
//! ```cargo
//! [dependencies]
//! autobot-devtools = { path = "devtools", features = ["github"] }
//! ```

fn main() -> std::process::ExitCode {
    autobot_devtools::github::graph::lint_main()
}
