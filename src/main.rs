use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::{Parser, ValueEnum};
use notify::event::{ModifyKind, RenameMode};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use time_macros::format_description;
use tokio::fs::canonicalize;
use tokio::sync::{
    mpsc::{channel, Receiver},
    OnceCell, RwLock,
};
use tokio::task::JoinHandle;
use tracing::{error, trace, warn};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::time;
use walkdir::WalkDir;

use file_operations::FileOperationsManager;

mod file_operations;

/// Utility to sync changes from one dir to an other
#[derive(Parser, Debug)]
#[command(author, version, long_about = None)]
struct Args {
    /// Path of the directory to watch changes from
    #[arg(index(1), required(true))]
    source_dir: String,
    /// Path of the directory to write changes to
    #[arg(index(2), required(true))]
    target_dir: String,
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

#[derive(Debug, PartialEq)]
enum EntryType {
    File,
    Dir,
}

struct PathEntry {
    entry_type: EntryType,
    path: PathBuf,
}

impl From<PathEntry> for PathEntryType {
    fn from(pe: PathEntry) -> Self {
        PathEntryType {
            entry_type: pe.entry_type,
        }
    }
}

#[derive(Debug)]
pub struct PathEntryType {
    entry_type: EntryType,
}

#[derive(Debug)]
struct Dirs {
    source_dir: PathBuf,
    target_dir: PathBuf,
}

pub type ConcurrentFileStore = Arc<RwLock<HashMap<PathBuf, PathEntryType>>>;
pub type OngoingEventsLog = Arc<RwLock<HashMap<PathBuf, Arc<JoinHandle<()>>>>>;
const MAX_RWLOCK_READERS: u32 = (u32::MAX >> 3) - 1;

static SOURCE_AND_TARGET_DIR: OnceCell<Dirs> = OnceCell::const_new();

#[tokio::main]
async fn main() {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_timer(time::LocalTime::new(format_description!(
            "[hour]:[minute]:[second].[subsecond digits:3]"
        )))
        .with_ansi(false)
        .with_max_level(args.log_level)
        .with_target(false)
        .init();

    let mut path_does_not_exists = false;

    if !Path::new(&args.source_dir).exists() {
        error!("source dir : '{}' does not exists", &args.source_dir);
        path_does_not_exists = true
    }
    if !Path::new(&args.target_dir).exists() {
        error!("target dir : '{}' does not exists", &args.source_dir);
        path_does_not_exists = true
    }

    if path_does_not_exists {
        return;
    }

    SOURCE_AND_TARGET_DIR
        .set(Dirs {
            source_dir: canonicalize(Path::new(&args.source_dir))
                .await
                .expect("Impossible to convert the <SOURCE_DIR> to a valid path"),
            target_dir: canonicalize(Path::new(&args.target_dir))
                .await
                .expect("Impossible to convert the <TARGET_DIR> to a valid path"),
        })
        .unwrap();

    let path_list = get_path_list(&args.source_dir).await;
    let files_number = path_list.len();

    let mut files_store = HashMap::with_capacity(files_number);

    for pe in path_list {
        files_store.insert(pe.path.clone(), pe.into());
    }

    if let Err(e) = init_event_loop(files_store).await {
        error!("{}", e);
    };
}

async fn get_path_list(root_dir_path: &str) -> Vec<PathEntry> {
    let mut path_list = Vec::new();

    for entry in WalkDir::new(root_dir_path)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let pe;
        if entry.path().is_file() {
            pe = PathEntry {
                entry_type: EntryType::File,
                path: canonicalize(entry.into_path())
                    .await
                    .expect("Impossible to convert to a valid path"),
            };
        } else if entry.path().is_dir() {
            pe = PathEntry {
                entry_type: EntryType::Dir,
                path: canonicalize(entry.into_path())
                    .await
                    .expect("Impossible to convert to a valid path"),
            };
        } else {
            continue;
        }
        path_list.push(pe);
    }

    path_list
}

fn async_watcher() -> notify::Result<(RecommendedWatcher, Receiver<notify::Result<Event>>)> {
    let (tx, rx) = channel(1);

    // Automatically select the best implementation for your platform.
    // You can also access each implementation directly e.g. INotifyWatcher.
    let watcher =
        RecommendedWatcher::new(move |res| tx.blocking_send(res).unwrap(), Config::default())?;

    Ok((watcher, rx))
}

async fn init_event_loop(files_store: HashMap<PathBuf, PathEntryType>) -> notify::Result<()> {
    let (mut watcher, mut rx) = async_watcher()?;

    // Add a path to be watched. All files and directories at that path and
    // below will be monitored for changes.
    watcher.watch(
        &SOURCE_AND_TARGET_DIR.get().unwrap().source_dir,
        RecursiveMode::Recursive,
    )?;

    let files_store_arc = Arc::new(RwLock::with_max_readers(files_store, MAX_RWLOCK_READERS));
    let ongoing_events = Arc::new(RwLock::with_max_readers(HashMap::new(), MAX_RWLOCK_READERS));

    if let Some(path_str) = SOURCE_AND_TARGET_DIR.get().unwrap().source_dir.to_str() {
        // Check if the path starts with the prefix and remove the first three characters
        if let Some(stripped) = path_str.strip_prefix(r"\\?\") {
            println!("Ready - Waiting for changes on '{}'", stripped);
        } else {
            println!("Ready - Waiting for changes on '{}'", path_str);
        };
    } else {
        panic!("Path contains invalid Unicode");
    }

    while let Some(res) = rx.recv().await {
        match res {
            Ok(event) => {
                trace!("{:?}", event);

                let file_store = files_store_arc.clone();
                let ongoing_events_clone = ongoing_events.clone();

                // TODO: paths is cloned a lot
                let paths = event.paths.clone();

                let join_handle = Arc::new(tokio::spawn(handle_event(
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

async fn handle_event(
    event: Event,
    file_store_clone: Arc<RwLock<HashMap<PathBuf, PathEntryType>>>,
    ongoing_events: OngoingEventsLog,
) {
    let file_ops_manager = FileOperationsManager::new(file_store_clone);

    let paths = event.paths.clone();

    match event.kind {
        EventKind::Create(_) => {
            //file_ops_manager.copy(event).await;
        }
        EventKind::Modify(kind) => match kind {
            // TODO: Improve rename handling
            ModifyKind::Name(rename) => match rename {
                RenameMode::From => {
                    file_ops_manager.remove(event).await;
                }
                RenameMode::To => {
                    file_ops_manager.copy(event).await;
                }
                _ => {
                    file_ops_manager.copy(event).await;
                }
            },
            _ => {
                file_ops_manager.copy(event).await;
            }
        },
        EventKind::Remove(_) => {
            file_ops_manager.remove(event).await;
        }
        EventKind::Access(_) => {}
        _ => {
            warn!("Unknown event: {:?}", event)
        }
    }

    for path in paths {
        ongoing_events.write().await.remove(&path);
    }
}
