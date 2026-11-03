use thiserror::Error;

pub type Result<T> = std::result::Result<T, JarvixError>;

#[derive(Error, Debug)]
pub enum JarvixError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Page error: {0}")]
    Page(String),

    #[error("Buffer pool error: {0}")]
    BufferPool(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Transaction error: {0}")]
    Transaction(String),

    #[error("Concurrency/Lock error: {0}")]
    Lock(String),

    #[error("Deadlock detected for transaction {0}")]
    Deadlock(u64),

    #[error("WAL/Recovery error: {0}")]
    Wal(String),

    #[error("Catalog error: {0}")]
    Catalog(String),

    #[error("Index error: {0}")]
    Index(String),

    #[error("SQL syntax error: {0}")]
    Syntax(String),

    #[error("Semantic error: {0}")]
    Semantic(String),

    #[error("Execution error: {0}")]
    Execution(String),

    #[error("Network protocol error: {0}")]
    Protocol(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Internal error: {0}")]
    Internal(String),
}
