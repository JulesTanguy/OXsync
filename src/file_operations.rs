use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use blake3::hash;
use notify::Event;
use tokio::fs;
use tokio::sync::RwLock;
use tracing::{error, info};

use crate::utils::{PathType, Utils};
use crate::PathMetadata;

pub(crate) struct FileOperationsManager {
    file_store: Arc<RwLock<HashMap<PathBuf, PathMetadata>>>,
}

impl FileOperationsManager {
    pub fn new(file_store: Arc<RwLock<HashMap<PathBuf, PathMetadata>>>) -> Self {
        FileOperationsManager { file_store }
    }

    pub async fn copy(&self, event: Event) {
        // "paths" length is always 1 on Windows
        for src_path in event.paths {
            let v_path = Utils::path_to_verbatim(&src_path);

            if Utils::is_in_excluded_paths(&v_path, true).await {
                continue;
            }

            if Utils::args().exclude_temporary_editor_files && v_path.ends_with("~") {
                continue;
            }

            let path_str = v_path
                .strip_prefix(&Utils::args().source_dir)
                .unwrap()
                .to_str()
                .unwrap();

            let relative_path = v_path.strip_prefix(&Utils::args().source_dir).unwrap();
            let (dest_path, dirs) = Utils::get_destination_path_and_dirs(relative_path);

            let file_store_reader = self.file_store.read().await;

            if let Some(path_metadata) = file_store_reader.get(&v_path) {
                let path_metadata_clone = path_metadata.clone();
                drop(file_store_reader);

                match path_metadata_clone.path_type {
                    PathType::Dir => {
                        if Utils::create_dirs(&dest_path, path_str).await.is_ok() {
                            self.write_in_file_store(v_path, PathType::Dir).await;
                        }
                    }
                    PathType::File => {
                        let current_hash_opt = Utils::retry_read_file(&v_path, 3, 1000)
                            .await
                            .map(|file_content| hash(&file_content));

                        if current_hash_opt.is_none() {
                            // TODO : Check if exists before create
                            let _: Result<(), _> = fs::create_dir_all(&dirs).await;

                            if Utils::copy_file(&v_path, &dest_path, path_str)
                                .await
                                .is_ok()
                            {
                                self.write_in_file_store(v_path, PathType::File).await;
                            }
                            continue;
                        }

                        // TODO: Check Why it can panics ???
                        if current_hash_opt.unwrap() == path_metadata_clone.hash.unwrap() {
                            info!("file '{}' not copied : content is identical", path_str);
                        } else {
                            // TODO : Check if exists before create
                            let _: Result<(), _> = fs::create_dir_all(&dirs).await;

                            if Utils::copy_file(&v_path, &dest_path, path_str)
                                .await
                                .is_ok()
                            {
                                self.write_in_file_store(v_path, PathType::File).await;
                            }
                        }
                    }
                }
                continue;
            }

            drop(file_store_reader);

            if v_path.is_file() {
                if Utils::is_in_excluded_paths(&v_path, false).await {
                    continue;
                }

                // TODO : Check if exists before create
                let _: Result<(), _> = fs::create_dir_all(&dirs).await;

                if Utils::copy_file(&v_path, &dest_path, path_str)
                    .await
                    .is_ok()
                {
                    self.write_in_file_store(v_path, PathType::File).await;
                }
                continue;
            }


            if v_path.is_dir()
                && !dest_path.is_dir()
                && !Utils::is_in_excluded_paths(&v_path, false).await
                && Utils::create_dirs(&dest_path, path_str).await.is_ok()
            {
                self.write_in_file_store(v_path, PathType::Dir).await;
            }
        }
    }

    pub async fn remove(&self, event: Event) {
        // "paths" length is always 1 on Windows
        for src_path in event.paths {
            let v_path = Utils::path_to_verbatim(&src_path);

            if Utils::is_in_excluded_paths(&v_path, true).await {
                continue;
            }

            if Utils::args().exclude_temporary_editor_files && v_path.ends_with("~") {
                continue;
            }

            let path_str = v_path
                .strip_prefix(&Utils::args().source_dir)
                .unwrap()
                .to_str()
                .unwrap();

            let relative_path = v_path.strip_prefix(&Utils::args().source_dir).unwrap();
            let dest_path = Utils::get_destination_path(relative_path);

            if !dest_path.exists() {
                return;
            } else if dest_path.is_file() {
                if let Err(err) = fs::remove_file(dest_path).await {
                    Utils::handle_remove_err(err, path_str, PathType::File);
                } else {
                    info!("'{}' deleted", path_str);
                };
                self.file_store.write().await.remove(&v_path);
            } else if dest_path.is_dir() {
                if let Err(err) = fs::remove_dir_all(dest_path).await {
                    Utils::handle_remove_err(err, path_str, PathType::Dir);
                } else {
                    info!("'{}' deleted", path_str);
                };
                self.file_store.write().await.remove(&v_path);
            } else {
                error!("remove error: '{}' is not a file or a directory", path_str);
            }
        }
    }

    async fn write_in_file_store(&self, path: PathBuf, path_type: PathType) {
        let mut current_hash_opt = None;

        if path_type == PathType::File {
            current_hash_opt = Utils::retry_read_file(&path, 3, 1000)
                .await
                .map(|file_content| hash(&file_content));
        }

        if self.file_store.read().await.get(&path).is_none() {
            self.file_store.write().await.insert(
                path.clone(),
                PathMetadata {
                    path_type,
                    hash: current_hash_opt,
                    last_change: SystemTime::now(),
                },
            );
        } else {
            let mut file_store_writer = self.file_store.write().await;
            let path_metadata = file_store_writer.get_mut(&path).unwrap();
            if path_type == PathType::File {
                path_metadata.hash = current_hash_opt;
            }
            path_metadata.last_change = SystemTime::now();
        }
    }
}
