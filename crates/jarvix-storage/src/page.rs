use byteorder::{BigEndian, ByteOrder};
use jarvix_common::constants::PAGE_SIZE;
use jarvix_common::error::{JarvixError, Result};
use jarvix_common::types::{Lsn, PageId, SlotId};

/// Header layout offsets in bytes:
/// 0..8:   LSN (8 bytes)
/// 8..12:  PageId (4 bytes)
/// 12..14: PageType (2 bytes)
/// 14..16: lower (offset to end of slot array, 2 bytes)
/// 16..18: upper (offset to start of tuple data, 2 bytes)
/// 18..20: special (offset to special area, 2 bytes)
/// 20..24: checksum (CRC32, 4 bytes)
pub const PAGE_HEADER_SIZE: usize = 24;

pub const ITEM_ID_SIZE: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum PageType {
    Invalid = 0,
    Heap = 1,
    BTreeInternal = 2,
    BTreeLeaf = 3,
    Overflow = 4,
}

impl From<u16> for PageType {
    fn from(val: u16) -> Self {
        match val {
            1 => PageType::Heap,
            2 => PageType::BTreeInternal,
            3 => PageType::BTreeLeaf,
            4 => PageType::Overflow,
            _ => PageType::Invalid,
        }
    }
}

pub mod slot_flags {
    pub const UNUSED: u16 = 0x00;
    pub const USED: u16 = 0x01;
    pub const REDIRECTED: u16 = 0x02;
    pub const DEAD: u16 = 0x04;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemId {
    pub offset: u16,
    pub length: u16,
    pub flags: u16,
}

impl ItemId {
    pub fn read_from(buf: &[u8]) -> Self {
        let offset = BigEndian::read_u16(&buf[0..2]);
        let length = BigEndian::read_u16(&buf[2..4]);
        let flags = BigEndian::read_u16(&buf[4..6]);
        Self {
            offset,
            length,
            flags,
        }
    }

    pub fn write_to(&self, buf: &mut [u8]) {
        BigEndian::write_u16(&mut buf[0..2], self.offset);
        BigEndian::write_u16(&mut buf[2..4], self.length);
        BigEndian::write_u16(&mut buf[4..6], self.flags);
    }
}

/// An 8 KB in-memory Page frame.
#[repr(C, align(4096))]
pub struct Page {
    pub data: [u8; PAGE_SIZE],
}

impl Clone for Page {
    fn clone(&self) -> Self {
        let mut page = Page::new();
        page.data.copy_from_slice(&self.data);
        page
    }
}

impl Default for Page {
    fn default() -> Self {
        Self::new()
    }
}

impl Page {
    pub fn new() -> Self {
        let mut page = Self {
            data: [0u8; PAGE_SIZE],
        };
        page.init(0, PageType::Heap);
        page
    }

    pub fn init(&mut self, page_id: PageId, page_type: PageType) {
        self.data.fill(0);
        self.set_lsn(0);
        self.set_page_id(page_id);
        self.set_page_type(page_type);
        self.set_lower(PAGE_HEADER_SIZE as u16);
        self.set_upper(PAGE_SIZE as u16);
        self.set_special(PAGE_SIZE as u16);
        self.set_checksum(0);
    }

    // --- Header accessors ---

    pub fn lsn(&self) -> Lsn {
        BigEndian::read_u64(&self.data[0..8])
    }

    pub fn set_lsn(&mut self, lsn: Lsn) {
        BigEndian::write_u64(&mut self.data[0..8], lsn);
    }

    pub fn page_id(&self) -> PageId {
        BigEndian::read_u32(&self.data[8..12])
    }

    pub fn set_page_id(&mut self, id: PageId) {
        BigEndian::write_u32(&mut self.data[8..12], id);
    }

    pub fn page_type(&self) -> PageType {
        BigEndian::read_u16(&self.data[12..14]).into()
    }

