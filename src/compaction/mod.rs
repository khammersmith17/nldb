use crate::sstable::SSTableCache;
use tokio::sync::mpsc::Receiver;
/*
* Waits for signal from db thread.
*
* on signal:
*   get a reference to all tables.
*   while there are tables to merge.
* */
pub async fn compaction_fn(receiver: Receiver<u8>, sstable_cache: SSTableCache) {
    while let Some(_singal) = receiver.recv().await {
        let tables = sstable_cache.clone_tables();
        todo!()
    }
}
