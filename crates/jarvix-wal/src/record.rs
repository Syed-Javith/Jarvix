use byteorder::{BigEndian, ByteOrder};

use jarvix_common::constants::{INVALID_LSN, INVALID_PAGE_ID, INVALID_SLOT_ID};
use jarvix_common::error::{JarvixError, Result};
use jarvix_common::types::{Lsn, PageId, SlotId, TxnId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LogRecordType {
    Invalid = 0,
    Begin = 1,
    Commit = 2,
    Abort = 3,
    Insert = 4,
    Update = 5,
    Delete = 6,
    Clr = 7, // Compensation Log Record (for undo during recovery)
}

impl From<u8> for LogRecordType {
    fn from(val: u8) -> Self {
        match val {
            1 => LogRecordType::Begin,
            2 => LogRecordType::Commit,
            3 => LogRecordType::Abort,
            4 => LogRecordType::Insert,
            5 => LogRecordType::Update,
            6 => LogRecordType::Delete,
            7 => LogRecordType::Clr,
            _ => LogRecordType::Invalid,
        }
    }
}

/// A physical/physiological WAL record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRecord {
    pub lsn: Lsn,
    pub prev_lsn: Lsn,
    pub txn_id: TxnId,
    pub record_type: LogRecordType,
    pub page_id: PageId,
    pub slot_id: SlotId,
    pub before_image: Vec<u8>,
    pub after_image: Vec<u8>,
    pub undo_next_lsn: Lsn, // Used by CLR to bypass already undone records
}

impl LogRecord {
    pub fn new_begin(txn_id: TxnId, prev_lsn: Lsn) -> Self {
        Self {
            lsn: INVALID_LSN,
            prev_lsn,
            txn_id,
            record_type: LogRecordType::Begin,
            page_id: INVALID_PAGE_ID,
            slot_id: INVALID_SLOT_ID,
            before_image: Vec::new(),
            after_image: Vec::new(),
            undo_next_lsn: INVALID_LSN,
        }
    }

    pub fn new_commit(txn_id: TxnId, prev_lsn: Lsn) -> Self {
        Self {
            lsn: INVALID_LSN,
            prev_lsn,
            txn_id,
            record_type: LogRecordType::Commit,
            page_id: INVALID_PAGE_ID,
            slot_id: INVALID_SLOT_ID,
            before_image: Vec::new(),
            after_image: Vec::new(),
            undo_next_lsn: INVALID_LSN,
        }
    }

    pub fn new_abort(txn_id: TxnId, prev_lsn: Lsn) -> Self {
        Self {
            lsn: INVALID_LSN,
            prev_lsn,
            txn_id,
            record_type: LogRecordType::Abort,
            page_id: INVALID_PAGE_ID,
            slot_id: INVALID_SLOT_ID,
            before_image: Vec::new(),
            after_image: Vec::new(),
            undo_next_lsn: INVALID_LSN,
        }
    }

    pub fn new_insert(txn_id: TxnId, prev_lsn: Lsn, page_id: PageId, slot_id: SlotId, data: Vec<u8>) -> Self {
        Self {
            lsn: INVALID_LSN,
            prev_lsn,
            txn_id,
            record_type: LogRecordType::Insert,
            page_id,
            slot_id,
            before_image: Vec::new(),
            after_image: data,
            undo_next_lsn: INVALID_LSN,
        }
    }

    pub fn new_update(
        txn_id: TxnId,
        prev_lsn: Lsn,
        page_id: PageId,
        slot_id: SlotId,
        before: Vec<u8>,
        after: Vec<u8>,
    ) -> Self {
        Self {
            lsn: INVALID_LSN,
            prev_lsn,
            txn_id,
            record_type: LogRecordType::Update,
            page_id,
            slot_id,
            before_image: before,
            after_image: after,
            undo_next_lsn: INVALID_LSN,
        }
    }

    pub fn new_delete(txn_id: TxnId, prev_lsn: Lsn, page_id: PageId, slot_id: SlotId, deleted_data: Vec<u8>) -> Self {
        Self {
            lsn: INVALID_LSN,
            prev_lsn,
            txn_id,
            record_type: LogRecordType::Delete,
            page_id,
            slot_id,
            before_image: deleted_data,
            after_image: Vec::new(),
            undo_next_lsn: INVALID_LSN,
        }
    }

