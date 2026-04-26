//! ekv Flash trait implementation over the shared flash module.
//!
//! Delegates all operations to [`crate::fw::flash`] which owns the QSPI
//! peripheral behind an async mutex.  The ekv partition starts at
//! [`crate::fw::flash::KV_OFFSET`].

use crate::fw::flash;
use ekv::flash::PageID;

/// Number of 4 KiB pages available to ekv.
pub const KV_PAGE_COUNT: usize = flash::KV_PAGES;

/// Adapter that implements `ekv::flash::Flash` via the shared flash module.
pub struct SharedFlash;

impl ekv::flash::Flash for SharedFlash {
    type Error = flash::FlashError;

    fn page_count(&self) -> usize {
        KV_PAGE_COUNT
    }

    async fn erase(&mut self, page_id: PageID) -> Result<(), Self::Error> {
        let addr = flash::KV_OFFSET + (page_id.index() * flash::PAGE_SIZE) as u32;
        flash::erase(addr).await
    }

    async fn read(
        &mut self,
        page_id: PageID,
        offset: usize,
        data: &mut [u8],
    ) -> Result<(), Self::Error> {
        let addr = flash::KV_OFFSET + (page_id.index() * flash::PAGE_SIZE + offset) as u32;
        flash::read(addr, data).await
    }

    async fn write(
        &mut self,
        page_id: PageID,
        offset: usize,
        data: &[u8],
    ) -> Result<(), Self::Error> {
        let addr = flash::KV_OFFSET + (page_id.index() * flash::PAGE_SIZE + offset) as u32;
        flash::write(addr, data).await
    }
}
