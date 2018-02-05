extern crate docopt;

use std::error::Error;
use std::fs;
use std::fs::File;
use std::io::prelude::*;
use std::boxed::Box;

type Result<T> = std::result::Result<T, Box<Error>>;

pub fn run(_query: &str, filename: &str) -> Result<()> {
    // Check our file is a text file.
    let metadata = match fs::metadata(filename) {
        Ok(m) => (m),
        Err(e) => {
            let err: Box<Error> = From::from(format!("{} for {}.", e, filename));
            return Err(err);
        }
    };

    if metadata.is_file() == false {
        let err: Box<Error> = From::from(format!("{} is not a file.", filename));
        return Err(err);
    }

    if metadata.len() > 10000000 {
        let err: Box<Error> = From::from(format!("{} is over 10MB!", filename));
        return Err(err);
    }

    // We're going to load the zone file into this.
    let zone = read_zone(filename)?;

    // For now, print what we've read.
    println!("{}", zone);

    // We're good!
    Ok(())
}

fn read_zone(filename: &str) -> Result<String> {
    // We're going to load the zone file into this.
    let mut zone = String::new();

    // Open and load the file, let errors bubble up.
    match File::open(filename)?.read_to_string(&mut zone) {
        Ok(_) => (),
        Err(e) => {
            let err: Box<Error> = From::from(format!("{} for {}.", e, filename));
            return Err(err);
        }
    };

    Ok(zone)
}

pub struct Config<'a> {
    pub query: &'a str,
    pub filename: &'a str,
    pub version: bool,
}

impl<'a> Config<'a> {
    pub fn new(args: &'a docopt::ArgvMap) -> Config<'a> {
        Config {
            query: args.get_str("<query>"),
            filename: args.get_str("<file>"),
            version: args.get_bool("--version"),
        }
    }
}
