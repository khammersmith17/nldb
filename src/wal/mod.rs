use super::constants;
use crate::disk::encode;
use crate::disk::{DiskRecord, decode};
use crate::memtable::inner::MemtableNode;
use crate::util;
use memmap2::{Mmap, MmapOptions};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/*
* WAL LOG FORMAT
* Data insert record
*   [0_u8][log size varint][key length varint][key][data length varint][data]
*
* Tombstone insert record
*   [1_u8][log size varint][key length varint][key]
* */

fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
}

pub struct Wal {
    fd: File,
    buffer: Vec<u8>,
    last_flush: u128,
}

impl Wal {
    #[cfg(test)]
    pub fn from_fd(fd: File) -> Wal {
        let buffer = Vec::with_capacity(constants::DEFAULT_WAL_BUFFER_CAPACITY);
        let last_flush = unix_ms();
        Wal {
            fd,
            last_flush,
            buffer,
        }
    }

    pub(crate) fn new() -> std::io::Result<Wal> {
        let wal_file_name = util::generate_wal_file_name();
        let fd = File::create_new(wal_file_name)?;
        let buffer = Vec::with_capacity(constants::DEFAULT_WAL_BUFFER_CAPACITY);
        let last_flush = unix_ms();
        Ok(Wal {
            fd,
            last_flush,
            buffer,
        })
    }

    fn needs_flush(&self) -> bool {
        let now: u128 = unix_ms();
        self.buffer.len() >= constants::DEFAULT_WAL_BUFFER_SIZE
            || now > (self.last_flush + constants::DEFAULT_WAL_BUFFER_FLUSH_TIME)
    }

    pub(crate) fn write_log(&mut self, node: &MemtableNode) {
        // Flush before buffering if needed.
        if self.needs_flush() {
            self.flush()
        }

        let log = encode::encode_memtable_node(node);
        self.buffer.extend(log);
    }

    fn flush(&mut self) {
        let _ = self.fd.write(&self.buffer);
        let _ = self.fd.flush();
        self.buffer.clear();
        self.last_flush = unix_ms();
    }
}

pub struct WalIterator {
    _fd: File,
    buffer: Mmap,
    offset: usize,
}

impl WalIterator {
    pub(crate) fn new(path: &Path) -> std::io::Result<WalIterator> {
        let fd = File::open(path)?;
        let mmap = unsafe { MmapOptions::new().map(&fd)? };

        Ok(WalIterator {
            _fd: fd,
            buffer: mmap,
            offset: 0_usize,
        })
    }
}

impl Iterator for WalIterator {
    type Item = DiskRecord;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset >= self.buffer.len() {
            return None;
        }

        decode::decode_disk_record(&self.buffer, &mut self.offset)
    }
}
