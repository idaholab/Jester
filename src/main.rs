extern crate core;

mod plugin;

use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_yaml::from_reader;
use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use tokio::sync::RwLock;

use crate::plugin::Plugin;
use jester_core::{DataSourceMessage, ProcessorReader};
use std::alloc::System;
use std::os::unix::thread;

// needed to make sure we don't accidentally free a string in our plugin system
#[global_allocator]
static ALLOCATOR: System = System;

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Arguments {
    #[clap(short, long, value_parser, value_name = "FILE")]
    config_file: Option<PathBuf>,
    #[clap(short, long)]
    plugin_path: Option<String>,
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
    metadata_data_source_id: String,
}

type DataSources = Arc<RwLock<HashMap<String, mpsc::SyncSender<DataSourceMessage>>>>;

#[tokio::main]
async fn main() {
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

    let plugin_path = match cli.plugin_path {
        None => {
            println!("You must provide a project plugin in order for Jester to function properly");
            std::process::exit(1);
        }
        Some(p) => p,
    };

    if config_file.directories.len() <= 0 {
        println!("Must provide directories to monitor");
        std::process::exit(1)
    }

    // run through all the files and open a single connection to the data sources, was meant to
    // hold a websocket connection
    let mut data_source_channels: DataSources = DataSources::default();
    for directory in &config_file.directories {
        for file in &directory.files {
            data_source_thread(data_source_channels.clone(), &file.data_source_id).await;
        }
    }

    let mut functions: Option<Plugin> = None;

    unsafe {
        let mut external_functions =
            Plugin::new("/Users/darrjw/IdeaProjects/jester-isu/target/debug/libjester_isu.so")
                .expect("Plugin loading failed");

        functions = Some(external_functions)
    }

    let mut plugin = match functions {
        None => {
            panic!("unable to load plugin")
        }
        Some(p) => p,
    };

    let plugin = Arc::new(RwLock::new(plugin));

    // for each directory start a file watcher - we start these in threads because we'll be tailing
    // the files
    let mut handles = vec![];
    for directory in config_file.directories {
        let channels = data_source_channels.clone();
        let inner_plugin = plugin.clone();
        let thread = tokio::spawn(async move {
            for (_, chan) in channels.read().await.iter() {
                inner_plugin.clone().read().await.process(
                    ProcessorReader::new(),
                    chan.clone(),
                    chan.clone(),
                );
            }
        });

        handles.push(thread);
    }

    // if all the directory threads finish it means the directory's no longer exist and the program
    // should be exited with an error
    let mut results = vec![];
    for handle in handles {
        results.push(handle.await);
    }

    for result in results {
        match result {
            Ok(_) => {}
            Err(e) => println!("error in spawned watcher {:?}", e),
        }
    }
    println!("Directories listed in configuration file no longer exist or cannot be listened to, exiting..");
    std::process::exit(0)
}

async fn data_source_thread(data_sources: DataSources, data_source_id: &String) {
    if !data_sources
        .read()
        .await
        .contains_key(data_source_id.as_str())
    {
        let (tx, mut rx) = mpsc::sync_channel(2048);
        tokio::spawn(async move {
            while let Ok(message) = rx.recv() {
                println!("{:?}", message);
                // BODY WHERE WE SEND THINGS OR ACT ON DATA SOURCE MESSAGES
            }
        });

        match data_sources
            .write()
            .await
            .insert(data_source_id.clone(), tx)
        {
            Some(_) => {}
            None => {}
        }
    }
}
