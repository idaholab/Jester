use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_yaml::from_reader;
use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

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
    metadata_data_source_id: String,
}

#[derive(Debug)]
enum DataSourceMessage {
    Test(String),
    Data(Vec<u8>),
    Close,
}

type DataSources = Arc<RwLock<HashMap<String, mpsc::Sender<DataSourceMessage>>>>;

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

    // for each directory start a file watcher - we start these in threads because we'll be tailing
    // the files
    let mut handles = vec![];
    for directory in config_file.directories {
        let channels = data_source_channels.clone();
        let thread = tokio::spawn(async move {
            for (id, channel) in channels.read().await.iter() {
                channel
                    .send(DataSourceMessage::Test("bob".to_string()))
                    .await;
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

    println!("Directories listed in configuration file no longer exist or cannot be listened to, exiting..");
    for result in results {
        match result {
            Ok(_) => {}
            Err(e) => println!("error in spawned watcher {:?}", e),
        }
    }
    std::process::exit(0)
}

async fn data_source_thread(data_sources: DataSources, data_source_id: &String) {
    if !data_sources
        .read()
        .await
        .contains_key(data_source_id.as_str())
    {
        let (tx, mut rx) = mpsc::channel::<DataSourceMessage>(2048);
        tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                println!("{:?}", message);
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
