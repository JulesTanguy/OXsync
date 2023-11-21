use std::collections::HashMap;
use std::fmt::Debug;
use std::path::{Component, Path, PathBuf, Prefix};
use std::process::abort;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use async_recursion::async_recursion;
use blake3::Hash;
use clap::Parser;
use glob::glob;
use notify::event::{ModifyKind, RenameMode};
use notify::{Config, Event, EventKind, RecommendedWatcher, Watcher};
use regex::Regex;
use time_macros::format_description;
use tokio::fs;
use tokio::fs::canonicalize;
use tokio::sync::mpsc::{channel, Receiver};
use tokio::sync::{OnceCell, RwLock};
use tokio::task::JoinHandle;
use tokio::time::sleep;
use tracing::{error, info, warn};
use tracing_subscriber::fmt::time::LocalTime;

use crate::file_operations::FileOperationsManager;
use crate::Args;

pub(crate) struct Utils;

static ARGS: OnceCell<Args> = OnceCell::const_new();
static EXCLUDED_PATHS: OnceCell<Arc<RwLock<Vec<PathBuf>>>> = OnceCell::const_new();
static EXCLUDED_PATHS_PATTERNS: OnceCell<String> = OnceCell::const_new();

#[derive(Debug, PartialEq, Clone)]
pub enum PathType {
    File,
    Dir,
}

#[derive(Debug, Clone)]
pub struct PathMetadata {
    pub path_type: PathType,
    pub hash: Option<Hash>,
    pub last_change: SystemTime,
}

impl Utils {
    pub const MAX_RWLOCK_READERS: u32 = (u32::MAX >> 3) - 1;
    pub fn fmt_path(path: &Path) -> String {
        if let Some(path_str) = path.to_str() {
            // Check if the path starts with the prefix and remove the first three characters
            return if let Some(stripped) = path_str.strip_prefix(r"\\?\") {
                stripped.to_string()
            } else {
                path_str.to_string()
            };
        } else {
            error!("Path contains invalid Unicode");
            abort()
        }
    }

    pub fn args<'a>() -> &'a Args {
        ARGS.get().unwrap()
    }

    // TODO : Improve performance
    pub async fn is_in_excluded_paths(path: &PathBuf, use_cached: bool) -> bool {
        if Self::args().exclude.is_none() {
            return false;
        }
        let v_path = Self::path_to_verbatim(path);
        if use_cached {
            let excluded_paths_reader = EXCLUDED_PATHS.get().unwrap().read().await;
            println!("{:?}", excluded_paths_reader);
            println!("{:?}", v_path);
            if excluded_paths_reader.contains(&v_path) {
                return true;
            }

        }

        let glob_result_iter =
            glob(EXCLUDED_PATHS_PATTERNS.get().unwrap()).unwrap_or_else(|error| {
                eprintln!("exclude pattern error : {}", error);
                abort()
            });

        let mut excluded_paths_writer = EXCLUDED_PATHS.get().unwrap().write().await;
        for glob_result in glob_result_iter {
            if let Ok(glob_path) = glob_result {
                println!("{:?}", glob_path);
                if &glob_path == path {
                    return true;
                }
                if !excluded_paths_writer.contains(&glob_path) {
                    excluded_paths_writer.push(glob_path);
                }
            } else {
                warn!("exclude pattern error: {}", glob_result.unwrap_err());
            }
        }

        false
    }

    pub async fn parse_args() {
        let mut args = Args::parse();

        if !Path::new(&args.source_dir).exists() {
            eprintln!(
                "source dir : '{}' does not exists",
                Self::fmt_path(&args.source_dir)
            );
            abort()
        }

        if !Path::new(&args.target_dir).exists() {
            eprintln!(
                "target dir : '{}' does not exists",
                Self::fmt_path(&args.target_dir)
            );
            abort()
        }

        args.source_dir = canonicalize(Path::new(&args.source_dir))
            .await
            .unwrap_or_else(|_| {
                eprintln!(
                    "impossible to convert source dir '{}' to a valid path",
                    Self::fmt_path(&args.source_dir)
                );
                abort()
            });

        args.target_dir = canonicalize(Path::new(&args.target_dir))
            .await
            .unwrap_or_else(|_| {
                eprintln!(
                    "impossible to convert target dir '{}' to a valid path",
                    Self::fmt_path(&args.target_dir)
                );
                abort()
            });



        let exclude_pattern = args.exclude.clone();

        if let Some(pattern) = exclude_pattern {
            // TODO: plz no regex
            let regex = r"[\*\?\(\[\]\)]";
            let re = Regex::new(regex).unwrap();

            let mut excluded_paths_values = Vec::new();
            for parsed_pattern in pattern.split(',') {
                if re.is_match(parsed_pattern) {
                    for glob_result in glob(&pattern).unwrap_or_else(|error| {
                        eprintln!("exclude pattern error : {}", error);
                        abort()
                    }) {
                        if let Ok(path) = glob_result {
                            let path_tmp = canonicalize(path).await.unwrap();
                            excluded_paths_values.push(Self::path_to_verbatim(&path_tmp));
                        } else {
                            info!("exclude pattern error: {}", glob_result.unwrap_err());
                        }
                    }
                } else {
                    let path = Path::new(parsed_pattern);
                    if path.exists() {
                        let path_tmp = canonicalize(path).await.unwrap();
                        excluded_paths_values.push(Self::path_to_verbatim(&path_tmp));
                    }
                }
            }

            //println!("{:?}", excluded_paths_values);
            let excluded_paths = Arc::new(RwLock::with_max_readers(
                excluded_paths_values,
                Self::MAX_RWLOCK_READERS,
            ));

            EXCLUDED_PATHS_PATTERNS.set(pattern).unwrap();
            EXCLUDED_PATHS.set(excluded_paths).unwrap();
        }

        ARGS.set(args).unwrap();
    }

