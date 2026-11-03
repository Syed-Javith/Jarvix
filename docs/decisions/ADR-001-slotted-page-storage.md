# ADR-001: Slotted Page Storage Architecture

## Status
Accepted

## Context
A relational database storage engine must store variable-length records (such as `VARCHAR` strings or nullable columns) across fixed-size physical disk pages (8192 bytes). 
There are two primary approaches:
1. **Fixed-offset records**: Simple array indexing, but either limits columns to fixed widths or requires massive internal fragmentation and wasted disk space.
2. **Slotted Page Architecture (PostgreSQL & InnoDB)**: Maintains an array of item pointers (slot directory) at the bottom of the page growing downward, while tuple data is appended from the end of the page growing upward. Record IDs remain stable as `(PageId, SlotId)` even if tuple data is defragmented or compacted.

## Decision
We implement a PostgreSQL-compatible **Slotted Page Layout** in `jarvix-storage`:
- Page size fixed at **8192 bytes** (8 KB).
- A 24-byte header containing `lsn`, `page_id`, `page_type`, `lower`, `upper`, `special`, and `checksum`.
- A slot directory where each slot (`ItemId`) contains 6 bytes: `offset: u16`, `length: u16`, and `flags: u16`.
- Tuples are allocated from `upper` downward, and slots grow from `lower` upward.
- Contiguous free space is tracked as `upper - lower`.
- In-place compaction reorganizes surviving tuples and reclaims fragmented dead space without altering assigned `SlotId`s.

## Consequences
### Positive
- Efficiently packs variable-length records with zero external fragmentation.
- Stable `RecordId` addressing: external secondary B+Tree indexes reference `(page_id, slot_id)`, meaning internal page tuple shifts during deletion do not invalidate index pointers.
- Alignment with PostgreSQL page semantics allows predictable I/O patterns and straightforward testing against reference database behaviors.

### Trade-offs
- Adding or modifying variable-length tuples requires bound checks and occasional page compaction routines.
- Extra memory overhead of 6 bytes per tuple for the slot pointer.
