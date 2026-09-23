pub mod data;
pub mod error;
mod lexer;
pub mod name;
pub mod query;
mod ttl;
pub mod zone;

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use data::DataQuery;
pub use error::Error;
use name::Name;
use query::Query;
use zone::Record;

pub struct Options {
    pub query: String,
    /// `-` reads from stdin.
    pub file: PathBuf,
    /// Record types to keep, uppercased. Empty keeps every type.
    pub record_types: Vec<String>,
    /// Origin to start from, until the file sets its own `$ORIGIN`.
    pub origin: Option<Name>,
    pub json: bool,
    /// A name or IP address to look for in the rdata.
    pub data: Option<String>,
}

impl Options {
    /// Whether `rtype` is one of the types asked for, ignoring case.
    pub fn wants_type(&self, rtype: &str) -> bool {
        self.record_types.is_empty()
            || self
                .record_types
                .iter()
                .any(|t| t.eq_ignore_ascii_case(rtype))
    }
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
    let data = opts
        .data
        .as_deref()
        .map(|d| DataQuery::parse(d, zone.origin.as_ref()))
        .transpose()?;
    let matches: Vec<&Record> = zone
        .records
        .iter()
        .filter(|r| data.as_ref().is_none_or(|d| d.matches(r)))
        .filter(|r| query.matches(&r.name))
        .filter(|r| opts.wants_type(&r.rtype))
        .collect();

    let written = if opts.json {
        serde_json::to_writer_pretty(&mut *out, &matches)
            .map_err(io::Error::from)
            .and_then(|()| writeln!(out))
    } else {
        matches.iter().try_for_each(|r| writeln!(out, "{r}"))
    };
    written.map_err(Error::Output)?;

    Ok(matches.len())
}

/// Formats a record as a tab-separated zone file line.
pub fn format_record(r: &Record) -> String {
    r.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(types: &[&str]) -> Options {
        Options {
            query: ".".into(),
            file: "-".into(),
            record_types: types.iter().map(|t| t.to_string()).collect(),
            origin: None,
            json: false,
            data: None,
        }
    }

    #[test]
    fn no_types_wants_everything() {
        assert!(opts(&[]).wants_type("TXT"));
    }

    #[test]
    fn wants_any_listed_type() {
        let o = opts(&["A", "AAAA"]);
        assert!(o.wants_type("A"));
        assert!(o.wants_type("aaaa"));
        assert!(!o.wants_type("MX"));
    }
}
