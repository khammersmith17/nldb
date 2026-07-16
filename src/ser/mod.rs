use crate::error::NldbError;
use crate::memtable::inner::Blob;
use crate::util;
use bytes::{BufMut, BytesMut};
/*
* 1 byte success flag
* 0x01 - success
* 0x00 - not success
*
* ERROR CODES:
*  0x01 - Key not found.
*  0x02 - Invalid Query.
*  0x03 - Internal Error.
*
* On all paths:
*   A successful operation starts with a 0x01 byte.
*   An unsuccessful operation starts with a 0x00 byte. Then an error code, as listed above.
*
* Per operation success path:
* GET
*   When key is found:
*       [0x01][blob len varint][blob]
*   When key is not found:
*       [0x00][0x01]
*
* INSERT
*   [0x01]
*
* DELETE
*   [0x01]
*
* */

/// On delete, construct the error message compliant to the above protocol.
pub fn construct_error_message(e: NldbError, buffer: &mut BytesMut) {
    buffer.clear();
    buffer.put_u8(0x00);
    let error_code: u8 = match e {
        NldbError::InvalidQuery => 0x02,
        _ => 0x03,
    };
    buffer.put_u8(error_code);
}

pub fn insert_response(buffer: &mut BytesMut) {
    buffer.clear();
    buffer.put_slice(&[0x01]);
}

pub fn delete_response(buffer: &mut BytesMut) {
    buffer.clear();
    buffer.put_slice(&[0x01]);
}

pub fn serialize_get_response(blob_opt: Option<Blob>, buffer: &mut BytesMut) {
    buffer.clear();
    if let Some(blob) = blob_opt {
        let (varint, size) = util::encode_varint(blob.len());
        buffer.put_u8(0x01_u8);
        buffer.put_slice(&varint[..size]);
        buffer.put_slice(&blob);
    } else {
        buffer.put_slice(&[0x00, 0x01]);
    }
}
