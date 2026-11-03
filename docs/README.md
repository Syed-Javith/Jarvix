# JavixDB Documentation & Architecture Guide

Welcome to the comprehensive architecture, design, and Rust engineering documentation for **JavixDB** (`jarvix`)—an independent, concurrent, ACID-compliant relational database management system built in Rust based on reverse-engineered PostgreSQL and MySQL/InnoDB principles.

---

## Architecture Chapters

Each chapter provides an in-depth breakdown of subsystem mechanics, data structures, execution lifecycles, and beginner-friendly Rust learning explanations:

| Chapter | Document | Subsystem / Focus |
|---|---|---|
| **00** | [System Overview](architecture/00-system-overview.md) | High-level architecture, subsystem boundaries, and request flow |
| **01** | [Workspace Architecture](architecture/01-workspace-architecture.md) | Cargo multi-crate modular structure and separation of concerns |
| **02** | [Module Dependencies](architecture/02-module-dependencies.md) | Acyclic dependency graph and compile-time boundaries |
| **03** | [Server Architecture](architecture/03-server-architecture.md) | TCP acceptor, session workers, and multi-threading |
| **04** | [Network Architecture](architecture/04-network-architecture.md) | PostgreSQL v3.0 wire protocol, packet codec, and framing |
| **05** | [SQL Processing](architecture/05-sql-processing.md) | Lexer, tokens, Pratt recursive descent parser, and AST |
| **06** | [Query Planner](architecture/06-query-planner.md) | Semantic analysis, catalog symbol binding, and logical plan generation |
| **07** | [Query Optimizer](architecture/07-query-optimizer.md) | Rule-based optimization, predicate pushdown, and index selection |
| **08** | [Query Executor](architecture/08-query-executor.md) | Vectorized Volcano iterator model and physical plan execution |
| **09** | [Storage Engine](architecture/09-storage-engine.md) | Disk manager, 8 KB page format, and physical I/O |
| **10** | [Page Management](architecture/10-page-management.md) | Slotted page layout, slot directory, item pointers, and compaction |
| **11** | [Buffer Pool](architecture/11-buffer-pool.md) | Clock eviction algorithm, frame pinning, and RAII guards |
| **12** | [Indexing](architecture/12-indexing.md) | Disk-backed B+Tree, internal and leaf pages, and node splitting |
| **13** | [Transaction Manager](architecture/13-transaction-manager.md) | ACID lifecycle, commit/abort protocols, and state transitions |
| **14** | [MVCC](architecture/14-mvcc.md) | Multi-Version Concurrency Control, ReadViews, and undo logs |
| **15** | [Lock Manager](architecture/15-lock-manager.md) | Strict 2PL, S/X table/row locks, and DFS deadlock detection |
| **16** | [WAL & Recovery](architecture/16-wal-and-recovery.md) | Write-Ahead Logging, group commit, and ARIES 3-phase recovery |
| **17** | [System Catalog](architecture/17-catalog.md) | Schema registry, OID allocation, and table/index metadata |
| **18** | [Authentication](architecture/18-authentication.md) | PostgreSQL v3 handshake, SSL probe, and session parameters |
| **19** | [Concurrency](architecture/19-concurrency.md) | Thread-per-connection model, `Arc`, `RwLock`, `Send`, and `Sync` |
| **20** | [Benchmarking](architecture/20-benchmarking.md) | Microbenchmarks, OLTP throughput, latency metrics, and profiling |

---

## Architectural Diagrams (Mermaid)

All diagrams are available in raw `.mmd` format in the [`diagrams/`](diagrams/) directory:

- [system-context.mmd](diagrams/system-context.mmd) – Top-level context diagram.
- [workspace-structure.mmd](diagrams/workspace-structure.mmd) – Crate workspace organization.
- [module-dependencies.mmd](diagrams/module-dependencies.mmd) – Crate compilation dependency DAG.
- [startup-sequence.mmd](diagrams/startup-sequence.mmd) – Database server bootstrap sequence.
- [connection-lifecycle.mmd](diagrams/connection-lifecycle.mmd) – Client connection and handshake lifecycle.
- [sql-execution-flow.mmd](diagrams/sql-execution-flow.mmd) – Complete end-to-end query execution path.
- [query-planning.mmd](diagrams/query-planning.mmd) – AST to logical plan compilation.
- [query-execution.mmd](diagrams/query-execution.mmd) – Vectorized Volcano operator iteration.
- [storage-read-write.mmd](diagrams/storage-read-write.mmd) – DiskManager page read/write path.
- [buffer-pool-lifecycle.mmd](diagrams/buffer-pool-lifecycle.mmd) – Frame pinning, unpinning, and Clock eviction.
- [btree-operations.mmd](diagrams/btree-operations.mmd) – B+Tree lookup, insertion, and leaf splitting.
- [transaction-lifecycle.mmd](diagrams/transaction-lifecycle.mmd) – Transaction state machine (Active, Committed, Aborted).
- [mvcc-visibility.mmd](diagrams/mvcc-visibility.mmd) – ReadView visibility boundary checks.
- [locking-and-deadlocks.mmd](diagrams/locking-and-deadlocks.mmd) – Lock requests, queuing, and wait-for graph cycle detection.
- [wal-write-flow.mmd](diagrams/wal-write-flow.mmd) – WAL record generation and synchronous flush.
- [crash-recovery.mmd](diagrams/crash-recovery.mmd) – ARIES Analysis, Redo, and Undo recovery phases.
- [benchmark-architecture.mmd](diagrams/benchmark-architecture.mmd) – Multi-client benchmark harness.

---

## Architecture Decision Records (ADRs)

Design decisions and trade-offs are documented in [`decisions/`](decisions/):
- [ADR-001: Slotted Page Storage Architecture](decisions/ADR-001-slotted-page-storage.md)
- [ADR-002: Undo-Log-Based Multi-Version Concurrency Control (MVCC)](decisions/ADR-002-undo-log-mvcc.md)
- [ADR-003: Strict Two-Phase Locking (SS2PL) with Wait-For Graph Deadlock Detection](decisions/ADR-003-strict-2pl-deadlock-detection.md)
- [ADR-004: Vectorized Volcano Iterator Model for Query Execution](decisions/ADR-004-vectorized-volcano-execution.md)

---

## Rust Learning Topics Covered in JavixDB

Every architecture document and crate README includes structured Rust tutorials for engineers learning Rust through database systems:
1. **Ownership, Move Semantics & Borrowing**: Slices vs Vectors, immutable vs mutable references.
2. **Lifetimes & Deref Coercion**: RAII guards (`PageReadGuard`, `PageWriteGuard`), implementing `Deref` and `Drop`.
3. **Smart Pointers**: `Arc<T>`, `Mutex<T>`, `RwLock<T>`, `AtomicU64`, and atomic ordering (`SeqCst`, `Acquire`/`Release`).
4. **Data Race Freedom**: Why the compiler enforces `Send` and `Sync`.
5. **Zero-Copy Serialization**: Byte decoding via `byteorder` without extra allocations.
6. **Error Handling**: Custom `JarvixError` enums, `Result<T>`, and the `?` operator.
