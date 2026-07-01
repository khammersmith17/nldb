use crate::disk::DiskRecord;
use crate::memtable::inner::NodeData;
use crate::sstable::iterator::SSTableIterator;
use crate::sstable::{SSTable, SSTableCache, compaction_writer::CompactionWriter};
use std::cmp::Ordering;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::fs::{read_dir, remove_file};
use std::path::Path;
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

/*
* Generate iterators for all tables currently cached.
*
* Seed heap with one record per table.
*
* while heap not empty:
*   pop the smallest node
*
*   clear any nodes from the heap that have the same key
*   cleared nodes are replaced with the next node from the same table
*
*   smallest node is written to compaction writer
*
*   next node is taken from the iterator for the flushed node.
*
* Finish writing SSTable file.
* Load next in memory SSTable.
* Replace all cache SSTables with the compact SSTable.
* Remove all stale SSTable disk files.
* */
pub async fn compaction_fn(mut receiver: Receiver<u8>, sstable_cache: SSTableCache) {
    while let Some(_singal) = receiver.recv().await {
        let tables = sstable_cache.clone_tables().await;
        let num_tables = tables.len();
        let mut table_iters = tables_to_iterators(tables).await;

        let mut writer =
            CompactionWriter::new().expect("Unable to open SSTable file for CompactionWriter");
        let mut heap: BinaryHeap<Reverse<HeapNode>> = BinaryHeap::new();

        for i in 0..num_tables {
            if let Some(record) = table_iters[i].next() {
                let heap_node = HeapNode::new(record, i);
                heap.push(Reverse(heap_node));
            }
        }

        while !heap.is_empty() {
            let Some(Reverse(smallest)) = heap.pop() else {
                break;
            };

            let smallest_key = &smallest.record.key;

            while let Some(ref record_ref) = heap.peek() {
                let inner_ref = &record_ref.0;
                if &inner_ref.record.key != smallest_key {
                    break;
                }
                let table_idx = inner_ref.table_idx;

                heap.pop().unwrap();
                if let Some(record) = table_iters[table_idx].next() {
                    let heap_node = HeapNode::new(record, table_idx);
                    heap.push(Reverse(heap_node));
                }
            }

            let HeapNode {
                record: disk_record,
                table_idx,
            } = smallest;

            if matches!(disk_record.data, NodeData::Data(_)) {
                let _ = writer.push(disk_record);
            }

            if let Some(record) = table_iters[table_idx].next() {
                heap.push(Reverse(HeapNode::new(record, table_idx)));
            }
        }

        let new_table_path = writer.finish().expect("Unable to flush compaction writer");
        let sstable = SSTable::from_path(new_table_path.clone())
            .expect("Unable to create in memory SSTable representation");

        sstable_cache.replace_with_compact_table(sstable).await;

        let dir =
            read_dir(Path::new(".")).expect("Unable to read pwd to remove stale SSTable files.");

        for entry in dir {
            let entry = entry.expect("Unable to get directory entry");

            let path = entry.path();

            if new_table_path == path {
                continue;
            }

            if !path.is_file() {
                continue;
            }
            let Some(path_ext) = path.extension() else {
                continue;
            };

            if path_ext == "sstable" {
                remove_file(&path).expect("Unable to delete stale SSTable");
            }
        }
    }
}
