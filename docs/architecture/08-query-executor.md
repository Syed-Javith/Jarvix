# 08 - Query Executor: Vectorized Volcano Engine

## 1. Subsystem Architecture

JavixDB implements a modern **Vectorized Volcano Iterator Model**. 
- In a classic Volcano model, calling `next()` returns a **single tuple**. For a table with 1,000,000 rows, this requires 1,000,000 virtual function calls through the execution tree, trashing CPU instruction caches.
- In JavixDB's **Vectorized Volcano Model**, `next()` returns a **`VectorBatch`** containing up to 1024 tuples. This maximizes CPU L1 cache locality, amortizes virtual method calls by $1024\times$, and allows loop-unrolled SIMD evaluation.

```mermaid
flowchart TD
    subgraph VolcanoTree["Physical Volcano Pipeline"]
        PE["ProjectionExecutor"]
        IE["IndexScanExecutor"]
        SE["SeqScanExecutor"]
        LE["LimitExecutor"]
    end

    subgraph BatchFlow["Vectorized Batch Streaming"]
        BATCH["VectorBatch\n(Vec&lt;Tuple&gt; up to 1024 items)"]
    end

    subgraph KernelIntegration["Kernel State Coordination"]
        BPM["BufferPoolManager\n(Page pinning & frames)"]
        BTREE["BTreeIndex\n(Point search & range scans)"]
        LOCK["LockManager\n(Strict 2PL Shared/Exclusive locks)"]
        WAL["WalManager\n(Insert/Update/Delete logging)"]
    end

    PE -->|calls next()| IE
    IE -->|calls next()| SE
    IE -->|emits| BATCH
    PE -->|emits projected| BATCH
    IE <--> BPM
    IE <--> BTREE
    IE <--> LOCK
    SE <--> LOCK
    SE <--> BPM
```

---

## 2. The `Executor` Trait

```rust
// In jarvix-executor/src/executors/mod.rs:
pub trait Executor {
    /// Initialize executor state, acquire 2PL locks, and open underlying cursors.
    fn init(&mut self) -> Result<()>;

    /// Fetches the next vector batch of tuples. Returns Ok(None) when exhausted.
    fn next(&mut self) -> Result<Option<VectorBatch>>;

    /// Returns the relational schema of tuples emitted by this node.
    fn schema(&self) -> &Schema;
}
```

---

## 3. Physical Executor Nodes

1. **`SeqScanExecutor`**:
   - Acquires Shared 2PL lock on the table.
   - Scans pages sequentially from `first_page_id` up to `disk_manager.num_pages()`.
   - Deserializes valid slotted page tuples and applies filter predicates.
2. **`IndexScanExecutor`**:
   - Acquires Shared 2PL lock on the table.
   - Performs a binary search on the B+Tree index root, returning matching `RecordId(page_id, slot_id)`.
   - Directly fetches the target page frame from `BufferPoolManager` without scanning unrelated pages.
3. **`ProjectionExecutor`**:
   - Evaluates column projections from child batch into output tuples.
4. **`InsertExecutor`**:
   - Acquires Exclusive 2PL table lock.
   - Evaluates row expressions, appends to slotted page, writes WAL Insert record, and inserts keys into secondary B+Tree indexes.
5. **`UpdateExecutor` & `DeleteExecutor`**:
   - Scans matching tuples, marks slots dead, writes new versions, appends WAL records, and updates secondary indexes.
