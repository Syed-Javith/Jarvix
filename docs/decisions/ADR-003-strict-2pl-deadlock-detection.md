# ADR-003: Strict Two-Phase Locking (SS2PL) with Wait-For Graph Deadlock Detection

## Status
Accepted

## Context
Concurrent database transactions modifying intersecting data sets can produce anomalies like lost updates, dirty writes, or non-serializable interleavings.
Concurrency control approaches include:
1. **Optimistic Concurrency Control (OCC)**: Transactions execute without locks and validate conflicts at commit time. Under high write contention, OCC experiences massive abort rates and wasted work.
2. **Pessimistic Strict Two-Phase Locking (SS2PL)**: Transactions acquire locks on resources prior to modification and hold all locks until `COMMIT` or `ROLLBACK`. Conflicting transactions are queued, preventing conflicting writes.

A major risk of 2PL is deadlocks (cyclic lock wait dependencies). Databases handle this via lock timeouts or active cycle detection.

## Decision
We implement **Strict Two-Phase Locking (SS2PL) with Wait-For Graph (WFG) Depth-First Search Deadlock Detection** in `jarvix-txn`:
- Support for Shared (S) and Exclusive (X) locks at Table and Row granularities.
- Re-entrancy and subsumption: transactions holding an Exclusive lock are granted Shared requests immediately without re-queuing.
- Whenever a lock request is blocked, directed wait edges are added to a global `waits_for` graph (`requesting_txn -> holding_txn`).
- A recursive Depth-First Search (DFS) detects cycles immediately upon contention.
- If a cycle is detected, the requesting transaction is aborted with `JarvixError::Deadlock(txn_id)` rather than waiting for an arbitrary timeout.
- Threads are suspended efficiently using Rust's `std::sync::Condvar`.

## Consequences
### Positive
- Strict serializability and absence of dirty writes.
- Instantaneous deadlock resolution: deadlocks are caught in microseconds at the point of contention rather than waiting seconds for a timeout.
- Thread-safe, non-busy-waiting synchronization using native OS condition variables.

### Trade-offs
- Graph traversal adds overhead during lock contention, though DFS traversal on typical transaction counts (< 100 concurrent writers) takes less than $10\,\mu\text{s}$.
- Aborted transactions must be retried by the client or application layer.
