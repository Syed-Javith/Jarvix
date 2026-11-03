use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use std::sync::Mutex;
use byteorder::{BigEndian, ByteOrder};

use jarvix_common::constants::{DB_FILE_MAGIC, DB_FORMAT_VERSION, PAGE_SIZE};
use jarvix_common::error::{JarvixError, Result};
use jarvix_common::types::PageId;

use crate::page::Page;

/// DiskManager handles low-level 8 KB page reads, writes, and allocations to database files.
pub struct DiskManager {
    file_path: PathBuf,
    file: Mutex<File>,
    num_pages: AtomicU32,
}

impl DiskManager {
    /// Opens an existing database file or creates a new one initialized with a DB header.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file_path = path.as_ref().to_path_buf();
        let exists = file_path.exists();

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&file_path)?;

        let page_count = if !exists || file.metadata()?.len() == 0 {
            // Write database file header in Page 0 (Metadata Page)
            let mut meta_page = Page::new();
            meta_page.data[0..4].copy_from_slice(&DB_FILE_MAGIC);
            BigEndian::write_u32(&mut meta_page.data[4..8], DB_FORMAT_VERSION);
            BigEndian::write_u32(&mut meta_page.data[8..12], 1); // 1 page (header page)

            file.seek(SeekFrom::Start(0))?;
            file.write_all(&meta_page.data)?;
            file.flush()?;
            1
        } else {
            let file_len = file.metadata()?.len();
            if file_len % (PAGE_SIZE as u64) != 0 {
                return Err(JarvixError::Storage(format!(
                    "Corrupted database file size {}: not a multiple of PAGE_SIZE {}",
                    file_len, PAGE_SIZE
                )));
            }
            (file_len / PAGE_SIZE as u64) as u32
        };

        Ok(Self {
            file_path,
            file: Mutex::new(file),
            num_pages: AtomicU32::new(page_count),
        })
    }

    pub fn num_pages(&self) -> u32 {
        self.num_pages.load(Ordering::Acquire)
    }

    /// Allocates a new physical page at the end of the file and returns its PageId.
    pub fn allocate_page(&self) -> Result<PageId> {
        let mut file = self.file.lock().map_err(|e| JarvixError::Storage(e.to_string()))?;
        let new_page_id = self.num_pages.fetch_add(1, Ordering::SeqCst);

        let offset = (new_page_id as u64) * (PAGE_SIZE as u64);
        file.seek(SeekFrom::Start(offset))?;

        let empty_page = [0u8; PAGE_SIZE];
        file.write_all(&empty_page)?;

        Ok(new_page_id)
    }

    /// Reads an 8 KB page from disk into the target page buffer.
    pub fn read_page(&self, page_id: PageId, page: &mut Page) -> Result<()> {
        let mut file = self.file.lock().map_err(|e| JarvixError::Storage(e.to_string()))?;
        let offset = (page_id as u64) * (PAGE_SIZE as u64);
        let file_len = file.metadata()?.len();

        if offset + (PAGE_SIZE as u64) > file_len {
            return Err(JarvixError::Storage(format!(
                "Read out of bounds: page_id {} beyond file length {}",
                page_id, file_len
            )));
        }

        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(&mut page.data)?;
        Ok(())
    }

    /// Writes an 8 KB page buffer directly to the disk at page_id offset.
    pub fn write_page(&self, page_id: PageId, page: &Page) -> Result<()> {
        let mut file = self.file.lock().map_err(|e| JarvixError::Storage(e.to_string()))?;
        let offset = (page_id as u64) * (PAGE_SIZE as u64);

        file.seek(SeekFrom::Start(offset))?;
        file.write_all(&page.data)?;
        Ok(())
    }

    /// Forces all buffered database page writes to physical storage (fsync).
    pub fn sync(&self) -> Result<()> {
        let file = self.file.lock().map_err(|e| JarvixError::Storage(e.to_string()))?;
        file.sync_data()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDbFile(std::path::PathBuf);
    impl TempDbFile {
        fn new(name: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!("jarvix_test_{}_{}.db", name, nanos));
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
    fn test_disk_manager_alloc_write_read() {
        let temp_file = TempDbFile::new("storage");
        let disk_mgr = DiskManager::open(temp_file.path()).unwrap();

        assert_eq!(disk_mgr.num_pages(), 1); // Header page

        let p1 = disk_mgr.allocate_page().unwrap();
        assert_eq!(p1, 1);

        let mut write_page = Page::new();
        write_page.init(p1, crate::page::PageType::Heap);
        write_page.insert_tuple(b"Persisted disk tuple").unwrap();

        disk_mgr.write_page(p1, &write_page).unwrap();
        disk_mgr.sync().unwrap();

        let mut read_page = Page::new();
        disk_mgr.read_page(p1, &mut read_page).unwrap();

        assert_eq!(read_page.page_id(), p1);
        assert_eq!(read_page.get_tuple(0).unwrap(), b"Persisted disk tuple");
    }
}
