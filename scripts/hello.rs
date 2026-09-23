#!/usr/bin/env rust-script
//! Smoke test of the rust-script entry convention.
//!
//! ```cargo
//! [dependencies]
//! autobot-devtools = { path = "devtools" }
//! ```

fn main() -> Result<(), autobot_devtools::Error> {
    let top = autobot_devtools::git::toplevel(".")?;
    println!("autobot-devtools resolved; repository at {top}");
    Ok(())
}
