use jester_core::errors::ProcessorError;
use jester_core::{DataSourceMessage, PluginDeclaration, Processor, ProcessorReader};
use libloading::Library;
use std::ffi::OsStr;
use std::io;
use std::rc::Rc;
use std::sync::mpsc::SyncSender;

/// A proxy object which wraps a [`Processor`] and makes sure it can't outlive
/// the library it came from.
pub struct PluginProxy {
    function: Box<dyn Processor>,
    _lib: Rc<Library>,
}

impl Processor for PluginProxy {
    fn process(
        &self,
        input: ProcessorReader,
        timeseries_chan: SyncSender<DataSourceMessage>,
        metadata_chan: SyncSender<DataSourceMessage>,
    ) -> Result<(), ProcessorError> {
        self.function.process(input, timeseries_chan, metadata_chan)
    }
}

pub struct Plugin {
    functions: PluginProxy,
    library: Rc<Library>,
}

unsafe impl Send for Plugin {}
unsafe impl Sync for Plugin {}

// Plugin represents the external library loaded at runtime and creates and internal proxy around it
// all interaction with the library should be wrapped by this plugin implementation so as to make sure
// the library always outlives what's calling it and it doesn't get accidentally freed
impl Plugin {
    pub unsafe fn new<P: AsRef<OsStr>>(library_path: P) -> Result<Plugin, io::Error> {
        let library = match Library::new(library_path) {
            Ok(l) => l,
            Err(e) => {
                panic!("unable to load plugin {:?}", e)
            }
        };

        let library = Rc::new(library);

        // get a pointer to the plugin_declaration symbol.
        let decl = match library.get::<*mut PluginDeclaration>(b"plugin_declaration\0") {
            Ok(d) => d,
            Err(e) => {
                panic!("unable to load plugin declaration {:?}", e)
            }
        };

        let decl = decl.read();

        // version checks to prevent accidental ABI incompatibilities
        if decl.rustc_version != jester_core::RUSTC_VERSION
            || decl.core_version != jester_core::CORE_VERSION
        {
            return Err(io::Error::new(io::ErrorKind::Other, "Version mismatch"));
        }

        let mut registrar = PluginRegistrar::new(Rc::clone(&library));

        (decl.register)(&mut registrar);

        Ok(Plugin {
            functions: registrar.functions.unwrap(),
            library,
        })
    }

    pub fn process(
        &self,
        input: ProcessorReader,
        timeseries_chan: SyncSender<DataSourceMessage>,
        metadata_chan: SyncSender<DataSourceMessage>,
    ) -> Result<(), ProcessorError> {
        self.functions
            .process(input, timeseries_chan, metadata_chan)
    }
}

// Copy in the plugin registration code
struct PluginRegistrar {
    functions: Option<PluginProxy>,
    lib: Rc<Library>,
}

impl PluginRegistrar {
    fn new(lib: Rc<Library>) -> PluginRegistrar {
        PluginRegistrar {
            lib,
            functions: None,
        }
    }
}

impl jester_core::PluginRegistrar for PluginRegistrar {
    fn register_function(&mut self, function: Box<dyn Processor>) {
        let proxy = PluginProxy {
            function,
            _lib: Rc::clone(&self.lib),
        };
        self.functions = Some(proxy)
    }
}
