use crate::compaction::{CompactionSignal, SSTableLoadAck, sstable_background};
use crate::config::NldbConfig;
use crate::error::MemtableError;
use crate::memtable::{
    Memtable,
    immutable::ImmutableMemtable,
    inner::{Blob, MemtableFlushSignal, MemtableQuery, NodeData, flush_memtable},
};
use crate::sstable::SSTableCache;
use dash_cache::DashCache;
use std::num::NonZeroUsize;
use std::sync::{Arc, atomic::AtomicBool};
use tokio::sync::mpsc::{self, Sender};

/// Inner memtable struct that holds all database implementation.
/// On write/delete, if that memtable is full, it will return an error. On
/// [MemtableError::TableFull], the table will be rotated out while being flushed.
#[derive(Debug)]
pub struct NldbInner {
    memtable: Memtable,
    sstable_cache: SSTableCache,
    cache: DashCache<String, Blob>,
    poisoned: Arc<AtomicBool>,
    signal: Sender<CompactionSignal>,
    memtable_channel: Sender<MemtableFlushSignal>,
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

        // TODO: tune correct boundry on compaction channel size.
        let (signal, receiver) = mpsc::channel::<CompactionSignal>(100);

        // TODO: tune correct boundry on memtable flush channel size.
        let (memtable_channel, table_flush_recv) = mpsc::channel(100);

        let (sstable_ack_tx, sstable_ack_rx) = mpsc::channel::<SSTableLoadAck>(1);
        let immutable = ImmutableMemtable::new();
        let _compaction_handle = tokio::task::spawn(sstable_background(
            receiver,
            sstable_cache.clone(),
            immutable.clone(),
            sstable_ack_tx,
        ));

        let shared_immutable = immutable.clone();
        let shared_flag = poisoned.clone();
        let compaction_signal = signal.clone();

        // Do not save the handle, let Drop take care of clean up.
        let _ = tokio::task::spawn(flush_memtable(
            shared_immutable,
            compaction_signal,
            shared_flag,
            table_flush_recv,
            sstable_ack_rx,
        ));

        NldbInner {
            memtable,
            sstable_cache,
            cache,
            poisoned,
            signal,
            immutable,
            memtable_channel,
        }
    }

    // First check cache.
    // Second check memtable.
    // Third check immutable table being flushed to disk.
    // Fourth check SSTables.
    pub async fn get(&self, key: String) -> Option<Blob> {
        self.check_poison_flag();
        let cached = self.cache.get(&key).await;

        if cached.is_some() {
            return cached;
        }

        if let Some(blob) = self.read_memtable(&key).await {
            self.read_through_cache(key.clone(), blob.clone()).await;
            return Some(blob);
        }

        if let Some(blob) = self.read_immutable_table(&key).await {
            self.read_through_cache(key.clone(), blob.clone()).await;
            return Some(blob);
        }

        if let Ok(value) = self.sstable_cache.search(&key).await {
            self.read_through_cache(key, value.clone()).await;
            return Some(value);
        }

        None
    }

    async fn read_memtable(&self, key: &str) -> Option<Blob> {
        match self.memtable.get(&key).await {
            MemtableQuery::Data(blob) => Some(blob),
            MemtableQuery::Tombstone => None,
            _ => None,
        }
    }

    async fn read_immutable_table(&self, key: &str) -> Option<Blob> {
        match self.immutable.get(&key).await {
            MemtableQuery::Data(blob) => Some(blob),
            MemtableQuery::Tombstone => None,
            _ => None,
        }
    }

    async fn read_through_cache(&self, key: String, value: Blob) {
        self.cache.insert(key.to_string(), value.clone()).await;
    }

    // Evict a key if cached on a insert/delete. DashCache internally handles the evict only if it
    // exists.
    #[inline]
    async fn evict_if_cached(&self, key: &str) {
        let _ = self.cache.evict(key).await;
    }

    pub async fn delete(&self, key: String) {
        self.check_poison_flag();
        self.evict_if_cached(&key).await;
        match self.memtable.delete(key).await {
            Ok(_) => {}
            Err(e) => {
                let MemtableError::TableFull(key, value) = e;
                self.rotate_table().await;
                // SAFETY: We now know the memtable has space.
                match value {
                    NodeData::Data(_) => {
                        unreachable!()
                    }
                    NodeData::Tombstone => {
                        let _ = self.memtable.delete(key).await;
                    }
                }
            }
        }
    }

    pub async fn write(&self, key: String, value: Blob) {
        self.check_poison_flag();
        self.evict_if_cached(&key).await;
        match self.memtable.insert(key, value).await {
            Ok(_) => return,
            Err(e) => {
                let MemtableError::TableFull(key, value) = e;
                self.rotate_table().await;
                // SAFETY: We now know the memtable has space.
                match value {
                    NodeData::Data(blob) => {
                        let _ = self.memtable.insert(key, blob).await;
                    }
                    NodeData::Tombstone => {
                        unreachable!()
                    }
                }
            }
        }
    }

    async fn rotate_table(&self) {
        let full_table = self
            .memtable
            .rotate()
            .await
            .expect("Unable to create fresh memtable on rotate");

        self.immutable.insert(full_table).await;
        self.memtable_channel
            .send(MemtableFlushSignal::Flush)
            .await
            .expect("Unable to send memtable flush signal");
    }

    fn check_poison_flag(&self) {
        let flag = self.poisoned.load(std::sync::atomic::Ordering::Acquire);
        if flag {
            panic!("Database reached poisoned state")
        }
    }
}

impl Drop for NldbInner {
    fn drop(&mut self) {
        // When this is dropped, signal to background working thread to exit.
        self.signal
            .blocking_send(CompactionSignal::Shutdown)
            .expect("Unable to send compaction shutdown signal");

        self.memtable_channel
            .blocking_send(MemtableFlushSignal::Shutdown)
            .expect("Unable to send memtable flush shutdown signal");
    }
}
