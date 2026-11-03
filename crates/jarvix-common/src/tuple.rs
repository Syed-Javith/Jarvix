use serde::{Deserialize, Serialize};
use crate::error::{JarvixError, Result};
use crate::types::{RecordId, Value};

/// Relational Tuple representing an in-memory row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tuple {
    pub values: Vec<Value>,
    pub rid: Option<RecordId>,
}

impl Tuple {
    pub fn new(values: Vec<Value>) -> Self {
        Self { values, rid: None }
    }

    pub fn with_rid(values: Vec<Value>, rid: RecordId) -> Self {
        Self {
            values,
            rid: Some(rid),
        }
    }

    pub fn value(&self, idx: usize) -> Option<&Value> {
        self.values.get(idx)
    }

    /// Serializes tuple values to binary format.
    pub fn encode(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(&self.values)
            .map_err(|e| JarvixError::Storage(format!("Tuple serialization failed: {}", e)))
    }

    /// Deserializes binary representation into a Tuple.
    pub fn decode(bytes: &[u8], rid: Option<RecordId>) -> Result<Self> {
        let values: Vec<Value> = serde_json::from_slice(bytes)
            .map_err(|e| JarvixError::Storage(format!("Tuple deserialization failed: {}", e)))?;
        Ok(Self { values, rid })
    }
}
