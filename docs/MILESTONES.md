# Milestones

This roadmap starts with a minimal in-memory database and progresses toward an SQLite-inspired architecture. Each milestone should end with tests and a short design note in the repo.

## Milestone 0: Project Harness

### Goal

Create a repeatable development environment, test harness, and small public API before implementing database features.

### Core Concepts

- Library vs binary separation
- Integration tests as SQL behavior tests
- Error types and API boundaries
- Reproducible examples

### Data Structures

- `Database` handle
- Shared `DbError`
- `QueryResult`

### Suggested Implementation Steps

1. Create a Rust crate.
2. Add `Database::new()`.
3. Add `Database::execute(sql: &str)`.
4. Return a placeholder error for unsupported SQL.
5. Add a tiny REPL binary.
6. Add the first integration test file.

### Edge Cases

- Empty SQL input
- Unsupported statement
- Error display should be useful

### Tests

- Creating a database does not panic
- Empty SQL returns a parse error
- REPL can call the same public API as tests

### Stretch Goals

- Add `cargo fmt` and `cargo clippy` CI
- Add benchmark harness with Criterion later

## Milestone 1: In-Memory Typed Tables

### Goal

Support `CREATE TABLE`, `INSERT`, and `SELECT` over typed in-memory rows.

### Core Concepts

- Relations, tuples, attributes
- Schema validation
- Runtime values vs declared types
- Row-store layout

### Data Structures

- `TableSchema`
- `Column`
- `DataType`
- `Value`
- `Row = Vec<Value>`
- `Table { schema, rows }`
- `HashMap<table_name, Table>`

### Suggested Implementation Steps

1. Define supported data types: `INT`, `TEXT`, `BOOL`.
2. Normalize identifiers.
3. Store tables in a catalog map.
4. Validate row arity on insert.
5. Validate each value against the column type.
6. Return selected rows as `QueryResult::Rows`.

### Edge Cases

- Duplicate table names
- Duplicate column names
- Unknown table
- Unknown column
- Wrong number of inserted values
- Wrong value type
- Case-insensitive keywords and identifiers

### Tests

- Create a table with three typed columns
- Insert valid rows
- Reject type mismatches
- Reject arity mismatches
- Select all rows
- Select projected columns

### Stretch Goals

- Add `NULL` with nullable column metadata
- Add default values
- Add `PRIMARY KEY` metadata without enforcing it yet

## Milestone 2: SQL Parser and AST

### Goal

Separate SQL parsing from execution by producing an AST.

### Core Concepts

- Lexing vs parsing
- Keywords vs identifiers
- Literals
- AST design
- Syntax errors vs semantic errors

### Data Structures

- `Token`
- `Statement`
- `Expression`
- `Predicate`
- `Projection`
- `Assignment`

### Suggested Implementation Steps

1. Tokenize identifiers, numbers, strings, punctuation, and comments.
2. Parse `CREATE TABLE`.
3. Parse `INSERT INTO ... VALUES`.
4. Parse `SELECT ... FROM ... WHERE`.
5. Parse `UPDATE ... SET ... WHERE`.
6. Parse `DELETE FROM ... WHERE`.
7. Keep semantic checks out of the parser.

### Edge Cases

- Unterminated strings
- Escaped quotes in strings
- Negative integers
- Extra tokens after a statement
- Reserved words used as identifiers
- Empty column lists

### Tests

- Parser unit tests for each statement
- String escaping test
- Case-insensitive keyword tests
- Invalid syntax tests

### Stretch Goals

- Add Pratt parsing for expressions
- Add source spans for better errors
- Add parser recovery for REPL-friendly diagnostics

## Milestone 3: CRUD Execution Engine

### Goal

Execute basic `SELECT`, `UPDATE`, and `DELETE` with predicates and projections.

### Core Concepts

- Binding names to schema objects
- Separating semantic validation from parsing
- Table scans
- Predicate evaluation
- Projection
- Mutation semantics

### Data Structures

- Bound column references
- Predicate evaluation function
- Assignment list
- Result set

### Suggested Implementation Steps

