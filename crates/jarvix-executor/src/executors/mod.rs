pub mod ddl;
pub mod delete;
pub mod index_scan;
pub mod insert;
pub mod limit;
pub mod projection;
pub mod seq_scan;
pub mod update;

use jarvix_common::error::Result;
use jarvix_common::schema::Schema;
use crate::batch::VectorBatch;

/// Abstract Volcano / Iterator execution node producing vectorized batches.
pub trait Executor {
    /// Initialize the executor state, acquire necessary locks, and open underlying cursors.
    fn init(&mut self) -> Result<()>;

    /// Fetches the next vector batch of tuples. Returns Ok(None) when exhausted.
    fn next(&mut self) -> Result<Option<VectorBatch>>;

    /// Returns the schema of the tuples produced by this executor.
    fn schema(&self) -> &Schema;
}
