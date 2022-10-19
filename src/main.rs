use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_yaml::from_reader;
use std::fs::File;
use std::path::PathBuf;

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Arguments {
    #[clap(short, long, value_parser, value_name = "FILE")]
    config_file: Option<PathBuf>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Config {
    api_key: String,
    api_secret: String,
    deep_lynx_url: String,
    directories: Vec<DirectoryConfig>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct DirectoryConfig {
    path: String,
    files: Vec<FileConfig>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct FileConfig {
    pattern: String,
    container_id: String,
    data_source_id: String,
}

fn main() {
    let cli: Arguments = Arguments::parse();
    let config_file_path = match cli.config_file {
        None => {
            println!("You must provide a configuration file path");
            std::process::exit(1);
        }
        Some(p) => p,
    };

    let config_file = match File::open(config_file_path) {
        Ok(f) => f,
        Err(e) => {
            println!("Unable to open config file {}", e);
            std::process::exit(1);
        }
    };

    let config_file: Config = match from_reader(config_file) {
        Ok(t) => t,
        Err(e) => {
            println!("Unable to parse config file {}", e);
            std::process::exit(1);
        }
    };

    println!("{:?}", config_file);

    // fetch DeepLynx timeseries data source by ID

    // deserialize the data source's config into the timeseries column setup
}
