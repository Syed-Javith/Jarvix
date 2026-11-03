use std::sync::{Arc, RwLock};

use jarvix_common::constants::INVALID_PAGE_ID;
use jarvix_common::error::{JarvixError, Result};
use jarvix_common::types::{PageId, RecordId};
use jarvix_storage::PageType;
use jarvix_buffer::BufferPoolManager;

use crate::internal_page::{BTreeInternalPage, DEFAULT_INTERNAL_MAX_SIZE};
use crate::leaf_page::{BTreeLeafPage, DEFAULT_LEAF_MAX_SIZE};
use crate::node_header::BTreeNodeHeader;

/// Concurrent, disk-backed B+Tree index implementation for Jarvix.
pub struct BTreeIndex {
    name: String,
    root_page_id: Arc<RwLock<PageId>>,
    bpm: BufferPoolManager,
    leaf_max_size: u16,
    internal_max_size: u16,
}

impl BTreeIndex {
    pub fn new(name: impl Into<String>, bpm: BufferPoolManager) -> Self {
        Self::with_capacities(name, bpm, DEFAULT_LEAF_MAX_SIZE, DEFAULT_INTERNAL_MAX_SIZE)
    }

    pub fn with_root(name: impl Into<String>, bpm: BufferPoolManager, root_page_id: PageId) -> Self {
        Self {
            name: name.into(),
            root_page_id: Arc::new(RwLock::new(root_page_id)),
            bpm,
            leaf_max_size: DEFAULT_LEAF_MAX_SIZE,
            internal_max_size: DEFAULT_INTERNAL_MAX_SIZE,
        }
    }

