pub mod decode;
pub mod encode;
use crate::constants;
use crate::error::SSTableError;
use crate::memtable::inner::{Blob, NodeData};
use crate::util;
use std::cmp::Ordering;
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

impl PartialEq for DiskRecord {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl Eq for DiskRecord {}

// Sort based on Keys.
impl PartialOrd for DiskRecord {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.key.cmp(&other.key))
    }
}

impl Ord for DiskRecord {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap()
    }
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

        let (key_size, bytes_walked) = util::decode_varint(&block_buffer, offset);
        offset += bytes_walked;

        let key = &block_buffer[offset..offset + key_size as usize];
        if !key.eq(search_key.as_bytes()) {
            offset = log_end;
            continue;
        }

        if is_tombstone(header) {
            return Err(SSTableError::Tombstone);
        }

        offset += key_size as usize;
        let (data_size, bytes_walked) = util::decode_varint(&block_buffer, offset);
        offset += bytes_walked;

        return Ok(block_buffer[offset..offset + data_size as usize].to_vec());
    }
    Err(SSTableError::DiskRecordNotFound)
}

#[cfg(test)]
mod tests {
    use crate::disk::decode::decode_disk_record;
    use crate::disk::encode::{encode_insert_record, encode_tombstone_record};
    use crate::memtable::inner::NodeData;
    use crate::util;

    fn make_insert_record(key: &str, data: &[u8]) -> Vec<u8> {
        let (key_varint, varint_len) = util::encode_varint(key.len());
        encode_insert_record(key, &key_varint[..varint_len], data)
    }

    fn make_tombstone_record(key: &str) -> Vec<u8> {
        let (key_varint, varint_len) = util::encode_varint(key.len());
        encode_tombstone_record(key, &key_varint[..varint_len])
    }

    #[test]
    fn insert_record_roundtrip() {
        let key = "hello";
        let data = b"world";
        let buf = make_insert_record(key, data);

        let mut offset = 0;
        let record = decode_disk_record(&buf, &mut offset).unwrap();

        assert_eq!(record.key, key);
        assert!(matches!(record.data, NodeData::Data(ref d) if d == data));
        assert_eq!(offset, buf.len());
    }

    #[test]
    fn tombstone_record_roundtrip() {
        let key = "deleted_key";
        let buf = make_tombstone_record(key);

        let mut offset = 0;
        let record = decode_disk_record(&buf, &mut offset).unwrap();

        assert_eq!(record.key, key);
        assert!(matches!(record.data, NodeData::Tombstone));
        assert_eq!(offset, buf.len());
    }

    #[test]
    fn insert_record_empty_data() {
        let key = "emptyval";
        let buf = make_insert_record(key, b"");

        let mut offset = 0;
        let record = decode_disk_record(&buf, &mut offset).unwrap();

        assert_eq!(record.key, key);
        assert!(matches!(record.data, NodeData::Data(ref d) if d.is_empty()));
    }

    #[test]
    fn insert_record_large_key_and_data() {
        let key = "k".repeat(200);
        let data = vec![0xAB_u8; 500];
        let buf = make_insert_record(&key, &data);

        let mut offset = 0;
        let record = decode_disk_record(&buf, &mut offset).unwrap();

        assert_eq!(record.key, key);
        assert!(matches!(record.data, NodeData::Data(ref d) if *d == data));
        assert_eq!(offset, buf.len());
    }

    #[test]
    fn multiple_records_sequential_decode() {
        let buf1 = make_insert_record("alpha", b"1");
        let buf2 = make_tombstone_record("beta");
        let buf3 = make_insert_record("gamma", b"3");

        let mut combined = Vec::new();
        combined.extend_from_slice(&buf1);
        combined.extend_from_slice(&buf2);
        combined.extend_from_slice(&buf3);

        let mut offset = 0;
        let r1 = decode_disk_record(&combined, &mut offset).unwrap();
        let r2 = decode_disk_record(&combined, &mut offset).unwrap();
        let r3 = decode_disk_record(&combined, &mut offset).unwrap();

        assert_eq!(r1.key, "alpha");
        assert!(matches!(r1.data, NodeData::Data(ref d) if d == b"1"));
        assert_eq!(r2.key, "beta");
        assert!(matches!(r2.data, NodeData::Tombstone));
        assert_eq!(r3.key, "gamma");
        assert!(matches!(r3.data, NodeData::Data(ref d) if d == b"3"));
        assert_eq!(offset, combined.len());
    }

    #[test]
    fn invalid_header_returns_none() {
        let buf = [0xFF_u8, 0x00];
        let mut offset = 0;
        let result = decode_disk_record(&buf, &mut offset);
        assert!(result.is_none());
    }
}
