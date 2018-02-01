use std::env;
use std::error::Error;
use std::fs::File;
use std::io::prelude::*;
use std::process;

const USAGE: &'static str = "
zoneq

Usage:
  zoneq <query> [<file>...]
  zoneq -h | --help
  zoneq --version

Options:
  -h --help     Show this screen.
  --version     Show version.
";

const VERSION: &'static str = env!("CARGO_PKG_VERSION");

fn main() {
    let args: Vec<String> = env::args().collect();

    let config = Config::new(&args).unwrap_or_else(|err| {
        println!("Problem parsing arguments: {}", err);
        process::exit(1);
    });

    if config.help {
        println!("{}", USAGE);
        process::exit(0);
    }

    if config.version {
        println!("zoneq {}", VERSION);
        process::exit(0);
    }
    
    if let Some(ref q) = config.query {
        println!("The query is {}", q);
    }

    if let Err(e) = run(config) {
        println!("Application error: {}", e);
        process::exit(1);
    }
}

fn run(config: Config) -> Result<(), Box<Error>> {
    if let Some(ref filename) = config.filename {
        let mut f = File::open(filename).expect("file not found");

        let mut contents = String::new();

        f.read_to_string(&mut contents)
            .expect("something went wrong reading the file");

        println!("With zone:\n{}", contents);
    }

    Ok(())
}

struct Config {
    query: Option<String>,
    filename: Option<String>,
    help: bool,
    version: bool
}

impl Config {
    fn new(args: &[String]) -> Result<Config, &'static str> {
        if args.len() == 1 {
            return Ok(Config {
                query: None,
                filename: None,
                help: true,
                version: false
            });
        }

        for arg in args {
            if arg == "-h" || arg == "--help" {
                return Ok(Config {
                    query: None,
                    filename: None,
                    help: true,
                    version: false
                });
            }
            
            if arg == "--version" {
                return Ok(Config {
                    query: None,
                    filename: None,
                    help: false,
                    version: true
                });
            }
        }

        let query = args.get(1).cloned();
        let filename = args.get(2).cloned();

        Ok(Config {
            query,
            filename,
            help: false,
            version: false
        })
    }
}