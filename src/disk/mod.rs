pub mod decode;
pub mod encode;
use crate::memtable::inner::NodeData;

/*
* Disk LOG FORMAT
* Data insert record
*   [0_u8][log size varint][key length varint][key][data length varint][data]
*
* Tombstone insert record
*   [1_u8][log size varint][key length varint][key]
* */

pub struct DiskRecord {
    pub key: String,
    pub data: NodeData,
}
