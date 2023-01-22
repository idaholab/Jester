use std::io;
use thiserror::Error;

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
