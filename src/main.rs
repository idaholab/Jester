///# Jester
///
/// A configurable file watcher and data/file uploader to the DeepLynx data warehouse.
///
/// -------------
///
/// ## Requirements
/// Jester has no requirements apart from the ability to run a binary on the host system. We are compiling for all OSes and architectures. If we lack one that you need, please either contact us or build this program from source.
///
/// ### Building from Source Requirements
///
/// - Rust ^1.6.5
///
///
/// ## Usage
/// ```shell
/// Usage: jester [OPTIONS]
//
/// Options:
///   -c, --config-file <FILE>         
///   -p, --plugin-path <PLUGIN_PATH>  
///   -h, --help                       Print help
///   -V, --version                    Print version
///
/// ```
/// Running Jester is very simple and requires only that you provide it a configuration file. You may also optionally provide it a plugin, in the form of a Rust compiled `.so`. More information about that can be found in the code and `jester_core` library.
///
/// ### Configuration File
/// Included in this repository is a sample configuration - `config.sample.yml`. Jester expects your configuration file to follow the same format and be a YAML document. In order to make this more convenient to understand, we are including the sample YAML file here in this readme.
///
/// #### Sample Configuration File
/// ```yaml
/// api_key: "YOUR DEEPLYNX API KEY"
/// api_secret: "YOUR DEEPLYNX API SECRET"
/// deep_lynx_url: "http://localhost:8090"
/// files: # can contain multiple files
///   - data_source_id: 469 # OPTIONAL timeseries data source,  but you need this or metadata data source
///     metadata_data_source_id: 1 # OPTIONAL metadata data source, but you need this or data source
///     container_id: 1
///     path_pattern: "./sample_dir/*.csv"
/// ```
///
/// Please note that the `files` property can contain multiple `file` objects. A `file` consists of a UNIX style glob `path_pattern` (Jester will watch all directories and files that match this pattern), a `container_id`, and either or both of `data_source_id` or `metadata_data_source_id`. You may include both data sources if you want the fallback functionality to send the files to both data sources, or if your plugin requires both a timeseries and metadata data source.
///
/// ### Default Behavior
/// Jester can be configured to run with a project, or file specific plugin. There is default behavior however, for when no plugin is supplied. This section describes that behavior.
///
/// In case of a plugin not being supplied Jester will do the following with all files that match your `path_pattern` provided in the configuration:
/// 1. Checks to see if a plugin is present, if no plugin, will continue with the following steps
/// 2. If `data_source_id` is present, Jester will attempt to upload the watched file to a timeseries DeepLynx data source. Keep in mind that this endpoint only accepts `.json`. and `.csv` files currently.
/// 3. If `metadata_data_source_id` is present, Jester will attempt to upload the watched file to a standard DeepLynx data source. This endpoint accepts `.csv`, `.json` and `.xml` files.
/// 4. Records the watched/transmitted file into its internal database as normal.
///
/// ### Project/File Plugins
/// Jester ships with the ability to accept project or file specific plugins in the form of Rust compiled dynamically linked libraries. When a path to this dynamic library is provided when running Jester, it will attempt to load that library and use it to process your watched file instead of falling back on the default behavior (explained above).
///
/// More information about how to build these plugins can be found in the `jester_core` folder and in code level comments. We will update this document with examples soon.
extern crate core;

mod deep_lynx;
mod errors;
mod plugin;
mod templates;

use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_yaml::from_reader;
use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::plugin::Plugin;
use adler::adler32;
use chrono::{DateTime, Utc};
use jester_core::DataSourceMessage;
use std::alloc::System;
use std::fs;
use std::io::BufReader;
use std::str::FromStr;
use std::time::Duration;

use crate::errors::WatcherError;
use glob::glob;
use log::{debug, error, info};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Pool, Sqlite, SqlitePool};

