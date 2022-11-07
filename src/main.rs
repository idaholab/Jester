use std::collections::HashMap;
use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_yaml::from_reader;
use std::fs::File;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};

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

#[derive(Debug)]
enum DataSourceMessage {
    Data(Vec<u8>),
    Close
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

    // run through all the files and open a single connection to the data sources, was meant to
    // hold a websocket connection
    let mut data_source_channels: HashMap<String, Sender<DataSourceMessage>> = HashMap::new();
    for directory in &config_file.directories {
        for file in &directory.files {
            if !data_source_channels.contains_key(file.data_source_id.as_str()) {
                let (tx, rx) = channel::<DataSourceMessage>();
                std::thread::spawn(move || {
                    while let Ok(message) = rx.recv(){
                        println!("{:?}", message);
                    }
                });

                match data_source_channels.insert(file.data_source_id.clone(), tx) {
                    Some(_) => {},
                    None => {
                        println!("unable to start data source hash map");
                        std::process::exit(1)
                    }
                }
            }
        }
    }


    // for each directory start a file watcher - we start these in threads because we'll be tailing
    // the files
    let mut handles = vec![];
    for directory in config_file.directories {
        let channels = data_source_channels.clone();
        let thread = std::thread::spawn(move || {
            channels.iter();
        });
        handles.push(thread);
    }

    // if all the directory threads finish it means the directory's no longer exist and the program
    // should be exited with an error
    for handle in handles {
        let _ = handle.join();
    }

    println!("Directories listed in configuration file no longer exist or cannot be listened to");
    std::process::exit(1)
}
