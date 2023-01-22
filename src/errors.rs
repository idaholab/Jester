use std::io;
use thiserror::Error;

// Project specific errors and wrappers of other libraries errors so we can always return ours but
// still be able to use ? notation
#[derive(Error, Debug)]
pub enum WatcherError {
    #[error("unknown processor error")]
    Unknown,
    #[error("pattern matching error")]
    PatternError(#[from] glob::PatternError),
    #[error("glob matching error")]
    GlobError(#[from] glob::GlobError),
    #[error("io")]
    IOError(#[from] io::Error),
    #[error("time parse error")]
    TimeParseError(#[from] chrono::ParseError),
}
