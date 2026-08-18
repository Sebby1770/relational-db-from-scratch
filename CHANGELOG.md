# Changelog

All notable changes to **relational-db-from-scratch** are documented here.

## [0.4.0] - 2026-08-18

### Added
- `HAVING` over grouped aggregates: `SELECT dept, COUNT(*) FROM t GROUP BY dept HAVING COUNT(*) > 1`
- `SELECT DISTINCT` on one or more projected columns
- `INSERT INTO dest SELECT ... FROM src` with optional joins and `WHERE`; column count must match
- `MIN(column)`, `MAX(column)`, and `AVG(column)` (AVG is truncated integer division)
- `BETWEEN` and `IN` list predicates: `col BETWEEN a AND b`, `col IN (1, 2, 3)`
- `EXPLAIN` reports `Having` and `Distinct` when those clauses are present
- Integration tests for the 0.4 query surface

### Changed
- README, roadmap snapshot, and SQL examples cover HAVING, DISTINCT, INSERT SELECT, and richer predicates
- `SELECT` AST now carries `DISTINCT` and `HAVING`; `INSERT` accepts a `VALUES` or `SELECT` source

## [0.3.0] - 2026-08-18

### Added
- `INNER JOIN` / `JOIN` with nested-loop execution: `FROM t1 JOIN t2 ON t1.col = t2.col`
- `LEFT JOIN` with NULL-extended unmatched right rows
- Qualified columns (`t1.col`) and unambiguous short names after a join
- Table aliases (`FROM users u JOIN orders o ON u.id = o.user_id`)
- `GROUP BY` with hash aggregation
- `COUNT(*)` and `SUM(column)` in the select list, including mixed grouped projections
- `LIKE` predicates: `col LIKE 'foo%'`, `'%bar'`, `'%mid%'`, optional `ESCAPE`
- `LIMIT n OFFSET m` (OFFSET is applied after `ORDER BY`, then LIMIT)
- CSV import via `COPY table FROM 'path'` and the REPL meta command `.import <path> <table>`
- `EXPLAIN` reports `nested loop join` for joins and `hash group by` for grouped queries
- Integration tests for JOIN, GROUP BY, LIKE, OFFSET, and CSV import (38 tests total)

### Changed
- `SELECT` AST now carries joins, `GROUP BY`, and `OFFSET`
- README, roadmap snapshot, and SQL examples cover the 0.3 query surface

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