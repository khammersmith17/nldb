use super::DiskRecord;
use crate::memtable::inner::NodeData;
use crate::util;

pub fn decode_insert_log(buffer: &[u8], offset: &mut usize) -> DiskRecord {
    let (log_size, bytes_walked) = util::decode_varint(buffer, *offset);
    *offset += bytes_walked;

    let start = *offset;

    let (key_length, bytes_walked) = util::decode_varint(buffer, *offset);
    *offset += bytes_walked;

    let key = unsafe {
        String::from_utf8_unchecked(buffer[*offset..*offset + key_length as usize].to_vec())
    };

    *offset += key_length as usize;

    let (data_len, bytes_walked) = util::decode_varint(buffer, *offset);
    *offset += bytes_walked;

    let data = buffer[*offset..*offset + data_len as usize].to_vec();
    *offset += data_len as usize;

    debug_assert_eq!(log_size as usize, *offset - start);

    DiskRecord {
        key,
        data: NodeData::Data(data),
    }
}

pub fn decode_tombstone_log(buffer: &[u8], offset: &mut usize) -> DiskRecord {
    let (log_size, bytes_walked) = util::decode_varint(buffer, *offset);
    *offset += bytes_walked;

    let start = *offset;

    let (key_length, bytes_walked) = util::decode_varint(buffer, *offset);
    *offset += bytes_walked;

    let key = unsafe {
        String::from_utf8_unchecked(buffer[*offset..*offset + key_length as usize].to_vec())
    };

    *offset += key_length as usize;

    debug_assert_eq!(log_size as usize, *offset - start);

    DiskRecord {
        key,
        data: NodeData::Tombstone,
    }
}
