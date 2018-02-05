extern crate docopt;
extern crate zoneq;

use docopt::Docopt;
use std::process;
use zoneq::Config;

const USAGE: &'static str = "
zoneq

Usage:
  zoneq <query> <file> [--type=<type>]
  zoneq -h | --help
  zoneq --version

Filters:
  --type <type>     Filter query by record type.

Options: 
  -h, --help        Show this screen.
  --version         Show the version.
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
        println!("Oh no! {}", e);
        process::exit(1);
    }
}