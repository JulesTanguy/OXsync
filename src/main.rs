mod file_operations;
use file_operations::FileOperationsManager;

use std::collections::HashMap;
use std::error::Error;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use futures::{
    channel::mpsc::{channel, Receiver},
    SinkExt, StreamExt,
};
use indicatif::{ProgressBar, ProgressState, ProgressStyle};
use notify::event::{ModifyKind, RenameMode};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use time_macros::format_description;
use tokio::fs;
use tokio::io::AsyncReadExt;
use tokio::sync::{Mutex, RwLock};
use tracing::{error, trace, warn};
use tracing_subscriber::fmt::time;
use tracing_subscriber::EnvFilter;

/// Simple program to sync changes from one dir to an other
#[derive(Parser, Debug)]
#[command(author, version, long_about = None)]
struct Args {
    /// path of the directory to watch changes from
    #[arg(short, long, required(true))]
    source_dir: String,
    /// path of the directory to write changes to
    #[arg(short, long, required(true))]
    target_dir: String,
    /// do not display diff when a text file is modified
    #[arg(long)]
    no_diffs: bool,
    /// display progress when loading files
    #[arg(long)]
    progress: bool,
}

#[derive(Debug, PartialEq)]
enum EntryType {
    File,
    Dir,
}

struct PathEntry {
    entry_type: EntryType,
    path: PathBuf,
    content: Option<Vec<u8>>,
}

impl From<PathEntry> for PathEntryValues {
    fn from(pe: PathEntry) -> Self {
        PathEntryValues {
            entry_type: pe.entry_type,
            content: pe.content,
        }
    }
}

#[derive(Debug)]
pub struct PathEntryValues {
    entry_type: EntryType,
    content: Option<Vec<u8>>,
}

type BoxedError = Box<dyn Error + Send + Sync>;
pub type ConcurrentFileStore = Arc<RwLock<HashMap<PathBuf, PathEntryValues>>>;
const MAX_RWLOCK_READERS: u32 = 2048;

#[tokio::main]
async fn main() -> Result<(), BoxedError> {
    tracing_subscriber::fmt()
        .with_timer(time::LocalTime::new(format_description!(
            "[hour]:[minute]:[second].[subsecond digits:3]"
        )))
        .with_ansi(false)
        .with_env_filter(EnvFilter::from_env("OXSYNC_LOG"))
        .with_target(false)
        .init();

    if let Err(err) = get_all().await {
        error!("{}", err.to_string())
    };
    Ok(())
}

async fn get_all() -> Result<(), BoxedError> {
    let args = Args::parse();

    if !Path::new(&args.source_dir).exists() {
        return Err(format!("source dir : '{}' does not exists", &args.source_dir).into());
    }
    if !Path::new(&args.target_dir).exists() {
        return Err(format!("target dir : '{}' does not exists", &args.source_dir).into());
    }

    let path_list = get_path_list(&args.source_dir);
    let files_number = path_list.len();

    let mut files_store = HashMap::with_capacity(files_number);

    for pe in path_list {
        files_store.insert(pe.path.clone(), pe.into());
    }

    if args.progress {
        let pb = init_progress_bar(files_number).await;

        let files_read = files_store
            .iter_mut()
            .map(|file_store_row| async { read_filepath(file_store_row, Some(pb.clone())).await });

        futures::future::join_all(files_read).await;

        pb.lock().await.finish_and_clear();
    } else {
        let files_read = files_store
            .iter_mut()
            .map(|file_store_row| async { read_filepath(file_store_row, None).await });

        futures::future::join_all(files_read).await;
    }

    println!("{:?}", files_store);

    async_watch(&args.source_dir, &args.target_dir, files_store).await?;

    Ok(())
}

async fn init_progress_bar(files_number: usize) -> Arc<Mutex<ProgressBar>> {
    let pb = Arc::new(Mutex::new(ProgressBar::new(files_number as u64)));
    pb.clone().lock().await.set_style(
        ProgressStyle::with_template(
            "Loading files [{wide_bar:.cyan/blue}] {percent}% ({elapsed})",
        )
        .unwrap()
        .with_key("eta", |state: &ProgressState, w: &mut dyn Write| {
            write!(w, "{:.1}s", state.eta().as_secs_f64()).unwrap()
        })
        .progress_chars("#>-"),
    );
    pb
}

