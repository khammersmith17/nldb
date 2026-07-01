pub mod bloom_filter;
pub mod compaction_writer;
pub mod encode;
pub mod footer;
pub mod iterator;
use crate::constants;
use crate::disk;
use crate::error::SSTableError;
use crate::memtable::inner::Blob;
use crate::ssindex::SstIndex;
use crate::util;
use std::collections::VecDeque;
use std::fs::File;
use std::io::Read;
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

    pub async fn replace_with_compact_table(&self, new_table: SSTable) {
        let mut cache_handle = self.cache.write().await;
        cache_handle.clear();
        cache_handle.push_front(Arc::new(Mutex::new(new_table)));
    }

    pub async fn clone_tables(&self) -> Vec<Arc<Mutex<SSTable>>> {
        let handle = self.cache.read().await;
        handle.iter().cloned().collect()
    }
}

fn validate_buffer_and_get_version(fd: &mut File) -> Result<u16, SSTableError> {
    let mut header_buffer = vec![0_u8; 4];
    fd.read_exact(&mut header_buffer)?;
    if header_buffer != constants::NLDB_SSTABLE_HEADER {
        return Err(SSTableError::InvalidSSTableFile);
    }

    let mut version_buffer = vec![0_u8; 2];
    fd.read_exact(&mut version_buffer)?;
    let version_arr = util::get_be_array2(version_buffer);
    Ok(u16::from_be_bytes(version_arr))
}

pub struct SSTable {
    pub index: SstIndex,
    pub fd: File,
    version: u16,
}

impl SSTable {
    pub fn from_path(file_name: PathBuf) -> Result<SSTable, SSTableError> {
        let fd = File::open(file_name)?;
        Self::from_fd(fd)
    }

    pub fn from_fd(mut fd: File) -> Result<SSTable, SSTableError> {
        let version = validate_buffer_and_get_version(&mut fd)?;
        let index = SstIndex::from_disk_sstable(&mut fd)?;
        Ok(SSTable { index, fd, version })
    }

    pub fn search(&mut self, key: &str) -> Result<Blob, SSTableError> {
        let Some((start, end)) = self.index.range_search_start(key) else {
            return Err(SSTableError::DiskRecordNotFound);
        };

        self.search_data_block(start, end, key)
    }

    pub fn read_data_section(&mut self) -> Vec<u8> {
        todo!()
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
