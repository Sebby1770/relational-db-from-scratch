# Architecture Notes

The project should evolve in layers. Each layer earns its complexity by solving a concrete limitation in the previous milestone.

## High-Level Pipeline

```text
SQL text
  -> lexer/tokenizer
  -> parser
  -> AST
  -> binder/name resolver
  -> logical plan
  -> optimizer
  -> physical plan
  -> executor
  -> access methods
  -> storage engine
```

The current implementation combines several of these steps for simplicity. That is fine for milestone 1. As features grow, separate them deliberately:

- The parser should only understand syntax.
- The binder should resolve table names, column names, and types.
- The planner should describe what work must happen.
- The optimizer should choose an efficient equivalent plan.
- The executor should pull or push rows through physical operators.
- Storage should not know SQL exists.

## SQLite-Inspired Shape

SQLite roughly has:

- Front end: tokenizer, parser, semantic checks
- Code generator: compiles SQL into virtual database engine bytecode
- VDBE: bytecode interpreter
- B-Tree layer: table and index storage
- Pager: page cache, transactions, journal/WAL
- OS interface: files, locks, sync

For this project:

1. Start with direct AST execution.
2. Move to logical and physical plans.
3. Optionally add a tiny bytecode VM once execution operators feel repetitive.
4. Add a pager only when disk storage begins.
5. Add WAL only after the pager has clear page mutation semantics.

## Row Store vs Column Store

### Row Store

A row store keeps all values for one row together:

```text
[id=1, name='Ada', active=true]
[id=2, name='Grace', active=false]
```

Best for:

- OLTP workloads
- Point lookups
- Updating whole rows
- Returning many columns from a few records

Costs:

- Analytical queries scanning one column still read whole rows
- Compression is usually weaker than columnar layouts

This project starts as a row store because it matches SQLite and keeps CRUD execution intuitive.

### Column Store

A column store keeps each column separately:

```text
id:     [1, 2]
name:   ['Ada', 'Grace']
active: [true, false]
```

Best for:

- OLAP workloads
- Aggregations over few columns
- Compression and vectorized execution

Costs:

- Inserts and updates may touch many column arrays
- Reconstructing full rows costs work
- Transactional row-level changes are more complex

Stretch direction: add a columnar sidecar for analytical scans after the row-store engine is stable.

## In-Memory vs Disk-Backed Storage

### In-Memory

Advantages:

- Easy object ownership
- Simple tests
- No page layout or recovery yet
- Fast iteration while learning SQL semantics

Costs:

- No durability
- Pointers and heap allocations hide real storage concerns
- Data structures may not map cleanly to disk

### Disk-Backed

Advantages:

- Teaches pages, records, free lists, fragmentation, and recovery
- Enables real durability
- Forces stable binary formats and compatibility thinking

Costs:

- Every mutation needs failure-mode thinking
- Tests become slower and more elaborate
- Requires a buffer pool or pager abstraction

Recommended path: keep SQL and execution in memory first, then introduce a pager with fixed-size pages, page ids, and explicit serialization.

## Hash Indexes vs B-Trees

### Hash Index

Maps a key directly to row ids:

```text
key -> [row_id, row_id, ...]
```

Best for:

- Equality predicates: `WHERE id = 42`
- Simple implementation
- Teaching index maintenance on insert/update/delete

Costs:

- No ordered scans
- No range queries
- Resizing and collision handling matter
- Disk-backed hash indexes are possible but less general

Start here.

### B+ Tree

Keeps keys ordered in fixed-size nodes:

```text
root -> internal pages -> leaf pages
```

Best for:

- Equality and range predicates
- Ordered scans
- Disk-backed storage
- SQLite-like architecture

Costs:

- Node split/merge logic is subtle
- Page format matters
- Concurrency is harder

Build this after a hash index. You will appreciate why B+ trees dominate general-purpose databases.

## B-Trees vs LSM-Trees

### B+ Tree

Mutates pages in place and keeps data sorted.

Strengths:

- Great point reads and range scans
- Mature design for page-based storage
- Natural fit for SQLite-style single-file databases

Weaknesses:

- Random writes
- Page splits
- Careful recovery needed for in-place mutation

### LSM Tree

Writes to an in-memory memtable and flushes sorted runs to disk, then compacts them.

Strengths:

- High write throughput
- Sequential disk writes
- Good for write-heavy systems

Weaknesses:

- Read amplification
- Compaction complexity
- Tombstones and snapshots are subtle

Recommended path: implement B+ tree first for SQLite inspiration. Treat an LSM tree as an advanced alternate storage engine.

## Simple Parsing vs Parser Generators

### Hand-Written Parser

Strengths:

- Best for learning
- Easy to debug
- Great for small SQL subsets
- No generated code to understand

Weaknesses:

- Error recovery is basic
- Grammar growth can become messy
- Harder to support full SQL precedence rules

Use this early.

### Parser Generator

Strengths:

- Grammar is explicit
- Better for larger SQL coverage
- Can produce better syntax diagnostics

Weaknesses:

- Learning the tool can distract from database internals
- Generated code can hide control flow
- Semantic analysis is still your job

Switch only when expression parsing, joins, nested queries, and operator precedence make the hand-written parser painful.

## Locking vs MVCC

### Locking

Transactions acquire locks before reading or writing.

Strengths:

- Easier first implementation
- Clear conflict behavior
- Teaches two-phase locking and deadlocks

Weaknesses:

- Readers and writers can block each other
- Deadlock detection or timeouts are needed
- Long reads hurt write throughput

Start with table-level locks, then row-level locks.

### MVCC

Each transaction sees a snapshot. Writers create new versions instead of overwriting visible data.

Strengths:

- Readers do not block writers
- Natural snapshot isolation
- Great teaching path for modern database concurrency

Weaknesses:

- Version chains and garbage collection
- Write-write conflict handling
- Harder recovery interactions

Recommended path: implement locking first to understand correctness, then MVCC to understand performance and isolation trade-offs.

## Rule-Based vs Cost-Based Optimization

### Rule-Based

Applies fixed transformations:

- Push filters below projections
- Use an index when predicate is `column = literal`
- Reorder simple predicates
- Remove unused columns

Strengths:

- Simple and deterministic
- Easy to test
- Good bridge from direct execution to planning

Weaknesses:

- Can choose bad join orders
- Ignores data distribution

### Cost-Based

Uses statistics to estimate plan cost:

- table cardinality
- distinct values
- histograms
- index selectivity
- estimated IO and CPU

Strengths:

- Better join order and access path choices
- Teaches real optimizer architecture

Weaknesses:

- Estimates can be wrong
- Requires statistics collection and invalidation
- More moving parts

Recommended path: implement rule-based optimization first, then add a tiny cost model.

## Execution Model Choices

### Volcano Iterator Model

Each operator implements `next()`:

```text
Project
  -> Filter
      -> SeqScan
```

Strengths:

- Easy to compose
- Classic educational model
- Operators can stream rows

Weaknesses:

- Function-call overhead
- Less cache friendly than vectorized execution

Start here.

### Vectorized Execution

Operators process batches of rows or column vectors.

Strengths:

- Better CPU cache behavior
- Fewer virtual calls
- Natural for analytical workloads

Weaknesses:

- More complex operator APIs
- Requires batch memory management

Stretch after query planning is mature.

## Early Correctness Rules

- Do not let storage know about SQL strings.
- Keep logical row identity separate from physical array index before adding indexes.
- Add tests before making storage persistent.
- Every index mutation should happen with the table mutation or not at all.
- Before transactions, document what is not atomic.
- Before disk storage, document what is not durable.

