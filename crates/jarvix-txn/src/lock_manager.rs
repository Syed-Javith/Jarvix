use std::collections::{HashMap, HashSet};
use std::sync::{Condvar, Mutex};

use jarvix_common::error::{JarvixError, Result};
use jarvix_common::types::{Oid, RecordId, TxnId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LockMode {
    Shared,
    Exclusive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LockKey {
    Table(Oid),
    Row(RecordId),
}

#[derive(Debug, Clone)]
struct LockRequest {
    txn_id: TxnId,
    lock_mode: LockMode,
    is_granted: bool,
}

#[derive(Default)]
struct LockRequestQueue {
    requests: Vec<LockRequest>,
}

impl LockRequestQueue {
    fn can_grant(&self, mode: LockMode, requesting_txn: TxnId) -> bool {
        let granted: Vec<&LockRequest> = self.requests.iter().filter(|r| r.is_granted).collect();
        if granted.is_empty() {
            return true;
        }

        // Re-entrancy & subsumption: If the requesting txn already holds an Exclusive lock,
        // it already has exclusive control, so both Shared and Exclusive can proceed immediately.
        if granted.iter().any(|r| r.txn_id == requesting_txn && r.lock_mode == LockMode::Exclusive) {
            return true;
        }

        // If transaction already holds the lock
        if granted.iter().any(|r| r.txn_id == requesting_txn && r.lock_mode == mode) {
            return true;
        }

        match mode {
            LockMode::Shared => {
                // Shared lock can be granted if all existing granted locks are Shared
                granted.iter().all(|r| r.lock_mode == LockMode::Shared)
            }
            LockMode::Exclusive => {
                // Exclusive lock can only be granted if the only holder is the requesting txn itself
                granted.len() == 1 && granted[0].txn_id == requesting_txn
            }
        }
    }
}

/// Strict Two-Phase Locking (2PL) Lock Manager with cycle-detection deadlock prevention.
pub struct LockManager {
    table: Mutex<HashMap<LockKey, LockRequestQueue>>,
    waits_for: Mutex<HashMap<TxnId, HashSet<TxnId>>>,
    cvar: Condvar,
}

impl LockManager {
    pub fn new() -> Self {
        Self {
            table: Mutex::new(HashMap::new()),
            waits_for: Mutex::new(HashMap::new()),
            cvar: Condvar::new(),
        }
    }

    pub fn acquire_table(&self, txn_id: TxnId, mode: LockMode, table_oid: Oid) -> Result<()> {
        self.acquire_lock(txn_id, LockKey::Table(table_oid), mode)
    }

    /// Acquires a lock on a resource under strict 2PL.
    pub fn acquire_lock(&self, txn_id: TxnId, key: LockKey, mode: LockMode) -> Result<()> {
        let mut table = self.table.lock().unwrap();
        let queue = table.entry(key).or_insert_with(LockRequestQueue::default);

        if queue.can_grant(mode, txn_id) {
            queue.requests.push(LockRequest {
                txn_id,
                lock_mode: mode,
                is_granted: true,
            });
            return Ok(());
        }

        // Lock cannot be granted immediately. Find holders to build wait-for graph
        let holders: Vec<TxnId> = queue
            .requests
            .iter()
            .filter(|r| r.is_granted)
            .map(|r| r.txn_id)
            .collect();

        // Update wait-for graph and check for deadlock
        {
            let mut wfg = self.waits_for.lock().unwrap();
            let waiting_on = wfg.entry(txn_id).or_insert_with(HashSet::new);
            for holder in &holders {
                if *holder != txn_id {
                    waiting_on.insert(*holder);
                }
            }

            if self.has_deadlock(&wfg, txn_id) {
                // Abort to prevent deadlock
                wfg.remove(&txn_id);
                return Err(JarvixError::Deadlock(txn_id));
            }
        }

        // Enqueue ungranted request
        queue.requests.push(LockRequest {
            txn_id,
            lock_mode: mode,
            is_granted: false,
        });

        // Wait until notified and lock can be granted
        loop {
            table = self.cvar.wait(table).unwrap();
            let queue = match table.get_mut(&key) {
                Some(q) => q,
                None => continue,
            };

            // Check if our request has now been granted
            let req_idx = queue
                .requests
                .iter()
                .position(|r| r.txn_id == txn_id && r.lock_mode == mode);

            if let Some(idx) = req_idx {
                if queue.requests[idx].is_granted {
                    self.waits_for.lock().unwrap().remove(&txn_id);
                    return Ok(());
                } else if queue.can_grant(mode, txn_id) {
                    queue.requests[idx].is_granted = true;
                    self.waits_for.lock().unwrap().remove(&txn_id);
                    return Ok(());
                }
            }
        }
    }

    /// Releases all locks held by a transaction at commit or abort.
    pub fn release_all_locks(&self, txn_id: TxnId) {
        let mut table = self.table.lock().unwrap();
        self.waits_for.lock().unwrap().remove(&txn_id);

        for queue in table.values_mut() {
            queue.requests.retain(|r| r.txn_id != txn_id);

            // Grant locks to next waiting requests in queue
            let mut grant_shared = false;
            for req in queue.requests.iter_mut() {
                if req.is_granted {
                    if req.lock_mode == LockMode::Shared {
                        grant_shared = true;
                    }
                    continue;
                }

                if req.lock_mode == LockMode::Shared {
                    req.is_granted = true;
                    grant_shared = true;
                } else if !grant_shared {
                    req.is_granted = true;
                    break;
                } else {
                    break;
                }
            }
        }

        self.cvar.notify_all();
    }

    /// Checks for a cycle in the Wait-For Graph using Depth-First Search (DFS).
    fn has_deadlock(&self, wfg: &HashMap<TxnId, HashSet<TxnId>>, start_txn: TxnId) -> bool {
        let mut visited = HashSet::new();
        let mut stack = HashSet::new();

        self.dfs_deadlock(wfg, start_txn, &mut visited, &mut stack)
    }

    fn dfs_deadlock(
        &self,
        wfg: &HashMap<TxnId, HashSet<TxnId>>,
        curr: TxnId,
        visited: &mut HashSet<TxnId>,
        stack: &mut HashSet<TxnId>,
    ) -> bool {
        if stack.contains(&curr) {
            return true; // Cycle detected!
        }
        if visited.contains(&curr) {
            return false;
        }

        visited.insert(curr);
        stack.insert(curr);

        if let Some(neighbors) = wfg.get(&curr) {
            for &next in neighbors {
                if self.dfs_deadlock(wfg, next, visited, stack) {
                    return true;
                }
            }
        }

        stack.remove(&curr);
        false
    }
}

impl Default for LockManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lock_shared_concurrency() {
        let lock_mgr = LockManager::new();
        let key = LockKey::Table(1);

        // Txn 1 and Txn 2 can concurrently acquire Shared lock on Table 1
        assert!(lock_mgr.acquire_lock(1, key, LockMode::Shared).is_ok());
        assert!(lock_mgr.acquire_lock(2, key, LockMode::Shared).is_ok());

        lock_mgr.release_all_locks(1);
        lock_mgr.release_all_locks(2);
    }

    #[test]
    fn test_deadlock_detection() {
        let lock_mgr = LockManager::new();
        let mut wfg = HashMap::new();

        // T1 waits for T2, T2 waits for T3, T3 waits for T1 -> Cycle!
        wfg.entry(1).or_insert_with(HashSet::new).insert(2);
        wfg.entry(2).or_insert_with(HashSet::new).insert(3);
        wfg.entry(3).or_insert_with(HashSet::new).insert(1);

        assert!(lock_mgr.has_deadlock(&wfg, 1));

        // Break cycle: T3 no longer waits for T1
        wfg.get_mut(&3).unwrap().remove(&1);
        assert!(!lock_mgr.has_deadlock(&wfg, 1));
    }
}
