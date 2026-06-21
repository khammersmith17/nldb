pub struct SSTableFooter {
    data_block_end: u64,
    index_block_end: u64,
    index_block_len: u64,
    bloom_filter_end: u64,
}
