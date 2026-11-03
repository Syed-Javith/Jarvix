use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, RwLock};

use jarvix_common::constants::INVALID_PAGE_ID;
use jarvix_common::error::{JarvixError, Result};
use jarvix_common::types::PageId;
use jarvix_storage::{DiskManager, Page, PageType};

use crate::frame::BufferFrame;
use crate::guard::{PageReadGuard, PageWriteGuard};
use crate::replacer::{ClockReplacer, FrameId};

pub struct BufferPoolInner {
    pub(crate) pool_size: usize,
    pub(crate) frames: Vec<BufferFrame>,
    pub(crate) page_table: Mutex<HashMap<PageId, FrameId>>,
    pub(crate) replacer: Mutex<ClockReplacer>,
    pub(crate) free_list: Mutex<Vec<FrameId>>,
    pub(crate) disk_manager: Arc<DiskManager>,
}

impl BufferPoolInner {
    pub(crate) fn unpin_frame(&self, frame_id: FrameId, is_dirty: bool) {
        let frame = &self.frames[frame_id];
        if is_dirty {
            frame.is_dirty.store(true, Ordering::Release);
        }

        let prev = frame.pin_count.fetch_sub(1, Ordering::SeqCst);
        if prev == 1 {
            let mut replacer = self.replacer.lock().unwrap();
            replacer.set_evictable(frame_id, true);
        }
    }
}

/// BufferPoolManager coordinates cached pages between in-memory frames and on-disk storage.
#[derive(Clone)]
pub struct BufferPoolManager {
    inner: Arc<BufferPoolInner>,
}

impl BufferPoolManager {
    pub fn new(pool_size: usize, disk_manager: Arc<DiskManager>) -> Self {
        let mut frames = Vec::with_capacity(pool_size);
        let mut free_list = Vec::with_capacity(pool_size);

        for i in 0..pool_size {
            frames.push(BufferFrame::new());
            free_list.push(i);
        }

        let inner = Arc::new(BufferPoolInner {
            pool_size,
            frames,
            page_table: Mutex::new(HashMap::new()),
            replacer: Mutex::new(ClockReplacer::new(pool_size)),
            free_list: Mutex::new(free_list),
            disk_manager,
        });

        Self { inner }
    }

    pub fn pool_size(&self) -> usize {
        self.inner.pool_size
    }

    pub fn disk_manager(&self) -> &Arc<DiskManager> {
        &self.inner.disk_manager
    }

    /// Fetches a page for reading, returning an RAII PageReadGuard.
    pub fn fetch_page(&self, page_id: PageId) -> Result<PageReadGuard<'_>> {
        let frame_id = self.find_or_load_frame(page_id)?;
        let frame = &self.inner.frames[frame_id];

        // Safety note: Guard holds Arc to BufferPoolInner keeping the frames alive
        let guard = unsafe {
            let page_ref: &RwLock<Page> = &*(&frame.page as *const _);
            page_ref.read().unwrap()
        };

