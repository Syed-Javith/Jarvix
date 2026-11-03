use std::collections::HashSet;
use jarvix_common::types::TxnId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsolationLevel {
    ReadCommitted,
    RepeatableRead,
    Serializable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionState {
    Active,
    Committed,
    Aborted,
}

/// MVCC ReadView captures a consistent snapshot of the database state.
/// Follows InnoDB-style ReadView semantics:
/// - Any transaction ID < low_limit_id was already committed when this view was created.
/// - Any transaction ID >= high_limit_id was started after this view was created (invisible).
/// - Any transaction ID in active_txns was actively running and uncommitted when this view was created (invisible).
#[derive(Debug, Clone)]
pub struct ReadView {
    pub creator_txn_id: TxnId,
    pub low_limit_id: TxnId,
    pub high_limit_id: TxnId,
    pub active_txns: HashSet<TxnId>,
}

impl ReadView {
    pub fn new(creator_txn_id: TxnId, active_txns: HashSet<TxnId>, high_limit_id: TxnId) -> Self {
        let low_limit_id = active_txns.iter().cloned().min().unwrap_or(high_limit_id);
        Self {
            creator_txn_id,
            low_limit_id,
            high_limit_id,
            active_txns,
        }
    }

    /// Determines if modifications made by `row_txn_id` are visible to this ReadView.
    pub fn is_visible(&self, row_txn_id: TxnId) -> bool {
        // Changes made by this transaction itself are always visible
        if row_txn_id == self.creator_txn_id {
            return true;
        }

        // Transactions committed before the oldest active transaction are visible
        if row_txn_id < self.low_limit_id {
            return true;
        }

        // Transactions started after this ReadView was created are invisible
        if row_txn_id >= self.high_limit_id {
            return false;
        }

        // Transactions that were active when this ReadView was created are invisible
        !self.active_txns.contains(&row_txn_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_view_visibility() {
        let mut active = HashSet::new();
        active.insert(100);
        active.insert(102);

        // Active txns: 100, 102; high_limit: 105; low_limit: 100; creator: 102
        let view = ReadView::new(102, active, 105);

        // Txn 90: committed before low limit -> visible
        assert!(view.is_visible(90));

        // Txn 100: active when view created -> invisible
        assert!(!view.is_visible(100));

        // Txn 101: committed between low and high limit -> visible
        assert!(view.is_visible(101));

        // Txn 102: creator itself -> visible
        assert!(view.is_visible(102));

        // Txn 105: started after view -> invisible
        assert!(!view.is_visible(105));
        assert!(!view.is_visible(110));
    }
}
