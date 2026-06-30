use crate::disk::DiskRecord;
use crate::sstable::iterator::SSTableIterator;
use crate::sstable::{SSTable, SSTableCache};
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc::Receiver;

/*
* Waits for signal from db thread.
*
* on signal:
*   get a reference to all tables.
*   while there are tables to merge.
* */
#[derive(PartialEq, Eq)]
struct HeapNode {
    record: DiskRecord,
    table_idx: usize,
}

impl PartialOrd for HeapNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let ord = self.record.cmp(&other.record);
        match ord {
            Ordering::Equal => Some(self.table_idx.cmp(&other.table_idx)),
            _ => Some(ord),
        }
    }
}

impl Ord for HeapNode {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap()
    }
}

impl HeapNode {
    fn new(record: DiskRecord, table_idx: usize) -> HeapNode {
        HeapNode { record, table_idx }
    }
}

async fn tables_to_iterators(tables: Vec<Arc<Mutex<SSTable>>>) -> Vec<SSTableIterator> {
    let mut iters = Vec::with_capacity(tables.len());
    for table in tables.into_iter() {
        iters.push(SSTableIterator::new(table).await);
    }

    iters
}
pub async fn compaction_fn(mut receiver: Receiver<u8>, sstable_cache: SSTableCache) {
    while let Some(_singal) = receiver.recv().await {
        let tables = sstable_cache.clone_tables().await;
        let mut iters = tables_to_iterators(tables).await.into_iter().enumerate();

        let mut heap: BinaryHeap<HeapNode> = BinaryHeap::new();

        todo!()
    }
}
