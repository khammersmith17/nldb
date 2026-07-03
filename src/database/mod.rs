use crate::compaction::{CompactionSignal, sstable_background};
use crate::config::NldbConfig;
use crate::error::MemtableError;
use crate::memtable::{
    Memtable,
    immutable::ImmutableMemtable,
    inner::{Blob, flush_memtable},
};
use crate::sstable::SSTableCache;
use dash_cache::DashCache;
use std::num::NonZeroUsize;
use std::sync::{Arc, atomic::AtomicBool};
use tokio::sync::mpsc::{self, Sender};

// TODO: Check poisoned flag every so often.

pub struct NldbInner {
    memtable: Memtable,
    sstable_cache: SSTableCache,
    cache: DashCache<String, Blob>,
    poisoned: Arc<AtomicBool>,
    signal: Sender<CompactionSignal>,
    immutable: ImmutableMemtable,
}

impl NldbInner {
    pub fn new(config: &NldbConfig) -> NldbInner {
        let memtable = Memtable::new(
            config.max_memtable_size as usize,
            config.max_memtable_nodes as usize,
        )
        .expect("Unable to create memtable");

        let cache = DashCache::new(NonZeroUsize::new(config.cache_size as usize).unwrap());
        let sstable_cache = SSTableCache::new(config.compaction_rate as usize);
        let poisoned = Arc::new(AtomicBool::new(false));
        let (signal, receiver) = mpsc::channel::<CompactionSignal>(5);
        let immutable = ImmutableMemtable::new();
        let _compaction_handle = tokio::task::spawn(sstable_background(
            receiver,
            sstable_cache.clone(),
            immutable.clone(),
        ));
        NldbInner {
            memtable,
            sstable_cache,
            cache,
            poisoned,
            signal,
            immutable,
        }
    }

    // First check cache.
    // Second check memtable.
    // Third check SSTables.
    pub async fn get(&self, key: String) -> Option<Blob> {
        let cached = self.cache.get(&key).await;

        if cached.is_some() {
            return cached;
        }

        if let Some(data) = self.memtable.get(&key).await {
            self.cache_read(key, data.clone()).await;
            return Some(data);
        }

        if let Ok(value) = self.sstable_cache.search(&key).await {
            self.cache_read(key, value.clone()).await;
            return Some(value);
        }

        None
    }

    async fn cache_read(&self, key: String, value: Blob) {
        self.cache.insert(key.to_string(), value.clone()).await;
    }

    #[inline]
    async fn evict_if_cached(&self, key: &str) {
        if self.cache.contains(key).await {
            let _ = self.cache.evict(key).await;
        }
    }

    pub async fn delete(&self, key: String) -> Result<(), MemtableError> {
        self.evict_if_cached(&key).await;
        self.memtable.delete(key).await
    }

    pub async fn write(&self, key: String, value: Blob) {
        self.evict_if_cached(&key).await;
        match self.memtable.insert(key, value).await {
            Ok(_) => return,
            Err(e) => match e {
                MemtableError::TableFull(key, value) => {
                    self.rotate_table().await;
                    // SAFETY: We now know the memtable has space.
                    let _ = self.memtable.insert(key, value).await;
                }
            },
        }
    }

    async fn rotate_table(&self) {
        let full_table = self
            .memtable
            .rotate()
            .await
            .expect("Unable to rotate memtable");
        let signal = self.signal.clone();
        let flag = self.poisoned.clone();

        // Take full ownership of the full table.
        let table = Arc::try_unwrap(full_table)
            .expect("More than one strong count of full table")
            .into_inner();

        self.immutable.insert(table).await;

        // Pass the table off to be flushed.
        let immutable = self.immutable.clone();
        tokio::task::spawn_blocking(move || flush_memtable(immutable, signal, flag));
    }
}

impl Drop for NldbInner {
    fn drop(&mut self) {
        self.signal
            .blocking_send(CompactionSignal::Shutdown)
            .expect("Unable to send shutdown signal");
    }
}
