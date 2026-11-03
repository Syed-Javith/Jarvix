use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use jarvix_common::constants::INVALID_LSN;
use jarvix_common::error::{JarvixError, Result};
use jarvix_common::types::{Lsn, TxnId};
use jarvix_wal::{LogRecord, WalManager};

use crate::lock_manager::{LockKey, LockManager};
use crate::mvcc::{IsolationLevel, ReadView, TransactionState};
use crate::undo::UndoLogManager;

/// A runtime Transaction handle.
pub struct Transaction {
    txn_id: TxnId,
    isolation_level: IsolationLevel,
    state: RwLock<TransactionState>,
    prev_lsn: AtomicU64,
    read_view: RwLock<Option<ReadView>>,
    held_locks: Mutex<HashSet<LockKey>>,
}

impl Transaction {
    pub fn new(txn_id: TxnId, isolation_level: IsolationLevel) -> Self {
        Self {
            txn_id,
            isolation_level,
            state: RwLock::new(TransactionState::Active),
            prev_lsn: AtomicU64::new(INVALID_LSN),
            read_view: RwLock::new(None),
            held_locks: Mutex::new(HashSet::new()),
        }
    }

    pub fn txn_id(&self) -> TxnId {
        self.txn_id
    }

    pub fn isolation_level(&self) -> IsolationLevel {
        self.isolation_level
    }

    pub fn state(&self) -> TransactionState {
        *self.state.read().unwrap()
    }

    pub fn prev_lsn(&self) -> Lsn {
        self.prev_lsn.load(Ordering::Acquire)
    }

    pub fn set_prev_lsn(&self, lsn: Lsn) {
        self.prev_lsn.store(lsn, Ordering::Release);
    }

    pub fn read_view(&self) -> Option<ReadView> {
        self.read_view.read().unwrap().clone()
    }

    pub fn set_read_view(&self, view: ReadView) {
        *self.read_view.write().unwrap() = Some(view);
    }

    pub fn record_lock(&self, key: LockKey) {
        self.held_locks.lock().unwrap().insert(key);
    }
}

/// TransactionManager coordinates ACID transactions, MVCC read views, and 2PL locking.
pub struct TransactionManager {
    next_txn_id: AtomicU64,
    active_txns: Mutex<HashMap<TxnId, Arc<Transaction>>>,
    wal_manager: Arc<WalManager>,
    lock_manager: Arc<LockManager>,
    undo_manager: Arc<UndoLogManager>,
}

impl TransactionManager {
    pub fn new(
        wal_manager: Arc<WalManager>,
        lock_manager: Arc<LockManager>,
        undo_manager: Arc<UndoLogManager>,
    ) -> Self {
        Self {
            next_txn_id: AtomicU64::new(1),
            active_txns: Mutex::new(HashMap::new()),
            wal_manager,
            lock_manager,
            undo_manager,
        }
    }

    pub fn lock_manager(&self) -> &Arc<LockManager> {
        &self.lock_manager
    }

    pub fn undo_manager(&self) -> &Arc<UndoLogManager> {
        &self.undo_manager
    }

    pub fn get_transaction(&self, txn_id: TxnId) -> Option<Arc<Transaction>> {
        self.active_txns.lock().unwrap().get(&txn_id).cloned()
    }

    pub fn commit_by_id(&self, txn_id: TxnId) -> Result<()> {
        let txn = self.get_transaction(txn_id).ok_or_else(|| {
            JarvixError::Transaction(format!("Transaction {} not active", txn_id))
        })?;
        self.commit(&txn)
    }

    pub fn abort_by_id(&self, txn_id: TxnId) -> Result<()> {
        let txn = self.get_transaction(txn_id).ok_or_else(|| {
            JarvixError::Transaction(format!("Transaction {} not active", txn_id))
        })?;
        self.abort(&txn)
    }