1. Resolve table names.
2. Resolve column names to indexes.
3. Validate predicate literal types.
4. Implement sequential scan.
5. Apply optional `WHERE`.
6. Apply projection for `SELECT`.
7. Apply assignments for `UPDATE`.
8. Retain or remove rows for `DELETE`.

### Edge Cases

- Updating all rows when there is no `WHERE`
- Deleting all rows when there is no `WHERE`
- Predicate type mismatch
- Duplicate projection columns
- Updating the same column twice

### Tests

- `SELECT name FROM users WHERE id = 1`
- `UPDATE users SET active = true WHERE id = 2`
- `DELETE FROM users WHERE active = false`
- Unknown column errors
- No-match predicates return zero rows or zero count

### Stretch Goals

- Add comparison operators: `<`, `<=`, `>`, `>=`, `!=`
- Add boolean operators: `AND`, `OR`, `NOT`
- Add expression evaluation in projections

## Milestone 4: Catalog, Constraints, and Row Identity

### Goal

Introduce metadata management and stable row identifiers so indexes and transactions have something reliable to point at.

### Core Concepts

- Catalog tables
- Row ids
- Constraints
- Logical identity vs physical storage location
- Tombstones vs compaction

### Data Structures

- `RowId`
- `Slot { row_id, row, deleted }`
- `Catalog`
- `Constraint`
- `PrimaryKey`
- `UniqueConstraint`

### Suggested Implementation Steps

1. Add an increasing `RowId` per table.
2. Store rows as slots instead of raw `Vec<Row>`.
3. Return row ids from inserts internally.
4. Add catalog structs for tables and indexes.
5. Enforce `PRIMARY KEY` uniqueness with a simple map.
6. Add `NOT NULL` and `UNIQUE`.

### Edge Cases

- Deleted rows referenced by stale row ids
- Reusing row ids after deletion
- Updating a primary key
- Constraint violation during multi-row changes

### Tests

- Row id remains stable after deletion of another row
- Duplicate primary key rejected
- `NOT NULL` column rejects `NULL`
- Updating unique value checks conflicts

### Stretch Goals

- Add `CREATE INDEX` metadata
- Add `DROP TABLE`
- Add catalog persistence later

## Milestone 5: In-Memory Indexes

### Goal

Speed up equality predicates using secondary indexes.

### Core Concepts

- Access paths
- Index maintenance
- Covering vs non-covering indexes
- Unique vs non-unique indexes
- Write amplification

### Data Structures

- `HashMap<Value, Vec<RowId>>`
- `Index`
- `IndexKey`
- `RowIdSet`
- Optional `BTreeMap<Value, Vec<RowId>>` for ordered tests

### Suggested Implementation Steps

1. Add `CREATE INDEX idx ON table(column)`.
2. Build an index from existing rows.
3. Maintain index entries on insert.
4. Maintain index entries on delete.
5. Maintain index entries on update if indexed columns change.
6. Use the index for `WHERE indexed_column = literal`.
7. Add `EXPLAIN` to show whether a scan or index lookup is used.

### Edge Cases

- Duplicate keys in non-unique indexes
- Updating indexed values
- Deleting indexed rows
- Index on a missing column
- Type mismatch in index probe
- Stale row ids

### Tests

- Query result is identical with and without index
- `EXPLAIN` shows index lookup
- Insert after index creation is visible through index
- Delete removes index entry
- Update moves row from old key to new key

### Stretch Goals

- Multi-column composite indexes
- Unique indexes
- Covering index scans
- Ordered `BTreeMap` index for range predicates

## Milestone 6: Logical Plans and Rule-Based Optimizer

### Goal

Stop executing AST nodes directly. Convert SQL into logical plans, then transform those plans before execution.

### Core Concepts

- Logical algebra
- Physical operators
- Predicate pushdown
- Projection pushdown
- Access path selection
- `EXPLAIN`

### Data Structures

- `LogicalPlan`
- `PhysicalPlan`
- `PlanNode`
- `SeqScan`
- `IndexScan`
- `Filter`
- `Project`

### Suggested Implementation Steps

1. Convert `SELECT` AST to logical plan.
2. Add `Filter` and `Project` nodes.
3. Add a binder phase before planning.
4. Add rewrite rules.
5. Select `IndexScan` when an index matches equality predicate.
6. Execute physical plans through operator structs.
7. Add `EXPLAIN`.

