//! Standalone async QSPI flash access for the bootloader.
//!
//! Self-contained port of the app's `src/fw/flash.rs` (no `crate::fw`
//! dependency) so DFU mode can expose the FAT12 partition over USB MSC.
//!
//! Geometry MUST match the app exactly so the volume the bootloader formats
//! is the same one the app reads/writes:
//!   - ekv KV store — first 1 MiB (0x000000–0x0FFFFF)
//!   - FAT12 / USB mass storage — second 1 MiB (0x100000–0x1FFFFF)

use embassy_nrf::{Peri, bind_interrupts, peripherals, qspi};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;

/// One erase sector (4 KiB).
pub const PAGE_SIZE: usize = 4096;

/// Flash chip capacity in bytes (2 MiB — ZD25WQ16C).
pub const FLASH_TOTAL_BYTES: usize = 2 * 1024 * 1024;

/// ekv KV store partition: first 1 MiB.
pub const KV_BYTES: usize = 1024 * 1024;

/// FAT12 partition: second 1 MiB (0x100000–0x1FFFFF).
pub const FAT_OFFSET: u32 = KV_BYTES as u32;
pub const FAT_BYTES: usize = FLASH_TOTAL_BYTES - KV_BYTES;

#[derive(Debug, Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum FlashError {
    Hardware,
    NotInitialised,
}

bind_interrupts!(pub struct QspiIrqs {
    QSPI => qspi::InterruptHandler<peripherals::QSPI>;
});

#[repr(C, align(4))]
struct AlignedBuf([u8; PAGE_SIZE]);

static FLASH: Mutex<CriticalSectionRawMutex, Option<(qspi::Qspi<'static>, AlignedBuf)>> =
    Mutex::new(None);

/// Initialise the QSPI peripheral and verify the chip via JEDEC ID.
/// Call once before any flash access.
pub async fn init(
    qspi_periph: Peri<'static, peripherals::QSPI>,
    sck: Peri<'static, peripherals::P0_21>,
    csn: Peri<'static, peripherals::P0_25>,
    io0: Peri<'static, peripherals::P0_20>,
    io1: Peri<'static, peripherals::P0_24>,
    io2: Peri<'static, peripherals::P0_22>,
    io3: Peri<'static, peripherals::P0_23>,
) -> Result<(), [u8; 3]> {
    let mut cfg = qspi::Config::default();
    cfg.capacity = FLASH_TOTAL_BYTES as u32;
    cfg.read_opcode = qspi::ReadOpcode::FASTREAD;
    cfg.write_opcode = qspi::WriteOpcode::PP;
    cfg.frequency = qspi::Frequency::M32;

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

    let mut guard = FLASH.lock().await;
    *guard = Some((qspi, AlignedBuf([0u8; PAGE_SIZE])));
    Ok(())
}

/// Read bytes from absolute flash address (4-byte alignment handled
/// internally via a bounce buffer).
pub async fn read(addr: u32, data: &mut [u8]) -> Result<(), FlashError> {
    let mut guard = FLASH.lock().await;
    let (qspi, buf) = guard.as_mut().ok_or(FlashError::NotInitialised)?;

    let mut remaining = data.len();
    let mut data_off = 0usize;
    let mut flash_addr = addr;

    while remaining > 0 {
        let aligned_addr = flash_addr & !3;
        let skip = (flash_addr - aligned_addr) as usize;
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
