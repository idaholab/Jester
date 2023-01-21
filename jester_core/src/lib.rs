pub mod errors;

use crate::errors::ProcessorError;
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::SyncSender;

pub static CORE_VERSION: &str = env!("CARGO_PKG_VERSION");
pub static RUSTC_VERSION: &str = env!("RUSTC_VERSION");

#[derive(Debug)]
pub enum DataSourceMessage {
    File(PathBuf),
    Test(String),
    Data(Vec<u8>),
    Close,
}

pub struct ProcessorReader {}

impl ProcessorReader {
    pub fn new() -> ProcessorReader {
        ProcessorReader {}
    }
}

impl Read for ProcessorReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        Ok(2)
    }
}

pub trait Processor {
    fn process(
        &self,
        input: ProcessorReader,
        timeseries_chan: SyncSender<DataSourceMessage>,
        metadata_chan: SyncSender<DataSourceMessage>,
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
