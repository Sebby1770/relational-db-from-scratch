# 6-10 Week Roadmap

This schedule assumes focused part-time work. If you only have weekends, treat each week as a phase rather than a calendar week.

## Implementation Snapshot

This repository now includes a broad implementation pass across the full roadmap, plus the 0.4.0 query surface (2026-08-18):

- Weeks 1-2: working SQL parser, CRUD execution, expressions, sorting, limits, and count.
- Week 3: stable row ids, inline constraints, and catalog-like table/index metadata.
- Week 4: secondary hash indexes, index maintenance, and `EXPLAIN` access path output.
- Week 5: scan/filter/project style execution, a simple access-path planner, and nested-loop `INNER`/`LEFT JOIN`.
- Week 6: `COUNT(*)`, `SUM`, `MIN`, `MAX`, `AVG` (truncated INT), `GROUP BY` hash aggregation, `HAVING`, `DISTINCT`, `ORDER BY`, `LIMIT n OFFSET m`, `LIKE`, `BETWEEN`, `IN`, `INSERT SELECT`, and CSV `COPY` / `.import`.
- Week 7: `BEGIN`, `COMMIT`, `ROLLBACK`, undo logging, and a tested lock manager.
- Week 8: tested slotted page abstraction for the disk-storage path.
- Week 9: tested educational B+ tree for search, insert, splits, and range scans.
- Week 10: tested WAL record format and optimizer statistics module.

Still next after 0.4.0: hash join, cost-based join order, window functions, subqueries, and on-disk table files.

The important distinction: this is now a working educational database plus tested internals components. It is not yet a production SQLite clone with durable recovery, concurrent SQL sessions, MVCC snapshots, or a fully cost-based optimizer.

## Week 1: Minimal In-Memory Database

Deliver:

- Typed schemas
- Row store
- Basic parser
- `CREATE TABLE`
- `INSERT`
- `SELECT`
- Tests through `Database::execute`

Study:

- Relational model basics
- Tuple layout
- SQL parsing basics

Checkpoint:

- You can create a table, insert rows, and select them.

## Week 2: CRUD and Expressions

Deliver:

- `WHERE` predicates beyond equality
- `UPDATE`
- `DELETE`
- Expression AST
- Better parser errors

Study:

- Expression parsing
- Predicate evaluation
- Three-valued SQL logic if adding `NULL`

Checkpoint:

- You can mutate data and trust type checking.

## Week 3: Catalog, Constraints, and Row IDs

Deliver:

- Stable row ids
- Catalog structs
- `PRIMARY KEY`
- `UNIQUE`
- `NOT NULL`
- Basic `CREATE INDEX` metadata

Study:

- System catalogs
- Constraint enforcement
- Logical vs physical identity

Checkpoint:

- Rows have stable identity independent of vector position.

## Week 4: Indexes and Access Paths

Deliver:

- Hash index for equality lookup
- Index maintenance on insert/update/delete
- `EXPLAIN`
- Rule: use index for matching equality predicate

Study:

- Hash tables
- Secondary indexes
- Covering indexes
- Write amplification

Checkpoint:

- Indexed and non-indexed plans return identical results.

## Week 5: Plans, Operators, and Joins

Deliver:

- Logical plan tree
- Physical operators
- Sequential scan
- Index scan
- Filter
- Project
- Nested-loop join

Study:

- Relational algebra
- Volcano iterator model
- Query plan trees

Checkpoint:

- SQL no longer executes directly from the AST.

## Week 6: Aggregation, Sorting, and Rule Optimizer

Deliver:

- `COUNT`, `SUM`, `MIN`, `MAX`
- `GROUP BY`
- `ORDER BY`
- `LIMIT`
- Predicate and projection pushdown

Study:

- Grouping algorithms
- Sorting
- Rule-based optimization

Checkpoint:

- You can explain how a query is transformed before execution.

## Week 7: Transactions and Locking

Deliver:

- `BEGIN`, `COMMIT`, `ROLLBACK`
- Undo log in memory
- Table-level locks
- Basic isolation tests

Study:

- ACID
- Two-phase locking
- Isolation anomalies
- Deadlocks

Checkpoint:

- Rollback reliably undoes inserts, updates, and deletes.

## Week 8: MVCC or Disk Pages

Choose one path.

MVCC path:

- Versioned rows
- Transaction snapshots
- Visibility rules
- Garbage collection sketch

Disk path:

- Fixed-size pages
- Record serialization
- Slotted page layout
- Pager API

Checkpoint:

- You understand either modern concurrency or physical storage deeply enough to explain the trade-off.

## Week 9: B+ Tree Storage

Deliver:

- B+ tree search
- Leaf insert and split
- Internal split
- Range scan
- Invariant tests

Study:

- B+ tree structure
- Page-oriented data structures
- Separator keys

Checkpoint:

- Random inserts produce a valid sorted tree.

## Week 10: WAL and Cost-Based Optimization

Choose based on what excites you.

Durability path:

- WAL records
- Commit records
- Crash/restart tests
- Checkpoints

Optimizer path:

- Table statistics
- Selectivity estimates
- Simple cost model
- Join order choices

Checkpoint:

- The database either survives a crash or starts making data-informed plan choices.
