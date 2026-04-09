//! QSPI flash layer: init, JEDEC verification, and ekv Flash trait implementation.
//!
//! Provides the `QspiFlash` type that implements `ekv::flash::Flash`, the
//! `init_qspi()` helper that verifies the chip via JEDEC ID, and the interrupt
//! binding used by both this module and callers.

use core::cell::UnsafeCell;

use embassy_nrf::{Peri, bind_interrupts, peripherals, qspi};
use ekv::flash::PageID;

// ---------------------------------------------------------------------------
// Flash geometry
// ---------------------------------------------------------------------------

/// One erase sector on the ZD25WQ16 (4 KiB). Also ekv's PAGE_SIZE.
pub const PAGE_SIZE: usize = 4096;

/// Flash chip capacity in bytes (ZD25WQ16CTIGT: 16 Mbit = 2 MiB).
pub const FLASH_TOTAL_BYTES: usize = 2 * 1024 * 1024;

/// Fraction of flash dedicated to the KV store (1/2 = 1 MiB).
pub const KV_FLASH_BYTES: usize = FLASH_TOTAL_BYTES / 2;

/// Number of 4 KiB pages in the KV store (256 pages = 1 MiB).
pub const KV_PAGE_COUNT: usize = KV_FLASH_BYTES / PAGE_SIZE;

// ---------------------------------------------------------------------------
// Interrupt binding
// ---------------------------------------------------------------------------

bind_interrupts!(pub struct QspiIrqs {
    QSPI => qspi::InterruptHandler<peripherals::QSPI>;
});

// ---------------------------------------------------------------------------
// Aligned staging buffer for QSPI DMA
// ---------------------------------------------------------------------------

/// `Qspi::read` and `Qspi::write` require the data buffer pointer to be
/// 4-byte aligned.  Slices coming from ekv may not satisfy this, so we
/// always bounce through this staging area.
///
/// Safety: ekv's `Database` serialises all flash ops through its internal
/// mutex, so only one flash operation runs at a time and this buffer is
/// never accessed concurrently.
#[repr(C, align(4))]
struct AlignedBuf([u8; PAGE_SIZE]);

struct StagingCell(UnsafeCell<AlignedBuf>);
unsafe impl Sync for StagingCell {}

static STAGING: StagingCell = StagingCell(UnsafeCell::new(AlignedBuf([0u8; PAGE_SIZE])));

// ---------------------------------------------------------------------------
// QspiFlash — ekv Flash implementation
// ---------------------------------------------------------------------------

pub struct QspiFlash {
    pub qspi: qspi::Qspi<'static>,
}

impl ekv::flash::Flash for QspiFlash {
    type Error = qspi::Error;

    fn page_count(&self) -> usize {
        KV_PAGE_COUNT
    }

    async fn erase(&mut self, page_id: PageID) -> Result<(), Self::Error> {
        let addr = (page_id.index() * PAGE_SIZE) as u32;
        self.qspi.erase(addr).await
    }

    async fn read(&mut self, page_id: PageID, offset: usize, data: &mut [u8]) -> Result<(), Self::Error> {
        let addr = (page_id.index() * PAGE_SIZE + offset) as u32;
        // Safety: single-task access guaranteed by ekv's internal mutex.
        let buf = unsafe { &mut (*STAGING.0.get()).0 };
        self.qspi.read(addr, &mut buf[..data.len()]).await?;
        data.copy_from_slice(&buf[..data.len()]);
        Ok(())
    }

    async fn write(&mut self, page_id: PageID, offset: usize, data: &[u8]) -> Result<(), Self::Error> {
        let addr = (page_id.index() * PAGE_SIZE + offset) as u32;
        // Safety: single-task access guaranteed by ekv's internal mutex.
        let buf = unsafe { &mut (*STAGING.0.get()).0 };
        buf[..data.len()].copy_from_slice(data);
        self.qspi.write(addr, &buf[..data.len()]).await
    }
}

// ---------------------------------------------------------------------------
// QSPI initialisation
// ---------------------------------------------------------------------------

/// Initialise the QSPI peripheral and verify the flash chip via JEDEC ID.
///
/// Returns the `Qspi` instance on success, or the raw JEDEC bytes on failure
/// (all-0xFF = no device, all-0x00 = bus fault).
pub fn init_qspi<'d>(
    qspi_periph: Peri<'d, peripherals::QSPI>,
    irqs: QspiIrqs,
    sck: Peri<'d, peripherals::P0_21>,
    csn: Peri<'d, peripherals::P0_25>,
    io0: Peri<'d, peripherals::P0_20>,
    io1: Peri<'d, peripherals::P0_24>,
    io2: Peri<'d, peripherals::P0_22>,
    io3: Peri<'d, peripherals::P0_23>,
) -> Result<qspi::Qspi<'d>, [u8; 3]> {
    // ZD25WQ16CTIGT: 16 Mbit = 2 MiB. Use single-SPI opcodes (FASTREAD/PP)
    // rather than quad I/O — quad requires the QE status-register bit which we
    // do not configure.  Single-SPI is adequate for infrequent KV store access.
    let mut cfg = qspi::Config::default();
    cfg.capacity = FLASH_TOTAL_BYTES as u32;
    cfg.read_opcode = qspi::ReadOpcode::FASTREAD;
    cfg.write_opcode = qspi::WriteOpcode::PP;

    let mut qspi = qspi::Qspi::new(qspi_periph, irqs, sck, csn, io0, io1, io2, io3, cfg);

    let mut jedec = [0u8; 3];
    let _ = qspi.blocking_custom_instruction(0x9F, &[], &mut jedec);
    if jedec == [0xFF; 3] || jedec == [0x00; 3] {
        return Err(jedec);
    }

    defmt::info!(
        "QSPI flash JEDEC ID: {:02X} {:02X} {:02X}",
        jedec[0], jedec[1], jedec[2]
    );
    Ok(qspi)
}
