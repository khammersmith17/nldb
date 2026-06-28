pub mod decode;
pub mod encode;
use crate::constants;
use crate::error::SSTableError;
use crate::memtable::inner::{Blob, NodeData};
use crate::util;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

/*
* Disk LOG FORMAT
* Data insert record
*   [0_u8][log size varint][key length varint][key][data length varint][data]
*
* Tombstone insert record
*   [1_u8][log size varint][key length varint][key]
* */

fn is_tombstone(header: u8) -> bool {
    header == constants::TOMBSTONE_LOG_HEADER
}

pub struct DiskRecord {
    pub key: String,
    pub data: NodeData,
}

pub fn search_data_block(
    fd: &mut File,
    block_start: u64,
    block_end: u64,
    search_key: &str,
) -> Result<Blob, SSTableError> {
    let block_size = (block_end - block_start) as usize;
    fd.seek(SeekFrom::Start(block_start))?;
    let mut block_buffer = vec![0_u8; block_size];
    fd.read_exact(&mut block_buffer)?;

    let mut offset = 0_usize;

    while offset < block_size {
        let header = block_buffer[offset];
        offset += 1;

        let (log_size, bytes_walked) = util::decode_varint(&block_buffer, offset);
        offset += bytes_walked;
        let log_end = offset + log_size as usize;

        if is_tombstone(header) {
            return Err(SSTableError::Tombstone);
        }

        let (key_size, bytes_walked) = util::decode_varint(&block_buffer, offset);
        offset += bytes_walked;

        let key = &block_buffer[offset..offset + key_size as usize];
        if !key.eq(search_key.as_bytes()) {
            offset = log_end;
            continue;
        }

        offset += key_size as usize;
        let (data_size, bytes_walked) = util::decode_varint(&block_buffer, offset);
        offset += bytes_walked;

        return Ok(block_buffer[offset..offset + data_size as usize].to_vec());
    }
    Err(SSTableError::DiskRecordNotFound)
}
