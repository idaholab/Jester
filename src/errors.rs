use thiserror::Error;

#[derive(Error, Debug)]
pub enum WatcherError {
    #[error("unknown processor error")]
    Unknown,
    #[error("glob matching error")]
    Glob(#[from] glob::PatternError),
}
