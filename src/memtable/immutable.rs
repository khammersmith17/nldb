use super::inner::{MemtableInner, MemtableQuery, flush_memtable_inner};
use std::collections::VecDeque;
use std::fs::File;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone, Debug)]
pub struct ImmutableMemtable {
    table: Arc<RwLock<VecDeque<Arc<MemtableInner>>>>,
}

impl ImmutableMemtable {
    pub fn new() -> ImmutableMemtable {
        let table = Arc::new(RwLock::new(VecDeque::new()));
        ImmutableMemtable { table }
    }

    pub async fn pop(&self) {
        let mut handle = self.table.write().await;
        handle.pop_front();
    }

    pub async fn get(&self, key: &str) -> MemtableQuery {
        let handle = self.table.read().await;
        for inner in handle.iter().rev() {
            let result = inner.get(key);
            match result {
                MemtableQuery::Data(_) => return result,
                MemtableQuery::Tombstone => return MemtableQuery::Tombstone,
                _ => {}
            }
        }

        MemtableQuery::None
    }

    pub async fn insert(&self, table: MemtableInner) {
        let mut handle = self.table.write().await;
        handle.push_back(table.into());
    }

    // This is only called by the background thread. Is does not block readers as this is held by a
    // read lock. Short scoped read lock is the only contention here between any threads.
    pub async fn flush(&self, fd: &mut File) -> std::io::Result<()> {
        let head = {
            let handle = self.table.read().await;

            let head = Arc::clone(
                handle
                    .front()
                    .expect("Flush on immutable table without any tables in the queue"),
            );
            head
        };

        flush_memtable_inner(fd, head)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memtable::inner::{MemtableInner, MemtableQuery, NodeData};
    use std::fs;
    use std::path::PathBuf;

    struct WalGuard(PathBuf);
    impl Drop for WalGuard {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn make_inner(entries: &[(&str, NodeData)]) -> (MemtableInner, WalGuard) {
        let (mut inner, path) = MemtableInner::new_for_test();
        for (key, data) in entries {
            inner.insert(key.to_string(), data.clone()).unwrap();
        }
        (inner, WalGuard(path))
    }

    #[tokio::test]
    async fn test_empty_returns_none() {
        let imm = ImmutableMemtable::new();
        assert_eq!(imm.get("key").await, MemtableQuery::None);
    }

    #[tokio::test]
    async fn test_insert_and_get() {
        let imm = ImmutableMemtable::new();
        let (inner, _guard) = make_inner(&[("k", NodeData::Data(b"v".to_vec()))]);
        imm.insert(inner).await;
        assert_eq!(imm.get("k").await, MemtableQuery::Data(b"v".to_vec()));
        assert_eq!(imm.get("missing").await, MemtableQuery::None);
    }

    #[tokio::test]
    async fn test_newest_wins() {
        let imm = ImmutableMemtable::new();
        let (inner_a, _ga) = make_inner(&[("k", NodeData::Data(b"v1".to_vec()))]);
        let (inner_b, _gb) = make_inner(&[("k", NodeData::Data(b"v2".to_vec()))]);
        imm.insert(inner_a).await; // older
        imm.insert(inner_b).await; // newer
        assert_eq!(imm.get("k").await, MemtableQuery::Data(b"v2".to_vec()));
    }

    #[tokio::test]
    async fn test_tombstone_hides_older_data() {
        let imm = ImmutableMemtable::new();
        let (inner_a, _ga) = make_inner(&[("k", NodeData::Data(b"v1".to_vec()))]);
        let (inner_b, _gb) = make_inner(&[("k", NodeData::Tombstone)]);
        imm.insert(inner_a).await; // older
        imm.insert(inner_b).await; // newer tombstone
        assert_eq!(imm.get("k").await, MemtableQuery::Tombstone);
    }

    #[tokio::test]
    async fn test_pop_removes_oldest() {
        let imm = ImmutableMemtable::new();
        let (inner_a, _ga) = make_inner(&[("only_a", NodeData::Data(b"v".to_vec()))]);
        let (inner_b, _gb) = make_inner(&[("only_b", NodeData::Data(b"v".to_vec()))]);
        imm.insert(inner_a).await; // older
        imm.insert(inner_b).await; // newer
        imm.pop().await;
        assert_eq!(imm.get("only_a").await, MemtableQuery::None);
        assert_eq!(imm.get("only_b").await, MemtableQuery::Data(b"v".to_vec()));
    }
}
