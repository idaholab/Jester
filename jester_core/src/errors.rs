use crate::DataSourceMessage;
use std::any::Any;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProcessorError {
    #[error("unknown processor error")]
    Unknown,
    #[error("thread error")]
    ThreadError(Box<dyn Any + Send>),
    #[error("plugin error: {0}")]
    PluginError(#[from] anyhow::Error),
    #[error("channel send error")]
    ChannelSendError(#[from] tokio::sync::mpsc::error::SendError<DataSourceMessage>),
}
