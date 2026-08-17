# Relational Database From Scratch

An educational, SQLite-inspired relational database built in Rust.

See [CHANGELOG.md](CHANGELOG.md) for release history.

The project starts as a small in-memory row store and grows toward indexing, query planning, disk pages, transactions, concurrency control, and recovery.

This repository is intentionally designed for learning database internals properly. The code avoids magic: the parser is hand-written, the in-memory storage engine is inspectable, and the advanced internals are split into small modules with tests.

## Current Status

Implemented SQL-facing features:

- Typed table schemas: `INT`, `TEXT`, `BOOL`
- Stable row ids behind table storage
- In-memory row-store tables
- Inline constraints: `PRIMARY KEY`, `UNIQUE`, `NOT NULL`
- `CREATE TABLE`
- `CREATE INDEX` and `CREATE UNIQUE INDEX`
- `INSERT INTO ... VALUES`
- `SELECT`, projection, `COUNT(*)`, `SUM(column)`, `WHERE`, `GROUP BY`, `ORDER BY`, `LIMIT n OFFSET m`
- `INNER JOIN` / `JOIN` and `LEFT JOIN` with nested-loop execution
- Qualified columns (`users.name`) plus unambiguous short names after a join
- Predicates: `=`, `!=`, `<`, `<=`, `>`, `>=`, `LIKE`, `AND`, `OR`
- `UPDATE ... SET ... WHERE ...`
- `DELETE FROM ... WHERE ...`
- `COPY table FROM 'file.csv'` and REPL `.import <path> <table>` (CSV header = columns)
- `EXPLAIN` for scan vs index lookup, nested-loop join, and hash aggregation
- `BEGIN`, `COMMIT`, `ROLLBACK` with an in-memory undo log
- `ANALYZE` collects per-table statistics used by `EXPLAIN`
- SQL-correct `NULL` handling in `WHERE` predicates (unknown comparisons filter out)
- Compound `AND` predicates can use equality indexes
- Table-level lock manager integrated with active transactions
- O(1) row lookup via hash-backed table storage
- Strict type checking
- REPL meta commands: `.tables`, `.schema`, `.import`, `.help`
- Persistent storage with snapshot files + append-only WAL replay
- `Database::open(path)` and `cargo run -- <data-dir>` for durable sessions
- `CHECKPOINT` writes a snapshot and truncates the WAL
- GitHub Actions CI (`fmt`, `clippy`, `cargo test`)
- Integration tests for SQL behavior and internals modules

Implemented learning modules:

- Secondary hash indexes
- Table-level lock manager
- Slotted page abstraction
- Educational B+ tree with search, insert, split, and range scan
- WAL record encoding and decoding
- Table statistics and equality-cardinality estimates

Example:

```sql
CREATE TABLE users (
  id INT PRIMARY KEY,
  email TEXT UNIQUE NOT NULL,
  name TEXT NOT NULL,
  active BOOL
);

CREATE INDEX users_email_idx ON users(email);

INSERT INTO users VALUES (1, 'ada@example.com', 'Ada Lovelace', true);
INSERT INTO users VALUES (2, 'grace@example.com', 'Grace Hopper', false);

EXPLAIN SELECT id, name FROM users WHERE email = 'ada@example.com';
SELECT id, name FROM users WHERE active = true ORDER BY id DESC LIMIT 10 OFFSET 0;

CREATE TABLE orders (id INT PRIMARY KEY, user_id INT, total INT);
INSERT INTO orders VALUES (10, 1, 120);

SELECT users.name, orders.total
FROM users
JOIN orders ON users.id = orders.user_id
WHERE orders.total > 100;

SELECT active, COUNT(*), SUM(id) FROM users GROUP BY active;
SELECT name FROM users WHERE name LIKE 'Ada%';
COPY users FROM 'users.csv';

BEGIN;
UPDATE users SET active = true WHERE id = 2;
DELETE FROM users WHERE id = 1;
ROLLBACK;
```

## Quick Start

```bash
cargo test
cargo run
```

Persistent mode with snapshot + WAL replay:

```bash
mkdir -p data
cargo run -- data
```

Inside the REPL:

```text
db> CREATE TABLE users (id INT, name TEXT, active BOOL);
db> INSERT INTO users VALUES (1, 'Ada Lovelace', true);
db> SELECT * FROM users;
db> .import users.csv users
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
  storage.rs      In-memory row store with row ids and index maintenance
  index.rs        Secondary hash index
  transaction.rs  Undo records and transaction state
  concurrency.rs  Table-level lock manager
  pager.rs        Slotted page learning module
  bplus_tree.rs   Educational B+ tree
  wal.rs          WAL record format
  optimizer.rs    Statistics and estimates
  row.rs          Row representation
  value.rs        Runtime SQL values
  error.rs        Shared database errors
tests/
  basic_sql.rs    Baseline SQL tests
  advanced_sql.rs Constraints, indexes, predicates, transactions
  v03_sql.rs      JOIN, GROUP BY, LIKE, OFFSET, CSV import
  internals.rs    Locking, pages, B+ tree, WAL, statistics
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

## Roadmap Coverage

The 6-10 week plan is now represented in code. Weeks 1-7 are integrated into the SQL engine. Weeks 8-10 are implemented as tested learning modules rather than a production disk-backed transactional engine.

Still intentionally not claimed as production-complete:

- Durable crash recovery
- Concurrent SQL sessions
- MVCC visibility rules
- Hash join and cost-based join ordering
- `HAVING`, window functions, and subqueries
- Real on-disk table files backed by the B+ tree

Those are the right next deepening steps after this broad pass.

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