async fn read_filepath(
    file_store_row: (&PathBuf, &mut PathEntryValues),
    pb: Option<Arc<Mutex<ProgressBar>>>,
) -> Result<()> {
    if file_store_row.1.entry_type == EntryType::Dir {
        return Ok(());
    }

    let mut file = fs::OpenOptions::new()
        .read(true)
        .open(file_store_row.0)
        .await?;

    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).await?;
    file_store_row.1.content.replace(buffer);

    trace!(
        "path: {:?}, content: {:?}",
        file_store_row.0,
        file_store_row.1
    );

    if let Some(pb) = pb {
        pb.lock().await.inc(1);
    }
    Ok(())
}

fn get_path_list(root_dir_path: &str) -> Vec<PathEntry> {
    let mut path_list = Vec::new();

    for entry in walkdir::WalkDir::new(root_dir_path)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let pe;
        if entry.path().is_file() {
            pe = PathEntry {
                entry_type: EntryType::File,
                path: entry.into_path(),
                content: None,
            };
        } else if entry.path().is_dir() {
            pe = PathEntry {
                entry_type: EntryType::Dir,
                path: entry.into_path(),
                content: None,
            };
        } else {
            continue;
        }
        path_list.push(pe);
    }

    print!("\r\n");

    path_list
}

fn async_watcher() -> notify::Result<(RecommendedWatcher, Receiver<notify::Result<Event>>)> {
    let (mut tx, rx) = channel(1);

    // Automatically select the best implementation for your platform.
    // You can also access each implementation directly e.g. INotifyWatcher.
    let watcher = RecommendedWatcher::new(
        move |res| {
            futures::executor::block_on(async {
                tx.send(res).await.unwrap();
            })
        },
        Config::default(),
    )?;

    Ok((watcher, rx))
}

async fn async_watch(
    source_dir: &str,
    target_dir: &str,
    files_store: HashMap<PathBuf, PathEntryValues>,
) -> notify::Result<()> {
    let (mut watcher, mut rx) = async_watcher()?;

    // Add a path to be watched. All files and directories at that path and
    // below will be monitored for changes.
    watcher.watch(source_dir.as_ref(), RecursiveMode::Recursive)?;
    println!("Ready - Waiting for changes on {}", source_dir);

    let files_store_arc = Arc::new(RwLock::with_max_readers(files_store, MAX_RWLOCK_READERS));
    while let Some(res) = rx.next().await {
        match res {
            Ok(event) => {
                trace!("{:?}", event);
                let file_store_clone = files_store_arc.clone();
                let handle_event = move || async {
                    event_handler(event, file_store_clone, source_dir, target_dir).await;
                };
                handle_event().await;
            }
            Err(e) => error!("watch error: {:?}", e),
        }
    }

    Ok(())
}

async fn event_handler(
    event: Event,
    file_store_clone: Arc<RwLock<HashMap<PathBuf, PathEntryValues>>>,
    source_dir: &str,
    target_dir: &str,
) {
    let file_ops_manager = FileOperationsManager::new(
        file_store_clone,
        source_dir.to_string(),
        target_dir.to_string(),
    );

    match event.kind {
        EventKind::Create(_) => {
            let _ = file_ops_manager.copy_created(event).await;
        }
        EventKind::Modify(kind) => match kind {
            // TODO: Improve rename handling
            ModifyKind::Name(rename) => match rename {
                RenameMode::From => {
                    let _ = file_ops_manager.remove(event).await;
                }
                RenameMode::To => {
                    let _ = file_ops_manager.copy_created(event).await;
                }
                _ => {
                    let _ = file_ops_manager.copy_modified(event).await;
                }
            },
            _ => {
                let _ = file_ops_manager.copy_modified(event).await;
            }
        },
        EventKind::Remove(_) => {
            let _ = file_ops_manager.remove(event).await;
        }
        EventKind::Access(_) => {}
        _ => {
            warn!("Unknown event: {:?}", event)
        }
    }
}
