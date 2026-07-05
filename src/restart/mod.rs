use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

enum DatabaseFile {
    Wal,
    SSTable,
    Other,
}

impl DatabaseFile {
    fn from_path(path: &Path) -> Option<DatabaseFile> {
        let ext_bytes = path.extension()?.as_encoded_bytes();
        match ext_bytes {
            b"sstable" => Some(DatabaseFile::SSTable),
            b"log"
                if path
                    .file_name()
                    .expect("Unable to retrieve filename on file type parsing")
                    .to_str()
                    .expect("Unable to get string out of filename on file type parsing")
                    .starts_with("wal") =>
            {
                Some(DatabaseFile::Wal)
            }
            _ => Some(DatabaseFile::Other),
        }
    }
}

pub struct WalArtifact {
    pub filename: PathBuf,
    ts: u128,
}

impl WalArtifact {
    fn new(filename: PathBuf) -> WalArtifact {
        let name = filename.file_name().unwrap().to_str().unwrap();
        // "wal.{ts}.log" → nth(1) is the timestamp
        let ts: u128 = name
            .split('.')
            .nth(1)
            .expect("Invalid WAL filename")
            .parse()
            .expect("Invalid WAL timestamp");
        WalArtifact { filename, ts }
    }
}

impl PartialEq for WalArtifact {
    fn eq(&self, other: &Self) -> bool {
        self.filename == other.filename
    }
}

impl Eq for WalArtifact {}

impl PartialOrd for WalArtifact {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.ts.cmp(&other.ts))
    }
}

impl Ord for WalArtifact {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap()
    }
}

pub struct SSTableArtifact {
    pub filename: PathBuf,
    ts: u128,
}

impl SSTableArtifact {
    fn new(filename: PathBuf) -> SSTableArtifact {
        let name = filename.file_name().unwrap().to_str().unwrap();
        // "{ts}.sstable" → next() is the timestamp
        let ts: u128 = name
            .split('.')
            .next()
            .expect("Invalid SSTable filename")
            .parse()
            .expect("Invalid SSTable timestamp");
        SSTableArtifact { filename, ts }
    }
}

impl PartialEq for SSTableArtifact {
    fn eq(&self, other: &Self) -> bool {
        self.filename == other.filename
    }
}

impl Eq for SSTableArtifact {}

impl PartialOrd for SSTableArtifact {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.ts.cmp(&other.ts))
    }
}

impl Ord for SSTableArtifact {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap()
    }
}

pub(crate) fn scan_directory(dir: &Path) -> Option<(Vec<SSTableArtifact>, Vec<WalArtifact>)> {
    let mut wal_files: Vec<WalArtifact> = Vec::new();
    let mut sstable_files: Vec<SSTableArtifact> = Vec::new();

    for entry in WalkDir::new(dir).into_iter() {
        let entry = entry.expect("Unable to acquire entry");
        let path = entry.path();

        let Some(filetype) = DatabaseFile::from_path(path) else {
            continue;
        };
        match filetype {
            DatabaseFile::Wal => {
                wal_files.push(WalArtifact::new(path.to_path_buf()));
            }
            DatabaseFile::SSTable => {
                sstable_files.push(SSTableArtifact::new(path.to_path_buf()));
            }
            _ => continue,
        }
    }

    if wal_files.is_empty() && sstable_files.is_empty() {
        return None;
    };

    // Sort SSTables from newest to oldest, as this is how they will be read into SSTableCache.
    sstable_files.sort_by(|a, b| b.cmp(a));
    wal_files.sort();

    Some((sstable_files, wal_files))
}

pub fn get_restart_state() -> Option<(Vec<SSTableArtifact>, Vec<WalArtifact>)> {
    scan_directory(Path::new("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

    static DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> TempDir {
            let id = DIR_COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
            let path = PathBuf::from(format!("test_restart_dir_{id}"));
            fs::create_dir_all(&path).unwrap();
            TempDir(path)
        }

        fn create(&self, name: &str) {
            fs::File::create(self.0.join(name)).unwrap();
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn test_empty_directory_returns_none() {
        let dir = TempDir::new();
        let result = scan_directory(&dir.0);
        drop(dir);
        assert!(result.is_none());
    }

    #[test]
    fn test_sstable_files_detected() {
        let dir = TempDir::new();
        dir.create("1000.sstable");
        dir.create("2000.sstable");
        let result = scan_directory(&dir.0).unwrap();
        drop(dir);
        assert_eq!(result.0.len(), 2);
        assert!(result.1.is_empty());
    }

    #[test]
    fn test_sstables_sorted_newest_first() {
        let dir = TempDir::new();
        dir.create("1000.sstable");
        dir.create("3000.sstable");
        dir.create("2000.sstable");
        let (sstables, _) = scan_directory(&dir.0).unwrap();
        drop(dir);
        assert_eq!(sstables[0].filename.file_name().unwrap(), "3000.sstable");
        assert_eq!(sstables[1].filename.file_name().unwrap(), "2000.sstable");
        assert_eq!(sstables[2].filename.file_name().unwrap(), "1000.sstable");
    }

    #[test]
    fn test_wal_files_detected() {
        let dir = TempDir::new();
        dir.create("wal.1000.log");
        let result = scan_directory(&dir.0).unwrap();
        drop(dir);
        assert_eq!(result.1.len(), 1);
    }

    #[test]
    fn test_wals_sorted_oldest_first() {
        let dir = TempDir::new();
        dir.create("wal.3000.log");
        dir.create("wal.1000.log");
        dir.create("wal.2000.log");
        let (_, wals) = scan_directory(&dir.0).unwrap();
        drop(dir);
        assert_eq!(wals[0].filename.file_name().unwrap(), "wal.1000.log");
        assert_eq!(wals[1].filename.file_name().unwrap(), "wal.2000.log");
        assert_eq!(wals[2].filename.file_name().unwrap(), "wal.3000.log");
    }

    #[test]
    fn test_non_database_files_ignored() {
        let dir = TempDir::new();
        dir.create("notes.txt");
        dir.create("config.json");
        let result = scan_directory(&dir.0);
        drop(dir);
        assert!(result.is_none());
    }

    #[test]
    fn test_non_wal_log_files_ignored() {
        let dir = TempDir::new();
        dir.create("other.log");
        let result = scan_directory(&dir.0);
        drop(dir);
        assert!(result.is_none());
    }

    #[test]
    fn test_both_wal_and_sstable_detected() {
        let dir = TempDir::new();
        dir.create("1000.sstable");
        dir.create("wal.2000.log");
        let (sstables, wals) = scan_directory(&dir.0).unwrap();
        drop(dir);
        assert_eq!(sstables.len(), 1);
        assert_eq!(wals.len(), 1);
    }
}
