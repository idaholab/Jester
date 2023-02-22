use std::any::Any;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProcessorError {
    #[error("unknown processor error")]
    Unknown,
    #[error("thread error")]
    ThreadError(Box<dyn Any + Send>),
    #[error(transparent)]
    PluginError(#[from] anyhow::Error),
}
