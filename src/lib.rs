extern crate pest;
#[macro_use]
extern crate pest_derive;

mod parser;

use std::boxed::Box;
use std::error::Error;
use std::fs;
use std::fs::File;
use std::io::prelude::*;
use std::result::Result;

pub fn run(_query: &str, filename: &str) -> Result<(), Box<dyn Error>> {
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
    parser::parse(&raw)?;

    Ok(())
}
