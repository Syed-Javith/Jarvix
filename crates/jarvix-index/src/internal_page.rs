use byteorder::{BigEndian, ByteOrder};
use jarvix_common::constants::INVALID_PAGE_ID;
use jarvix_common::error::{JarvixError, Result};
use jarvix_common::types::PageId;
use jarvix_storage::{Page, PageType};

use crate::node_header::{BTreeNodeHeader, BTREE_NODE_HEADER_SIZE};

pub const DEFAULT_INTERNAL_MAX_SIZE: u16 = 250;

/// BTreeInternalPage layout:
/// - Page Header: 24 bytes
/// - BTree Node Header: 16 bytes (offsets 24..40)
/// - Keys: max_size * 8 bytes (i64)
/// - Values (Child PageIds): (max_size + 1) * 4 bytes (u32)
pub struct BTreeInternalPage;

impl BTreeInternalPage {
    pub fn init(page: &mut Page, max_size: u16) {
        BTreeNodeHeader::init(page, PageType::BTreeInternal, max_size);
    }

    pub fn size(page: &Page) -> u16 {
        BTreeNodeHeader::get_size(page)
    }

    pub fn max_size(page: &Page) -> u16 {
        BTreeNodeHeader::get_max_size(page)
    }

    pub fn parent_page_id(page: &Page) -> PageId {
        BTreeNodeHeader::get_parent_page_id(page)
    }

    pub fn set_parent_page_id(page: &mut Page, parent: PageId) {
        BTreeNodeHeader::set_parent_page_id(page, parent);
    }

    fn key_offset(idx: usize) -> usize {
        BTREE_NODE_HEADER_SIZE + idx * 8
    }

    fn val_offset(page: &Page, idx: usize) -> usize {
        let max_size = Self::max_size(page) as usize;
        BTREE_NODE_HEADER_SIZE + max_size * 8 + idx * 4
    }

    pub fn key_at(page: &Page, idx: usize) -> i64 {
        let off = Self::key_offset(idx);
        BigEndian::read_i64(&page.data[off..off + 8])
    }

    pub fn set_key_at(page: &mut Page, idx: usize, key: i64) {
        let off = Self::key_offset(idx);
        BigEndian::write_i64(&mut page.data[off..off + 8], key);
    }

    pub fn val_at(page: &Page, idx: usize) -> PageId {
        let off = Self::val_offset(page, idx);
        BigEndian::read_u32(&page.data[off..off + 4])
    }

    pub fn set_val_at(page: &mut Page, idx: usize, pid: PageId) {
        let off = Self::val_offset(page, idx);
        BigEndian::write_u32(&mut page.data[off..off + 4], pid);
    }

    /// Initializes a newly created internal page with two children and one dividing key.
    pub fn populate_new_root(page: &mut Page, left_pid: PageId, key: i64, right_pid: PageId) {
        Self::set_val_at(page, 0, left_pid);
        Self::set_key_at(page, 0, key);
        Self::set_val_at(page, 1, right_pid);
        BTreeNodeHeader::set_size(page, 1);
    }

    /// Finds the child PageId to route the search for `key`.
    pub fn lookup(&self, page: &Page, key: i64) -> PageId {
        let size = Self::size(page) as usize;
        if size == 0 {
            return INVALID_PAGE_ID;
        }

        let mut low = 0;
        let mut high = size;

        while low < high {
            let mid = low + (high - low) / 2;
            let mid_key = Self::key_at(page, mid);
            if mid_key <= key {
                low = mid + 1;
            } else {
                high = mid;
            }
        }

        Self::val_at(page, low)
    }

    /// Inserts a new (key, child_pid) pair into the internal node.
    pub fn insert_after(&self, page: &mut Page, old_child: PageId, new_key: i64, new_child: PageId) -> Result<u16> {
        let size = Self::size(page) as usize;
        let max_size = Self::max_size(page) as usize;

        if size >= max_size {
            return Err(JarvixError::Index("Internal page is full".to_string()));
        }

        // Find index of old_child
        let mut child_idx = 0;
        let mut found = false;
        while child_idx <= size {
            if Self::val_at(page, child_idx) == old_child {
                found = true;
                break;
            }
            child_idx += 1;
        }

        if !found {
            return Err(JarvixError::Index(format!(
                "Child pointer {} not found in internal page {}",
                old_child,
                page.page_id()
            )));
        }

        let insert_idx = child_idx;

        // Shift keys and children right
        for i in (insert_idx..size).rev() {
            let k = Self::key_at(page, i);
            Self::set_key_at(page, i + 1, k);
        }
        for i in (insert_idx + 1..=size).rev() {
            let v = Self::val_at(page, i);
            Self::set_val_at(page, i + 1, v);
        }

        Self::set_key_at(page, insert_idx, new_key);
        Self::set_val_at(page, insert_idx + 1, new_child);

        let new_size = (size + 1) as u16;
        BTreeNodeHeader::set_size(page, new_size);
        Ok(new_size)
    }

    /// Splits an internal page into two.
    /// The middle key is promoted to the parent.
    /// Keys and children above the middle are moved to `recipient`.
    pub fn split_into(page: &mut Page, recipient: &mut Page) -> i64 {
        let size = Self::size(page) as usize;
        let mid = size / 2;
        let promoted_key = Self::key_at(page, mid);

        // Child pointers for recipient: mid + 1 to size
        let mut recipient_idx = 0;
        for i in (mid + 1)..size {
            let k = Self::key_at(page, i);
            Self::set_key_at(recipient, recipient_idx, k);
            recipient_idx += 1;
        }

        let mut child_idx = 0;
        for i in (mid + 1)..=size {
            let v = Self::val_at(page, i);
            Self::set_val_at(recipient, child_idx, v);
            child_idx += 1;
        }

        BTreeNodeHeader::set_size(page, mid as u16);
        BTreeNodeHeader::set_size(recipient, recipient_idx as u16);

        promoted_key
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_internal_page_lookup() {
        let mut page = Page::new();
        page.init(1, PageType::BTreeInternal);
        BTreeInternalPage::init(&mut page, 5);

        // keys: [20, 40]
        // children: [100, 200, 300]
        // < 20 -> 100
        // >= 20 and < 40 -> 200
        // >= 40 -> 300
        BTreeInternalPage::populate_new_root(&mut page, 100, 20, 200);
        let internal = BTreeInternalPage;
        internal.insert_after(&mut page, 200, 40, 300).unwrap();

        assert_eq!(BTreeInternalPage::size(&page), 2);
        assert_eq!(internal.lookup(&page, 5), 100);
        assert_eq!(internal.lookup(&page, 20), 200);
        assert_eq!(internal.lookup(&page, 25), 200);
        assert_eq!(internal.lookup(&page, 40), 300);
        assert_eq!(internal.lookup(&page, 50), 300);
    }
}
