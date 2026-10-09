use std::ffi::OsString;
use std::io::{self, BufWriter, ErrorKind, Write};
use std::process::ExitCode;

use zoneq::name::Name;
use zoneq::{Error, Options};

macro_rules! usage {
    () => {
        "Usage: zoneq [OPTIONS] <QUERY> <FILE>"
    };
}

const HELP: &str = concat!(
    "Query zone files.\n\n",
    usage!(),
    "

Arguments:
  <QUERY>  Owner name to match: `www`, `.example.com` or `.`
  <FILE>   Zone file to read, or - for stdin

Options:
      --type <TYPE>     Filter by record type, e.g. MX, or several like A,AAAA
      --origin <NAME>   Origin to start from, until the file sets its own $ORIGIN
      --json            Print matches as a JSON array
      --data <NAME|IP>  Only match records pointing at this name or IP address
      --resolve         Answer like the zone's server would, following CNAMEs and wildcards
  -h, --help            Print help
  -V, --version         Print version

QUERY is an owner name, like `www` or `www.example.com.`. Start it with a
dot to match that name and everything below it, or pass `.` to match every
record.

With --resolve, QUERY is a single name inside the zone, and the answer is
what the zone's authoritative server would give for it: CNAMEs followed,
wildcards expanded and delegations referred. --type then works like the
query type, and without it you get every type, like ANY.

Exits 0 when something matched, 1 when nothing did, 2 on error.
"
);

enum Command {
    Run(Options),
    Help,
    Version,
}

/// Reads the arguments after the program name. Options can come before,
/// between or after the two positionals, take their value as the next
/// argument or after `=`, and `--` ends them.
fn parse_args(args: impl Iterator<Item = OsString>) -> Result<Command, String> {
    let mut args = args.peekable();
    let mut record_types = Vec::new();
    let mut origin = None;
    let mut json = false;
    let mut data = None;
    let mut resolve = false;
    let mut positionals = Vec::new();

    while let Some(arg) = args.next() {
        // An argument that isn't UTF-8 can't be an option.
        let Some(arg) = arg.to_str().filter(|a| is_option(a)) else {
            positionals.push(arg);
            continue;
        };
        if arg == "--" {
            positionals.extend(args);
            break;
        }
        let (flag, inline) = match arg.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => (flag, Some(value)),
            _ => (arg, None),
        };
        let mut value = |shown: &str| match inline {
            Some(v) => Ok(v.to_string()),
            // A missing value shouldn't swallow the next option, so a value
            // that starts with a dash has to come after `=`.
            None => args
                .next_if(|next| next.to_str().is_none_or(|n| !is_option(n)))
                .ok_or_else(|| format!("'{shown}' needs a value"))?
                .into_string()
                .map_err(|v| format!("invalid value '{}' for '{shown}': not UTF-8", v.display())),
        };

        match flag {
            "-h" | "--help" | "-V" | "--version" | "--json" | "--resolve" if inline.is_some() => {
                return Err(format!("'{flag}' doesn't take a value"));
            }
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "--json" => {
                once(json, flag)?;
                json = true;
            }
            "--resolve" => {
                once(resolve, flag)?;
                resolve = true;
            }
            "--type" => {
                let shown = "--type <TYPE>";
                for raw in value(shown)?.split(',') {
                    if raw.is_empty() {
                        return Err(format!(
                            "invalid value '{raw}' for '{shown}': empty record type"
                        ));
                    }
                    record_types.push(raw.to_ascii_uppercase());
                }
            }
            "--origin" => {
                let shown = "--origin <NAME>";
                let raw = once(origin.is_some(), shown).and_then(|()| value(shown))?;
                let name = Name::parse(&raw, Some(&Name::root()))
                    .map_err(|e| format!("invalid value '{raw}' for '{shown}': {e}"))?;
                origin = Some(name);
            }
            "--data" => {
                let shown = "--data <NAME|IP>";
                data = Some(once(data.is_some(), shown).and_then(|()| value(shown))?);
            }
            _ => return Err(format!("unexpected argument '{arg}'")),
        }
    }

    if resolve && data.is_some() {
        return Err("'--resolve' can't be used with '--data <NAME|IP>'".into());
    }

    let mut positionals = positionals.into_iter();
    let (query, file) = match (positionals.next(), positionals.next(), positionals.next()) {
        (Some(query), Some(file), None) => (query, file),
        (_, _, Some(extra)) => {
            return Err(format!("unexpected argument '{}'", extra.display()));
        }
        (None, _, _) => return Err("<QUERY> and <FILE> are required".into()),
        (Some(_), None, _) => return Err("<FILE> is required".into()),
    };
    let query = query
        .into_string()
        .map_err(|q| format!("invalid value '{}' for '<QUERY>': not UTF-8", q.display()))?;

    Ok(Command::Run(Options {
        query,
        file: file.into(),
        record_types,
        origin,
        json,
        data,
        resolve,
    }))
}

/// `-` on its own is stdin, not an option.
fn is_option(arg: &str) -> bool {
    arg.starts_with('-') && arg != "-"
}

/// Fails if an option that can only be given once has been seen already.
fn once(seen: bool, shown: &str) -> Result<(), String> {
    if seen {
        return Err(format!("'{shown}' can only be given once"));
    }
    Ok(())
}

fn main() -> ExitCode {
    let opts = match parse_args(std::env::args_os().skip(1)) {
        Ok(Command::Run(opts)) => opts,
        // `print!` would panic on a closed pipe, and there's nothing
        // useful to do about one here.
        Ok(Command::Help) => {
            let _ = io::stdout().write_all(HELP.as_bytes());
            return ExitCode::SUCCESS;
        }
        Ok(Command::Version) => {
            let version = concat!("zoneq ", env!("ZONEQ_VERSION"), "\n");
            let _ = io::stdout().write_all(version.as_bytes());
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!(
                concat!("zoneq: {}\n\n", usage!(), "\nTry 'zoneq --help' for more."),
                e
            );
            return ExitCode::from(2);
        }
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
