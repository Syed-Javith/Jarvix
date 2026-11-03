use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

use jarvix_common::types::{RecordId, TxnId};
use crate::mvcc::ReadView;

pub const NULL_UNDO_PTR: u64 = 0;

/// An Undo Log entry holding previous row data for MVCC version reconstruction and rollback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoRecord {
    pub undo_ptr: u64,
    pub prev_undo_ptr: u64,
    pub txn_id: TxnId,
    pub rid: RecordId,
    pub before_image: Vec<u8>,
}

/// In-memory and persistent Undo Log Manager.
pub struct UndoLogManager {
    records: RwLock<HashMap<u64, UndoRecord>>,
    next_ptr: AtomicU64,
}

impl UndoLogManager {
    pub fn new() -> Self {
        Self {
            records: RwLock::new(HashMap::new()),
            next_ptr: AtomicU64::new(1),
        }
    }

    /// Appends an undo record and returns its unique rollback pointer.
    pub fn append_undo(
        &self,
        txn_id: TxnId,
        rid: RecordId,
        before_image: Vec<u8>,
        prev_undo_ptr: u64,
    ) -> u64 {
        let ptr = self.next_ptr.fetch_add(1, Ordering::SeqCst);
        let rec = UndoRecord {
            undo_ptr: ptr,
            prev_undo_ptr,
            txn_id,
            rid,
            before_image,
        };

        self.records.write().unwrap().insert(ptr, rec);
        ptr
    }

    /// Retrieves an undo record by rollback pointer.
    pub fn get_undo(&self, undo_ptr: u64) -> Option<UndoRecord> {
        self.records.read().unwrap().get(&undo_ptr).cloned()
    }

    /// Traverses the undo chain to reconstruct the visible tuple version for a given ReadView.
    pub fn reconstruct_version(&self, roll_ptr: u64, view: &ReadView) -> Option<Vec<u8>> {
        let mut curr_ptr = roll_ptr;

        while curr_ptr != NULL_UNDO_PTR {
            if let Some(undo_rec) = self.get_undo(curr_ptr) {
                if view.is_visible(undo_rec.txn_id) {
                    return Some(undo_rec.before_image);
                }
                curr_ptr = undo_rec.prev_undo_ptr;
            } else {
                break;
            }
        }

        None
    }

    /// Purges all undo records older than the minimum active transaction ID.
    pub fn purge(&self, oldest_active_txn: TxnId) {
        let mut map = self.records.write().unwrap();
        map.retain(|_, rec| rec.txn_id >= oldest_active_txn);
    }
}

impl Default for UndoLogManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_undo_log_reconstruction() {
        let undo_mgr = UndoLogManager::new();
        let rid = RecordId::new(1, 0);

        // Version 1 created by Txn 10: "Tuple V1"
        // Version 2 updated by Txn 20: "Tuple V2", points to V1
        let u1 = undo_mgr.append_undo(10, rid, b"Tuple V1".to_vec(), NULL_UNDO_PTR);
        // Version 3 updated by Txn 30: "Tuple V3", points to V2
        let u2 = undo_mgr.append_undo(20, rid, b"Tuple V2".to_vec(), u1);

        // ReadView for a reader that started when Txn 20 and Txn 30 were active
        let mut active = HashSet::new();
        active.insert(20);
        active.insert(30);
        let view = ReadView::new(40, active, 50);

        // Txn 20 is invisible, so reconstruct_version must follow chain back to Txn 10 ("Tuple V1")
        let reconstructed = undo_mgr.reconstruct_version(u2, &view).unwrap();
        assert_eq!(reconstructed, b"Tuple V1");
    }
}