### Edge Cases

- Rules that change result order
- Predicates referencing projected-away columns
- Index not usable because type or operator does not match
- Empty tables

### Tests

- Plan shape for simple select
- Filter pushdown does not change results
- Projection pushdown keeps needed predicate columns
- Index scan and seq scan return same rows
- `EXPLAIN` golden tests

### Stretch Goals

- Add a small rule engine
- Add plan pretty-printer
- Add optimizer trace output

## Milestone 7: Joins, Aggregation, and Sorting

### Goal

Support more relational algebra: joins, grouping, aggregate functions, and ordering.

### Core Concepts

- Nested-loop join
- Hash join
- Join predicates
- Grouping
- Aggregates
- Sorting and memory limits

### Data Structures

- `Join`
- `HashTable<JoinKey, Rows>`
- `AggregateState`
- `SortBuffer`
- `Expression`

### Suggested Implementation Steps

1. Parse table aliases and qualified columns.
2. Add inner nested-loop join.
3. Add hash join for equality predicates.
4. Parse and execute `COUNT`, `SUM`, `MIN`, `MAX`.
5. Add `GROUP BY`.
6. Add `ORDER BY`.
7. Add `LIMIT`.

### Edge Cases

- Ambiguous column names
- Empty join inputs
- Duplicate column names in output
- Aggregate over zero rows
- `COUNT(*)` vs `COUNT(column)`
- Sorting mixed values should be rejected or defined

### Tests

- Join two tables on id
- Join with no matches
- Aggregate count over all rows
- Grouped aggregate
- Order ascending and descending
- Limit after order

### Stretch Goals

- Left outer join
- Expression aliases
- `HAVING`
- External sort once disk exists

## Milestone 8: Transactions in Memory

### Goal

Add transaction boundaries and atomic commit/rollback for in-memory data.

### Core Concepts

- ACID
- Atomicity
- Isolation levels
- Transaction states
- Undo logging
- Write sets

### Data Structures

- `TransactionId`
- `Transaction`
- `UndoRecord`
- `WriteSet`
- `TransactionManager`

### Suggested Implementation Steps

1. Add `BEGIN`, `COMMIT`, `ROLLBACK`.
2. Track active transaction state.
3. Record undo information for inserts, updates, and deletes.
4. On rollback, apply undo records in reverse order.
5. On commit, discard undo records.
6. Define single-threaded isolation semantics first.

### Edge Cases

- Rollback after insert
- Rollback after update
- Rollback after delete
- Nested transactions should be rejected unless explicitly supported
- Error during transaction should not corrupt state

### Tests

- Insert then rollback leaves no row
- Update then rollback restores old value
- Delete then rollback restores row
- Commit makes changes visible
- Transaction commands outside valid state return errors

### Stretch Goals

- Savepoints
- Statement-level atomicity
- Transaction-local temp tables

## Milestone 9: Concurrent Transactions

### Goal

Allow multiple transactions to execute concurrently with a clear isolation model.

### Core Concepts

- Shared and exclusive locks
- Two-phase locking
- Deadlocks
- Snapshot isolation
- MVCC
- Visibility rules

### Data Structures

- `LockManager`
- `LockTable`
- `WaitForGraph`
- `VersionedRow`
- `CommitTimestamp`
- `ReadSet`
- `WriteSet`

### Suggested Implementation Steps

1. Start with coarse table-level locks.
2. Implement shared locks for reads and exclusive locks for writes.
3. Enforce two-phase locking.
4. Add deadlock timeout or wait-for graph detection.
5. Move to row-level locks.
6. Implement MVCC as an alternate isolation strategy.
7. Add snapshot visibility checks.

### Edge Cases

- Reader/writer conflicts
- Writer/writer conflicts
- Lock upgrade deadlocks
- Long-running readers
- Write skew under snapshot isolation
- Phantom reads

### Tests

- Two readers can proceed concurrently
- Writer blocks reader under strict locking
- Conflicting writers serialize
- Deadlock is detected or times out
- MVCC reader sees a stable snapshot
- Concurrent increment stress test

### Stretch Goals

