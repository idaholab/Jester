pub static CORE_VERSION: &str = env!("CARGO_PKG_VERSION");
pub static RUSTC_VERSION: &str = env!("RUSTC_VERSION");

pub trait Processor {
    fn process(&self, input: String) -> String;
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
