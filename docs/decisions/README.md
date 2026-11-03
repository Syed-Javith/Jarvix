# Architecture Decision Records (ADRs)

This directory documents the key architectural decisions made during the design and implementation of JavixDB.

Each ADR follows the standard format:
- **Title**: Context and number of the decision.
- **Status**: Proposed, Accepted, Deprecated, or Superseded.
- **Context**: The engineering problem, trade-offs, and reference database approaches (PostgreSQL vs MySQL/InnoDB).
- **Decision**: The selected architectural approach implemented in Rust.
- **Consequences**: The positive benefits and technical trade-offs of this decision.

## Index of Decisions

- [ADR-001: Slotted Page Storage Architecture](ADR-001-slotted-page-storage.md)
- [ADR-002: Undo-Log-Based Multi-Version Concurrency Control (MVCC)](ADR-002-undo-log-mvcc.md)
- [ADR-003: Strict Two-Phase Locking (SS2PL) with Wait-For Graph Deadlock Detection](ADR-003-strict-2pl-deadlock-detection.md)
- [ADR-004: Vectorized Volcano Iterator Model for Query Execution](ADR-004-vectorized-volcano-execution.md)
