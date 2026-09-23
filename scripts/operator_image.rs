#!/usr/bin/env rust-script
//! The operator image: `operator_image` builds it from a host build and prints its digest.
//!
//! ```cargo
//! [dependencies]
//! autobot-devtools = { path = "devtools" }
//! ```

fn main() -> Result<(), autobot_devtools::Error> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    autobot_devtools::image::cli(&args)
}
