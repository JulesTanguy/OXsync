use notify::Event;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{error, info};

use crate::ConcurrentFileStore;

pub(crate) struct FileOperationsManager {
    file_store: ConcurrentFileStore,
    source_dir: String,
    target_dir: String,
}

impl FileOperationsManager {
    pub fn new(file_store: ConcurrentFileStore, source_dir: String, target_dir: String) -> Self {
        FileOperationsManager {
            file_store,
            source_dir,
            target_dir,
        }
    }
    pub async fn copy_created(&self, event: Event) {
        self.copy(event).await;
    }
    pub async fn copy_modified(&self, event: Event) {
        self.copy(event).await;
    }

    async fn copy(&self, event: Event) {
        for src_path in event.paths {
            if self.is_dotgit(src_path.clone()) {
                continue;
            }

            let (dest_path, dirs) = self.get_destination_path_and_dirs(&src_path);

            let path_str = src_path
                .strip_prefix(&self.source_dir)
                .unwrap()
                .to_str()
                .unwrap();

            if src_path.is_dir() {
                if !dest_path.exists() {
                    let _: Result<(), _> = fs::create_dir_all(&dest_path).await;
                    info!("dir '{}' copied", path_str);
                }
                continue;
            }

            if dest_path.is_dir() {
                continue;
            }

            if src_path.is_file() {
                let file_store_reader = self.file_store.read().await;
                let file_content_opt = file_store_reader.get(&*src_path);

                if let Some(path_entry_values) = file_content_opt {
                    if let Some(content) = &path_entry_values.content {
                        if let Ok(current_content) = fs::read(&src_path).await {
                            let owned_content: Vec<u8> =
                                Vec::from_iter(content).into_iter().copied().collect();
                            drop(file_store_reader);

                            if owned_content == current_content {
                                info!("File identical");
                                continue;
                            } else {
                                let mut file_store_writer =
                                    self.file_store.clone().write_owned().await;
                                let stored_content =
                                    &mut file_store_writer.get_mut(&src_path).unwrap().content;
                                stored_content.replace(current_content.clone());
                                drop(file_store_writer);

                                let _: Result<(), _> = fs::create_dir_all(&dirs).await;
                                if let Err(err) = fs::copy(&src_path, &dest_path).await {
                                    error!(
                                        "failed to copy '{}', error: {}",
                                        path_str,
                                        err.to_string()
                                    );
                                    continue;
                                };
                                info!("file '{}' copied", path_str);
                                continue;
                            }
                        }
                    }
                }

                let _: Result<(), _> = fs::create_dir_all(&dirs).await;
                if let Err(err) = fs::copy(&src_path, &dest_path).await {
                    error!("failed to copy '{}', error: {}", path_str, err.to_string());
                    continue;
                };
                info!("file '{}' copied", path_str)
            }
        }
    }

    pub async fn remove(&self, event: Event) {
        for path in event.clone().paths {
            if self.is_dotgit(path.clone()) {
                continue;
            }

            let dest_path = self.get_destination_path(&path);

            let path_str = path
                .strip_prefix(&self.source_dir)
                .unwrap()
                .to_str()
                .unwrap();

            if dest_path.is_file() {
                if let Err(err) = fs::remove_file(dest_path).await {
                    error!(
                        "failed to remove file '{}', error: {}",
                        path_str,
                        err.to_string()
                    );
                    return;
                };
            } else if dest_path.is_dir() {
                if let Err(err) = fs::remove_dir_all(dest_path).await {
                    error!(
                        "failed to remove dir '{}', error: {}",
                        path_str,
                        err.to_string()
                    );
                    return;
                };
            } else if !dest_path.exists() {
                return;
            } else {
                error!("remove error: '{}' is not a file or a directory", path_str);
                return;
            }

            info!("'{}' deleted", path_str)
        }
    }

    fn get_destination_path_and_dirs(&self, path: &Path) -> (PathBuf, PathBuf) {
        let dest_path = self.get_destination_path(path);
        let dirs = dest_path.parent().unwrap().to_path_buf();

        (dest_path, dirs)
    }

    fn get_destination_path(&self, path: &Path) -> PathBuf {
        let path_stripped = path.strip_prefix(&self.source_dir).unwrap();
        let dest_path = Path::new(&self.target_dir).join(path_stripped);

        dest_path
    }

    fn is_dotgit(&self, path: PathBuf) -> bool {
        let path_stripped = path.strip_prefix(&self.source_dir).unwrap();
        if path_stripped.starts_with(".git") {
            return true;
        }
        false
    }
}
