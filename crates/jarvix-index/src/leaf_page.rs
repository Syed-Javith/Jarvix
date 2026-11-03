use byteorder::{BigEndian, ByteOrder};
use jarvix_common::error::{JarvixError, Result};
use jarvix_common::types::{PageId, RecordId};
use jarvix_storage::{Page, PageType};

use crate::node_header::{BTreeNodeHeader, BTREE_NODE_HEADER_SIZE};

pub const DEFAULT_LEAF_MAX_SIZE: u16 = 250;

/// BTreeLeafPage layout:
/// - Page Header: 24 bytes
/// - BTree Node Header: 16 bytes (offsets 24..40)
/// - Keys: max_size * 8 bytes (i64)
/// - Values: max_size * 6 bytes (RecordId: u32 page_id, u16 slot_id)
pub struct BTreeLeafPage;

impl BTreeLeafPage {
    pub fn init(page: &mut Page, max_size: u16) {
        BTreeNodeHeader::init(page, PageType::BTreeLeaf, max_size);
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

    pub fn next_page_id(page: &Page) -> PageId {
        BTreeNodeHeader::get_next_page_id(page)
    }

    pub fn set_next_page_id(page: &mut Page, next: PageId) {
        BTreeNodeHeader::set_next_page_id(page, next);
    }

    pub fn prev_page_id(page: &Page) -> PageId {
        BTreeNodeHeader::get_prev_page_id(page)
    }

    pub fn set_prev_page_id(page: &mut Page, prev: PageId) {
        BTreeNodeHeader::set_prev_page_id(page, prev);
    }

    fn key_offset(idx: usize) -> usize {
        BTREE_NODE_HEADER_SIZE + idx * 8
    }

    fn val_offset(page: &Page, idx: usize) -> usize {
        let max_size = Self::max_size(page) as usize;
        BTREE_NODE_HEADER_SIZE + max_size * 8 + idx * 6
    }

    pub fn key_at(page: &Page, idx: usize) -> i64 {
        let off = Self::key_offset(idx);
        BigEndian::read_i64(&page.data[off..off + 8])
    }

    pub fn set_key_at(page: &mut Page, idx: usize, key: i64) {
        let off = Self::key_offset(idx);
        BigEndian::write_i64(&mut page.data[off..off + 8], key);
    }

    pub fn val_at(page: &Page, idx: usize) -> RecordId {
        let off = Self::val_offset(page, idx);
        let pid = BigEndian::read_u32(&page.data[off..off + 4]);
        let slot = BigEndian::read_u16(&page.data[off + 4..off + 6]);
        RecordId::new(pid, slot)
    }

    pub fn set_val_at(page: &mut Page, idx: usize, rid: RecordId) {
        let off = Self::val_offset(page, idx);
        BigEndian::write_u32(&mut page.data[off..off + 4], rid.page_id);
        BigEndian::write_u16(&mut page.data[off + 4..off + 6], rid.slot_id);
    }

    /// Finds the index of the key using binary search.
    /// Returns Ok(index) if key is found, or Err(insertion_index) if not.
    pub fn find_key(page: &Page, key: i64) -> std::result::Result<usize, usize> {
        let size = Self::size(page) as usize;
        let mut low = 0;
        let mut high = size;

        while low < high {
            let mid = low + (high - low) / 2;
            let mid_key = Self::key_at(page, mid);
            if mid_key == key {
                return Ok(mid);
            } else if mid_key < key {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        Err(low)
    }

    /// Inserts (key, rid) in sorted order. Returns the new size, or error if full.
    pub fn insert(page: &mut Page, key: i64, rid: RecordId) -> Result<u16> {
        let size = Self::size(page) as usize;
        let max_size = Self::max_size(page) as usize;

        if size >= max_size {
            return Err(JarvixError::Index("Leaf page is full".to_string()));
        }

        let idx = match Self::find_key(page, key) {
            Ok(existing_idx) => {
                // Update existing key
                Self::set_val_at(page, existing_idx, rid);
                return Ok(size as u16);
            }
            Err(insert_idx) => insert_idx,
        };

        // Shift elements right
        for i in (idx..size).rev() {
            let k = Self::key_at(page, i);
            let v = Self::val_at(page, i);
            Self::set_key_at(page, i + 1, k);
            Self::set_val_at(page, i + 1, v);
        }

        Self::set_key_at(page, idx, key);
        Self::set_val_at(page, idx, rid);
        let new_size = (size + 1) as u16;
        BTreeNodeHeader::set_size(page, new_size);

        Ok(new_size)
    }

    /// Removes a key from the leaf page. Returns true if key was found and removed.
    pub fn remove(page: &mut Page, key: i64) -> bool {
        let idx = match Self::find_key(page, key) {
            Ok(i) => i,
            Err(_) => return false,
        };

        let size = Self::size(page) as usize;
        for i in idx..size - 1 {
            let k = Self::key_at(page, i + 1);
            let v = Self::val_at(page, i + 1);
            Self::set_key_at(page, i, k);
            Self::set_val_at(page, i, v);
        }

        BTreeNodeHeader::set_size(page, (size - 1) as u16);
        true
    }

    /// Splits this leaf page into two: moves the upper half of elements into `recipient_page`.
    /// Returns the promoted key (the first key of the recipient page).
    pub fn split_into(page: &mut Page, recipient: &mut Page) -> i64 {
        let size = Self::size(page) as usize;
        let mid = size / 2;

        let mut recipient_idx = 0;
        for i in mid..size {
            let k = Self::key_at(page, i);
            let v = Self::val_at(page, i);
            Self::set_key_at(recipient, recipient_idx, k);
            Self::set_val_at(recipient, recipient_idx, v);
            recipient_idx += 1;
        }

        BTreeNodeHeader::set_size(page, mid as u16);
        BTreeNodeHeader::set_size(recipient, recipient_idx as u16);

        // Update leaf sibling links
        let old_next = Self::next_page_id(page);
        Self::set_next_page_id(recipient, old_next);
        Self::set_prev_page_id(recipient, page.page_id());
        Self::set_next_page_id(page, recipient.page_id());

        // First key of recipient is promoted to parent
        Self::key_at(recipient, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_leaf_insert_and_find() {
        let mut page = Page::new();
        BTreeLeafPage::init(&mut page, 10);

        assert_eq!(BTreeLeafPage::size(&page), 0);

        BTreeLeafPage::insert(&mut page, 20, RecordId::new(1, 2)).unwrap();
        BTreeLeafPage::insert(&mut page, 10, RecordId::new(1, 1)).unwrap();
        BTreeLeafPage::insert(&mut page, 30, RecordId::new(1, 3)).unwrap();

        assert_eq!(BTreeLeafPage::size(&page), 3);
        assert_eq!(BTreeLeafPage::key_at(&page, 0), 10);
        assert_eq!(BTreeLeafPage::key_at(&page, 1), 20);
        assert_eq!(BTreeLeafPage::key_at(&page, 2), 30);

        assert_eq!(BTreeLeafPage::find_key(&page, 20), Ok(1));
        assert_eq!(BTreeLeafPage::val_at(&page, 1), RecordId::new(1, 2));

        // Test remove
        assert!(BTreeLeafPage::remove(&mut page, 20));
        assert_eq!(BTreeLeafPage::size(&page), 2);
        assert_eq!(BTreeLeafPage::key_at(&page, 0), 10);
        assert_eq!(BTreeLeafPage::key_at(&page, 1), 30);
    }

    #[test]
    fn test_leaf_split() {
        let mut left = Page::new();
        left.init(10, PageType::BTreeLeaf);
        BTreeLeafPage::init(&mut left, 4);

        for i in 1..=4 {
            BTreeLeafPage::insert(&mut left, i * 10, RecordId::new(1, i as u16)).unwrap();
        }

        let mut right = Page::new();
        right.init(11, PageType::BTreeLeaf);
        BTreeLeafPage::init(&mut right, 4);

        let promoted_key = BTreeLeafPage::split_into(&mut left, &mut right);

        assert_eq!(promoted_key, 30);
        assert_eq!(BTreeLeafPage::size(&left), 2);
        assert_eq!(BTreeLeafPage::size(&right), 2);

        assert_eq!(BTreeLeafPage::key_at(&left, 0), 10);
        assert_eq!(BTreeLeafPage::key_at(&left, 1), 20);

        assert_eq!(BTreeLeafPage::key_at(&right, 0), 30);
        assert_eq!(BTreeLeafPage::key_at(&right, 1), 40);

        assert_eq!(BTreeLeafPage::next_page_id(&left), 11);
        assert_eq!(BTreeLeafPage::prev_page_id(&right), 10);
    }
}
