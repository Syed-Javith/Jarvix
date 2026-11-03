//! Write-Ahead Logging (WAL) and ARIES Recovery subsystem for Jarvix.

pub mod manager;
pub mod record;
pub mod recovery;

pub use manager::WalManager;
pub use record::{LogRecord, LogRecordType};
pub use recovery::RecoveryManager;