    pub fn set_page_type(&mut self, page_type: PageType) {
        BigEndian::write_u16(&mut self.data[12..14], page_type as u16);
    }

    pub fn lower(&self) -> u16 {
        BigEndian::read_u16(&self.data[14..16])
    }

    pub fn set_lower(&mut self, lower: u16) {
        BigEndian::write_u16(&mut self.data[14..16], lower);
    }

    pub fn upper(&self) -> u16 {
        BigEndian::read_u16(&self.data[16..18])
    }

    pub fn set_upper(&mut self, upper: u16) {
        BigEndian::write_u16(&mut self.data[16..18], upper);
    }

    pub fn special(&self) -> u16 {
        BigEndian::read_u16(&self.data[18..20])
    }

    pub fn set_special(&mut self, special: u16) {
        BigEndian::write_u16(&mut self.data[18..20], special);
    }

    pub fn checksum(&self) -> u32 {
        BigEndian::read_u32(&self.data[20..24])
    }

    pub fn set_checksum(&mut self, checksum: u32) {
        BigEndian::write_u32(&mut self.data[20..24], checksum);
    }

    // --- Capacity and Slot Methods ---

    pub fn slot_count(&self) -> usize {
        let lower = self.lower() as usize;
        if lower < PAGE_HEADER_SIZE {
            0
        } else {
            (lower - PAGE_HEADER_SIZE) / ITEM_ID_SIZE
        }
    }

    pub fn free_space(&self) -> usize {
        let lower = self.lower() as usize;
        let upper = self.upper() as usize;
        if upper >= lower {
            upper - lower
        } else {
            0
        }
    }

    pub fn item_id(&self, slot_id: SlotId) -> Option<ItemId> {
        let count = self.slot_count();
        if (slot_id as usize) >= count {
            return None;
        }
        let offset = PAGE_HEADER_SIZE + (slot_id as usize) * ITEM_ID_SIZE;
        Some(ItemId::read_from(&self.data[offset..offset + ITEM_ID_SIZE]))
    }

    fn write_item_id(&mut self, slot_id: SlotId, item: &ItemId) {
        let offset = PAGE_HEADER_SIZE + (slot_id as usize) * ITEM_ID_SIZE;
        item.write_to(&mut self.data[offset..offset + ITEM_ID_SIZE]);
    }

    /// Insert raw tuple bytes into the slotted page.
    pub fn insert_tuple(&mut self, tuple_data: &[u8]) -> Result<SlotId> {
        let len = tuple_data.len();
        let needed_space = len + ITEM_ID_SIZE;

        if self.free_space() < needed_space {
            return Err(JarvixError::Page(format!(
                "Page {} has insufficient free space: needed {}, available {}",
                self.page_id(),
                needed_space,
                self.free_space()
            )));
        }

        // Check if there is an existing UNUSED or DEAD slot to reuse
        let slot_id = {
            let mut reused_slot = None;
            for s in 0..self.slot_count() as SlotId {
                if let Some(item) = self.item_id(s) {
                    if item.flags == slot_flags::UNUSED || item.flags == slot_flags::DEAD {
                        reused_slot = Some(s);
                        break;
                    }
                }
            }
            reused_slot
        };

        let new_upper = self.upper() - len as u16;
        self.set_upper(new_upper);

        // Copy tuple data to upper area
        let upper_idx = new_upper as usize;
        self.data[upper_idx..upper_idx + len].copy_from_slice(tuple_data);

        let final_slot_id = match slot_id {
            Some(s) => {
                let item = ItemId {
                    offset: new_upper,
                    length: len as u16,
                    flags: slot_flags::USED,
                };
                self.write_item_id(s, &item);
                s
            }
            None => {
                let s = self.slot_count() as SlotId;
                let item = ItemId {
                    offset: new_upper,
                    length: len as u16,
                    flags: slot_flags::USED,
                };
                let new_lower = self.lower() + ITEM_ID_SIZE as u16;
                self.set_lower(new_lower);
                self.write_item_id(s, &item);
                s
            }
        };

        Ok(final_slot_id)
    }

