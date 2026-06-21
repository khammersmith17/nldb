struct BloomFilter {
    bit_map: Vec<u64>,
    num_hashes: usize,
}

pub struct SstIndex {
    keys: Vec<String>,
    offsets: Vec<u64>,
}
