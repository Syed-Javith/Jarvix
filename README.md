# Jarvix Database Kernel

An independent, concurrent, persistent, ACID-compliant relational database management system written entirely from scratch in Rust, synthesizing architectural principles from PostgreSQL and MySQL/InnoDB.

```
       ██╗ █████╗ ██████╗ ██╗   ██╗██╗██╗  ██╗
       ██║██╔══██╗██╔══██╗██║   ██║██║╚██╗██╔╝
       ██║███████║██████╔╝██║   ██║██║ ╚███╔╝ 
  ██   ██║██╔══██║██╔══██╗╚██╗ ██╔╝██║ ██╔██╗ 
  ╚█████╔╝██║  ██║██║  ██║ ╚████╔╝ ██║██╔╝ ██╗
   ╚════╝ ╚═╝  ╚═╝╚═╝  ╚═╝  ╚═══╝  ╚═╝╚═╝  ╚═╝
   Jarvix Relational Database Management System
   PostgreSQL v3 Wire Compatible | Rust Native Engine
```

---

## Architectural Highlights

- **Storage Engine:** 8 KB slotted pages with line pointer arrays, compact tuple storage, and automatic in-page defragmentation.
- **Buffer Pool Manager:** Frame-level pinning and Clock second-chance eviction, with RAII `PageReadGuard` and `PageWriteGuard` guaranteeing data-race freedom.
- **B+Tree Indexing:** Concurrent disk-backed B+Tree supporting point lookups, insertions with dynamic root growth, deletions, and horizontal range scans via sibling pointers.
- **Concurrency Control & MVCC:** Undo-log based MVCC (preventing table bloat and eliminating table-wide vacuum locks) combined with strict Two-Phase Locking (2PL) and cycle-detection deadlock prevention.
- **Durability & Recovery:** ARIES-compliant Write-Ahead Logging (WAL) with group commit, monotonic LSN sequencing, and 3-phase crash recovery (`Analysis`, `Redo`, `Undo` with CLRs).
- **SQL Frontend & Optimization:** Handcrafted recursive-descent SQL parser, semantic analyzer, catalog schema binder, and rule-based query optimizer (filter pushdown, projection pruning, index selection).
- **Execution Model:** Vectorized Volcano iterator processing batches of up to 1024 tuples (`VectorBatch`) for cache locality and low per-tuple interpretation overhead.
- **Wire Protocol:** PostgreSQL v3 wire protocol compatibility for out-of-the-box support with `psql`, JDBC, Python `psycopg2`, and native drivers.
- **Native REPL Client:** Interactive CLI terminal (`jarvix-cli`) with formatted ASCII tabular output.

---

## Workspace Crates

Jarvix is organized as a modular Rust workspace consisting of 14 specialized crates:

| Crate | Responsibility |
| :--- | :--- |
| [`jarvix-common`](crates/jarvix-common) | Foundational primitives (`PageId`, `SlotId`, `RecordId`, `Lsn`, `TxnId`, `Value`, `DataType`, `Schema`, `Tuple`) and errors. |
| [`jarvix-storage`](crates/jarvix-storage) | 8 KB slotted page format, tuple insertion/retrieval, item pointer headers, and `DiskManager` disk I/O. |
| [`jarvix-buffer`](crates/jarvix-buffer) | Buffer pool manager, clock eviction replacer, atomic frame pins, and RAII page guards. |
| [`jarvix-index`](crates/jarvix-index) | Disk-backed concurrent B+Tree indexing engine. |
| [`jarvix-wal`](crates/jarvix-wal) | Write-ahead logging engine, log records, LSN sequence, group commit, and ARIES recovery. |
| [`jarvix-txn`](crates/jarvix-txn) | Transaction coordinator, MVCC ReadView, undo logs, 2PL lock manager with deadlock cycle detection. |
| [`jarvix-catalog`](crates/jarvix-catalog) | Relational system catalog, schema management, and table/index metadata. |
| [`jarvix-sql`](crates/jarvix-sql) | SQL lexer, recursive-descent parser, and AST representation. |
| [`jarvix-analyzer`](crates/jarvix-analyzer) | Semantic analysis, type checking, and catalog schema validation. |
| [`jarvix-optimizer`](crates/jarvix-optimizer) | Logical planning, index selection, filter pushdown, and projection pruning. |
| [`jarvix-executor`](crates/jarvix-executor) | Vectorized physical query execution engine (`SeqScan`, `IndexScan`, `Insert`, `Update`, `Delete`, `Projection`, `Limit`). |
| [`jarvix-protocol`](crates/jarvix-protocol) | PostgreSQL v3 client/server wire protocol framing and codec. |
| [`jarvix-server`](crates/jarvix-server) | Network listener, connection worker pool, and query execution coordinator. |
| [`jarvix-cli`](crates/jarvix-cli) | Native interactive REPL terminal client and server launcher. |

---

## Quick Start & Usage

### 1. Build the Entire System

```bash
cargo build --release
```

### 2. Run the Full Test Suite

```bash
cargo test --workspace
```

### 3. Start the Jarvix Server Daemon

```bash
# Launch server on default port 5432
cargo run -p jarvix-cli -- --server

# Or specify custom port and data storage directory
cargo run -p jarvix-cli -- --server --port 5432 --data-dir ./data
```

### 4. Connect via the Jarvix REPL Client

In another terminal window:

```bash
cargo run -p jarvix-cli
```

Or connect using the standard PostgreSQL client `psql`:

```bash
psql -h 127.0.0.1 -p 5432 -U jarvix -d jarvix
```

### 5. Interactive SQL Examples

```sql
-- 1. Create a table
CREATE TABLE employees (id INT, name VARCHAR, department VARCHAR, salary INT);

-- 2. Create a B+Tree index on an integer key
CREATE INDEX idx_emp_id ON employees (id);

-- 3. Insert records
INSERT INTO employees VALUES (101, 'Alice', 'Engineering', 125000), (102, 'Bob', 'Design', 85000), (103, 'Charlie', 'Engineering', 140000);

-- 4. Select with index acceleration
SELECT id, name, salary FROM employees WHERE id = 101;

-- 5. Select with filtering and projection
SELECT id, name, salary FROM employees WHERE salary > 90000;

-- 6. Update records
UPDATE employees SET salary = 135000 WHERE id = 101;

-- 7. Delete records
DELETE FROM employees WHERE id = 102;

-- 8. Transaction Control
BEGIN;
INSERT INTO employees VALUES (104, 'Diana', 'Marketing', 92000);
COMMIT;
```

---

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.
