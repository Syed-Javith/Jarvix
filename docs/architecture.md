# Jarvix Architecture Specification

## 1. PostgreSQL Reverse-Engineering Insights

- **Process Model:** `fork()` per connection coordinated via shared memory (`shmem`) and semaphores.
- **Storage:** Heap file format (unordered tuples in 8 KB pages) + secondary indexes holding physical RIDs (`PageId:SlotId`).
- **MVCC:** Tuples store header versions (`xmin`, `xmax`, `cid`). Updates create new tuple versions in heap; dead tuples cleaned up via `VACUUM`.
- **Durability:** WAL with ARIES-style recovery, `full_page_writes` after checkpoint to guard against torn pages.

## 2. MySQL / InnoDB Reverse-Engineering Insights

- **Thread Model:** Connection thread pool with shared global buffers.
- **Storage:** Clustered index B+Tree where data rows live strictly in primary key leaf pages; secondary indexes point to PK.
- **MVCC:** In-place row modification with previous versions appended to Undo Log segments (`DB_ROLL_PTR`). Purge threads asynchronously free undo logs.
- **Durability:** Physiological redo logging with a Doublewrite Buffer to avoid torn pages without bloating the redo log.

## 3. Jarvix Kernel Design Synthesis

1. **Storage Subsystem:** Slotted page architecture (8 KB frame) with line pointer array growing downward and tuple data growing upward from bottom of page. Supports in-page compaction and defragmentation.
2. **Buffer Management:** Fixed-size buffer pool using Clock-Pro second-chance eviction. Frame access is mediated by RAII `PageReadGuard` and `PageWriteGuard` managing atomic pin counts and dirty flags.
3. **Transaction & MVCC:** Undo-log based MVCC avoiding PostgreSQL's table bloat and `VACUUM` locks, paired with strict Two-Phase Locking (2PL) and wait-for graph deadlock detection.
4. **Logging & Recovery:** ARIES Write-Ahead Logging with 64-bit monotonic LSNs, fuzzy checkpointing, and Group Commit for high commit throughput.
5. **Execution:** Vectorized chunk-at-a-time iterator yielding batches of up to 1024 tuples to maximize cache locality and instruction pipelining.
6. **Wire Protocol:** PostgreSQL v3 protocol compatibility for seamless integration with existing tools (`psql`, JDBC, ORMs).
