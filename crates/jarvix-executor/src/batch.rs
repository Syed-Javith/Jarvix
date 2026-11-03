use jarvix_common::tuple::Tuple;

pub const DEFAULT_BATCH_SIZE: usize = 1024;

/// A vectorized batch of tuples for cache-friendly query processing.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorBatch {
    tuples: Vec<Tuple>,
}

impl VectorBatch {
    pub fn new() -> Self {
        Self {
            tuples: Vec::with_capacity(DEFAULT_BATCH_SIZE),
        }
    }

    pub fn with_tuples(tuples: Vec<Tuple>) -> Self {
        Self { tuples }
    }

    pub fn push(&mut self, tuple: Tuple) {
        self.tuples.push(tuple);
    }

    pub fn is_empty(&self) -> bool {
        self.tuples.is_empty()
    }

    pub fn len(&self) -> usize {
        self.tuples.len()
    }

    pub fn tuples(&self) -> &[Tuple] {
        &self.tuples
    }

    pub fn into_tuples(self) -> Vec<Tuple> {
        self.tuples
    }
}

impl Default for VectorBatch {
    fn default() -> Self {
        Self::new()
    }
}
