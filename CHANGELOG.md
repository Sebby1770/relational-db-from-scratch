# Changelog

All notable changes to **relational-db-from-scratch** are documented here.

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