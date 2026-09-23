use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

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

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Error::Parse {
                path,
                line,
                message,
            } => write!(f, "{}:{line}: {message}", path.display()),
            Error::Query(message) => write!(f, "bad query: {message}"),
            Error::Output(source) => write!(f, "writing output: {source}"),
        }
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
