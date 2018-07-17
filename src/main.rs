extern crate docopt;
extern crate zoneq;
#[macro_use] extern crate nom;

use docopt::Docopt;
use std::process;

const USAGE: &'static str = "
zoneq

Usage:
  zoneq [--type=<type>] <query> <file>
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
        eprintln!("Oh no! {}", e);
        process::exit(1);
    }
}

struct Config<'a> {
    query: &'a str,
    filename: &'a str,
    record_type: &'a str,
    version: bool,
}

impl<'a> Config<'a> {
    fn new(args: &'a docopt::ArgvMap) -> Config<'a> {
        Config {
            query: args.get_str("<query>"),
            filename: args.get_str("<file>"),
            record_type: args.get_str("<type>"),
            version: args.get_bool("--version"),
        }
    }
}
