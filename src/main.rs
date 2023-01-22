extern crate core;

mod errors;
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
use adler::adler32;
use chrono::{DateTime, Utc};
use deeplynx_rust_sdk::DeepLynxAPI;
use jester_core::DataSourceMessage;
use std::alloc::System;
use std::fs;
use std::io::BufReader;
use std::str::FromStr;
use std::sync::mpsc::{SendError, SyncSender};
use std::time::Duration;

use crate::errors::WatcherError;
use glob::glob;
use log::{debug, error, info, trace, warn};
use sqlx::sqlite::{SqliteConnectOptions, SqliteQueryResult};
use sqlx::{Pool, Sqlite, SqlitePool};

use env_logger;
use include_dir::include_dir;
use sqlx::Error::RowNotFound;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio::time::sleep;

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
    files: Vec<FileConfig>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct FileConfig {
    path_pattern: String,
    container_id: u64,
    data_source_id: Option<u64>,
    metadata_data_source_id: Option<u64>,
}

type DataSources = Arc<RwLock<HashMap<u64, UnboundedSender<DataSourceMessage>>>>;

#[tokio::main]
async fn main() {
    env_logger::init();

    let cli: Arguments = Arguments::parse();
    let config_file_path = match cli.config_file {
        None => {
            error!("You must provide a configuration file path");
            std::process::exit(1);
        }
        Some(p) => p,
    };

    let config_file = match File::open(config_file_path) {
        Ok(f) => f,
        Err(e) => {
            error!("Unable to open config file {}", e);
            std::process::exit(1);
        }
    };

    let config_file: Config = match from_reader(config_file) {
        Ok(t) => t,
        Err(e) => {
            error!("Unable to parse config file {}", e);
            std::process::exit(1);
        }
    };

    if config_file.files.len() <= 0 {
        error!("Must provide directories to monitor");
        std::process::exit(1)
    }

    // create and/or connect to a local db file managed by sqlite
    let options = match SqliteConnectOptions::from_str("sqlite://.jester.db") {
        Ok(o) => o,
        Err(e) => {
            panic!("unable to create options for sqlite connection {:?}", e)
        }
    };

    let db = match SqlitePool::connect_with(options.create_if_missing(true)).await {
        Ok(d) => d,
        Err(e) => {
            panic!("unable to connect to sqlite database {:?}", e)
        }
    };

    // run the migrations
    include_dir!("./migrations");
    match sqlx::migrate!("./migrations").run(&db).await {
        Ok(_) => {}
        Err(e) => {
            panic!("error while running migrations {:?}", e)
        }
    }

    let mut client = match DeepLynxAPI::new(
        config_file.deep_lynx_url,
        Some(config_file.api_key),
        Some(config_file.api_secret),
    )
    .await
    {
        Ok(c) => c,
        Err(e) => {
            panic!("error while initializing DeepLynx API client: {:?}", e);
        }
    };

    // run through all the files and open a single connection to the data sources, was meant to
    // hold a websocket connection
    let data_source_channels: DataSources = DataSources::default();
    for file in &config_file.files {
        if file.data_source_id.is_some() {
            data_source_thread(
                data_source_channels.clone(),
                client.clone(),
                file.data_source_id.clone(),
                file.container_id,
            )
            .await;
        }

        if file.metadata_data_source_id.is_some() {
            data_source_thread(
                data_source_channels.clone(),
                client.clone(),
                file.metadata_data_source_id.clone(),
                file.container_id,
            )
            .await;
        }
    }

    let mut plugin: Option<Plugin> = None;
    match cli.plugin_path {
        None => {}
        Some(p) => unsafe {
            let external_functions = Plugin::new(p).expect("Plugin loading failed");

            plugin = Some(external_functions)
        },
    };

    let plugin = Arc::new(RwLock::new(plugin));

    // for each directory start a file watcher - we start these in threads because we'll be tailing
    // the files
    let mut handles = vec![];
    for file in config_file.files {
        // all cheap clones of pointers to main systems, setups
        let channels = data_source_channels.clone();
        let inner_plugin = plugin.clone();
        let db = db.clone();

        let thread =
            tokio::spawn(async move { watch_file(file, channels, inner_plugin, db).await });

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
            Err(e) => error!("error in spawned watcher {:?}", e),
        }
    }
    warn!("Directories listed in configuration file no longer exist or cannot be listened to, exiting..");
    std::process::exit(0)
}