        Ok(PageReadGuard {
            frame_id,
            guard,
            inner: Arc::clone(&self.inner),
        })
    }

    /// Fetches a page for writing, returning an RAII PageWriteGuard.
    pub fn fetch_page_mut(&self, page_id: PageId) -> Result<PageWriteGuard<'_>> {
        let frame_id = self.find_or_load_frame(page_id)?;
        let frame = &self.inner.frames[frame_id];

        let guard = unsafe {
            let page_ref: &RwLock<Page> = &*(&frame.page as *const _);
            page_ref.write().unwrap()
        };

        Ok(PageWriteGuard {
            frame_id,
            guard,
            inner: Arc::clone(&self.inner),
        })
    }

    /// Allocates a new physical page on disk and returns its write guard.
    pub fn new_page(&self, page_type: PageType) -> Result<(PageId, PageWriteGuard<'_>)> {
        let new_page_id = self.inner.disk_manager.allocate_page()?;

        let mut page_table = self.inner.page_table.lock().unwrap();
        let frame_id = self.acquire_frame(&mut page_table)?;

        let frame = &self.inner.frames[frame_id];
        frame.page_id.store(new_page_id, Ordering::Release);
        frame.pin_count.store(1, Ordering::SeqCst);
        frame.is_dirty.store(true, Ordering::Release);

        page_table.insert(new_page_id, frame_id);

        let mut write_guard = frame.page.write().unwrap();
        write_guard.init(new_page_id, page_type);

        let guard = unsafe {
            let page_ref: &RwLock<Page> = &*(&frame.page as *const _);
            drop(write_guard);
            page_ref.write().unwrap()
        };

        Ok((
            new_page_id,
            PageWriteGuard {
                frame_id,
                guard,
                inner: Arc::clone(&self.inner),
            },
        ))
    }

    /// Flushes a specific page to disk if it is currently buffered and dirty.
    pub fn flush_page(&self, page_id: PageId) -> Result<()> {
        let page_table = self.inner.page_table.lock().unwrap();
        if let Some(&frame_id) = page_table.get(&page_id) {
            let frame = &self.inner.frames[frame_id];
            let page_guard = frame.page.read().unwrap();
            self.inner.disk_manager.write_page(page_id, &page_guard)?;
            frame.is_dirty.store(false, Ordering::Release);
        }
        Ok(())
    }

    /// Flushes all dirty pages in the buffer pool to disk.
    pub fn flush_all(&self) -> Result<()> {
        let page_table = self.inner.page_table.lock().unwrap();
        for (&page_id, &frame_id) in page_table.iter() {
            let frame = &self.inner.frames[frame_id];
            if frame.is_dirty.load(Ordering::Acquire) {
                let page_guard = frame.page.read().unwrap();
                self.inner.disk_manager.write_page(page_id, &page_guard)?;
                frame.is_dirty.store(false, Ordering::Release);
            }
        }
        self.inner.disk_manager.sync()?;
        Ok(())
    }

    fn find_or_load_frame(&self, page_id: PageId) -> Result<FrameId> {
        let mut page_table = self.inner.page_table.lock().unwrap();

        // Cache hit
        if let Some(&frame_id) = page_table.get(&page_id) {
            let frame = &self.inner.frames[frame_id];
            frame.pin_count.fetch_add(1, Ordering::SeqCst);
            self.inner.replacer.lock().unwrap().set_evictable(frame_id, false);
            return Ok(frame_id);
        }

        // Cache miss: find frame from free list or evict victim
        let frame_id = self.acquire_frame(&mut page_table)?;
        let frame = &self.inner.frames[frame_id];

        // Load page from disk
        {
            let mut page_guard = frame.page.write().unwrap();
            self.inner.disk_manager.read_page(page_id, &mut page_guard)?;
        }

        frame.page_id.store(page_id, Ordering::Release);
        frame.pin_count.store(1, Ordering::SeqCst);
        frame.is_dirty.store(false, Ordering::Release);

        page_table.insert(page_id, frame_id);
        self.inner.replacer.lock().unwrap().set_evictable(frame_id, false);

        Ok(frame_id)
    }

    fn acquire_frame(&self, page_table: &mut HashMap<PageId, FrameId>) -> Result<FrameId> {
        // Try free list first
        if let Some(frame_id) = self.inner.free_list.lock().unwrap().pop() {
            return Ok(frame_id);
        }

        // Evict from replacer
        let victim_id = self.inner.replacer.lock().unwrap().victim().ok_or_else(|| {
            JarvixError::BufferPool("All buffer pool frames are currently pinned".to_string())
        })?;

        let frame = &self.inner.frames[victim_id];
        let old_page_id = frame.page_id.load(Ordering::Acquire);

        if old_page_id != INVALID_PAGE_ID {
            // Flush old page if dirty
            if frame.is_dirty.load(Ordering::Acquire) {
                let page_guard = frame.page.read().unwrap();
                self.inner.disk_manager.write_page(old_page_id, &page_guard)?;
                frame.is_dirty.store(false, Ordering::Release);
            }
            page_table.remove(&old_page_id);
        }

        Ok(victim_id)
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
    fn test_buffer_pool_new_page_and_fetch() {
        let temp_file = TempDbFile::new("buffer");
        let disk_mgr = Arc::new(DiskManager::open(temp_file.path()).unwrap());
        let bpm = BufferPoolManager::new(2, disk_mgr);

        let (page_id_1, mut guard1) = bpm.new_page(PageType::Heap).unwrap();
        assert_eq!(page_id_1, 1);
        guard1.insert_tuple(b"Tuple on Page 1").unwrap();
        drop(guard1);

        let (page_id_2, mut guard2) = bpm.new_page(PageType::Heap).unwrap();
        assert_eq!(page_id_2, 2);
        guard2.insert_tuple(b"Tuple on Page 2").unwrap();
        drop(guard2);

        // Read Page 1 back
        let read1 = bpm.fetch_page(page_id_1).unwrap();
        assert_eq!(read1.get_tuple(0).unwrap(), b"Tuple on Page 1");
        drop(read1);

        // Cause eviction by creating Page 3 (pool size is 2)
        let (page_id_3, mut guard3) = bpm.new_page(PageType::Heap).unwrap();
        assert_eq!(page_id_3, 3);
        guard3.insert_tuple(b"Tuple on Page 3").unwrap();
        drop(guard3);

        // Fetch evicted page back from disk
        let read_evicted = bpm.fetch_page(page_id_1).unwrap();
        assert_eq!(read_evicted.get_tuple(0).unwrap(), b"Tuple on Page 1");
    }
}
