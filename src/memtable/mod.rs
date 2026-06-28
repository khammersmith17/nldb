use std::sync::Arc;
use tokio::sync::RwLock;
pub(crate) mod inner;
use crate::error::MemtableError;
use inner::{Blob, MemtableInner, NodeData};
use std::path::Path;

pub struct Memtable {
    inner: Arc<RwLock<MemtableInner>>,
    max_size: usize,
    max_nodes: usize,
}

impl Memtable {
    pub fn new(max_size: usize, max_nodes: usize) -> std::io::Result<Memtable> {
        let inner = Arc::new(RwLock::new(MemtableInner::new(max_size, max_nodes)?));
        Ok(Memtable {
            inner,
            max_size,
            max_nodes,
        })
    }

    pub fn new_from_wal(
        wal_filepath: &Path,
        max_size: usize,
        max_nodes: usize,
    ) -> std::io::Result<Memtable> {
        let inner_table = MemtableInner::from_wal(wal_filepath, max_size, max_nodes)?;
        let inner = Arc::new(RwLock::new(inner_table));

        Ok(Memtable {
            inner,
            max_size,
            max_nodes,
        })
    }

    pub async fn insert(&self, key: String, data: Blob) -> Result<(), MemtableError> {
        let node_data = NodeData::Data(data);
        let mut handle = self.inner.write().await;
        handle.insert(key, node_data)?;
        Ok(())
    }

    pub async fn get(&self, key: &str) -> Option<Blob> {
        let handle = self.inner.read().await;
        handle.get(key)
    }

    pub async fn delete(&self, key: String) -> Result<(), MemtableError> {
        let tombstone = NodeData::Tombstone;
        let mut handle = self.inner.write().await;
        handle.insert(key, tombstone)
    }

    pub async fn rotate(&self) -> std::io::Result<Arc<RwLock<MemtableInner>>> {
        let mut handle = self.inner.write().await;
        let full_table = Arc::clone(&self.inner);
        *handle = MemtableInner::new(self.max_size, self.max_nodes)?;
        Ok(full_table)
    }
}
