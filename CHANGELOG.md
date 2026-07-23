# Changelog

All notable changes to **relational-db-from-scratch** are documented here.

## [Unreleased]

### Added
- Aggregation: `GROUP BY`, `HAVING`, and the `COUNT`/`SUM`/`AVG`/`MIN`/`MAX`
  functions, with `DISTINCT` and `AS` aliases, executed by a first-seen-order
  `HashAggregate`. NULL handling follows the SQL standard: `COUNT(*)` counts
  NULL rows, other aggregates skip NULLs, an aggregate over no values is NULL
  (COUNT is 0), an ungrouped aggregate always yields one row, and NULLs form
  a single group. `SUM`/`AVG` require a numeric column and check for i64
  overflow; `AVG` is truncated integer division (no float type in the value
  model). `EXPLAIN` gains a `HashAggregate` line. (`src/parser.rs`,
  `src/execution.rs`)
- `tests/aggregation.rs`: 20 tests covering the NULL rules, empty inputs,
  multi-column grouping, DISTINCT, HAVING with AND/OR, and type errors.

### Changed
- `Statement::Select` carries `group_by` and `having`; `Projection` gains an
  `Aggregate` variant. The bare `SELECT COUNT(*)` fast path is unchanged.
- Fixed two pre-existing clippy lints (`db.rs`, `main.rs`) so
  `clippy -D warnings` passes.

## [0.2.0] - 2026-07-04

### Added
- Disk persistence via `Database::open(path)` and `cargo run -- <data-dir>`
- Snapshot catalog (`database.snapshot`) and append-only WAL (`wal.log`)
- Working `CHECKPOINT` command and `.checkpoint` REPL meta command
- `codec` and `persistence` modules for catalog encoding
- SQL-correct `NULL` semantics in `WHERE` clauses
- Compound `AND` predicates can use equality indexes
- `ANALYZE` populates statistics used by `EXPLAIN` row estimates
- Table-level lock manager integrated with active transactions
- Hash-backed O(1) row lookup in table storage
- `DROP TABLE` and `DROP INDEX` SQL support
- REPL helpers: `.tables`, `.schema`, `.storage`, query timing on stderr
- Parser, NULL, persistence, and DROP tests (29 total)
- GitHub Actions CI (`fmt`, `clippy`, `test`)
- `CHANGELOG.md`

### Changed
- `CHECKPOINT` now performs a real snapshot flush when persistence is enabled
- README documents durable mode and quick-start examples

## [0.1.0] - 2026-07-04

### Added
- In-memory SQL engine with CRUD, indexes, transactions, and `EXPLAIN`
- Educational internals: B+ tree, pager, WAL records, lock manager, optimizer stats
- Documentation roadmap and integration tests