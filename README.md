This is a rather naive implementation of a LSM Tree style database for fun. 

Implementation status
[X] memtable (red black tree)
[X] write ahead logging 
[X] sstable serialization with index blocks
[X] bloom filter on disk and in memory
[X] SSTable Index data strucutre on disk and in memory
[ ] memtable rotation
    [ ] handling reads on full in memory memtable until flushed
[ ] compaction
[ ] reading from multiple SSTables on disk
[ ] database API


The database API will be rather simple, support GET, INSERT, and DELETE. This will be a single node to start, then if I want to take it further, probably an eventually consistent distributed set up.
