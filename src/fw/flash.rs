//! Shared external flash access with mutex-protected multiplexing.
//!
//! The external QSPI flash (2 MiB) is partitioned into:
//!   - **ekv KV store** — first 1 MiB (0x000000–0x0FFFFF)
//!   - **FAT12 / USB mass storage** — second 1 MiB (0x100000–0x1FFFFF)
//!
//! This module owns the flash behind an async mutex so concurrent callers
//! (ekv and USB MSC) are serialized automatically.

use embassy_nrf::{Peri, bind_interrupts, peripherals, qspi};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;

// ---------------------------------------------------------------------------
// Flash geometry
// ---------------------------------------------------------------------------

/// One erase sector (4 KiB).
pub const PAGE_SIZE: usize = 4096;

/// Flash chip capacity in bytes (2 MiB).
pub const FLASH_TOTAL_BYTES: usize = 2 * 1024 * 1024;

/// ekv KV store partition: first 1 MiB (0x000000–0x0FFFFF).
pub const KV_OFFSET: u32 = 0;
pub const KV_BYTES: usize = 1024 * 1024;
pub const KV_PAGES: usize = KV_BYTES / PAGE_SIZE;

/// FAT12 partition: second 1 MiB (0x100000–0x1FFFFF).
pub const FAT_OFFSET: u32 = KV_BYTES as u32;
pub const FAT_BYTES: usize = FLASH_TOTAL_BYTES - KV_BYTES;
pub const FAT_PAGES: usize = FAT_BYTES / PAGE_SIZE;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum FlashError {
    OutOfBounds,
    Hardware,
    NotInitialised,
}

// ---------------------------------------------------------------------------
// Singleton QSPI instance + aligned DMA buffer
// ---------------------------------------------------------------------------

bind_interrupts!(struct QspiIrqs {
    QSPI => qspi::InterruptHandler<peripherals::QSPI>;
});

#[repr(C, align(4))]
struct AlignedBuf([u8; PAGE_SIZE]);

static FLASH: Mutex<CriticalSectionRawMutex, Option<(qspi::Qspi<'static>, AlignedBuf)>> =
    Mutex::new(None);

// ---------------------------------------------------------------------------
// Initialisation
// ---------------------------------------------------------------------------

/// Initialise the QSPI peripheral and verify the flash chip via JEDEC ID.
///
/// Call once at startup before any flash access (KV store, USB storage).
pub async fn init(
    qspi_periph: Peri<'_, peripherals::QSPI>,
    sck: Peri<'_, peripherals::P0_21>,
    csn: Peri<'_, peripherals::P0_25>,
    io0: Peri<'_, peripherals::P0_20>,
    io1: Peri<'_, peripherals::P0_24>,
    io2: Peri<'_, peripherals::P0_22>,
    io3: Peri<'_, peripherals::P0_23>,
) -> Result<(), [u8; 3]> {
    let mut cfg = qspi::Config::default();
    cfg.capacity = FLASH_TOTAL_BYTES as u32;
    cfg.read_opcode = qspi::ReadOpcode::FASTREAD;
    cfg.write_opcode = qspi::WriteOpcode::PP;

    let mut qspi = qspi::Qspi::new(qspi_periph, QspiIrqs, sck, csn, io0, io1, io2, io3, cfg);

    let mut jedec = [0u8; 3];
    let _ = qspi.blocking_custom_instruction(0x9F, &[], &mut jedec);
    if jedec == [0xFF; 3] || jedec == [0x00; 3] {
        return Err(jedec);
    }

    defmt::info!(
        "QSPI flash JEDEC ID: {:02X} {:02X} {:02X}",
        jedec[0],
        jedec[1],
        jedec[2],
    );

    // Safety: init() is called from main() which never returns.
    let qspi: qspi::Qspi<'static> = unsafe { core::mem::transmute(qspi) };

    let mut guard = FLASH.lock().await;
    *guard = Some((qspi, AlignedBuf([0u8; PAGE_SIZE])));
    Ok(())
}

// ---------------------------------------------------------------------------
// Flash operations (mutex-protected)
// ---------------------------------------------------------------------------

/// Read bytes from absolute flash address.
///
/// Handles QSPI alignment requirements internally: the address and length
/// passed to the hardware are always 4-byte aligned via the bounce buffer.
pub async fn read(addr: u32, data: &mut [u8]) -> Result<(), FlashError> {
    let mut guard = FLASH.lock().await;
    let (qspi, buf) = guard.as_mut().ok_or(FlashError::NotInitialised)?;

    let mut remaining = data.len();
    let mut data_off = 0usize;
    let mut flash_addr = addr;

    while remaining > 0 {
        // Align address down to 4 bytes.
        let aligned_addr = flash_addr & !3;
        let skip = (flash_addr - aligned_addr) as usize;
        // Read enough to cover skip + remaining, rounded up to 4 bytes, capped to
        // buffer.
        let raw_len = ((skip + remaining + 3) & !3).min(PAGE_SIZE);
        qspi.read(aligned_addr, &mut buf.0[..raw_len])
            .await
            .map_err(|_| FlashError::Hardware)?;
        let usable = (raw_len - skip).min(remaining);
        data[data_off..data_off + usable].copy_from_slice(&buf.0[skip..skip + usable]);
        data_off += usable;
        flash_addr += usable as u32;
        remaining -= usable;
    }
    Ok(())
}

/// Write bytes to absolute flash address.  Target must be erased first.
///
/// Handles QSPI alignment requirements: address and length are 4-byte
/// aligned via the bounce buffer with read-modify-write for partial words.
pub async fn write(addr: u32, data: &[u8]) -> Result<(), FlashError> {
    let mut guard = FLASH.lock().await;
    let (qspi, buf) = guard.as_mut().ok_or(FlashError::NotInitialised)?;

    let mut remaining = data.len();
    let mut data_off = 0usize;
    let mut flash_addr = addr;

    while remaining > 0 {
        let aligned_addr = flash_addr & !3;
        let skip = (flash_addr - aligned_addr) as usize;
        let raw_len = ((skip + remaining + 3) & !3).min(PAGE_SIZE);

        // If partial word at start or end, read existing data first.
        if skip > 0 || (skip + remaining) < raw_len {
            qspi.read(aligned_addr, &mut buf.0[..raw_len])
                .await
                .map_err(|_| FlashError::Hardware)?;
        }

        let usable = (raw_len - skip).min(remaining);
        buf.0[skip..skip + usable].copy_from_slice(&data[data_off..data_off + usable]);
        qspi.write(aligned_addr, &buf.0[..raw_len])
            .await
            .map_err(|_| FlashError::Hardware)?;

        data_off += usable;
        flash_addr += usable as u32;
        remaining -= usable;
    }
    Ok(())
}

/// Erase one 4 KiB sector at absolute flash address (must be sector-aligned).
pub async fn erase(addr: u32) -> Result<(), FlashError> {
    let mut guard = FLASH.lock().await;
    let (qspi, _) = guard.as_mut().ok_or(FlashError::NotInitialised)?;
    qspi.erase(addr).await.map_err(|_| FlashError::Hardware)
}
