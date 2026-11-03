use std::sync::atomic::{AtomicBool, AtomicU32};
use std::sync::RwLock;

use jarvix_common::constants::INVALID_PAGE_ID;
use jarvix_storage::Page;

pub struct BufferFrame {
    pub page: RwLock<Page>,
    pub page_id: AtomicU32,
    pub pin_count: AtomicU32,
    pub is_dirty: AtomicBool,
}

impl BufferFrame {
    pub fn new() -> Self {
        Self {
            page: RwLock::new(Page::new()),
            page_id: AtomicU32::new(INVALID_PAGE_ID),
            pin_count: AtomicU32::new(0),
            is_dirty: AtomicBool::new(false),
        }
    }
}
