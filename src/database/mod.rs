pub mod inner;
use crate::config::NldbConfig;
use crate::memtable::inner::Blob;
use crate::restart::{SSTableArtifact, WalArtifact};
use inner::NldbInner;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Nldb {
    inner: Arc<NldbInner>,
}

impl Nldb {
    pub fn new(
        sstable_files: Vec<SSTableArtifact>,
        wal_files: Vec<WalArtifact>,
        config: &NldbConfig,
    ) -> Nldb {
        let inner = Arc::new(NldbInner::new(sstable_files, wal_files, config));

        Nldb { inner }
    }

    pub async fn write(&self, key: String, value: Blob) {
        self.inner.write(key, value).await
    }

    pub async fn get(&self, key: String) -> Option<Blob> {
        self.inner.get(key).await
    }

    pub async fn delete(&self, key: String) {
        self.inner.delete(key).await
    }
}
