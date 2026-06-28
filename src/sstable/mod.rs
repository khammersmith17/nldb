pub mod bloom_filter;
pub mod encode;
pub mod footer;
use crate::disk;
use crate::error::SSTableError;
use crate::memtable::inner::Blob;
use crate::ssindex::SstIndex;
use std::collections::VecDeque;
use std::fs::File;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

pub struct SSTableCache {
    cache: Arc<RwLock<VecDeque<Arc<Mutex<SSTable>>>>>,
}

impl SSTableCache {
    pub fn new() -> SSTableCache {
        let cache = Arc::new(RwLock::new(VecDeque::new()));
        SSTableCache { cache }
    }

    pub async fn search(&self, key: &str) -> Result<Blob, SSTableError> {
        let handle = self.cache.read().await;
        let num_tables = handle.len();

        for i in 0..num_tables {
            let search_result = {
                let mut table_handle = handle[i].lock().await;
                table_handle.search(key)
            };

            if search_result.is_ok() {
                return search_result;
            }

            match search_result {
                Err(SSTableError::Tombstone) => return Err(SSTableError::DiskRecordNotFound),
                _ => continue,
            }
        }
        Err(SSTableError::DiskRecordNotFound)
    }

    pub async fn push(&self, table: SSTable) -> usize {
        let len = {
            let wrapped_table = Arc::new(Mutex::new(table));
            let mut handle = self.cache.write().await;
            handle.push_front(wrapped_table);
            handle.len()
        };

        len
    }

    pub async fn pop(&self) -> (SSTable, usize) {
        todo!()
    }

    pub async fn clone_tables(&self) -> Vec<Arc<Mutex<SSTable>>> {
        let handle = self.cache.read().await;
        handle.iter().cloned().collect()
    }
}

pub struct SSTable {
    index: SstIndex,
    fd: File,
}

impl SSTable {
    pub fn from_path(file_name: PathBuf) -> std::io::Result<SSTable> {
        let mut fd = File::open(file_name)?;
        let index = SstIndex::from_disk_sstable(&mut fd)?;
        Ok(SSTable { index, fd })
    }

    pub fn from_fd(mut fd: File) -> std::io::Result<SSTable> {
        let index = SstIndex::from_disk_sstable(&mut fd)?;
        Ok(SSTable { index, fd })
    }

    pub fn search(&mut self, key: &str) -> Result<Blob, SSTableError> {
        let Some((start, end)) = self.index.range_search_start(key) else {
            return Err(SSTableError::DiskRecordNotFound);
        };

        self.search_data_block(start, end, key)
    }

    fn search_data_block(
        &mut self,
        start_offset: u64,
        end_offset: u64,
        key: &str,
    ) -> Result<Blob, SSTableError> {
        disk::search_data_block(&mut self.fd, start_offset, end_offset, key)
    }
}
