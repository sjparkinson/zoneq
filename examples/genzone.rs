//! Writes a synthetic zone to stdout, for timing and profiling the CLI.
//!
//! cargo run --release --example genzone -- 1000000 > target/bench-1m.zone

#[path = "../benches/support/zonegen.rs"]
mod zonegen;

use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(records) = std::env::args().nth(1).and_then(|n| n.parse().ok()) else {
        eprintln!("usage: genzone <RECORDS>");
        return ExitCode::from(2);
    };
    let zone = zonegen::generate(records);
    match io::stdout().lock().write_all(zone.as_bytes()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("genzone: {e}");
            ExitCode::FAILURE
        }
    }
}
