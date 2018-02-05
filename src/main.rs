extern crate docopt;
extern crate zoneq;

use docopt::Docopt;
use std::process;
use zoneq::Config;

const USAGE: &'static str = "
zoneq

Usage:
  zoneq <query> <file>
  zoneq -h | --help
  zoneq --version

Options:
  -h, --help    Show this screen.
  --version     Show the version.
";

const VERSION: &'static str = env!("CARGO_PKG_VERSION");

fn main() {
    let args = Docopt::new(USAGE)
        .and_then(|opts| opts.parse())
        .unwrap_or_else(|e| e.exit());

    let config = Config::new(&args);

    if config.version {
        println!("zoneq {}", VERSION);
        process::exit(0);
    }

    if let Err(e) = zoneq::run(config.query, config.filename) {
        println!("Oh no! {}", format_err(e.to_string()));
        process::exit(1);
    }
}

fn format_err(err: String) -> String {
    let mut c = err.chars();

    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}