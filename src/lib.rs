pub mod error;
mod lexer;
pub mod name;
pub mod query;
mod ttl;
pub mod zone;

use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub use error::Error;
use name::Name;
use query::Query;
use zone::Record;

pub struct Options {
    pub query: String,
    /// `-` reads from stdin.
    pub file: PathBuf,
    pub record_type: Option<String>,
    /// Origin to start from, until the file sets its own `$ORIGIN`.
    pub origin: Option<Name>,
    pub json: bool,
}

/// Parses the zone, writes matching records to `out`, and returns how many
/// there were.
pub fn run(opts: &Options, out: &mut impl Write) -> Result<usize, Error> {
    let origin = opts.origin.clone();
    let zone = if opts.file == Path::new("-") {
        zone::parse_reader(io::stdin().lock(), Path::new("<stdin>"), origin)?
    } else {
        zone::parse_file(&opts.file, origin)?
    };

    let query = Query::parse(&opts.query, zone.origin.as_ref())?;
    let matches: Vec<&Record> = zone
        .records
        .iter()
        .filter(|r| query.matches(&r.name))
        .filter(|r| {
            opts.record_type
                .as_ref()
                .is_none_or(|t| r.rtype.eq_ignore_ascii_case(t))
        })
        .collect();

    let written = if opts.json {
        serde_json::to_writer_pretty(&mut *out, &matches)
            .map_err(io::Error::from)
            .and_then(|()| writeln!(out))
    } else {
        matches
            .iter()
            .try_for_each(|r| writeln!(out, "{}", format_record(r)))
    };
    written.map_err(Error::Output)?;

    Ok(matches.len())
}

/// Formats a record as a tab-separated zone file line.
pub fn format_record(r: &Record) -> String {
    format!(
        "{}\t{}\t{}\t{}\t{}",
        r.name,
        r.ttl,
        r.class,
        r.rtype,
        r.rdata.join(" ")
    )
}
