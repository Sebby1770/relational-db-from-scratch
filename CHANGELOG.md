# Changelog

All notable changes to **relational-db-from-scratch** are documented here.

## [0.6.0] - 2026-08-18

### Added
- `EXCEPT` and `INTERSECT` set operations (deduplicating)
- `CROSS JOIN` (cartesian product) and `RIGHT JOIN` (NULL-extended unmatched left rows)
- Hash join for `INNER JOIN` (`EXPLAIN` reports `hash join`)
- `CREATE TABLE [IF NOT EXISTS] name AS SELECT ...`
- `CREATE TABLE IF NOT EXISTS` and `DROP TABLE IF EXISTS`
- `TRUNCATE TABLE`
- `ALTER TABLE t RENAME TO u` (transaction-safe)
- Multi-row `INSERT INTO t VALUES (1), (2)`
- `COALESCE(column, literal)` in the SELECT list
- `ORDER BY 1` (1-based output column position)
- Integration tests for the 0.6 query surface

### Changed
- `INNER JOIN` now uses a hash join instead of nested loops
- `UnionPart` carries a `SetOp` (`UNION` / `EXCEPT` / `INTERSECT`)
- README, roadmap snapshot, and SQL examples cover the 0.6 surface

## [0.5.0] - 2026-08-18

### Added
- `IS NULL` and `IS NOT NULL` predicates
- `NOT IN` and `NOT BETWEEN` predicates
- `COUNT(column)` counts non-null values, distinct from `COUNT(*)`
- `UNION` (deduplicating) and `UNION ALL` of SELECT statements with matching column counts
- `CASE WHEN pred THEN v1 ELSE v2 END` in the SELECT list
- `ALTER TABLE t ADD COLUMN c INT` appends a nullable column (existing rows become `NULL`)
- CSV export via `COPY table TO 'path.csv'` and the REPL meta command `.export <path> <table>`
- Integration tests for the 0.5 query surface

### Changed
- README, roadmap snapshot, and SQL examples cover NULL predicates, UNION, CASE, ALTER, and CSV export
- `SELECT` AST now carries `UNION` tails; `COPY` accepts `FROM` or `TO`

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