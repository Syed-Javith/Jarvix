use jarvix_common::error::Result;
use jarvix_common::schema::Schema;

use crate::batch::VectorBatch;
use crate::executors::Executor;

pub struct LimitExecutor {
    limit: usize,
    child: Box<dyn Executor>,
    emitted: usize,
}

impl LimitExecutor {
    pub fn new(limit: usize, child: Box<dyn Executor>) -> Self {
        Self {
            limit,
            child,
            emitted: 0,
        }
    }
}

impl Executor for LimitExecutor {
    fn init(&mut self) -> Result<()> {
        self.child.init()?;
        self.emitted = 0;
        Ok(())
    }

    fn next(&mut self) -> Result<Option<VectorBatch>> {
        if self.emitted >= self.limit {
            return Ok(None);
        }

        match self.child.next()? {
            Some(batch) => {
                let remaining = self.limit - self.emitted;
                let tuples = batch.into_tuples();
                let take_count = tuples.len().min(remaining);
                self.emitted += take_count;

                let limited_tuples: Vec<_> = tuples.into_iter().take(take_count).collect();
                Ok(Some(VectorBatch::with_tuples(limited_tuples)))
            }
            None => Ok(None),
        }
    }

    fn schema(&self) -> &Schema {
        self.child.schema()
    }
}
