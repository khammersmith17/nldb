use super::inner::{Blob, MemtableInner, flush_memtable_inner};
use std::fs::File;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct ImmutableMemtable {
    table: Arc<RwLock<Option<MemtableInner>>>,
}

impl ImmutableMemtable {
    pub fn new() -> ImmutableMemtable {
        let table = Arc::new(RwLock::new(None));
        ImmutableMemtable { table }
    }

    pub async fn clear(&self) {
        let mut handle = self.table.write().await;
        *handle = None;
    }

    pub async fn get(&self, key: &str) -> Option<Blob> {
        let handle = self.table.read().await;
        if let Some(ref inner) = *handle {
            return inner.get(key);
        }
        None
    }

    pub async fn insert(&self, table: MemtableInner) {
        let mut handle = self.table.write().await;
        debug_assert!(handle.is_none());
        *handle = Some(table);
    }

    pub async fn flush(&self, fd: &mut File) -> std::io::Result<()> {
        let handle = self.table.read().await;
        let Some(ref inner) = *handle else {
            unreachable!()
        };
        flush_memtable_inner(fd, inner)
    }
}
