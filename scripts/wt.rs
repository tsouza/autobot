#!/usr/bin/env rust-script
//! Worktree per pull request: `wt new <issue#>`, `wt list`, `wt rm <issue#>`.
//!
//! ```cargo
//! [dependencies]
//! autobot-devtools = { path = "devtools", features = ["github"] }
//! ```

fn main() -> Result<(), autobot_devtools::Error> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    autobot_devtools::worktree::cli(&args)
}