- Serializable snapshot isolation
- Predicate locks
- Deterministic scheduler for tests

## Milestone 10: Disk Pages, Pager, and B+ Tree Storage

### Goal

Move from heap-allocated in-memory rows to disk-backed fixed-size pages and B+ trees.

### Core Concepts

- Page ids
- Slotted pages
- Record serialization
- Buffer pool
- Dirty pages
- B+ tree node splits
- Free lists

### Data Structures

- `PageId`
- `Page`
- `BufferPool`
- `Frame`
- `RecordId`
- `SlottedPage`
- `BPlusTree`
- `InternalNode`
- `LeafNode`

### Suggested Implementation Steps

1. Define a fixed page size, such as 4096 bytes.
2. Serialize values and rows into bytes.
3. Implement slotted pages.
4. Implement a pager that reads and writes pages by id.
5. Add an in-memory buffer pool with dirty tracking.
6. Implement B+ tree search.
7. Implement B+ tree insert and leaf split.
8. Add internal splits.
9. Use B+ tree leaves as table storage or as indexes over heap records.

### Edge Cases

- Records larger than a page
- Page full
- Root split
- Duplicate keys
- Separator key updates
- Free space fragmentation
- Endianness and versioning

### Tests

- Serialize and deserialize values
- Insert enough records to split leaves
- Insert enough records to split root
- Range scan returns sorted keys
- Reopen database file and read rows
- Fuzz insert orders and verify tree invariants

### Stretch Goals

- Deletion and node merge/rebalance
- Overflow pages for large values
- Page checksum
- Pluggable storage engines

## Milestone 11: WAL, Recovery, and Durability

### Goal

Make committed transactions survive crashes and uncommitted transactions disappear after restart.

### Core Concepts

- Write-ahead logging
- Redo and undo
- Checkpoints
- Force vs steal buffer policies
- Fsync and durability
- Recovery idempotence

### Data Structures

- `LogSequenceNumber`
- `WalRecord`
- `PageImage`
- `CommitRecord`
- `Checkpoint`
- Dirty page table

### Suggested Implementation Steps

1. Add WAL record format.
2. Append log records before dirty pages are flushed.
3. Log transaction commit.
4. Replay committed changes on startup.
5. Roll back or ignore uncommitted changes.
6. Add checkpoints to bound recovery time.
7. Add tests that kill and reopen the process.

### Edge Cases

- Crash before commit record
- Crash after commit before page flush
- Crash during checkpoint
- Partial log record
- Corrupt page or log checksum
- Replaying the same record twice

### Tests

- Committed insert survives restart
- Rolled-back insert does not survive
- Crash before commit loses change
- Crash after commit preserves change
- Recovery can run multiple times

### Stretch Goals

- ARIES-style recovery
- Group commit
- WAL file recycling

## Milestone 12: Cost-Based Optimizer and Advanced Storage

### Goal

Teach the database to choose plans using statistics, and optionally explore LSM-tree storage.

### Core Concepts

- Cardinality estimation
- Selectivity
- Histograms
- Join ordering
- Cost models
- LSM memtables and compaction

### Data Structures

- `TableStats`
- `ColumnStats`
- `Histogram`
- `Memo`
- `Cost`
- `MemTable`
- `SortedRun`
- `BloomFilter`

### Suggested Implementation Steps

1. Add `ANALYZE` to collect table and column stats.
2. Estimate filter selectivity.
3. Estimate join cardinality.
4. Compare sequential scan vs index scan cost.
5. Compare nested-loop vs hash join cost.
6. Add simple dynamic programming join ordering.
7. Optionally build an LSM storage engine behind the same access API.

### Edge Cases

- Stale statistics
- Highly skewed data
- Correlated columns
- Very small tables where seq scan wins
- LSM tombstone visibility
- Compaction while readers exist

### Tests

- `ANALYZE` populates stats
- Optimizer chooses index for selective predicate
- Optimizer chooses seq scan for tiny table
- Join order changes with table sizes
- Cost estimates are explainable in `EXPLAIN`

### Stretch Goals

- Cascades-style optimizer memo
- Adaptive query execution
- Vectorized scans
- Bloom filters for joins or LSM runs

