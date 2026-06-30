use crate::constants;
use crate::disk::{DiskRecord, encode};
use crate::sstable::{
    bloom_filter::BloomFilter,
    encode::{encode_footer, encode_index_block},
};
use crate::util;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

fn key_from_record_buffer(buffer: &[u8]) -> String {
    let (key_len_varint, bytes_walked) = util::decode_varint(buffer, 0_usize);

    unsafe {
        String::from_utf8_unchecked(
            buffer[bytes_walked..bytes_walked + key_len_varint as usize].to_vec(),
        )
    }
}

pub struct CompactionWriter {
    fd: File,
    filename: PathBuf,
    index: Vec<(String, u64)>,
    disk_offset: u64,
    write_buffer: Vec<u8>,
    bloom_filter: BloomFilter,
}

impl CompactionWriter {
    pub fn new() -> std::io::Result<CompactionWriter> {
        let filename = util::generate_sstable_file_name();
        let mut fd = File::create(&filename)?;

        fd.write(&constants::NLDB_SSTABLE_HEADER)?;
        fd.write(&constants::V0_HEADER.to_be_bytes())?;

        let index = Vec::with_capacity(1024);
        let write_buffer = Vec::with_capacity(4096);

        let bloom_filter = BloomFilter::new(1024);
        Ok(CompactionWriter {
            fd,
            filename,
            index,
            disk_offset: constants::HEADER_SIZE,
            write_buffer,
            bloom_filter,
        })
    }

    pub fn push(&mut self, record: DiskRecord) -> std::io::Result<()> {
        self.bloom_filter.insert(record.key.as_str());
        let encoded_record = encode::merge_encode_record(record);
        let record_size = encoded_record.len();

        let before = self.disk_offset % constants::DISK_BLOCK_SIZE;
        let after = (self.disk_offset + record_size as u64) % constants::DISK_BLOCK_SIZE;

        if before > after || self.disk_offset == constants::HEADER_SIZE {
            let key = key_from_record_buffer(&encoded_record);
            self.index.push((key, self.disk_offset));
        }

        if self.write_buffer.len() + record_size >= self.write_buffer.capacity() {
            self.fd.write(&self.write_buffer)?;
        }

        self.write_buffer.extend(encoded_record);

        Ok(())
    }

    pub fn finish(mut self) -> std::io::Result<PathBuf> {
        let index_block_start = self.disk_offset;
        let index_block_len = self.index.len();
        let index_block_buffer = encode_index_block(self.index);
        let bloom_filter_start = index_block_start + index_block_buffer.len() as u64;
        self.fd.write(&index_block_buffer)?;
        let bloom_filter_buffer = self.bloom_filter.serialize();
        self.fd.write(&bloom_filter_buffer)?;
        let footer = encode_footer(
            index_block_start as u64,
            index_block_len as u64,
            bloom_filter_start as u64,
        );
        self.fd.write(&footer)?;
        self.fd.flush()?;
        Ok(self.filename)
    }
}