    pub fn new_clr(txn_id: TxnId, prev_lsn: Lsn, page_id: PageId, slot_id: SlotId, data: Vec<u8>, undo_next: Lsn) -> Self {
        Self {
            lsn: INVALID_LSN,
            prev_lsn,
            txn_id,
            record_type: LogRecordType::Clr,
            page_id,
            slot_id,
            before_image: Vec::new(),
            after_image: data,
            undo_next_lsn: undo_next,
        }
    }

    /// Serializes the log record to raw bytes.
    /// Format:
    /// - total_len: u32 (includes total_len itself)
    /// - lsn: u64
    /// - prev_lsn: u64
    /// - txn_id: u64
    /// - record_type: u8
    /// - page_id: u32
    /// - slot_id: u16
    /// - undo_next_lsn: u64
    /// - before_len: u32, before_bytes
    /// - after_len: u32, after_bytes
    pub fn serialize(&self, buf: &mut Vec<u8>) {
        let before_len = self.before_image.len() as u32;
        let after_len = self.after_image.len() as u32;
        let fixed_header_len = 4 + 8 + 8 + 8 + 1 + 4 + 2 + 8 + 4 + 4;
        let total_len = fixed_header_len + before_len as usize + after_len as usize;

        let mut header = [0u8; 43];
        BigEndian::write_u32(&mut header[0..4], total_len as u32);
        BigEndian::write_u64(&mut header[4..12], self.lsn);
        BigEndian::write_u64(&mut header[12..20], self.prev_lsn);
        BigEndian::write_u64(&mut header[20..28], self.txn_id);
        header[28] = self.record_type as u8;
        BigEndian::write_u32(&mut header[29..33], self.page_id);
        BigEndian::write_u16(&mut header[33..35], self.slot_id);
        BigEndian::write_u64(&mut header[35..43], self.undo_next_lsn);

        buf.extend_from_slice(&header);

        let mut lens = [0u8; 8];
        BigEndian::write_u32(&mut lens[0..4], before_len);
        BigEndian::write_u32(&mut lens[4..8], after_len);
        buf.extend_from_slice(&lens);

        buf.extend_from_slice(&self.before_image);
        buf.extend_from_slice(&self.after_image);
    }

    /// Deserializes a single log record from the slice.
    /// Returns (LogRecord, bytes_consumed).
    pub fn deserialize(data: &[u8]) -> Result<(Self, usize)> {
        if data.len() < 51 {
            return Err(JarvixError::Wal("Incomplete WAL record header".to_string()));
        }

        let total_len = BigEndian::read_u32(&data[0..4]) as usize;
        if data.len() < total_len {
            return Err(JarvixError::Wal("Incomplete WAL record body".to_string()));
        }

        let lsn = BigEndian::read_u64(&data[4..12]);
        let prev_lsn = BigEndian::read_u64(&data[12..20]);
        let txn_id = BigEndian::read_u64(&data[20..28]);
        let record_type = LogRecordType::from(data[28]);
        let page_id = BigEndian::read_u32(&data[29..33]);
        let slot_id = BigEndian::read_u16(&data[33..35]);
        let undo_next_lsn = BigEndian::read_u64(&data[35..43]);

        let before_len = BigEndian::read_u32(&data[43..47]) as usize;
        let after_len = BigEndian::read_u32(&data[47..51]) as usize;

        let before_start = 51;
        let before_end = before_start + before_len;
        let after_start = before_end;
        let after_end = after_start + after_len;

        if after_end > total_len {
            return Err(JarvixError::Wal("Corrupted WAL record length".to_string()));
        }

        let before_image = data[before_start..before_end].to_vec();
        let after_image = data[after_start..after_end].to_vec();

        Ok((
            Self {
                lsn,
                prev_lsn,
                txn_id,
                record_type,
                page_id,
                slot_id,
                before_image,
                after_image,
                undo_next_lsn,
            },
            total_len,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_record_serde() {
        let mut rec = LogRecord::new_update(
            100,
            50,
            1,
            2,
            b"before_data".to_vec(),
            b"after_data".to_vec(),
        );
        rec.lsn = 12345;

        let mut buf = Vec::new();
        rec.serialize(&mut buf);

        let (deserialized, consumed) = LogRecord::deserialize(&buf).unwrap();
        assert_eq!(consumed, buf.len());
        assert_eq!(deserialized, rec);
    }
}
