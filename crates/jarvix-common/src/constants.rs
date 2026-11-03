//! Core physical and logical constants for Jarvix.

/// Physical database page size (8 KB, identical to PostgreSQL default).
pub const PAGE_SIZE: usize = 8192;

/// Sentinel invalid Page ID.
pub const INVALID_PAGE_ID: u32 = u32::MAX;

/// Sentinel invalid Slot ID within a slotted page.
pub const INVALID_SLOT_ID: u16 = u16::MAX;

/// Sentinel invalid Log Sequence Number (LSN).
pub const INVALID_LSN: u64 = 0;

/// Sentinel invalid Transaction ID.
pub const INVALID_TXN_ID: u64 = 0;

/// Magic bytes for Jarvix database file headers: "JARV" (0x4A, 0x41, 0x52, 0x56)
pub const DB_FILE_MAGIC: [u8; 4] = [0x4A, 0x41, 0x52, 0x56];

/// Current file format version.
pub const DB_FORMAT_VERSION: u32 = 1;
