use clap::Parser;
use notify::{RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::OnceCell;
use tokio::time::Instant;
use tokio_stream::StreamExt;

use crate::event_handler::EventHandler;
use crate::file_store::FileStore;
use start::Start;
use utils::PathMetadata;
use utils::Utils;

mod event_handler;
mod file_operations;
mod file_store;
mod macros;
mod start;
mod utils;

/// Sync changes from a directory to another
#[derive(Parser, Debug)]
#[command(author, version, long_about = None)]
pub struct Args {
    /// Path of the directory to watch changes from
    #[arg(index(1), required(true))]
    source_dir: PathBuf,
    /// Path of the directory to write changes to
    #[arg(index(2), required(true))]
    target_dir: PathBuf,
    /// Exclude file or dir from the <SOURCE_DIR>, can be used multiple times
    #[arg(long, short)]
    exclude: Vec<PathBuf>,
    /// Exclude files with names ending by a tilde `~`
    #[arg(long, visible_alias("no-tmp"))]
    no_temporary_editor_files: bool,
    /// Ignore creation events
    #[arg(long, visible_alias("no-create"))]
    no_creation_events: bool,
    /// Exclude `.git`, `.idea` dirs + enables `no-temporary-editor-files`, `no-creation-events` options
    #[arg(long, visible_alias("ide"))]
    ide_mode: bool,
    /// Display the time spent copying the file
    #[arg(long, visible_alias("stats"))]
    statistics: bool,
    /// Set the log level to trace
    #[arg(long)]
    trace: bool,
}

pub static LOG_TRACE: OnceCell<bool> = OnceCell::const_new();

#[tokio::main]
async fn main() {
    Start::parse_args().await;
    if let Err(e) = init_event_loop().await {
        err!("{}", e);
    };
}

async fn init_event_loop() -> notify::Result<()> {
    let (mut watcher, mut rx) = Start::fs_watcher()?;

    // Add a path to be watched. All files and directories at that path and
    // below will be monitored for changes.
    watcher.watch(&Utils::args().source_dir, RecursiveMode::Recursive)?;

    let file_store = FileStore::new();

    info!(
        "Ready - Waiting for changes on '{}'",
        Utils::fmt_path(&Utils::args().source_dir)
    );
    
    let eh = Arc::new(EventHandler::new(file_store));

    while let Some(res) = rx.next().await {
        match res {
            Ok(event) => {
                let emit_time = Instant::now();
                trace!("{:?}", event);
                let eee = Arc::clone(&eh);
                tokio::spawn(async move { 
                    eee.handle_event(emit_time, event).await;
                } );
            }
            Err(e) => err!("watch error: {:?}", e),
        }
    }

    Ok(())
}