    /// Begins a new transaction, records a Begin WAL record, and assigns an MVCC ReadView.
    pub fn begin(&self, isolation_level: IsolationLevel) -> Result<Arc<Transaction>> {
        let txn_id = self.next_txn_id.fetch_add(1, Ordering::SeqCst);
        let txn = Arc::new(Transaction::new(txn_id, isolation_level));

        // Append Begin record to WAL
        let begin_rec = LogRecord::new_begin(txn_id, INVALID_LSN);
        let begin_lsn = self.wal_manager.append_record(begin_rec)?;
        txn.set_prev_lsn(begin_lsn);

        // Snapshot active transactions for MVCC ReadView
        let mut active_map = self.active_txns.lock().unwrap();
        let active_set: HashSet<TxnId> = active_map.keys().cloned().collect();
        let high_limit_id = txn_id + 1;

        let read_view = ReadView::new(txn_id, active_set, high_limit_id);
        txn.set_read_view(read_view);

        active_map.insert(txn_id, Arc::clone(&txn));
        Ok(txn)
    }

    /// Commits a transaction: writes Commit WAL record, flushes WAL (durability), and releases locks.
    pub fn commit(&self, txn: &Transaction) -> Result<()> {
        let mut state = txn.state.write().unwrap();
        if *state != TransactionState::Active {
            return Err(JarvixError::Transaction("Transaction is not in active state".to_string()));
        }

        // Write Commit record to WAL and enforce flush (Group Commit)
        let commit_rec = LogRecord::new_commit(txn.txn_id, txn.prev_lsn());
        let commit_lsn = self.wal_manager.append_record(commit_rec)?;
        self.wal_manager.flush(commit_lsn)?;

        // Release all 2PL locks
        self.lock_manager.release_all_locks(txn.txn_id);

        *state = TransactionState::Committed;
        self.active_txns.lock().unwrap().remove(&txn.txn_id);

        Ok(())
    }

    /// Aborts a transaction: writes Abort WAL record and releases all held locks.
    pub fn abort(&self, txn: &Transaction) -> Result<()> {
        let mut state = txn.state.write().unwrap();
        if *state != TransactionState::Active {
            return Err(JarvixError::Transaction("Transaction is not in active state".to_string()));
        }

        let abort_rec = LogRecord::new_abort(txn.txn_id, txn.prev_lsn());
        let abort_lsn = self.wal_manager.append_record(abort_rec)?;
        self.wal_manager.flush(abort_lsn)?;

        self.lock_manager.release_all_locks(txn.txn_id);

        *state = TransactionState::Aborted;
        self.active_txns.lock().unwrap().remove(&txn.txn_id);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock_manager::LockMode;

    struct TempWal(std::path::PathBuf);
    impl TempWal {
        fn new(name: &str) -> Self {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let path = std::env::temp_dir().join(format!("jarvix_txn_wal_{}_{}.wal", name, nanos));
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
    fn test_transaction_begin_commit_lifecycle() {
        let temp = TempWal::new("txn");
        let wal_mgr = Arc::new(WalManager::open(&temp.0).unwrap());
        let lock_mgr = Arc::new(LockManager::new());
        let undo_mgr = Arc::new(UndoLogManager::new());

        let txn_mgr = TransactionManager::new(wal_mgr, lock_mgr, undo_mgr);

        let t1 = txn_mgr.begin(IsolationLevel::RepeatableRead).unwrap();
        assert_eq!(t1.state(), TransactionState::Active);
        assert!(t1.read_view().is_some());

        // Acquire a table lock
        txn_mgr.lock_manager().acquire_lock(t1.txn_id(), LockKey::Table(10), LockMode::Exclusive).unwrap();

        // Commit transaction
        txn_mgr.commit(&t1).unwrap();
        assert_eq!(t1.state(), TransactionState::Committed);

        // Lock on Table 10 must now be released; another transaction can acquire it
        let t2 = txn_mgr.begin(IsolationLevel::RepeatableRead).unwrap();
        assert!(txn_mgr.lock_manager().acquire_lock(t2.txn_id(), LockKey::Table(10), LockMode::Exclusive).is_ok());
        txn_mgr.abort(&t2).unwrap();
        assert_eq!(t2.state(), TransactionState::Aborted);
    }
}
