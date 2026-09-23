#!/usr/bin/env rust-script
//! Posts the `review-gate` commit status on a pull request's head SHA from its latest review
//! verdict; the only argument is the pull request number.
//!
//! ```cargo
//! [dependencies]
//! autobot-devtools = { path = "devtools", features = ["github"] }
//! ```

fn main() -> Result<(), autobot_devtools::Error> {
    autobot_devtools::github::verdict::main(std::env::args().skip(1))
}
