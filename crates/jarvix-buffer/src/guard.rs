use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::sync::{RwLockReadGuard, RwLockWriteGuard};

use jarvix_storage::Page;
use crate::manager::BufferPoolInner;
use crate::replacer::FrameId;

/// RAII Guard for reading a buffered page.
/// Automatically handles pin decrements and replacer updates upon drop.
pub struct PageReadGuard<'a> {
    pub(crate) frame_id: FrameId,
    pub(crate) guard: RwLockReadGuard<'a, Page>,
    pub(crate) inner: Arc<BufferPoolInner>,
}

impl<'a> Deref for PageReadGuard<'a> {
    type Target = Page;

    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}

impl<'a> Drop for PageReadGuard<'a> {
    fn drop(&mut self) {
        self.inner.unpin_frame(self.frame_id, false);
    }
}

/// RAII Guard for writing to a buffered page.
/// Automatically sets dirty flag and manages pin counts upon drop.
pub struct PageWriteGuard<'a> {
    pub(crate) frame_id: FrameId,
    pub(crate) guard: RwLockWriteGuard<'a, Page>,
    pub(crate) inner: Arc<BufferPoolInner>,
}

impl<'a> Deref for PageWriteGuard<'a> {
    type Target = Page;

    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}

impl<'a> DerefMut for PageWriteGuard<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.guard
    }
}

impl<'a> Drop for PageWriteGuard<'a> {
    fn drop(&mut self) {
        self.inner.unpin_frame(self.frame_id, true);
    }
}