async fn data_source_thread(
    data_sources: DataSources,
    mut api: DeepLynxAPI,
    data_source_id: Option<u64>,
    container_id: u64,
) {
    match data_source_id {
        None => {}
        Some(data_source_id) => {
            if !data_sources.write().await.contains_key(&data_source_id) {
                let (tx, mut rx) = unbounded_channel();
                tokio::spawn(async move {
                    while let Some(message) = rx.recv().await {
                        match message {
                            DataSourceMessage::File(path) => {
                                api.import(container_id, data_source_id, Some(path), None)
                                    .await;
                                ();
                            }
                            DataSourceMessage::Test(_) => {}
                            DataSourceMessage::Data(_) => {}
                            DataSourceMessage::Close => {}
                        }
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
    };
}

#[derive(sqlx::FromRow, Debug)]
struct DBFile {
    pattern: String, //pattern's are unique as in they are always relative paths from jester
    checksum: Option<u32>,
    created_at: String,
    transmitted_at: Option<String>,
}

async fn watch_file(
    file: FileConfig,
    data_sources: DataSources,
    plugin: Arc<RwLock<Option<Plugin>>>,
    db: Pool<Sqlite>,
) -> Result<(), WatcherError> {
    // for each file, run the glob matching and act on the results - eventually we can make this async
    // but since most OSes don't offer an async file event system, it's not the end of the world
    // match the pattern included by the user
    loop {
        info!("starting watch on {}", file.path_pattern);
        for entry in glob(file.path_pattern.as_str())? {
            let path = match entry {
                Ok(p) => p,
                Err(e) => return Err(WatcherError::GlobError(e)),
            };

            // we will need the checksum at some point
            let f = File::open(&path)?;
            let checksum = adler32(BufReader::new(f))?;

            // TODO: update this query with an optional time to make subsequent queries after we've been running faster
            match sqlx::query_as::<_, DBFile>("SELECT * FROM files WHERE pattern = ? LIMIT 1")
                .bind(&path.to_str().unwrap())
                .fetch_one(&db)
                .await
            {
                Ok(f) => {
                    // if the file exists in the db and has a transmitted at date we need to check the last modified date and make
                    // sure we shouldn't resend it TODO: clarify this functionality and modify to handle files where we stopped halfway
                    match f.transmitted_at {
                        None => {} // if not transmitted we assume it needs to be sent to DeepLynx
                        Some(transmitted_at) => {
                            let last_modified_at: DateTime<Utc> =
                                fs::metadata(&path)?.modified()?.into();
                            let transmitted_at =
                                DateTime::parse_from_rfc3339(transmitted_at.as_str())?;

                            if last_modified_at > transmitted_at {
                                // we should double check the checksum of the file in insure it really is different
                                // we're using adler here because it's great for quick data integrity checks and
                                // that's all we need really, if it's the same, skip the file
                                match f.checksum {
                                    None => {}
                                    Some(c) => {
                                        if c == checksum {
                                            debug!("file at {} hasn't changed since transmission, skipping", &path.to_str().unwrap());
                                            continue;
                                        }
                                    }
                                }
                            } else {
                                debug!(
                                    "file at {} hasn't changed since transmission, skipping",
                                    &path.to_str().unwrap()
                                );
                                continue;
                            }
                        }
                    }
                }
                Err(e) => {
                    match e {
                        RowNotFound => {
                            // if it's not found, enter it in the db
                            match sqlx::query("INSERT INTO files(pattern, created_at) VALUES (?,?)")
                                .bind(&path.to_str().unwrap())
                                .bind(Utc::now().to_rfc3339())
                                .execute(&db)
                                .await
                            {
                                Ok(_) => {
                                    debug!("file at {} added to database", &path.to_str().unwrap())
                                }
                                Err(e) => {
                                    error!(
                        "unable to update the database with initial record for {}: {:?}",
                        path.to_str().unwrap(),
                        e
                    )
                                }
                            }
                        }
                        _ => {
                            error!("unable to fetch files from db {:?}", e);
                            continue;
                        }
                    }
                }
            };

            // now that we know we don't have this file in the db, or that it needs to be reprocessed - do so
            if plugin.read().await.is_some() {
                // TODO: hook up plugin specific processing
                continue;
            }

            // fall back to the default behavior of simply sending the file to DeepLynx, we do this
            // by sending a message to the relevant data sources with the file's path
            match &file.data_source_id {
                None => {}
                Some(id) => match data_sources.write().await.get(id) {
                    None => {}
                    Some(channel) => {
                        let p = path.clone();
                        match channel.send(DataSourceMessage::File(p)) {
                            Ok(_) => {
                                debug!(
                                    "transmitted file at {} to the data source",
                                    &path.to_str().unwrap()
                                );
                            }
                            Err(e) => {
                                error!("error sending file message to DataSource {:?}", e)
                            }
                        }
                    }
                },
            }

            match &file.metadata_data_source_id {
                None => {}
                Some(id) => match data_sources.write().await.get(id) {
                    None => {}
                    Some(channel) => {
                        let p = path.clone();
                        match channel.send(DataSourceMessage::File(p)) {
                            Ok(_) => {
                                debug!(
                                    "transmitted file at {} to the data source",
                                    &path.to_str().unwrap()
                                );
                            }
                            Err(e) => {
                                error!("error sending file message to DataSource {:?}", e)
                            }
                        };
                    }
                },
            }

            // update the file setting its transmitted time
            // capture any errors in the db if the transmission runs into issues
            match sqlx::query("UPDATE files SET transmitted_at = ?, checksum = ? WHERE pattern = ?")
                .bind(Utc::now().to_rfc3339())
                .bind(checksum)
                .bind(&path.to_str().unwrap())
                .execute(&db)
                .await
            {
                Ok(_) => {
                    info!("file at {} transmitted to DeepLynx", path.to_str().unwrap())
                }
                Err(e) => {
                    error!(
                        "unable to update the database with transmit time for {}: {:?}",
                        path.to_str().unwrap(),
                        e
                    )
                }
            }
        }

        sleep(Duration::from_secs(1)).await
    }
}
