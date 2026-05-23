# Reference Topics

Use these topics as a study map. The point is not to memorize database lore; it is to connect each feature you build to a real systems idea.

## Relational Foundations

- Relations, tuples, attributes
- Relational algebra: selection, projection, join, aggregation
- SQL semantics vs relational algebra
- NULL and three-valued logic
- Keys, constraints, and normalization

## Parsing and Binding

- Lexers and recursive descent parsers
- Pratt parsers for expressions
- AST design
- Name resolution
- Type checking
- Parser generators such as LALRPOP, pest, nom, or ANTLR

## Storage

- Row stores
- Column stores
- Slotted pages
- Record serialization
- Page ids and record ids
- Free space management
- Buffer pools
- Dirty page flushing

## Indexes

- Hash tables
- B-Trees and B+ trees
- Sparse vs dense indexes
- Clustered vs secondary indexes
- Composite keys
- Covering indexes
- Bloom filters

## Query Execution

- Volcano iterator model
- Vectorized execution
- Sequential scan
- Index scan
- Filter and projection
- Nested-loop join
- Hash join
- Sort-merge join
- Aggregation algorithms

## Query Optimization

- Logical vs physical plans
- Predicate pushdown
- Projection pushdown
- Join reordering
- Cardinality estimation
- Histograms
- Cost models
- Rule-based optimization
- Cost-based optimization

## Transactions

- ACID
- Undo logging
- Redo logging
- Write-ahead logging
- Checkpoints
- Two-phase locking
- Deadlock detection
- Isolation levels
- MVCC
- Snapshot isolation
- Serializable isolation

## Disk and Recovery

- fsync and durability
- Atomic page writes
- WAL replay
- Crash consistency
- Checksums
- Torn writes
- Recovery idempotence

## Systems Engineering

- API boundaries
- Invariant testing
- Property-based testing
- Fuzzing parsers
- Deterministic concurrency tests
- Benchmarking
- Profiling

## Books and Courses

- Database System Concepts by Silberschatz, Korth, and Sudarshan
- Database Management Systems by Ramakrishnan and Gehrke
- Designing Data-Intensive Applications by Martin Kleppmann
- CMU 15-445/645 Database Systems
- Architecture of a Database System by Hellerstein, Stonebraker, and Hamilton
- SQLite documentation: file format, query planner, locking, WAL, and opcode docs

## Source Code Worth Reading

- SQLite
- DuckDB
- PostgreSQL executor and planner modules
- BusTub educational database
- RocksDB for LSM-tree architecture
- sled for Rust storage-engine ideas

