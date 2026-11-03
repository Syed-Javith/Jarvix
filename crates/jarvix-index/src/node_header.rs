use byteorder::{BigEndian, ByteOrder};
use jarvix_common::constants::INVALID_PAGE_ID;
use jarvix_common::types::PageId;
use jarvix_storage::{Page, PageType};

/// BTree node header starts immediately after the standard 24-byte Page Header.
/// Offsets:
/// 24..26: size (u16)
/// 26..28: max_size (u16)
/// 28..32: parent_page_id (u32)
/// 32..36: next_page_id (u32, leaf only)
/// 36..40: prev_page_id (u32, leaf only)
pub const BTREE_NODE_HEADER_SIZE: usize = 40;

pub struct BTreeNodeHeader;

impl BTreeNodeHeader {
    pub fn get_size(page: &Page) -> u16 {
        BigEndian::read_u16(&page.data[24..26])
    }

    pub fn set_size(page: &mut Page, size: u16) {
        BigEndian::write_u16(&mut page.data[24..26], size);
    }

    pub fn get_max_size(page: &Page) -> u16 {
        BigEndian::read_u16(&page.data[26..28])
    }

    pub fn set_max_size(page: &mut Page, max_size: u16) {
        BigEndian::write_u16(&mut page.data[26..28], max_size);
    }

    pub fn get_parent_page_id(page: &Page) -> PageId {
        BigEndian::read_u32(&page.data[28..32])
    }

    pub fn set_parent_page_id(page: &mut Page, parent: PageId) {
        BigEndian::write_u32(&mut page.data[28..32], parent);
    }

    pub fn get_next_page_id(page: &Page) -> PageId {
        BigEndian::read_u32(&page.data[32..36])
    }

    pub fn set_next_page_id(page: &mut Page, next: PageId) {
        BigEndian::write_u32(&mut page.data[32..36], next);
    }

    pub fn get_prev_page_id(page: &Page) -> PageId {
        BigEndian::read_u32(&page.data[36..40])
    }

    pub fn set_prev_page_id(page: &mut Page, prev: PageId) {
        BigEndian::write_u32(&mut page.data[36..40], prev);
    }

    pub fn init(page: &mut Page, page_type: PageType, max_size: u16) {
        page.set_page_type(page_type);
        Self::set_size(page, 0);
        Self::set_max_size(page, max_size);
        Self::set_parent_page_id(page, INVALID_PAGE_ID);
        Self::set_next_page_id(page, INVALID_PAGE_ID);
        Self::set_prev_page_id(page, INVALID_PAGE_ID);
    }
}
