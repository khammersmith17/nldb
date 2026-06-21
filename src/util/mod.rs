use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const LEAST_BYTE_USIZE: usize = 0x7F;
const LEAST_BYTE_U8: u8 = 0x7F;
const CONTINUATION: u8 = 0x80;

pub fn generate_wal_file_name() -> PathBuf {
    let start_ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    format!("wal.{start_ts}.log").into()
}

pub fn generate_sstable_file_name() -> PathBuf {
    let start_ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    format!("{start_ts}.sstable").into()
}

pub fn encode_varint(mut value: usize) -> ([u8; 10], usize) {
    let mut buffer = [0_u8; 10];
    let mut idx = 0_usize;

    loop {
        let mut byte = (value & LEAST_BYTE_USIZE) as u8;
        // shift passed this varint byte
        value >>= 7;

        if value != 0 {
            // set continuation bit
            byte |= CONTINUATION;
            buffer[idx] = byte;
            idx += 1;
        } else {
            buffer[idx] = byte;
            idx += 1;
            break;
        }
    }
    (buffer, idx)
}

pub fn decode_varint(buffer: &[u8], mut offset: usize) -> (u64, usize) {
    let mut varint = 0_u64;
    let start = offset;

    loop {
        let byte = buffer[offset];
        varint |= ((byte & LEAST_BYTE_U8) as u64) << (7 * (offset - start));
        offset += 1;

        if byte & CONTINUATION == 0 {
            break;
        }
    }
    (varint, offset - start)
}