    /// Retrieve reference to tuple bytes for a given slot ID.
    pub fn get_tuple(&self, slot_id: SlotId) -> Option<&[u8]> {
        let item = self.item_id(slot_id)?;
        if item.flags != slot_flags::USED {
            return None;
        }
        let start = item.offset as usize;
        let end = start + item.length as usize;
        if end > PAGE_SIZE {
            return None;
        }
        Some(&self.data[start..end])
    }

    /// Mark a tuple slot as DEAD.
    pub fn delete_tuple(&mut self, slot_id: SlotId) -> Result<()> {
        let mut item = self.item_id(slot_id).ok_or_else(|| {
            JarvixError::Page(format!("Slot {} out of bounds on page {}", slot_id, self.page_id()))
        })?;

        if item.flags != slot_flags::USED {
            return Err(JarvixError::Page(format!(
                "Slot {} on page {} is not in active state",
                slot_id,
                self.page_id()
            )));
        }

        item.flags = slot_flags::DEAD;
        self.write_item_id(slot_id, &item);
        Ok(())
    }

    /// Compacts the page by defragmenting tuple data, reclaiming space from deleted items.
    pub fn compact(&mut self) {
        let count = self.slot_count();
        let mut temp_page = Page::new();
        temp_page.init(self.page_id(), self.page_type());
        temp_page.set_lsn(self.lsn());

        for s in 0..count as SlotId {
            if let Some(item) = self.item_id(s) {
                if item.flags == slot_flags::USED {
                    let tuple_data = &self.data[item.offset as usize..(item.offset + item.length) as usize];
                    let _ = temp_page.insert_tuple(tuple_data);
                }
            }
        }

        self.data.copy_from_slice(&temp_page.data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_initialization() {
        let mut page = Page::new();
        page.init(42, PageType::Heap);
        assert_eq!(page.page_id(), 42);
        assert_eq!(page.page_type(), PageType::Heap);
        assert_eq!(page.lower(), PAGE_HEADER_SIZE as u16);
        assert_eq!(page.upper(), PAGE_SIZE as u16);
        assert_eq!(page.slot_count(), 0);
        assert_eq!(page.free_space(), PAGE_SIZE - PAGE_HEADER_SIZE);
    }

    #[test]
    fn test_tuple_insert_and_get() {
        let mut page = Page::new();
        page.init(1, PageType::Heap);

        let data1 = b"Hello, Jarvix Database Kernel!";
        let data2 = b"Secondary tuple data payload";

        let slot0 = page.insert_tuple(data1).expect("insert slot 0");
        let slot1 = page.insert_tuple(data2).expect("insert slot 1");

        assert_eq!(slot0, 0);
        assert_eq!(slot1, 1);
        assert_eq!(page.slot_count(), 2);

        let retrieved1 = page.get_tuple(slot0).expect("get slot 0");
        let retrieved2 = page.get_tuple(slot1).expect("get slot 1");

        assert_eq!(retrieved1, data1);
        assert_eq!(retrieved2, data2);
    }

    #[test]
    fn test_tuple_deletion_and_compaction() {
        let mut page = Page::new();
        page.init(1, PageType::Heap);

        let data1 = b"Record A";
        let data2 = b"Record B - to be deleted";
        let data3 = b"Record C";

        let s0 = page.insert_tuple(data1).unwrap();
        let s1 = page.insert_tuple(data2).unwrap();
        let s2 = page.insert_tuple(data3).unwrap();

        page.delete_tuple(s1).unwrap();
        assert!(page.get_tuple(s1).is_none());
        assert_eq!(page.get_tuple(s0).unwrap(), data1);
        assert_eq!(page.get_tuple(s2).unwrap(), data3);

        page.compact();
        assert_eq!(page.get_tuple(0).unwrap(), data1);
        assert_eq!(page.get_tuple(1).unwrap(), data3);
    }
}
