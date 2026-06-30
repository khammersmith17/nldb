#[derive(Debug)]
pub enum MemtableError {
    TableFull,
}

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
