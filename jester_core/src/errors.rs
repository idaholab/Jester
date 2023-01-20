use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProcessorError {
    #[error("unknown processor error")]
    Unknown,
}
