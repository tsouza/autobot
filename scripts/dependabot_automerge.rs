#!/usr/bin/env rust-script
//! Enables GitHub auto-merge (squash) on a pull request; the only argument is its number or
//! its GraphQL node id.
//!
//! ```cargo
//! [dependencies]
//! autobot-devtools = { path = "devtools", features = ["github"] }
//! ```

fn main() -> Result<(), autobot_devtools::Error> {
    autobot_devtools::github::automerge::main(std::env::args().skip(1))
}
