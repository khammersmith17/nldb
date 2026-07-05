use crate::memtable::inner::NodeData;
use std::string::FromUtf8Error;
#[derive(Debug)]
pub enum MemtableError {
    TableFull(String, NodeData),
}

#[derive(Debug)]
pub enum SSTableError {
    DiskRecordNotFound,
    IOError(std::io::Error),
    Tombstone,
    InvalidSSTableFile,
}

impl From<std::io::Error> for SSTableError {
    fn from(err: std::io::Error) -> SSTableError {
        SSTableError::IOError(err)
    }
}

#[derive(Debug)]
pub enum NldbError {
    InvalidQuery,
}

impl From<FromUtf8Error> for NldbError {
    fn from(_err: FromUtf8Error) -> NldbError {
        NldbError::InvalidQuery
    }
}
