use notify::Event;
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::{error, info};

use crate::ConcurrentFileStore;

pub(crate) struct FileOperationsManager {
    #[allow(dead_code)]
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

    pub async fn copy(&self, event: Event) {
        for path in event.paths {
            if self.is_dotgit(path.clone()) {
                continue;
            }

            let (dest_path, dirs) = self.get_destination_path_and_dirs(&path);

            let path_str = path
                .strip_prefix(&self.source_dir)
                .unwrap()
                .to_str()
                .unwrap();

            if path.is_dir() {
                let _: Result<(), _> = fs::create_dir_all(&dest_path).await;
                info!("'{}' copied", path_str);
                continue;
            }

            let _: Result<(), _> = fs::create_dir_all(&dirs).await;

            if let Err(err) = fs::copy(&path, &dest_path).await {
                error!("failed to copy '{}', error: {}", path_str, err.to_string());
                continue;
            };
            info!("'{}' copied", path_str)
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