    pub fn fs_watcher() -> notify::Result<(RecommendedWatcher, Receiver<notify::Result<Event>>)> {
        let (tx, rx) = channel(1);

        // Automatically select the best implementation for your platform.
        // You can also access each implementation directly e.g. INotifyWatcher.
        let watcher =
            RecommendedWatcher::new(move |res| tx.blocking_send(res).unwrap(), Config::default())?;

        Ok((watcher, rx))
    }

    pub fn init_tracing() {
        tracing_subscriber::fmt()
            .with_timer(LocalTime::new(format_description!(
                "[hour]:[minute]:[second].[subsecond digits:3]"
            )))
            .with_ansi(false)
            .with_max_level(Self::args().log_level.clone())
            .with_target(false)
            .init();
    }

    pub fn handle_remove_err(err: std::io::Error, path_str: &str, entry_type: PathType) {
        let entry_type_str = match entry_type {
            PathType::File => "file",
            PathType::Dir => "dir",
        };

        if let Some(os_error_code) = err.raw_os_error() {
            if os_error_code == 2 || os_error_code == 3 {
                error!(
                    "failed to remove {} '{}', error: {}",
                    entry_type_str,
                    path_str,
                    err.to_string()
                );
            };
        } else {
            error!(
                "failed to remove {} '{}', error: {}",
                entry_type_str,
                path_str,
                err.to_string()
            );
        }
    }

    /// See https://github.com/dherman/verbatim
    pub fn path_to_verbatim(path: &Path) -> PathBuf {
        let mut components = path.components();
        match components.next() {
            Some(Component::Prefix(prefix)) => {
                let new_prefix = match prefix.kind() {
                    Prefix::Disk(letter) => {
                        let new_prefix_string = format!(r"\\?\{}:\", String::from_utf8_lossy(&[letter]));
                        let new_prefix = Path::new(&new_prefix_string).to_path_buf();
                        new_prefix
                    },
                    _ => {
                        return path.to_path_buf()
                    }
                };
                new_prefix.join(components)
            }
            Some(other) => {
                Path::new(r"\\?\").join(Path::new(&other)).join(components)
            }
            _ => {
                path.to_path_buf()
            }
        }
    }

    pub async fn copy_file(src_path: &Path, dest_path: &Path, path_str: &str) -> Result<(), ()> {
        if let Err(err) = fs::copy(src_path, dest_path).await {
            error!("failed to copy '{}', error: {}", path_str, err.to_string());
            Err(())
        } else {
            info!("file '{}' copied", path_str);
            Ok(())
        }
    }

    pub async fn create_dirs(dest_path: &Path, path_str: &str) -> Result<(), ()> {
        if let Err(err) = fs::create_dir_all(&dest_path).await {
            error!("failed to copy '{}', error: {}", path_str, err.to_string());
            Err(())
        } else {
            info!("dir '{}' created", path_str);
            Ok(())
        }
    }

    pub fn get_destination_path_and_dirs(relative_path: &Path) -> (PathBuf, PathBuf) {
        let dest_path = Self::get_destination_path(relative_path);
        let dirs = dest_path.parent().unwrap().to_path_buf();

        (dest_path, dirs)
    }

    pub fn get_destination_path(relative_path: &Path) -> PathBuf {
        Path::new(&Self::args().target_dir).join(relative_path)
    }

    pub async fn retry_read_file(
        path: &Path,
        retry_count: usize,
        wait_millis: usize,
    ) -> Option<Vec<u8>> {
        Self::retry_read_file_recursive(path, retry_count, wait_millis, 0).await
    }

    #[async_recursion]
    async fn retry_read_file_recursive(
        path: &Path,
        retry_count: usize,
        wait_millis: usize,
        i: usize,
    ) -> Option<Vec<u8>> {
        if i > retry_count {
            return None;
        }

        let read_result = fs::read(&path).await;

        return if let Ok(file_contents) = read_result {
            Some(file_contents)
        } else if let Some(os_error_code) = read_result.unwrap_err().raw_os_error() {
            if os_error_code == 32 {
                sleep(Duration::from_millis(wait_millis as u64)).await;
                Self::retry_read_file_recursive(path, retry_count, wait_millis, i + 1).await
            } else {
                None
            }
        } else {
            None
        };
    }

    pub async fn handle_event(
        event: Event,
        file_store_clone: Arc<RwLock<HashMap<PathBuf, PathMetadata>>>,
        ongoing_events: Arc<RwLock<HashMap<PathBuf, Arc<JoinHandle<()>>>>>,
    ) {
        let file_ops_manager = FileOperationsManager::new(file_store_clone);

        let paths = event.paths.clone();

        match event.kind {
            EventKind::Create(_) => {
                // TODO: Improve create handling
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
}
