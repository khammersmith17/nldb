// SSTABLE HEADER
pub const NLDB_SSTABLE_HEADER: [u8; 4] = [116, 114, 104, 102];
pub const V0_HEADER: u16 = 0_u16;
pub const HEADER_SIZE: u64 = 6_u64;

// DISK RECORD HEADERS
pub const INSERT_LOG_HEADER: u8 = 0_u8;
pub const TOMBSTONE_LOG_HEADER: u8 = 1_u8;

// SSTABLE CONFIGS
pub const DISK_BLOCK_SIZE: u64 = 4_000_u64; // 4 KB
pub const FOOTER_SIZE: u64 = 24_u64;
pub const SSTABLE_LIMIT: usize = 10_usize;

// DEFAULT WAL CONFIGS
pub const DEFAULT_WAL_BUFFER_SIZE: usize = 64_000_usize; // 64 KB
pub const DEFAULT_WAL_BUFFER_CAPACITY: usize = 100_000_usize; // 64 KB
pub const DEFAULT_WAL_BUFFER_FLUSH_TIME: u128 = 400_u128; // 400 ms