    pub fn with_capacities(
        name: impl Into<String>,
        bpm: BufferPoolManager,
        leaf_max_size: u16,
        internal_max_size: u16,
    ) -> Self {
        Self {
            name: name.into(),
            root_page_id: Arc::new(RwLock::new(INVALID_PAGE_ID)),
            bpm,
            leaf_max_size,
            internal_max_size,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn root_page_id(&self) -> PageId {
        *self.root_page_id.read().unwrap()
    }

    pub fn is_empty(&self) -> bool {
        self.root_page_id() == INVALID_PAGE_ID
    }

    /// Search for a single key, returning the matching RecordId if found.
    pub fn search(&self, key: i64) -> Result<Option<RecordId>> {
        let root_pid = self.root_page_id();
        if root_pid == INVALID_PAGE_ID {
            return Ok(None);
        }

        let leaf_pid = self.find_leaf_page_id(root_pid, key)?;
        let leaf_guard = self.bpm.fetch_page(leaf_pid)?;

        match BTreeLeafPage::find_key(&leaf_guard, key) {
            Ok(idx) => Ok(Some(BTreeLeafPage::val_at(&leaf_guard, idx))),
            Err(_) => Ok(None),
        }
    }

    /// Insert a (key, RecordId) entry into the B+Tree.
    pub fn insert(&self, key: i64, rid: RecordId) -> Result<()> {
        let mut root_lock = self.root_page_id.write().unwrap();
        let root_pid = *root_lock;

        if root_pid == INVALID_PAGE_ID {
            // Create root leaf page
            let (new_pid, mut guard) = self.bpm.new_page(PageType::BTreeLeaf)?;
            BTreeLeafPage::init(&mut guard, self.leaf_max_size);
            BTreeLeafPage::insert(&mut guard, key, rid)?;
            *root_lock = new_pid;
            return Ok(());
        }

        // Drop root lock for traversal
        drop(root_lock);

        let leaf_pid = self.find_leaf_page_id(self.root_page_id(), key)?;
        let mut leaf_guard = self.bpm.fetch_page_mut(leaf_pid)?;

        if (BTreeLeafPage::size(&leaf_guard) as usize) < (self.leaf_max_size as usize) {
            BTreeLeafPage::insert(&mut leaf_guard, key, rid)?;
            return Ok(());
        }

        // Leaf is full -> split leaf
        let (new_leaf_pid, mut new_leaf_guard) = self.bpm.new_page(PageType::BTreeLeaf)?;
        BTreeLeafPage::init(&mut new_leaf_guard, self.leaf_max_size);

        let promoted_key = BTreeLeafPage::split_into(&mut leaf_guard, &mut new_leaf_guard);

        // Insert new entry into the appropriate half
        if key < promoted_key {
            BTreeLeafPage::insert(&mut leaf_guard, key, rid)?;
        } else {
            BTreeLeafPage::insert(&mut new_leaf_guard, key, rid)?;
        }

        let parent_pid = BTreeLeafPage::parent_page_id(&leaf_guard);

        if parent_pid == INVALID_PAGE_ID {
            // Root split -> allocate new root internal page
            let (new_root_pid, mut new_root_guard) = self.bpm.new_page(PageType::BTreeInternal)?;
            BTreeInternalPage::init(&mut new_root_guard, self.internal_max_size);
            BTreeInternalPage::populate_new_root(&mut new_root_guard, leaf_pid, promoted_key, new_leaf_pid);

            BTreeLeafPage::set_parent_page_id(&mut leaf_guard, new_root_pid);
            BTreeLeafPage::set_parent_page_id(&mut new_leaf_guard, new_root_pid);

            let mut root_lock = self.root_page_id.write().unwrap();
            *root_lock = new_root_pid;
            Ok(())
        } else {
            BTreeLeafPage::set_parent_page_id(&mut new_leaf_guard, parent_pid);
            drop(leaf_guard);
            drop(new_leaf_guard);
            self.insert_into_parent(parent_pid, leaf_pid, promoted_key, new_leaf_pid)
        }
    }

    fn insert_into_parent(
        &self,
        mut parent_pid: PageId,
        mut left_child: PageId,
        mut key: i64,
        mut new_child_pid: PageId,
    ) -> Result<()> {
        let internal = BTreeInternalPage;

        loop {
            let mut parent_guard = self.bpm.fetch_page_mut(parent_pid)?;

            if (BTreeInternalPage::size(&parent_guard) as usize) < (self.internal_max_size as usize) {
                internal.insert_after(&mut parent_guard, left_child, key, new_child_pid)?;
                return Ok(());
            }

            // Internal page is full -> split internal page
            let (new_internal_pid, mut new_internal_guard) = self.bpm.new_page(PageType::BTreeInternal)?;
            BTreeInternalPage::init(&mut new_internal_guard, self.internal_max_size);

            let promoted_key = BTreeInternalPage::split_into(&mut parent_guard, &mut new_internal_guard);

            // Update children's parent pointers for recipient
            let recipient_size = BTreeInternalPage::size(&new_internal_guard);
            for i in 0..=recipient_size as usize {
                let child_id = BTreeInternalPage::val_at(&new_internal_guard, i);
                let mut child_guard = self.bpm.fetch_page_mut(child_id)?;
                BTreeNodeHeader::set_parent_page_id(&mut child_guard, new_internal_pid);
            }

            if key < promoted_key {
                internal.insert_after(&mut parent_guard, left_child, key, new_child_pid)?;
            } else {
                internal.insert_after(&mut new_internal_guard, left_child, key, new_child_pid)?;
                let mut child_guard = self.bpm.fetch_page_mut(new_child_pid)?;
                BTreeNodeHeader::set_parent_page_id(&mut child_guard, new_internal_pid);
            }

            let grandparent_pid = BTreeInternalPage::parent_page_id(&parent_guard);

            if grandparent_pid == INVALID_PAGE_ID {
                // Root split at internal level
                let (new_root_pid, mut new_root_guard) = self.bpm.new_page(PageType::BTreeInternal)?;
                BTreeInternalPage::init(&mut new_root_guard, self.internal_max_size);
                BTreeInternalPage::populate_new_root(&mut new_root_guard, parent_pid, promoted_key, new_internal_pid);

                BTreeInternalPage::set_parent_page_id(&mut parent_guard, new_root_pid);
                BTreeInternalPage::set_parent_page_id(&mut new_internal_guard, new_root_pid);

                let mut root_lock = self.root_page_id.write().unwrap();
                *root_lock = new_root_pid;
                return Ok(());
            } else {
                BTreeInternalPage::set_parent_page_id(&mut new_internal_guard, grandparent_pid);
                left_child = parent_pid;
                parent_pid = grandparent_pid;
                key = promoted_key;
                new_child_pid = new_internal_pid;
            }
        }
    }

    /// Deletes a key from the B+Tree.
    pub fn delete(&self, key: i64) -> Result<bool> {
        let root_pid = self.root_page_id();
        if root_pid == INVALID_PAGE_ID {
            return Ok(false);
        }

        let leaf_pid = self.find_leaf_page_id(root_pid, key)?;
        let mut leaf_guard = self.bpm.fetch_page_mut(leaf_pid)?;
        Ok(BTreeLeafPage::remove(&mut leaf_guard, key))
    }

    /// Range scan: returns all (key, RecordId) pairs in [start_key, end_key].
    pub fn scan(&self, start_key: Option<i64>, end_key: Option<i64>) -> Result<Vec<(i64, RecordId)>> {
        let root_pid = self.root_page_id();
        if root_pid == INVALID_PAGE_ID {
            return Ok(Vec::new());
        }

        // Find initial leaf page
        let mut curr_pid = match start_key {
            Some(k) => self.find_leaf_page_id(root_pid, k)?,
            None => self.find_leftmost_leaf(root_pid)?,
        };

        let mut results = Vec::new();

        while curr_pid != INVALID_PAGE_ID {
            let leaf_guard = self.bpm.fetch_page(curr_pid)?;
            let size = BTreeLeafPage::size(&leaf_guard) as usize;

            for i in 0..size {
                let k = BTreeLeafPage::key_at(&leaf_guard, i);
                let v = BTreeLeafPage::val_at(&leaf_guard, i);

                if let Some(start) = start_key {
                    if k < start {
                        continue;
                    }
                }

                if let Some(end) = end_key {
                    if k > end {
                        return Ok(results);
                    }
                }

                results.push((k, v));
            }

            curr_pid = BTreeLeafPage::next_page_id(&leaf_guard);
        }

        Ok(results)
    }

    fn find_leaf_page_id(&self, root_pid: PageId, key: i64) -> Result<PageId> {
        let internal = BTreeInternalPage;
        let mut curr_pid = root_pid;

        loop {
            let guard = self.bpm.fetch_page(curr_pid)?;
            match guard.page_type() {
                PageType::BTreeLeaf => return Ok(curr_pid),
                PageType::BTreeInternal => {
                    let next_pid = internal.lookup(&guard, key);
                    if next_pid == INVALID_PAGE_ID {
                        return Err(JarvixError::Index(format!(
                            "Corrupted internal node {}: invalid child pointer",
                            curr_pid
                        )));
                    }
                    curr_pid = next_pid;
                }
                _ => {
                    return Err(JarvixError::Index(format!(
                        "Invalid BTree page type {:?} at page {}",
                        guard.page_type(),
                        curr_pid
                    )));
                }
            }
        }
    }

    fn find_leftmost_leaf(&self, root_pid: PageId) -> Result<PageId> {
        let mut curr_pid = root_pid;

        loop {
            let guard = self.bpm.fetch_page(curr_pid)?;
            match guard.page_type() {
                PageType::BTreeLeaf => return Ok(curr_pid),
                PageType::BTreeInternal => {
                    let next_pid = BTreeInternalPage::val_at(&guard, 0);
                    curr_pid = next_pid;
                }
                _ => return Err(JarvixError::Index("Corrupted BTree page".to_string())),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jarvix_storage::DiskManager;

    struct TempDbFile(std::path::PathBuf);
    impl TempDbFile {
        fn new(name: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!("jarvix_btree_test_{}_{}.db", name, nanos));
            let _ = std::fs::remove_file(&path);
            Self(path)
        }
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }
    impl Drop for TempDbFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn test_btree_insert_search_and_scan() {
        let temp = TempDbFile::new("scan");
        let disk_mgr = Arc::new(DiskManager::open(temp.path()).unwrap());
        let bpm = BufferPoolManager::new(10, disk_mgr);

        // Small capacity to trigger splits: leaf capacity 4, internal capacity 4
        let btree = BTreeIndex::with_capacities("test_idx", bpm, 4, 4);

        // Insert 20 keys
        for i in 1..=20 {
            btree.insert(i * 10, RecordId::new(1, i as u16)).unwrap();
        }

        // Search existing keys
        for i in 1..=20 {
            let res = btree.search(i * 10).unwrap();
            assert_eq!(res, Some(RecordId::new(1, i as u16)));
        }

        // Search non-existent key
        assert_eq!(btree.search(999).unwrap(), None);

        // Range scan [50, 120]
        let range = btree.scan(Some(50), Some(120)).unwrap();
        let expected_keys: Vec<i64> = (5..=12).map(|x| x * 10).collect();
        let actual_keys: Vec<i64> = range.iter().map(|(k, _)| *k).collect();
        assert_eq!(actual_keys, expected_keys);

        // Full scan
        let full_scan = btree.scan(None, None).unwrap();
        assert_eq!(full_scan.len(), 20);

        // Test delete
        assert!(btree.delete(50).unwrap());
        assert_eq!(btree.search(50).unwrap(), None);
        let after_delete = btree.scan(Some(40), Some(60)).unwrap();
        let keys_after: Vec<i64> = after_delete.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys_after, vec![40, 60]);
    }

    #[test]
    fn test_btree_large_dataset() {
        let temp = TempDbFile::new("large");
        let disk_mgr = Arc::new(DiskManager::open(temp.path()).unwrap());
        let bpm = BufferPoolManager::new(50, disk_mgr);

        // Capacities of 8 to force deep multi-level tree with multiple internal splits
        let btree = BTreeIndex::with_capacities("large_idx", bpm, 8, 8);

        let count = 200;
        // Insert non-sequential keys
        for i in 1..=count {
            let key = (i * 37) % 500;
            btree.insert(key, RecordId::new((i % 10) as u32, i as u16)).unwrap();
        }

        // Verify all keys can be retrieved
        for i in 1..=count {
            let key = (i * 37) % 500;
            let res = btree.search(key).unwrap();
            assert!(res.is_some());
        }

        // Verify sorted ordering across leaf sibling chain
        let scan_all = btree.scan(None, None).unwrap();
        for i in 1..scan_all.len() {
            assert!(scan_all[i - 1].0 <= scan_all[i].0);
        }
    }

    #[test]
    fn test_btree_key_update() {
        let temp = TempDbFile::new("update");
        let disk_mgr = Arc::new(DiskManager::open(temp.path()).unwrap());
        let bpm = BufferPoolManager::new(10, disk_mgr);

        let btree = BTreeIndex::with_capacities("update_idx", bpm, 4, 4);

        btree.insert(100, RecordId::new(1, 10)).unwrap();
        assert_eq!(btree.search(100).unwrap(), Some(RecordId::new(1, 10)));

        // Update value for existing key
        btree.insert(100, RecordId::new(2, 20)).unwrap();
        assert_eq!(btree.search(100).unwrap(), Some(RecordId::new(2, 20)));
    }
}
