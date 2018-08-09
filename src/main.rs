#![feature(rust_2018_preview)]

extern crate docopt;

use docopt::Docopt;

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

fn main() -> Result<(), String> {
    let args = Docopt::new(USAGE)
        .and_then(|opts| opts.parse())
        .unwrap_or_else(|e| e.exit());

    let config = Config::new(&args);

    if config.version {
        println!("zoneq {}", VERSION);
        return Ok(());
    }

    if let Err(e) = zoneq::run(config.query, config.filename) {
        return Err(format!("{}", e));
    }

    Ok(())
}

struct Config<'a> {
    query: &'a str,
    filename: &'a str,
    version: bool,
}

impl<'a> Config<'a> {
    fn new(args: &'a docopt::ArgvMap) -> Config<'a> {
        Config {
            query: args.get_str("<query>"),
            filename: args.get_str("<file>"),
            version: args.get_bool("--version"),
        }
    }
}
