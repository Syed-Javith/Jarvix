use std::sync::{Arc, RwLock};
use jarvix_buffer::BufferPoolManager;
use jarvix_catalog::Catalog;
use jarvix_common::types::TxnId;
use jarvix_txn::lock_manager::LockManager;
use jarvix_txn::mvcc::ReadView;
use jarvix_txn::transaction_manager::TransactionManager;
use jarvix_wal::WalManager;

/// Shared runtime execution context for query plan nodes.
#[derive(Clone)]
pub struct ExecutionContext {
    pub bpm: BufferPoolManager,
    pub catalog: Arc<RwLock<Catalog>>,
    pub txn_mgr: Arc<TransactionManager>,
    pub wal_mgr: Arc<WalManager>,
    pub lock_mgr: Arc<LockManager>,
    pub txn_id: TxnId,
    pub read_view: Option<ReadView>,
}

impl ExecutionContext {
    pub fn new(
        bpm: BufferPoolManager,
        catalog: Arc<RwLock<Catalog>>,
        txn_mgr: Arc<TransactionManager>,
        wal_mgr: Arc<WalManager>,
        lock_mgr: Arc<LockManager>,
        txn_id: TxnId,
    ) -> Self {
        Self {
            bpm,
            catalog,
            txn_mgr,
            wal_mgr,
            lock_mgr,
            txn_id,
            read_view: None,
        }
    }

    pub fn with_read_view(mut self, read_view: ReadView) -> Self {
        self.read_view = Some(read_view);
        self
    }
}
