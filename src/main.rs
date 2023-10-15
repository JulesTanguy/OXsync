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
use tokio::io::AsyncReadExt;
use tokio::sync::{Mutex, RwLock};
use tracing::{error, warn};
use tracing_subscriber::fmt::time;

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
}

type BoxedError = Box<dyn Error + Send + Sync>;
pub type ConcurrentFileStore = Arc<RwLock<HashMap<PathBuf, Vec<u8>>>>;
const MAX_RWLOCK_READERS: u32 = 2048;

#[tokio::main]
async fn main() -> Result<(), BoxedError> {
    tracing_subscriber::fmt()
        .with_timer(time::LocalTime::new(format_description!(
            "[hour]:[minute]:[second].[subsecond digits:3]"
        )))
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

    print!("{esc}[2J{esc}[1;1H", esc = 27 as char);

    let files_list = get_filepath_list_simple(&args.source_dir);
    let files_number = files_list.len();

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

    let mut files_store = HashMap::with_capacity(files_number);
    for path in files_list {
        files_store.insert(path, Vec::new());
    }

    let files_read = files_store
        .iter_mut()
        .map(|file_store| async { read_file(file_store, pb.clone()).await });

    futures::future::join_all(files_read).await;
    pb.clone().lock().await.finish_and_clear();

    async_watch(&args.source_dir, &args.target_dir, files_store).await?;

    Ok(())
}

async fn read_file(
    file_store: (&PathBuf, &mut Vec<u8>),
    pb: Arc<Mutex<ProgressBar>>,
) -> Result<()> {
    let mut file = tokio::fs::OpenOptions::new()
        .read(true)
        .open(file_store.0)
        .await?;
    file.read_to_end(file_store.1).await?;
    pb.lock().await.inc(1);
    Ok(())
}

fn get_filepath_list_simple(root_dir_path: &str) -> Vec<PathBuf> {
    let mut files_list = Vec::new();
    let mut files_found = 0usize;

    for entry in walkdir::WalkDir::new(root_dir_path)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.path().is_file() {
            files_list.push(entry.into_path());
            files_found += 1;
            print!("\rFiles found : {}", files_found)
        }
    }

    print!("\r\n");

    files_list
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
    files_store: HashMap<PathBuf, Vec<u8>>,
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
                let file_store_clone = files_store_arc.clone();
                let test = move || async {
                    change_event_actions(event, file_store_clone, source_dir, target_dir).await;
                };
                test().await;
            }
            Err(e) => error!("watch error: {:?}", e),
        }
    }

    Ok(())
}

async fn change_event_actions(
    event: Event,
    file_store_clone: ConcurrentFileStore,
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
            let _ = file_ops_manager.copy(event).await;
        }
        EventKind::Modify(kind) => match kind {
            ModifyKind::Name(rename) => match rename {
                RenameMode::From => {
                    let _ = file_ops_manager.remove(event).await;
                }
                RenameMode::To => {
                    let _ = file_ops_manager.copy(event).await;
                }
                _ => {
                    let _ = file_ops_manager.copy(event).await;
                }
            },
            _ => {
                let _ = file_ops_manager.copy(event).await;
            }
        },
        EventKind::Remove(_) => {
            let _ = file_ops_manager.remove(event).await;
        }
        EventKind::Access(_) => {}
        _ => {
            warn!("Unknown action")
        }
    }
}
