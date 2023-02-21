pub mod errors;

use crate::errors::ProcessorError;
use sqlx::{Pool, Sqlite};
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::SyncSender;
use tokio::sync::mpsc::UnboundedSender;

pub static CORE_VERSION: &str = env!("CARGO_PKG_VERSION");
pub static RUSTC_VERSION: &str = env!("RUSTC_VERSION");

#[derive(Debug)]
pub enum DataSourceMessage {
    File(PathBuf),
    Data(Vec<u8>),
    Close,
}

// this represents the public interface a plugin must satisfy and provide via dynamic library
// THIS IS STILL UNDER CONSTRUCTION WE HAVE NOT FINALIZED THIS TRAIT AND THE CONTRACT IT MAKES
// BETWEEN JESTER AND YOUR LIBRARY USE AT YOUR OWN RISK
pub trait Processor {
    fn init(&self, db: Pool<Sqlite>) -> Result<(), ProcessorError>;

    fn process(
        &self,
        file: PathBuf,
        db: Pool<Sqlite>,
        timeseries_chan: Option<UnboundedSender<DataSourceMessage>>,
        graph_chan: Option<UnboundedSender<DataSourceMessage>>,
    ) -> Result<(), ProcessorError>;
}

pub struct PluginDeclaration {
    pub rustc_version: &'static str,
    pub core_version: &'static str,
    pub register: unsafe extern "C" fn(&mut dyn PluginRegistrar),
}

pub trait PluginRegistrar {
    fn register_function(&mut self, function: Box<dyn Processor>);
}

#[macro_export]
macro_rules! export_plugin {
    ($register:expr) => {
        #[doc(hidden)]
        #[no_mangle]
        pub static plugin_declaration: $crate::PluginDeclaration = $crate::PluginDeclaration {
            rustc_version: $crate::RUSTC_VERSION,
            core_version: $crate::CORE_VERSION,
            register: $register,
        };
    };
}
