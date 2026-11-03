# ADR-004: Vectorized Volcano Iterator Model for Query Execution

## Status
Accepted

## Context
Query execution engines execute physical query plans (trees of relational operators like Scan, Filter, Project, Limit).
Execution models include:
1. **Classic Volcano Iterator Model (Tuple-at-a-time)**: Operators implement `next() -> Option<Tuple>`. While simple and memory-efficient, calling a virtual function per individual tuple induces massive CPU branch mispredictions and dynamic dispatch overhead.
2. **Materialized Model (All-at-a-time)**: Each operator reads and buffers the entire intermediate result before passing it to the parent. This causes memory exhaustion on large datasets and fails on streaming queries.
3. **Vectorized Volcano Model (Batch-at-a-time)**: Operators process fixed-size batches (e.g. 1024 tuples) per `next_batch()` call.

## Decision
We implement a **Vectorized Volcano Iterator Model** in `jarvix-executor`:
- The execution trait defines:
  ```rust
  pub trait Executor: Send {
      fn init(&mut self) -> Result<()>;
      fn next(&mut self) -> Result<Option<RecordBatch>>;
      fn schema(&self) -> &Schema;
  }
  ```
- Physical plan nodes (e.g., `SeqScanExecutor`, `IndexScanExecutor`, `ProjectionExecutor`, `LimitExecutor`) stream `RecordBatch` instances (defaulting to 1024 tuples per batch).
- Within a batch, column values are processed in contiguous vectors, maximizing CPU L1/L2 cache locality and allowing loop unrolling.
- Streaming batch pipelining keeps memory consumption bounded regardless of total table size.

## Consequences
### Positive
- Amortizes dynamic dispatch and virtual function call overhead by a factor of 1024.
- High instruction cache efficiency and SIMD-friendly contiguous memory layout.
- Pipelined streaming: first results are returned to client immediately without waiting for entire query completion.

### Trade-offs
- Intermediate buffering of small batch buffers (negligible memory footprint ~64 KB per batch).
- Requires batch slicing and filtering logic compared to a simple scalar iterator.
