use crate::constants;
use crate::disk::encode;
use crate::memtable::inner::MemtableInner;
use crate::sstable::bloom_filter::BloomFilter;
use crate::util;
use std::fs::File;
use std::io::Write;

fn encode_index_block(index_block: Vec<(String, u64)>) -> Vec<u8> {
    let buffer_size: usize = index_block.iter().map(|(key, _)| key.len() + 8 + 2).sum();
    let mut buffer = vec![0_u8; buffer_size];
    let mut offset = 0_usize;

    index_block.iter().for_each(|(key, key_offset)| {
        let (key_len_varint, varint_len) = util::encode_varint(key.len());

        buffer[offset..offset + varint_len].copy_from_slice(&key_len_varint[..varint_len]);
        offset += varint_len;
        buffer[offset..offset + key.len()].copy_from_slice(&key.as_bytes());
        offset += key.len();
        buffer[offset..offset + 8].copy_from_slice(&key_offset.to_be_bytes());
        offset += 8;
    });
    buffer[..offset].to_vec()
}

fn encode_footer(
    data_block_end: u64,
    index_block_end: u64,
    index_block_count: u64,
    bloom_filter_end: u64,
) -> Vec<u8> {
    let mut buffer = vec![0_u8; 32];
    buffer[..8].copy_from_slice(&data_block_end.to_be_bytes());
    buffer[8..16].copy_from_slice(&index_block_end.to_be_bytes());
    buffer[16..24].copy_from_slice(&index_block_count.to_be_bytes());
    buffer[24..].copy_from_slice(&index_block_count.to_be_bytes());
    buffer
}

pub fn write_sstable(table: &MemtableInner, fd: &mut File) -> std::io::Result<()> {
    /*
     * Allocate an entire buffer of total size + number of records * 4 for variants.
     * DFS inorder to insert records in sorted order
     * */
    let mut disk_size = 0_usize;
    let mut index_block: Vec<(String, u64)> =
        Vec::with_capacity(table.current_size / constants::DISK_BLOCK_SIZE);
    let mut bloom_filter = BloomFilter::new(table.arena.len());
    inorder_flush(
        table,
        fd,
        table.root_node,
        &mut disk_size,
        &mut index_block,
        &mut bloom_filter,
    )?;

    let data_block_end = disk_size;
    let index_block_len = index_block.len();
    let index_block_buffer = encode_index_block(index_block);
    let index_block_end = data_block_end + index_block_buffer.len();
    fd.write(&index_block_buffer)?;
    let bloom_filter_buffer = bloom_filter.serialize();
    let bloom_filter_end = index_block_end + bloom_filter_buffer.len();
    fd.write(&bloom_filter_buffer)?;
    let footer = encode_footer(
        data_block_end as u64,
        index_block_end as u64,
        index_block_len as u64,
        bloom_filter_end as u64,
    );
    fd.write(&footer)?;
    fd.flush()?;
    Ok(())
}

fn inorder_flush(
    table: &MemtableInner,
    fd: &mut File,
    node_idx_opt: Option<usize>,
    disk_size: &mut usize,
    index_block: &mut Vec<(String, u64)>,
    bloom_filter: &mut BloomFilter,
) -> std::io::Result<()> {
    let Some(node_idx) = node_idx_opt else {
        return Ok(());
    };

    inorder_flush(
        table,
        fd,
        table.arena[node_idx].left,
        disk_size,
        index_block,
        bloom_filter,
    )?;
    {
        // flush current node
        let current_node = &table.arena[node_idx];
        let before = *disk_size % constants::DISK_BLOCK_SIZE;
        let record_offset = *disk_size;
        let disk_record = encode::encode_record(current_node);
        bloom_filter.insert(current_node.key.as_str());
        *disk_size += disk_record.len();
        let after = *disk_size % constants::DISK_BLOCK_SIZE;

        if before > after || record_offset == 0 {
            index_block.push((current_node.key.clone(), record_offset as u64));
        }
        fd.write(&disk_record)?;
    }
    inorder_flush(
        table,
        fd,
        table.arena[node_idx].right,
        disk_size,
        index_block,
        bloom_filter,
    )?;

    Ok(())
}
