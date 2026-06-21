This is a rather naive implementation of a LSM Tree style database. Currently implemented is a red-black tree memtable, write ahead logging, and sstable serialization with index blocks and bloom filters within the on disk SSTable data structure.

I still need to implement rotation when a memtable needs to be flushed, compaction, and the database API.

The database API will be rather simple, support GET, INSERT, and DELETE. This will be a single node to start, then if I want to take it further, probably an eventually consistent distributed set up.
