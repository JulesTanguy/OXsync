use std::collections::HashMap;
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
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::io::AsyncReadExt;
use tokio::sync::Mutex;

/// Simple program to sync changes from one dir to an other
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Name of the directory to watch changes from
    #[arg(short, long, required(true))]
    source_dir: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    get_all().await?;
    Ok(())
}

async fn get_all() -> Result<()> {
    let args = Args::parse();

    print!("{esc}[2J{esc}[1;1H", esc = 27 as char);

    let files_list = get_filespath_list_simple(&args.source_dir);
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
    println!("Ready - Waiting for changes on {}", &args.source_dir);

    async_watch(&args.source_dir).await?;

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

fn get_filespath_list_simple(root_dir_path: &str) -> Vec<PathBuf> {
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

async fn async_watch<P: AsRef<Path>>(path: P) -> notify::Result<()> {
    let (mut watcher, mut rx) = async_watcher()?;

    // Add a path to be watched. All files and directories at that path and
    // below will be monitored for changes.
    watcher.watch(path.as_ref(), RecursiveMode::Recursive)?;

    while let Some(res) = rx.next().await {
        match res {
            Ok(event) => println!("changed: {:?}", event),
            Err(e) => println!("watch error: {:?}", e),
        }
    }

    Ok(())
}
