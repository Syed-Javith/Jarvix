use std::collections::HashMap;
use std::sync::Arc;

use jarvix_common::constants::INVALID_PAGE_ID;
use jarvix_common::error::Result;
use jarvix_common::types::{Lsn, TxnId};
use jarvix_buffer::BufferPoolManager;

use crate::manager::WalManager;
use crate::record::{LogRecord, LogRecordType};

/// ARIES Crash Recovery Manager.
pub struct RecoveryManager {
    wal_manager: Arc<WalManager>,
    bpm: BufferPoolManager,
}

impl RecoveryManager {
    pub fn new(wal_manager: Arc<WalManager>, bpm: BufferPoolManager) -> Self {
        Self { wal_manager, bpm }
    }

    /// Performs complete 3-phase ARIES recovery: Analysis, Redo, and Undo.
    pub fn recover(&self) -> Result<()> {
        let records = self.wal_manager.read_all_records()?;
        if records.is_empty() {
            return Ok(());
        }

        // ==========================================
        // Phase 1: Analysis
        // ==========================================
        let mut active_txns: HashMap<TxnId, Lsn> = HashMap::new();

        for rec in &records {
            match rec.record_type {
                LogRecordType::Begin => {
                    active_txns.insert(rec.txn_id, rec.lsn);
                }
                LogRecordType::Commit | LogRecordType::Abort => {
                    active_txns.remove(&rec.txn_id);
                }
                LogRecordType::Insert
                | LogRecordType::Update
                | LogRecordType::Delete
                | LogRecordType::Clr => {
                    active_txns.insert(rec.txn_id, rec.lsn);
                }
                _ => {}
            }
        }

        // ==========================================
        // Phase 2: Redo (Repeat History)
        // ==========================================
        for rec in &records {
            if rec.page_id == INVALID_PAGE_ID {
                continue;
            }

            match rec.record_type {
                LogRecordType::Insert | LogRecordType::Clr => {
                    let mut guard = self.bpm.fetch_page_mut(rec.page_id)?;
                    if guard.lsn() < rec.lsn {
                        let _ = guard.insert_tuple(&rec.after_image);
                        guard.set_lsn(rec.lsn);
                    }
                }
                LogRecordType::Update => {
                    let mut guard = self.bpm.fetch_page_mut(rec.page_id)?;
                    if guard.lsn() < rec.lsn {
                        // In-place update or re-insert
                        let _ = guard.delete_tuple(rec.slot_id);
                        let _ = guard.insert_tuple(&rec.after_image);
                        guard.set_lsn(rec.lsn);
                    }
                }
                LogRecordType::Delete => {
                    let mut guard = self.bpm.fetch_page_mut(rec.page_id)?;
                    if guard.lsn() < rec.lsn {
                        let _ = guard.delete_tuple(rec.slot_id);
                        guard.set_lsn(rec.lsn);
                    }
                }
                _ => {}
            }
        }

        // ==========================================
        // Phase 3: Undo (Rollback Active Transactions)
        // ==========================================
        // Undo active transactions in reverse chronological order
        for rec in records.iter().rev() {
            if active_txns.contains_key(&rec.txn_id) {
                if rec.page_id == INVALID_PAGE_ID {
                    continue;
                }

                match rec.record_type {
                    LogRecordType::Insert => {
                        // Undo Insert: delete the inserted tuple
                        let mut guard = self.bpm.fetch_page_mut(rec.page_id)?;
                        let _ = guard.delete_tuple(rec.slot_id);

                        let clr = LogRecord::new_clr(
                            rec.txn_id,
                            rec.lsn,
                            rec.page_id,
                            rec.slot_id,
                            Vec::new(),
                            rec.prev_lsn,
                        );
                        let clr_lsn = self.wal_manager.append_record(clr)?;
                        guard.set_lsn(clr_lsn);
                    }
                    LogRecordType::Update => {
                        // Undo Update: restore before-image
                        let mut guard = self.bpm.fetch_page_mut(rec.page_id)?;
                        let _ = guard.delete_tuple(rec.slot_id);
                        let _ = guard.insert_tuple(&rec.before_image);

                        let clr = LogRecord::new_clr(
                            rec.txn_id,
                            rec.lsn,
                            rec.page_id,
                            rec.slot_id,
                            rec.before_image.clone(),
                            rec.prev_lsn,
                        );
                        let clr_lsn = self.wal_manager.append_record(clr)?;
                        guard.set_lsn(clr_lsn);
                    }
                    LogRecordType::Delete => {
                        // Undo Delete: restore deleted data
                        let mut guard = self.bpm.fetch_page_mut(rec.page_id)?;
                        let _ = guard.insert_tuple(&rec.before_image);

                        let clr = LogRecord::new_clr(
                            rec.txn_id,
                            rec.lsn,
                            rec.page_id,
                            rec.slot_id,
                            rec.before_image.clone(),
                            rec.prev_lsn,
                        );
                        let clr_lsn = self.wal_manager.append_record(clr)?;
                        guard.set_lsn(clr_lsn);
                    }
                    _ => {}
                }
            }
        }

        // Write Abort log record for each rolled-back transaction
        for (&txn_id, &last_lsn) in active_txns.iter() {
            let abort_rec = LogRecord::new_abort(txn_id, last_lsn);
            let abort_lsn = self.wal_manager.append_record(abort_rec)?;
            self.wal_manager.flush(abort_lsn)?;
        }

        // Flush all dirty pages to ensure recovery state is durable on disk
        self.bpm.flush_all()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jarvix_storage::{DiskManager, PageType};

    struct TestEnv {
        db_path: std::path::PathBuf,
        wal_path: std::path::PathBuf,
    }

    impl TestEnv {
        fn new(name: &str) -> Self {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let db_path = std::env::temp_dir().join(format!("jarvix_rec_db_{}_{}.db", name, nanos));
            let wal_path = std::env::temp_dir().join(format!("jarvix_rec_wal_{}_{}.wal", name, nanos));
            let _ = std::fs::remove_file(&db_path);
            let _ = std::fs::remove_file(&wal_path);
            Self { db_path, wal_path }
        }
    }

    impl Drop for TestEnv {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.db_path);
            let _ = std::fs::remove_file(&self.wal_path);
        }
    }

    #[test]
    fn test_recovery_committed_and_uncommitted_txns() {
        let env = TestEnv::new("crash");
        let disk_mgr = Arc::new(DiskManager::open(&env.db_path).unwrap());
        let bpm = BufferPoolManager::new(10, Arc::clone(&disk_mgr));
        let wal_mgr = Arc::new(WalManager::open(&env.wal_path).unwrap());

        // Allocate a page
        let (p1, _) = bpm.new_page(PageType::Heap).unwrap();

        // Transaction 1 (Committed): Inserts "Committed Tuple"
        let l1 = wal_mgr.append_record(LogRecord::new_begin(1, 0)).unwrap();
        let l2 = wal_mgr.append_record(LogRecord::new_insert(1, l1, p1, 0, b"Committed Tuple".to_vec())).unwrap();
        let l3 = wal_mgr.append_record(LogRecord::new_commit(1, l2)).unwrap();
        wal_mgr.flush(l3).unwrap();

        // Transaction 2 (Uncommitted - crashed mid-flight): Inserts "Uncommitted Tuple"
        let l4 = wal_mgr.append_record(LogRecord::new_begin(2, 0)).unwrap();
        let l5 = wal_mgr.append_record(LogRecord::new_insert(2, l4, p1, 1, b"Uncommitted Tuple".to_vec())).unwrap();
        wal_mgr.flush(l5).unwrap();

        // Simulate crash: Re-open DB and run RecoveryManager
        let recovery_mgr = RecoveryManager::new(Arc::clone(&wal_mgr), bpm.clone());
        recovery_mgr.recover().unwrap();

        // Verify: Page p1 must have "Committed Tuple" and NOT have "Uncommitted Tuple"
        let guard = bpm.fetch_page(p1).unwrap();
        assert_eq!(guard.get_tuple(0), Some(b"Committed Tuple".as_slice()));
        assert_eq!(guard.get_tuple(1), None); // slot 1 deleted by Undo
    }
}