use crate::deep_lynx::DeepLynxAPI;
use crate::templates::MAIN_PAGE_TEMPLATE;
use env_logger;
use futures::future::join_all;
use handlebars::Handlebars;
use include_dir::include_dir;
use serde_json::json;
use sqlx::Error::RowNotFound;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio::time::sleep;
use warp::Filter;

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
    #[clap(short, long, action)]
    web_server: bool,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Config {
    api_key: String,
    api_secret: String,
    deep_lynx_url: String,
    files: Vec<FileConfig>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
struct FileConfig {
    path_pattern: String,
    container_id: u64, // ids are 64 bit uints to match the data type DeepLynx uses
    timeseries_data_source_id: Option<u64>,
    graph_data_source_id: Option<u64>,
    scan_interval: u64,
    delete_after_read: Option<bool>,
}

// a thread-safe map of all the data sources - this insures we have only one active thread per data
// source - the key is (container_id, data_source_id) since data_source_id could be shared across
// different containers
type DataSources = Arc<RwLock<HashMap<(u64, u64), UnboundedSender<DataSourceMessage>>>>;

// this is for our simple webserver template engine code and helper for the endpoint
struct WithTemplate<T: Serialize> {
    name: &'static str,
    value: T,
}

fn render<T>(template: WithTemplate<T>, hbs: Arc<Handlebars<'_>>) -> impl warp::Reply
where
    T: Serialize,
{
    let render = hbs
        .render(template.name, &template.value)
        .unwrap_or_else(|err| err.to_string());
    warp::reply::html(render)
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let cli: Arguments = Arguments::parse();

    let config_file_path = match cli.config_file {
        None => {
            let mut p = PathBuf::new();
            p.push(".config.yml");

            p
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

    // create and/or connect to a local db file managed by sqlite - this is how we keep track of
    // the files we've seen for individual path patterns for an individual containers
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

    // run the migrations for initial schema and updates - this step helps guarantee that updated
    // jesters don't need to to wipe their local db to start over
    include_dir!("./migrations");
    match sqlx::migrate!("./migrations").run(&db).await {
        Ok(_) => {}
        Err(e) => {
            panic!("error while running migrations {:?}", e)
        }
    }

    // build the DeepLynx api client
    let client = match DeepLynxAPI::new(
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

    // run through all the files and open a single connection to the data sources this helps us
    // minimize the number of threads and channels needed - we could open one for each file, but as
    // they might be going to the same data source it doesn't make sense to waste system resources
    let data_source_channels: DataSources = DataSources::default();
    for file in &config_file.files {
        if file.timeseries_data_source_id.is_some() {
            data_source_thread(
                data_source_channels.clone(),
                client.clone(),
                file.timeseries_data_source_id.clone(),
                file.clone().container_id,
            )
            .await;
        }

        if file.graph_data_source_id.is_some() {
            data_source_thread(
                data_source_channels.clone(),
                client.clone(),
                file.graph_data_source_id.clone(),
                file.clone().container_id,
            )
            .await;
        }
    }

    // now we load the plugin if one has been provided - see the plugin files or jester-core for more info
    let mut plugin: Option<Plugin> = None;
    match cli.plugin_path {
        None => {}
        Some(p) => unsafe {
            let external_functions = Plugin::new(p).expect("Plugin loading failed");
            match external_functions.init(db.clone()) {
                Ok(_) => plugin = Some(external_functions),
                Err(e) => {
                    panic!("error while initializing plugin {:?}", e)
                }
            }
        },
    };

    // make sure the plugin is thread safe for access
    let plugin = Arc::new(RwLock::new(plugin));

    // for each directory start a file watcher - we start these in threads so we can not be bound by
    // I/O for a single file, and so we can watch multiple directories
    let mut handles = vec![];
    for file in config_file.files {
        // all cheap clones of pointers to main systems, setups, variables
        let channels = data_source_channels.clone();
        let inner_plugin = plugin.clone();
        let db = db.clone();

        let thread =
            tokio::spawn(async move { watch_file(file, channels, inner_plugin, db).await });

        handles.push(thread);
    }

    // run our webserver as long as they've turned it on
    if cli.web_server {
        let mut hb = Handlebars::new();
        // register the template
        hb.register_template_string("main.html", MAIN_PAGE_TEMPLATE)
            .unwrap();

        // Turn Handlebars instance into a Filter so we can combine it
        // easily with others...
        let hb = Arc::new(hb);

        // Create a reusable closure to render template
        let handlebars = move |with_template| render(with_template, hb.clone());
        let db_filter = warp::any().map(move || db.clone());

        let route = warp::get()
            .and(warp::path::end())
            .and(db_filter)
            .and_then(|pool| async move {
                Ok(sqlx::query_as::<_, DBFile>("SELECT * FROM files")
                    .fetch_all(&pool)
                    .await
                    .unwrap()) as Result<Vec<DBFile>, warp::reject::Rejection>
            })
            .map(|files| WithTemplate {
                name: "main.html",
                value: json!(MainPageTemplate { files }),
            })
            .map(handlebars);

        // we only serve on the local machine, don't want this exposed outside of it typically
        warp::serve(route).run(([0, 0, 0, 0], 3030)).await;
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
    error!("Directories listed in configuration file no longer exist or cannot be listened to, exiting..");
    std::process::exit(1)
}

// this contains the thread for each passed in datasource/container combination - this thread is
// is responsible for handling the messages passed to it by the file processors
async fn data_source_thread(
    data_sources: DataSources,
    mut api: DeepLynxAPI,
    data_source_id: Option<u64>,
    container_id: u64,
) {
    match data_source_id {
        None => {}
        Some(data_source_id) => {
            if !data_sources
                .write()
                .await
                .contains_key(&(container_id, data_source_id))
            {
                let (tx, mut rx) = unbounded_channel();
                tokio::spawn(async move {
                    while let Some(message) = rx.recv().await {
                        match message {
                            // currently we're only going to handle this message, as we worked on fallback
                            // behavior first - this message indicates to the data source that it should
                            // take the provided path and upload it to DeepLynx
                            DataSourceMessage::File(path) => {
                                match api
                                    .import(
                                        container_id.clone(),
                                        data_source_id.clone(),
                                        Some(path.0.clone()),
                                        None,
                                    )
                                    .await
                                {
                                    Ok(_) => {
                                        debug!("file successfully uploaded to DeepLynx");

                                        // if we've asked to delete the file, well, delete it
                                        if path.1 {
                                            match tokio::fs::remove_file(path.0).await {
                                                Ok(_) => {
                                                    debug!("file successfully removed after send")
                                                }
                                                Err(e) => {
                                                    debug!("unable to remove file {:?}", e)
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        error!("unable to upload file to DeepLynx {:?}", e)
                                    }
                                };

                                ();
                            }
                            DataSourceMessage::Data(d) => {
                                match api
                                    .import(
                                        container_id.clone(),
                                        data_source_id.clone(),
                                        None,
                                        Some(d),
                                    )
                                    .await
                                {
                                    Ok(_) => {
                                        debug!("file successfully uploaded to DeepLynx")
                                    }
                                    Err(e) => {
                                        error!("unable to upload file to DeepLynx {:?}", e)
                                    }
                                }
                            }
                            DataSourceMessage::Close => {}
                        }
                    }
                });

                match data_sources
                    .write()
                    .await
                    .insert((container_id.clone(), data_source_id.clone()), tx)
                {
                    Some(_) => {}
                    None => {}
                }
            }
        }
    };
}

// this represents the file table in the embedded database
#[derive(sqlx::FromRow, Debug, Serialize, Deserialize)]
struct DBFile {
    pattern: String, //pattern's are unique as in they are always relative paths from jester
    container_id: String,
    checksum: Option<u32>,
    created_at: String,
    transmitted_at: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct MainPageTemplate {
    files: Vec<DBFile>,
}

// watch_file is the meat of Jester - this is being run in a thread, so feel free to block or be
// infinite in here. watch file will take a file object from the config and run its path pattern,
// processing any files that match the pattern which haven't been seen before, or have changed
// since they were last processed
async fn watch_file(
    file: FileConfig,
    data_sources: DataSources,
    plugin: Arc<RwLock<Option<Plugin>>>,
    db: Pool<Sqlite>,
) -> Result<(), WatcherError> {
    // for each file, run the glob matching and act on the results - eventually we can make this async
    // but since most OSes don't offer an async file event system, it's not the end of the world
    // match the pattern included by the user
    info!("starting watch {}", file.path_pattern);
    loop {
        // sleep at the start so we always hit it
        sleep(Duration::from_secs(file.clone().scan_interval)).await;
        debug!("watching {}", file.path_pattern);
        let mut handles = vec![];
        for entry in glob(file.path_pattern.as_str())? {
            let path = match entry {
                Ok(p) => p,
                Err(e) => return Err(WatcherError::GlobError(e)),
            };

            let file_t = file.clone();
            let data_sources_t = data_sources.clone();
            let plugin_t = plugin.clone();
            let db_t = db.clone();

            let handle = tokio::spawn(async move {
                run_watch_action(file_t, data_sources_t, plugin_t, db_t, &path).await
            });

            handles.push(handle);
        }

        join_all(handles).await;
    }
}

async fn run_watch_action(
    file: FileConfig,
    data_sources: DataSources,
    plugin: Arc<RwLock<Option<Plugin>>>,
    db: Pool<Sqlite>,
    path: &PathBuf,
) -> Result<(), WatcherError> {
    // we will need the checksum at some point
    let f = File::open(&path)?;
    let checksum = adler32(BufReader::new(f))?;

    // TODO: update this query with an optional time to make subsequent queries after we've been running faster
    match sqlx::query_as::<_, DBFile>(
        "SELECT * FROM files WHERE pattern = ? AND container_id = ? LIMIT 1",
    )
    .bind(&path.to_str().unwrap())
    .bind(file.container_id.to_string())
    .fetch_one(&db)
    .await
    {
        Ok(f) => {
            // if the file exists in the db and has a transmitted at date we need to check the last modified date and make
            // sure we shouldn't resend it TODO: clarify this functionality and modify to handle files where we stopped halfway
            match f.transmitted_at {
                None => {} // if not transmitted we assume it needs to be sent to DeepLynx
                Some(transmitted_at) => {
                    let last_modified_at: DateTime<Utc> = fs::metadata(&path)?.modified()?.into();
                    let transmitted_at = DateTime::parse_from_rfc3339(transmitted_at.as_str())?;

                    if last_modified_at > transmitted_at {
                        // we should double check the checksum of the file in insure it really is different
                        // we're using adler here because it's great for quick data integrity checks and
                        // that's all we need really, if it's the same, skip the file
                        match f.checksum {
                            None => {}
                            Some(c) => {
                                if c == checksum {
                                    debug!(
                                        "file at {} hasn't changed since transmission, skipping",
                                        &path.to_str().unwrap()
                                    );
                                    return Ok(());
                                }
                            }
                        }
                    } else {
                        debug!(
                            "file at {} hasn't changed since transmission, skipping",
                            &path.to_str().unwrap()
                        );
                        return Ok(());
                    }
                }
            }
        }
        Err(e) => {
            match e {
                RowNotFound => {
                    // if it's not found, enter it in the db
                    match sqlx::query(
                        "INSERT INTO files(pattern, container_id, created_at) VALUES (?,?,?)",
                    )
                    .bind(&path.to_str().unwrap())
                    .bind(file.container_id.to_string())
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
                    return Ok(());
                }
            }
        }
    };

    // now that we know we don't have this file in the db, or that it needs to be reprocessed - do so
    if plugin.read().await.is_some() {
        let mut timeseries: Option<UnboundedSender<DataSourceMessage>> = None;
        let mut graph: Option<UnboundedSender<DataSourceMessage>> = None;

        match file.timeseries_data_source_id {
            None => {}
            Some(id) => {
                timeseries = data_sources
                    .read()
                    .await
                    .get(&(file.container_id, id))
                    .cloned()
            }
        }

        match file.graph_data_source_id {
            None => {}
            Some(id) => {
                graph = data_sources
                    .read()
                    .await
                    .get(&(file.clone().container_id, id))
                    .cloned()
            }
        }

        match plugin.read().await.as_ref().unwrap().process(
            path.clone(),
            db.clone(),
            timeseries,
            graph,
        ) {
            Ok(_) => {
                // update the file setting its transmitted time
                // capture any errors in the db if the transmission runs into issues
                // yes this is duplicated code, so we can pull it out into a function at some point -
                // I just needed something working now
                match sqlx::query(
                    "UPDATE files SET transmitted_at = ?, checksum = ? WHERE pattern = ? AND container_id = ?",
                )
                    .bind(Utc::now().to_rfc3339())
                    .bind(checksum)
                    .bind(&path.to_str().unwrap())
                    .bind(file.container_id.to_string())
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
            Err(e) => {
                error!(
                    "unable to process the watched file at {}: {}",
                    path.to_str().unwrap(),
                    e
                )
            }
        }
    } else {
        // fall back to the default behavior of simply sending the file to DeepLynx, we do this
        // by sending a message to the relevant data sources with the file's path
        match file.timeseries_data_source_id {
            None => {}
            Some(id) => match data_sources.write().await.get(&(file.container_id, id)) {
                None => {}
                Some(channel) => {
                    let p = path.clone();
                    match channel.send(DataSourceMessage::File((
                        p,
                        file.delete_after_read.unwrap_or(false),
                    ))) {
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

        match file.graph_data_source_id {
            None => {}
            Some(id) => match data_sources
                .write()
                .await
                .get(&(file.clone().container_id, id))
            {
                None => {}
                Some(channel) => {
                    let p = path.clone();
                    match channel.send(DataSourceMessage::File((
                        p,
                        file.delete_after_read.unwrap_or(false),
                    ))) {
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
        // yes this is duplicated code, so we can pull it out into a function at some point -
        // I just needed something working now
        match sqlx::query(
            "UPDATE files SET transmitted_at = ?, checksum = ? WHERE pattern = ? AND container_id = ?",
        )
            .bind(Utc::now().to_rfc3339())
            .bind(checksum)
            .bind(&path.to_str().unwrap())
            .bind(file.container_id.to_string())
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
    Ok(())
}
