extern crate docopt;

use std::error::Error;
use std::fs;
use std::fs::File;
use std::io::prelude::*;
use std::boxed::Box;

type Result<T> = std::result::Result<T, Box<Error>>;

pub fn run(_query: &str, filename: &str) -> Result<()> {
    // Check file meta.
    let metadata = fs::metadata(filename)?;

    // Only support files.
    if metadata.is_file() == false {
        return Err(From::from(format!("{} is not a file.", filename)));
    }

    // Keep it small...
    if metadata.len() > 10000000 {
        return Err(From::from(format!("{} is over 10MB!", filename)));
    }

    // We're going to load the zone file into this.
    let mut zone = String::new();

    // Open and load the file, let errors bubble up.
    File::open(filename)?.read_to_string(&mut zone)?;

    // For now, print what we've read.
    println!("{}", zone);

    // We're good!
    Ok(())
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
            version: args.get_bool("--version")
        }
    }
}
