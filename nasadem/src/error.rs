use std::{error::Error as StdError, fmt, io, path::PathBuf};

#[derive(Debug)]
#[allow(missing_docs, clippy::module_name_repetitions)]
pub enum NasademError {
    Io(io::Error),
    HgtName(std::path::PathBuf),
    HgtLen(u64, PathBuf),
}

impl fmt::Display for NasademError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NasademError::Io(err) => err.fmt(f),
            NasademError::HgtName(path) => write!(f, "invalid HGT name {path:?}"),
            NasademError::HgtLen(len, path) => {
                write!(f, "invalid HGT file len {len} for {path:?}")
            }
        }
    }
}

impl From<io::Error> for NasademError {
    fn from(other: io::Error) -> NasademError {
        NasademError::Io(other)
    }
}

impl StdError for NasademError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        use NasademError::{HgtLen, HgtName, Io};
        match self {
            Io(err) => err.source(),
            HgtName(_) | HgtLen(_, _) => None,
        }
    }
}
