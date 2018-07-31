extern crate docopt;
extern crate pest;
#[macro_use]
extern crate pest_derive;

mod parser;

use std::boxed::Box;
use std::error::Error;
use std::fs;
use std::fs::File;
use std::io::prelude::*;

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
    let mut raw = String::new();

    // Open and load the file, let errors bubble up.
    File::open(filename)?.read_to_string(&mut raw)?;

    // Do the parse.
    if let Err(e) = parser::parse(&raw) {
        return Err(e);
    }

    Ok(())
}
