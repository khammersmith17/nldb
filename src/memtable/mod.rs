use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
pub(crate) mod inner;
use crate::error::MemtableError;
use crate::util;
use inner::{MemtableInner, NodeData};
use std::path::Path;

pub struct Memtable {
    inner: Arc<RwLock<MemtableInner>>,
}

impl Memtable {
    pub fn new(max_size: usize, max_nodes: usize) -> std::io::Result<Memtable> {
        let inner = Arc::new(RwLock::new(MemtableInner::new(max_size, max_nodes)?));
        Ok(Memtable { inner })
    }

    pub fn new_from_wal(
        wal_filepath: &Path,
        max_size: usize,
        max_nodes: usize,
    ) -> std::io::Result<Memtable> {
        let inner_table = MemtableInner::from_wal(wal_filepath, max_size, max_nodes)?;
        let inner = Arc::new(RwLock::new(inner_table));

        Ok(Memtable { inner })
    }

    pub async fn insert(&self, key: String, data: Vec<u8>) -> Result<(), MemtableError> {
        let node_data = NodeData::Data(data);
        let mut handle = self.inner.write().await;
        handle.insert(key, node_data)?;
        Ok(())
    }

    pub async fn get(&self, key: &str) -> Option<Vec<u8>> {
        let handle = self.inner.read().await;
        handle.get(key)
    }

    pub async fn delete(&self, key: String) -> Result<(), MemtableError> {
        let tombstone = NodeData::Tombstone;
        let mut handle = self.inner.write().await;
        handle.insert(key, tombstone)
    }

    pub async fn flush(&self) -> PathBuf {
        let path = util::generate_sstable_file_name();
        todo!()
    }
}
