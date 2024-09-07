use std::path::PathBuf;

use notify::event::{ModifyKind, RenameMode};
use notify::{Event, EventKind};
use tokio::time::Instant;

use crate::file_operations::FileOperationsManager;
use crate::file_store::FileStore;
use crate::utils::Utils;
use crate::warn;

#[derive(Debug)]
pub struct EventHandler {
    file_store: FileStore,
    rename_from: Option<PathBuf>,
}

impl EventHandler {
    pub fn new(file_store: FileStore) -> Self {
        Self {
            file_store,
            rename_from: None,
        }
    }
    pub async fn handle_event(&self, emit_time: Instant, event: Event) {
        match event.kind {
            EventKind::Create(_) => {
                if !Utils::args().no_creation_events {
                    self.create(emit_time, event).await;
                }
            }
            EventKind::Modify(kind) => match kind {
                ModifyKind::Name(rename) => match rename {
                    RenameMode::From => {
                        self.rename(emit_time, event).await;
                    }
                    RenameMode::To => {
                        self.rename(emit_time, event).await;
                    }
                    _ => {
                        self.copy(emit_time, event).await;
                    }
                },
                _ => {
                    self.copy(emit_time, event).await;
                }
            },
            EventKind::Remove(_) => {
                self.remove(emit_time, event).await;
            }
            EventKind::Access(_) => {}
            _ => {
                warn!("Unknown event: {:?}", event);
            }
        }
    }

    async fn create(&self, emit_time: Instant, event: Event) {
        FileOperationsManager::create(&self.file_store, emit_time, event).await;
    }

    async fn rename(&self, emit_time: Instant, event: Event) {
        FileOperationsManager::rename(&self.file_store, emit_time, event, &mut None)
            .await;
    }

    async fn copy(&self, emit_time: Instant, event: Event) {
        FileOperationsManager::copy(&self.file_store, emit_time, event).await;
    }

    async fn remove(&self, emit_time: Instant, event: Event) {
        FileOperationsManager::remove(&self.file_store, emit_time, event).await;
    }
}
