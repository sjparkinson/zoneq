use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crate::zone::write_escaped;

#[derive(Debug)]
pub enum Error {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    Parse {
        path: PathBuf,
        line: usize,
        message: String,
    },
    Query(String),
    /// Writing the matches failed, e.g. with a broken pipe.
    Output(io::Error),
}

impl Error {
    pub(crate) fn io(path: &Path, source: io::Error) -> Error {
        Error::Io {
            path: path.to_path_buf(),
            source,
        }
    }

    pub(crate) fn parse(path: &Path, line: usize, message: impl Into<String>) -> Error {
        Error::Parse {
            path: path.to_path_buf(),
            line,
            message: message.into(),
        }
    }
}

/// Messages quote the zone file, so control characters in it come out as
/// escapes rather than reaching the terminal.
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Error::Io { path, source } => format!("{}: {source}", path.display()),
            Error::Parse {
                path,
                line,
                message,
            } => format!("{}:{line}: {message}", path.display()),
            Error::Query(message) => format!("bad query: {message}"),
            Error::Output(source) => format!("writing output: {source}"),
        };
        write_escaped(f, &text)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_escape_control_characters() {
        let err = Error::parse(Path::new("z"), 3, "found \x1b[2J");
        assert_eq!(err.to_string(), r"z:3: found \027[2J");
    }
}
