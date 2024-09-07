use crate::utils::PathMetadata;
use ahash::AHasher;
use lru::LruCache;
use std::hash::BuildHasherDefault;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

#[derive(Debug)]
pub struct FileStore {
    content: Arc<RwLock<LruCache<PathBuf, PathMetadata>>>,
}

impl FileStore {
    pub fn new() -> Self {
        let content: LruCache<PathBuf, PathMetadata> = LruCache::with_hasher(
            NonZeroUsize::new(32_768).unwrap(),
            BuildHasherDefault::<AHasher>::default(),
        );

        Self {
            content: Arc::new(RwLock::new(content)),
        }
    }
    // Create
    pub fn create(&self, key: PathBuf, value: PathMetadata) {
        let mut content = self.content.write().unwrap();
        content.put(key, value);
    }

    // Read
    pub fn read(&self, key: &PathBuf) -> Option<PathMetadata> {
        let content = self.content.read().unwrap();
        content.peek(key).cloned()
    }

    // Update
    pub fn update(&self, key: PathBuf, value: PathMetadata) {
        let mut content = self.content.write().unwrap();
        content.put(key, value);
    }

    // Delete
    pub fn delete(&self, key: &PathBuf) {
        let mut content = self.content.write().unwrap();
        content.pop(key);
    }
}
