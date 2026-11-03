use serde::{Deserialize, Serialize};
use std::fmt;

use crate::constants::{INVALID_PAGE_ID, INVALID_SLOT_ID};

/// Unique 32-bit identifier for a physical database page.
pub type PageId = u32;

/// 16-bit slot index within a slotted page.
pub type SlotId = u16;

/// 64-bit Log Sequence Number representing WAL byte offset.
pub type Lsn = u64;

/// 64-bit monotonic Transaction ID.
pub type TxnId = u64;

/// 32-bit Object Identifier for catalog entities (tables, indexes, columns).
pub type Oid = u32;

/// Physical pointer identifying a tuple's exact location on disk: (PageId, SlotId).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RecordId {
    pub page_id: PageId,
    pub slot_id: SlotId,
}

impl RecordId {
    pub const INVALID: Self = Self {
        page_id: INVALID_PAGE_ID,
        slot_id: INVALID_SLOT_ID,
    };

    pub fn new(page_id: PageId, slot_id: SlotId) -> Self {
        Self { page_id, slot_id }
    }

    pub fn is_valid(&self) -> bool {
        self.page_id != INVALID_PAGE_ID && self.slot_id != INVALID_SLOT_ID
    }
}

impl fmt::Display for RecordId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({}, {})", self.page_id, self.slot_id)
    }
}

/// Supported relational data types in Jarvix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DataType {
    Boolean,
    TinyInt,
    SmallInt,
    Integer,
    BigInt,
    Float,
    Double,
    Varchar(u32),
    Text,
    Timestamp,
    Null,
}

/// Runtime representation of a single typed value in a tuple/column.
#[derive(Debug, Clone, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum Value {
    Null,
    Boolean(bool),
    TinyInt(i8),
    SmallInt(i16),
    Integer(i32),
    BigInt(i64),
    Float(f32),
    Double(f64),
    Text(String),
    Timestamp(i64), // Microseconds since UNIX epoch
}

impl Value {
    pub fn data_type(&self) -> DataType {
        match self {
            Value::Null => DataType::Null,
            Value::Boolean(_) => DataType::Boolean,
            Value::TinyInt(_) => DataType::TinyInt,
            Value::SmallInt(_) => DataType::SmallInt,
            Value::Integer(_) => DataType::Integer,
            Value::BigInt(_) => DataType::BigInt,
            Value::Float(_) => DataType::Float,
            Value::Double(_) => DataType::Double,
            Value::Text(_) => DataType::Text,
            Value::Timestamp(_) => DataType::Timestamp,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "NULL"),
            Value::Boolean(b) => write!(f, "{}", b),
            Value::TinyInt(i) => write!(f, "{}", i),
            Value::SmallInt(i) => write!(f, "{}", i),
            Value::Integer(i) => write!(f, "{}", i),
            Value::BigInt(i) => write!(f, "{}", i),
            Value::Float(v) => write!(f, "{}", v),
            Value::Double(v) => write!(f, "{}", v),
            Value::Text(s) => write!(f, "{}", s),
            Value::Timestamp(t) => write!(f, "{}", t),
        }
    }
}
