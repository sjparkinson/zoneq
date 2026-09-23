use std::io::{self, BufWriter, ErrorKind, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use zoneq::name::Name;
use zoneq::{Error, Options};

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

/// Query zone files.
///
/// QUERY is an owner name, like `www` or `www.example.com.`. Start it with a
/// dot to match that name and everything below it, or pass `.` to match
/// every record.
///
/// Exits 0 when something matched, 1 when nothing did, 2 on error.
#[derive(Parser)]
#[command(version = env!("ZONEQ_VERSION"))]
struct Cli {
    /// Filter by record type, e.g. MX, or several like A,AAAA
    #[arg(long = "type", value_name = "TYPE", value_delimiter = ',', value_parser = parse_type)]
    record_types: Vec<String>,

    /// Origin to start from, until the file sets its own $ORIGIN
    #[arg(long, value_name = "NAME", value_parser = parse_origin)]
    origin: Option<Name>,

    /// Print matches as a JSON array
    #[arg(long)]
    json: bool,

    /// Only match records pointing at this name or IP address
    #[arg(long, value_name = "NAME|IP")]
    data: Option<String>,

    /// Owner name to match: `www`, `.example.com` or `.`
    query: String,

    /// Zone file to read, or - for stdin
    file: PathBuf,
}

fn parse_type(raw: &str) -> Result<String, String> {
    if raw.is_empty() {
        return Err("empty record type".into());
    }
    Ok(raw.to_ascii_uppercase())
}

fn parse_origin(raw: &str) -> Result<Name, String> {
    Name::parse(raw, Some(&Name::root()))
}

fn main() -> ExitCode {
    // Writes dhat-heap.json when dropped at the end of main.
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    let cli = Cli::parse();
    let opts = Options {
        query: cli.query,
        file: cli.file,
        record_types: cli.record_types,
        origin: cli.origin,
        json: cli.json,
        data: cli.data,
    };

    let mut out = BufWriter::new(io::stdout().lock());
    // Piping into `head` and friends shouldn't look like a failure, so a
    // broken pipe on the final flush keeps the exit code the matches earned.
    let result = zoneq::run(&opts, &mut out).and_then(|n| match out.flush() {
        Err(e) if e.kind() != ErrorKind::BrokenPipe => Err(Error::Output(e)),
        _ => Ok(n),
    });

    match result {
        Ok(0) => ExitCode::from(1),
        Ok(_) => ExitCode::SUCCESS,
        // Only writing matches fills the buffer, so something matched.
        Err(Error::Output(e)) if e.kind() == ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("zoneq: {e}");
            ExitCode::from(2)
        }
    }
}
