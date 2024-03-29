# Jester

A configurable file watcher and data/file uploader to the DeepLynx data warehouse.

-------------

## Requirements
Jester has no requirements apart from the ability to run a binary on the host system. We are compiling for all OSes and architectures. If we lack one that you need, please either contact us or build this program from source.

### Building from Source Requirements

- Rust ^1.6.5 (Nightly Branch)

#### Switching to Nightly
1. Run `rustup toolchain install nightly`
2. Run `rustup default nightly`


## Usage
```shell
Usage: jester [OPTIONS]

Options:
  -c, --config-file <FILE>         
  -p, --plugin-path <PLUGIN_PATH>  
  -h, --help                       Print help
  -V, --version                    Print version

```
Running Jester is very simple and requires only that you provide it a configuration file - either through the `-c` option or by placing a file called `.config.yml` in the same directory as Jester. You may also optionally provide it a plugin, in the form of a Rust compiled `.so`. More information about that can be found in the code and `jester_core` library.

### Configuration File
Included in this repository is a sample configuration - `config.sample.yml`. Jester expects your configuration file to follow the same format and be a YAML document. In order to make this more convenient to understand, we are including the sample YAML file here in this readme.

#### Sample Configuration File
```yaml
api_key: "YOUR DEEPLYNX API KEY"
api_secret: "YOUR DEEPLYNX API SECRET"
deep_lynx_url: "http://localhost:8090"
files: # can contain multiple files
  - data_source_id: 469 # OPTIONAL timeseries data source,  but you need this or metadata data source
    metadata_data_source_id: 1 # OPTIONAL metadata data source, but you need this or data source
    container_id: 1
    path_pattern: "./sample_dir/*.csv"
```

Please note that the `files` property can contain multiple `file` objects. A `file` consists of a UNIX style glob `path_pattern` (Jester will watch all directories and files that match this pattern), a `container_id`, and either or both of `data_source_id` or `metadata_data_source_id`. You may include both data sources if you want the fallback functionality to send the files to both data sources, or if your plugin requires both a timeseries and metadata data source.

### Default Behavior
Jester can be configured to run with a project, or file specific plugin. There is default behavior however, for when no plugin is supplied. This section describes that behavior.

In case of a plugin not being supplied Jester will do the following with all files that match your `path_pattern` provided in the configuration:
1. Checks to see if a plugin is present, if no plugin, will continue with the following steps
2. If `data_source_id` is present, Jester will attempt to upload the watched file to a timeseries DeepLynx data source. Keep in mind that this endpoint only accepts `.json`. and `.csv` files currently.
3. If `metadata_data_source_id` is present, Jester will attempt to upload the watched file to a standard DeepLynx data source. This endpoint accepts `.csv`, `.json` and `.xml` files.
4. Records the watched/transmitted file into its internal database as normal.

### Project/File Plugins
Jester ships with the ability to accept project or file specific plugins in the form of Rust compiled dynamically linked libraries. When a path to this dynamic library is provided when running Jester, it will attempt to load that library and use it to process your watched file instead of falling back on the default behavior (explained above).

Plugins must be compiled Rust `cdylib`. They must satisfy the `Processor` trait declared in `jester_core`. In order to make this as easy to use as possible, the code below is a sample of a library which when compiled can be included in Jester's plugin path.
```rust
use jester_core::errors::ProcessorError;
use jester_core::DataSourceMessage;
use sqlx::{Pool, Sqlite};
use std::path::PathBuf;
use tokio::sync::mpsc::UnboundedSender;

pub struct YourProcessor;

// implement the Processor trait
impl jester_core::Processor for YourProcessor {
    fn process(
        &self,
        file: PathBuf,
        db: Pool<Sqlite>,
        timeseries_chan: Option<UnboundedSender<DataSourceMessage>>,
        graph_chan: Option<UnboundedSender<DataSourceMessage>>,
    ) -> Result<(), ProcessorError> {
        // Your code implementation goes here
    }
}

// You must export the plugin and then call the registrar functions so we can actually hook into things
jester_core::export_plugin!(register);

extern "C" fn register(registrar: &mut dyn jester_core::PluginRegistrar) {
    registrar.register_function(Box::new(YourProcessor));
}

```

You can also find an example [here](https://github.inl.gov/Digital-Engineering/Jester-AGN).

More information about how to build these plugins can be found in the `jester_core` folder and in code level comments. We will update this document with examples soon.

### Webserver
Jester ships with an an optional internal webserver. To enable it simply pass the `-w` flag and navigate to `localhost:3030`. This simple web interface will help you monitor Jester's status.