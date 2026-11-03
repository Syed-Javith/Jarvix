use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use jarvix_common::error::{JarvixError, Result};
use jarvix_common::types::Lsn;

use crate::record::LogRecord;

/// WalManager manages the append-only Write-Ahead Log file and group commit synchronization.
pub struct WalManager {
    wal_path: PathBuf,
    file: Mutex<File>,
    buffer: Mutex<Vec<u8>>,
    next_lsn: AtomicU64,
    flushed_lsn: AtomicU64,
}

impl WalManager {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let wal_path = path.as_ref().to_path_buf();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&wal_path)?;

        let initial_lsn = file.metadata()?.len();

        Ok(Self {
            wal_path,
            file: Mutex::new(file),
            buffer: Mutex::new(Vec::with_capacity(64 * 1024)),
            next_lsn: AtomicU64::new(initial_lsn + 1),
            flushed_lsn: AtomicU64::new(initial_lsn),
        })
    }

    pub fn current_lsn(&self) -> Lsn {
        self.next_lsn.load(Ordering::Acquire)
    }

    pub fn flushed_lsn(&self) -> Lsn {
        self.flushed_lsn.load(Ordering::Acquire)
    }

    /// Appends a log record to the in-memory WAL buffer and assigns an LSN.
    pub fn append_record(&self, mut rec: LogRecord) -> Result<Lsn> {
        let mut temp_buf = Vec::new();
        rec.serialize(&mut temp_buf);
        let rec_len = temp_buf.len() as u64;

        let assigned_lsn = self.next_lsn.fetch_add(rec_len, Ordering::SeqCst);
        rec.lsn = assigned_lsn;

        // Re-serialize with assigned LSN
        let mut final_buf = Vec::with_capacity(rec_len as usize);
        rec.serialize(&mut final_buf);

        let mut buffer = self.buffer.lock().map_err(|e| JarvixError::Wal(e.to_string()))?;
        buffer.extend_from_slice(&final_buf);

        Ok(assigned_lsn)
    }

    /// Flushes WAL buffer to disk up to at least `target_lsn` (Group Commit).
    pub fn flush(&self, target_lsn: Lsn) -> Result<()> {
        if self.flushed_lsn() >= target_lsn {
            return Ok(());
        }

        let mut buffer = self.buffer.lock().map_err(|e| JarvixError::Wal(e.to_string()))?;
        let mut file = self.file.lock().map_err(|e| JarvixError::Wal(e.to_string()))?;

        if !buffer.is_empty() {
            file.seek(SeekFrom::End(0))?;
            file.write_all(&buffer)?;
            file.sync_data()?;
            buffer.clear();
        }

        self.flushed_lsn.store(self.current_lsn() - 1, Ordering::Release);
        Ok(())
    }

    /// Reads all valid log records sequentially from the WAL file.
    pub fn read_all_records(&self) -> Result<Vec<LogRecord>> {
        let mut file = self.file.lock().map_err(|e| JarvixError::Wal(e.to_string()))?;
        file.seek(SeekFrom::Start(0))?;

        let mut data = Vec::new();
        file.read_to_end(&mut data)?;

        let mut records = Vec::new();
        let mut offset = 0;

        while offset < data.len() {
            match LogRecord::deserialize(&data[offset..]) {
                Ok((rec, consumed)) => {
                    records.push(rec);
                    offset += consumed;
                }
                Err(_) => break, // Corrupted or incomplete record at tail
            }
        }

        Ok(records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempWal(PathBuf);
    impl TempWal {
        fn new(name: &str) -> Self {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let path = std::env::temp_dir().join(format!("jarvix_wal_{}_{}.wal", name, nanos));
            let _ = std::fs::remove_file(&path);
            Self(path)
        }
    }
    impl Drop for TempWal {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn test_wal_append_and_flush() {
        let temp = TempWal::new("append");
        let wal = WalManager::open(&temp.0).unwrap();

        let lsn1 = wal.append_record(LogRecord::new_begin(1, 0)).unwrap();
        let lsn2 = wal.append_record(LogRecord::new_insert(1, lsn1, 10, 0, b"tuple_1".to_vec())).unwrap();
        let lsn3 = wal.append_record(LogRecord::new_commit(1, lsn2)).unwrap();

        assert!(lsn2 > lsn1);
        assert!(lsn3 > lsn2);

        wal.flush(lsn3).unwrap();
        assert!(wal.flushed_lsn() >= lsn3);

        let records = wal.read_all_records().unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].record_type, crate::record::LogRecordType::Begin);
        assert_eq!(records[1].record_type, crate::record::LogRecordType::Insert);
        assert_eq!(records[2].record_type, crate::record::LogRecordType::Commit);
    }
}
