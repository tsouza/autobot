#!/usr/bin/env rust-script
//! Writes the SVG image of the banner text at the first argument to the second argument; the
//! rendering is that of `autobot_devtools::banner`.
//!
//! ```cargo
//! [dependencies]
//! autobot-devtools = { path = "devtools" }
//! ```

use std::process::ExitCode;

fn main() -> Result<ExitCode, autobot_devtools::Error> {
    let mut args = std::env::args().skip(1);
    match (args.next(), args.next(), args.next()) {
        (Some(input), Some(output), None) => autobot_devtools::banner::run(input, output),
        _ => {
            eprintln!("usage: banner.rs <banner.txt> <banner.svg>");
            Ok(ExitCode::FAILURE)
        }
    }
}
