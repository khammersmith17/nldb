use super::constants;
use crate::disk::encode;
use crate::disk::{DiskRecord, decode};
use crate::memtable::inner::MemtableNode;
use crate::util;
use memmap2::{Mmap, MmapOptions};
use std::fs::File;
use std::io::Write;
use std::path::Path;

/*
* WAL LOG FORMAT
* Data insert record
*   [0_u8][log size varint][key length varint][key][data length varint][data]
*
* Tombstone insert record
*   [1_u8][log size varint][key length varint][key]
* */

pub struct Wal {
    fd: File,
}

impl Wal {
    pub(crate) fn new() -> std::io::Result<Wal> {
        let wal_file_name = util::generate_wal_file_name();
        let fd = File::create_new(wal_file_name)?;
        Ok(Wal { fd })
    }

    pub(crate) fn write_log(&mut self, node: &MemtableNode) {
        let log = encode::encode_record(node);

        let _ = self.fd.write(&log);
        let _ = self.fd.flush();
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

        let log_header = self.buffer[self.offset];
        self.offset += 1;

        let record = match log_header {
            constants::INSERT_LOG_HEADER => {
                decode::decode_insert_log(&self.buffer, &mut self.offset)
            }
            constants::TOMBSTONE_LOG_HEADER => {
                decode::decode_tombstone_log(&self.buffer, &mut self.offset)
            }
            _ => return None,
        };

        Some(record)
    }
}
