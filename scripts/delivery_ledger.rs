#!/usr/bin/env rust-script
//! Prints how the pull requests merged in a date range were delivered: review rounds, blocking
//! finding classes, judge answers against the reviews, time to merge, lines changed, and the
//! red runs of required workflows on main. Arguments: `<from> [<to>]`, UTC dates `YYYY-MM-DD`.
//!
//! ```cargo
//! [dependencies]
//! autobot-devtools = { path = "devtools", features = ["ledger"] }
//! ```

fn main() -> Result<(), autobot_devtools::Error> {
    autobot_devtools::ledger::main(std::env::args().skip(1))
}
