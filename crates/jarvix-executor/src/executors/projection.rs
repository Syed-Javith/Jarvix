use jarvix_common::error::Result;
use jarvix_common::schema::Schema;
use jarvix_common::tuple::Tuple;

use crate::batch::VectorBatch;
use crate::executors::Executor;

pub struct ProjectionExecutor {
    projection_indices: Vec<usize>,
    child: Box<dyn Executor>,
    schema: Schema,
}

impl ProjectionExecutor {
    pub fn new(projection_indices: Vec<usize>, child: Box<dyn Executor>) -> Self {
        Self {
            projection_indices,
            child,
            schema: Schema::new(vec![]),
        }
    }
}

impl Executor for ProjectionExecutor {
    fn init(&mut self) -> Result<()> {
        self.child.init()?;
        let child_schema = self.child.schema();

        let mut projected_cols = Vec::with_capacity(self.projection_indices.len());
        for &idx in &self.projection_indices {
            let col = child_schema.column(idx).cloned().unwrap();
            projected_cols.push(col);
        }

        self.schema = Schema::new(projected_cols);
        Ok(())
    }

    fn next(&mut self) -> Result<Option<VectorBatch>> {
        match self.child.next()? {
            Some(batch) => {
                let mut projected_batch = VectorBatch::new();
                for tuple in batch.tuples() {
                    let mut values = Vec::with_capacity(self.projection_indices.len());
                    for &idx in &self.projection_indices {
                        values.push(tuple.value(idx).cloned().unwrap());
                    }
                    let projected_tuple = match tuple.rid {
                        Some(rid) => Tuple::with_rid(values, rid),
                        None => Tuple::new(values),
                    };
                    projected_batch.push(projected_tuple);
                }
                Ok(Some(projected_batch))
            }
            None => Ok(None),
        }
    }

    fn schema(&self) -> &Schema {
        &self.schema
    }
}
