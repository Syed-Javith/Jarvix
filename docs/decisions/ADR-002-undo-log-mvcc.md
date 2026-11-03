# ADR-002: Undo-Log-Based Multi-Version Concurrency Control (MVCC)

## Status
Accepted

## Context
Relational database management systems implement Multi-Version Concurrency Control (MVCC) to ensure readers do not block writers and writers do not block readers.
Two primary architectural paradigms exist:
1. **PostgreSQL Heap-Appended Multi-Versioning**: Every `UPDATE` creates a brand new tuple on the heap page with `xmin` and `xmax` header fields. Old versions remain on the table page until an asynchronous autovacuum process cleans them up. This causes write amplification and index bloat (table updates require updating all secondary indexes unless HOT applies).
2. **MySQL/InnoDB Undo-Log Multi-Versioning**: Modifications are applied in-place on the heap page, while the prior state (before-image) is recorded in an undo log chain. Readers construct older snapshots by traversing the undo chain backwards.

## Decision
We adopt the **MySQL/InnoDB Undo Log & ReadView architecture** in `jarvix-txn`:
- Updates are performed in-place on physical heap pages, eliminating table bloat.
- The `UndoLogManager` preserves the original row `before_image`.
- Point-in-time snapshot isolation is governed by a `ReadView` struct containing `low_limit_id`, `high_limit_id`, and `active_txns`.
- A row is visible if its modifying transaction committed prior to the `ReadView` creation and was not active in `active_txns`.
- When a transaction commits, its undo log entries can be immediately purged, simplifying space reclamation without needing a complex autovacuum subsystem.

## Consequences
### Positive
- Secondary indexes remain stable when non-indexed columns are updated (no HOT chain chasing).
- Zero table bloat on disk for tables undergoing heavy update workloads.
- Predictable and fast garbage collection: undo logs are purged when transactions finish.

### Trade-offs
- Long-running read queries on heavily modified tables must traverse undo chains to reconstruct older versions.
- Requires dedicated undo log management and synchronization.
