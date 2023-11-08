use std::io::Error;
use std::path::{Path, PathBuf};

use notify::Event;
use tokio::fs;
use tracing::{error, info};

use crate::{ConcurrentFileStore, EntryType, PathEntryType, SOURCE_AND_TARGET_DIR};

pub(crate) struct FileOperationsManager {
    file_store: ConcurrentFileStore,
}

impl FileOperationsManager {
    pub fn new(file_store: ConcurrentFileStore) -> Self {
        FileOperationsManager { file_store }
    }

    pub async fn copy(&self, event: Event) {
        // "paths" length is always 1 on Windows
        for src_path in event.paths {
            if is_dotgit(&src_path) {
                continue;
            }

            let path_str = src_path
                .strip_prefix(&SOURCE_AND_TARGET_DIR.get().unwrap().source_dir)
                .unwrap()
                .to_str()
                .unwrap();

            if path_str.ends_with('~') {
                continue;
            }

            let relative_path = src_path
                .strip_prefix(&SOURCE_AND_TARGET_DIR.get().unwrap().source_dir)
                .unwrap();
            let (dest_path, dirs) = get_destination_path_and_dirs(relative_path);

            if let Some(path_entry_values) = self.file_store.read().await.get(&src_path) {
                if path_entry_values.entry_type == EntryType::Dir {
                    create_dirs(&dest_path, path_str).await;
                }
                if path_entry_values.entry_type == EntryType::File {
                    copy_file(&src_path, &dest_path, path_str).await;
                }

                continue;
            }

            if src_path.is_dir() {
                if !dest_path.exists() {
                    create_dirs(&dest_path, path_str).await;
                }
                continue;
            }

            if dest_path.is_dir() {
                continue;
            }

            if src_path.is_file() {
                if self.file_store.read().await.get(&src_path).is_none() {
                    self.file_store.clone().write().await.insert(
                        src_path.clone(),
                        PathEntryType {
                            entry_type: EntryType::File,
                        },
                    );
                }

                let _: Result<(), _> = fs::create_dir_all(&dirs).await;
                copy_file(&src_path, &dest_path, path_str).await;
            }
        }
    }

    pub async fn remove(&self, event: Event) {
        // "paths" length is always 1 on Windows
        for src_path in event.paths {
            if is_dotgit(&src_path) {
                continue;
            }

            let path_str = src_path
                .strip_prefix(&SOURCE_AND_TARGET_DIR.get().unwrap().source_dir)
                .unwrap()
                .to_str()
                .unwrap();

            if path_str.ends_with('~') {
                continue;
            }

            let relative_path = src_path
                .strip_prefix(&SOURCE_AND_TARGET_DIR.get().unwrap().source_dir)
                .unwrap();
            let dest_path = get_destination_path(relative_path);

            if !dest_path.exists() {
                return;
            } else if dest_path.is_file() {
                if let Err(err) = fs::remove_file(dest_path).await {
                    self.handle_remove_err(err, path_str, EntryType::File);
                } else {
                    info!("'{}' deleted", path_str);
                };
                self.file_store.write().await.remove(&src_path);
            } else if dest_path.is_dir() {
                if let Err(err) = fs::remove_dir_all(dest_path).await {
                    self.handle_remove_err(err, path_str, EntryType::Dir);
                } else {
                    info!("'{}' deleted", path_str);
                };
                self.file_store.write().await.remove(&src_path);
            } else {
                error!("remove error: '{}' is not a file or a directory", path_str);
            }
        }
    }

    fn handle_remove_err(&self, err: Error, path_str: &str, entry_type: EntryType) {
        let entry_type_str = match entry_type {
            EntryType::File => "file",
            EntryType::Dir => "dir",
        };
        if err.raw_os_error().is_some() {
            let os_err = err.raw_os_error().unwrap();
            if os_err == 2 || os_err == 3 {
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
}

async fn copy_file(src_path: &Path, dest_path: &Path, path_str: &str) {
    if let Err(err) = fs::copy(src_path, dest_path).await {
        error!("failed to copy '{}', error: {}", path_str, err.to_string());
    } else {
        info!("file '{}' copied", path_str)
    };
}

async fn create_dirs(dest_path: &Path, path_str: &str) {
    if let Err(err) = fs::create_dir_all(&dest_path).await {
        error!("failed to copy '{}', error: {}", path_str, err.to_string());
    } else {
        info!("dir '{}' copied", path_str)
    };
}

fn get_destination_path_and_dirs(relative_path: &Path) -> (PathBuf, PathBuf) {
    let dest_path = get_destination_path(relative_path);
    let dirs = dest_path.parent().unwrap().to_path_buf();

    (dest_path, dirs)
}

fn get_destination_path(relative_path: &Path) -> PathBuf {
    Path::new(&SOURCE_AND_TARGET_DIR.get().unwrap().target_dir).join(relative_path)
}

fn is_dotgit(path: &Path) -> bool {
    let relative_path = path
        .strip_prefix(&SOURCE_AND_TARGET_DIR.get().unwrap().source_dir)
        .unwrap();
    relative_path.starts_with(".git")
}
