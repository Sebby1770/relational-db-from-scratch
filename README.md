# Relational Database From Scratch

An educational, SQLite-inspired relational database built in Rust. The project starts as a small in-memory row store and grows toward indexing, query planning, disk pages, transactions, concurrency control, and recovery.

This repository is intentionally designed for learning database internals properly. The early code avoids magic: the parser is hand-written, storage is a simple vector-backed row store, and the execution engine is small enough to inspect in one sitting.

## Current Status

Implemented milestone:

- Typed table schemas: `INT`, `TEXT`, `BOOL`
- In-memory row storage
- `CREATE TABLE`
- `INSERT INTO ... VALUES`
- `SELECT ... FROM ... WHERE column = literal`
- `UPDATE ... SET ... WHERE column = literal`
- `DELETE FROM ... WHERE column = literal`
- Strict type checking
- Basic REPL via `cargo run`
- Integration tests for the first SQL surface

Example:

```sql
CREATE TABLE users (id INT, name TEXT, active BOOL);
INSERT INTO users VALUES (1, 'Ada Lovelace', true);
INSERT INTO users VALUES (2, 'Grace Hopper', false);
SELECT id, name FROM users WHERE active = true;
UPDATE users SET active = true WHERE id = 2;
DELETE FROM users WHERE name = 'Ada Lovelace';
```

## Quick Start

```bash
cargo test
cargo run
```

Inside the REPL:

```text
db> CREATE TABLE users (id INT, name TEXT, active BOOL);
db> INSERT INTO users VALUES (1, 'Ada Lovelace', true);
db> SELECT * FROM users;
db> .quit
```

## Learning Goals

By the end of the project, you should understand:

- How a SQL string becomes an executable plan
- How row-oriented storage differs from column-oriented storage
- How indexes change lookup complexity and write cost
- Why query optimizers need statistics
- How transactions provide atomicity and isolation
- How locks and MVCC make different trade-offs
- How disk-backed databases organize pages, B-Trees, logs, and recovery
- Why SQLite uses a virtual machine architecture and a pager layer

## Suggested Module Structure

The current implementation keeps the first milestone compact:

```text
src/
  lib.rs          Public crate exports
  main.rs         Tiny line-oriented SQL REPL
  db.rs           Database handle and table catalog
  parser.rs       Hand-written SQL tokenizer and parser
  execution.rs    Statement execution and result formatting
  schema.rs       Table schemas, columns, and type validation
  storage.rs      In-memory row store
  row.rs          Row representation
  value.rs        Runtime SQL values
  error.rs        Shared database errors
tests/
  basic_sql.rs    End-to-end SQL tests
docs/
  ARCHITECTURE.md Design notes and trade-offs
  MILESTONES.md   Detailed build milestones
  ROADMAP.md      6-10 week implementation schedule
  SQL_EXAMPLES.md SQL surface to grow toward
  REFERENCES.md   Study topics and references
```

As the database grows, split the code further:

```text
src/
  sql/            lexer, parser, AST
  catalog/        tables, indexes, constraints, statistics
  storage/        heap files, pages, buffer pool, records
  access/         scans, index probes, table cursors
  planner/        logical and physical plans
  executor/       operators: scan, filter, project, join, aggregate
  index/          hash index, B+ tree, LSM experiments
  txn/            transaction manager, locks, MVCC, WAL
  pager/          disk pages, cache, page ids
```

## Implementation Order

1. Keep the current in-memory row store passing tests.
2. Expand the parser only when a new executor feature needs syntax.
3. Introduce logical plans before optimizing anything.
4. Add a hash index first, then a B+ tree.
5. Add joins and aggregation before a serious optimizer.
6. Add transactions in memory before durability.
7. Add a pager and B+ tree storage once the logical engine is stable.
8. Add WAL and recovery before claiming ACID durability.

## Testing Strategy

Use several layers of tests:

- Unit tests for parser, schema validation, and index data structures
- SQL-level integration tests through `Database::execute`
- Property tests for indexes and B+ tree invariants
- Crash/recovery tests once disk storage and WAL exist
- Concurrency stress tests once transactions can overlap
- Golden tests for `EXPLAIN` and planner output

Prefer tests that describe database behavior from SQL first. Then add lower-level tests around tricky structures such as page splits, tombstones, lock upgrades, and transaction visibility.

## Design Philosophy

This is a teaching database, so clarity wins early. SQLite is the inspiration, but the first versions should not copy SQLite's full machinery. Start with direct AST execution, then introduce planning, cursors, bytecode, pages, and logging only when the simpler design has become educationally limiting.

When a design choice appears, write down what you chose and why. Database systems are trade-off machines.

