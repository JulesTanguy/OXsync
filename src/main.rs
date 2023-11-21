use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use clap::{Parser, ValueEnum};
use notify::{RecursiveMode, Watcher};
use tokio::sync::RwLock;
use tracing::{error, info, trace};
use tracing_subscriber::filter::LevelFilter;

use utils::PathMetadata;
use utils::Utils;

mod file_operations;
mod utils;

/// Utility to sync changes from a directory to an other
#[derive(Parser, Debug)]
#[command(author, version, long_about = None)]
pub struct Args {
    /// Path of the directory to watch changes from
    #[arg(index(1), required(true))]
    source_dir: PathBuf,
    /// Path of the directory to write changes to
    #[arg(index(2), required(true))]
    target_dir: PathBuf,
    /// Exclude filenames ending with a tilde `~`
    #[arg(long, visible_alias("exclude-tmp"))]
    exclude_temporary_editor_files: bool,
    /// Exclude files or dirs
    #[arg(long, short)]
    exclude: Option<String>,
    // TODO: remove
    /// Log level
    #[arg(value_enum, long, default_value_t = LogLevel::Info)]
    log_level: LogLevel,
}

#[derive(Clone, Debug, ValueEnum)]
enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
    Off,
}

impl From<LogLevel> for LevelFilter {
    fn from(log_level: LogLevel) -> Self {
        match log_level {
            LogLevel::Error => LevelFilter::ERROR,
            LogLevel::Warn => LevelFilter::WARN,
            LogLevel::Info => LevelFilter::INFO,
            LogLevel::Debug => LevelFilter::DEBUG,
            LogLevel::Trace => LevelFilter::TRACE,
            LogLevel::Off => LevelFilter::OFF,
        }
    }
}

#[cfg(not(windows))]
compile_error!("non-windows targets aren't supported on this version");

#[tokio::main]
async fn main() {
    Utils::parse_args().await;
    Utils::init_tracing();

    if let Err(e) = init_event_loop().await {
        error!("{}", e);
    };
}

async fn init_event_loop() -> notify::Result<()> {
    let (mut watcher, mut rx) = Utils::fs_watcher()?;

    // Add a path to be watched. All files and directories at that path and
    // below will be monitored for changes.
    watcher.watch(&Utils::args().source_dir, RecursiveMode::Recursive)?;

    let files_store_arc = Arc::new(RwLock::with_max_readers(
        HashMap::new(),
        Utils::MAX_RWLOCK_READERS,
    ));
    let ongoing_events = Arc::new(RwLock::with_max_readers(
        HashMap::new(),
        Utils::MAX_RWLOCK_READERS,
    ));

    info!(
        "Ready - Waiting for changes on '{}'",
        Utils::fmt_path(&Utils::args().source_dir)
    );

    while let Some(res) = rx.recv().await {
        match res {
            Ok(event) => {
                trace!("{:?}", event);

                let file_store = files_store_arc.clone();
                let ongoing_events_clone = ongoing_events.clone();

                // TODO: paths is cloned a lot
                let paths = event.paths.clone();

                let join_handle = Arc::new(tokio::spawn(Utils::handle_event(
                    event,
                    file_store,
                    ongoing_events_clone,
                )));
                for path in paths {
                    let local_join_handle = join_handle.clone();
                    ongoing_events.write().await.insert(path, local_join_handle);
                }
            }
            Err(e) => error!("watch error: {:?}", e),
        }
    }

    Ok(())
}
