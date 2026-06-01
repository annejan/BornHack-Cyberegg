//! FAT12 storage backend for the bootloader's USB MSC interface.
//!
//! Bundles three things ported from the app firmware so DFU mode can expose
//! the *same* FAT12 volume the app uses:
//!   - device ID from FICR (per-badge volume label `CYBR` + 4 hex)
//!   - `format_if_needed()` — format-if-blank, identical geometry/label to
//!     the app's `src/fw/fat12.rs::format()`
//!   - `FatBlockDevice` — 512-byte LBA block device over the FAT partition
//!     (impl of [`crate::msc::BlockDevice`])

use core::sync::atomic::Ordering;

use crate::flash;
use crate::msc::BlockDevice;

// ---------------------------------------------------------------------------
// Device identity (FICR DEVICEADDR[0])
// ---------------------------------------------------------------------------

/// Two-byte device ID from the factory-programmed FICR device address.
fn device_id() -> [u8; 2] {
    let b = embassy_nrf::pac::FICR.deviceaddr(0).read().to_le_bytes();
    [b[0], b[1]]
}

/// Device ID as four uppercase ASCII hex bytes, e.g. `b"A3F7"`.
fn device_id_hex() -> [u8; 4] {
    let [id0, id1] = device_id();
    let h = |n: u8| if n < 10 { b'0' + n } else { b'A' + n - 10 };
    [h(id0 >> 4), h(id0 & 0xF), h(id1 >> 4), h(id1 & 0xF)]
}

// ---------------------------------------------------------------------------
// FAT12 format (mirror of src/fw/fat12.rs)
// ---------------------------------------------------------------------------

/// Detect a valid FAT12 boot sector (jump byte + non-zero geometry).
async fn is_formatted() -> bool {
    let mut buf = [0u8; 64];
    if flash::read(flash::FAT_OFFSET, &mut buf).await.is_err() {
        return false;
    }
    let bps = u16::from_le_bytes([buf[11], buf[12]]);
    (buf[0] == 0xEB || buf[0] == 0xE9) && bps != 0 && buf[13] != 0
}

/// Format the FAT partition as FAT12 with the per-badge volume label
/// (`CYBR` + 4 hex of the MCU device ID) — identical geometry to the app.
async fn format() -> Result<(), flash::FlashError> {
    let id_hex = device_id_hex();
    let [id0, id1] = device_id();
    let mut label = [b' '; 11];
    label[..4].copy_from_slice(b"CYBR");
    label[4..8].copy_from_slice(&id_hex);
    let serial = u32::from_le_bytes([id0, id1, 0x00, 0x00]);

    // -- Boot sector (sector 0) --
    let mut boot = [0u8; 512];
    boot[0] = 0xEB;
    boot[1] = 0x3C;
    boot[2] = 0x90;
    boot[3..11].copy_from_slice(b"mkfs.fat");
    boot[11..13].copy_from_slice(&512u16.to_le_bytes()); // bytes per sector
    boot[13] = 4; // sectors per cluster
    boot[14..16].copy_from_slice(&1u16.to_le_bytes()); // reserved sectors
    boot[16] = 2; // number of FATs
    boot[17..19].copy_from_slice(&512u16.to_le_bytes()); // root entry count
    boot[19..21].copy_from_slice(&2048u16.to_le_bytes()); // total sectors (1 MiB / 512)
    boot[21] = 0xF8; // media descriptor
    boot[22..24].copy_from_slice(&2u16.to_le_bytes()); // sectors per FAT
    boot[24..26].copy_from_slice(&2u16.to_le_bytes()); // sectors per track (dummy)
    boot[26..28].copy_from_slice(&1u16.to_le_bytes()); // heads (dummy)
    boot[36] = 0x80; // drive number
    boot[38] = 0x29; // extended boot signature
    boot[39..43].copy_from_slice(&serial.to_le_bytes());
    boot[43..54].copy_from_slice(&label);
    boot[54..62].copy_from_slice(b"FAT12   ");
    boot[510] = 0x55;
    boot[511] = 0xAA;

    flash::erase(flash::FAT_OFFSET).await?;
    flash::write(flash::FAT_OFFSET, &boot).await?;

    // -- FAT tables (FAT1 at sector 1, FAT2 at sector 3) --
    let mut fat = [0u8; 1024];
    fat[0] = 0xF8;
    fat[1] = 0xFF;
    fat[2] = 0xFF;
    let fat1_addr = flash::FAT_OFFSET + 512;
    let fat2_addr = fat1_addr + 1024;

    // Erase pages 1–4 covering FATs + root directory (page 0 erased above).
    for page in 1..5 {
        flash::erase(flash::FAT_OFFSET + page * flash::PAGE_SIZE as u32).await?;
    }

    flash::write(fat1_addr, &fat).await?;
    flash::write(fat2_addr, &fat).await?;

    // -- Root directory (sectors 5–36 = 512 entries) --
    let root_addr = flash::FAT_OFFSET + 5 * 512;
    let root_size = 512u32 * 32;

    let mut first_sector = [0u8; 512];
    first_sector[0..11].copy_from_slice(&label);
    first_sector[11] = 0x08; // volume label attribute
    flash::write(root_addr, &first_sector).await?;

    let zeros = [0u8; 512];
    for sector in 1..(root_size / 512) {
        flash::write(root_addr + sector * 512, &zeros).await?;
    }

    defmt::info!(
        "FAT12: formatted partition as {=[u8]:a} (serial {=u32:08x})",
        label,
        serial,
    );
    Ok(())
}

/// Format the partition if it isn't already a valid FAT12 filesystem.
pub async fn format_if_needed() -> Result<(), flash::FlashError> {
    if is_formatted().await {
        defmt::info!("FAT12: existing filesystem detected");
        Ok(())
    } else {
        defmt::info!("FAT12: no filesystem — formatting");
        format().await
    }
}

// ---------------------------------------------------------------------------
// Block device over the FAT partition
// ---------------------------------------------------------------------------

/// Maps 512-byte logical blocks to the FAT12 partition on QSPI flash.
pub struct FatBlockDevice;

impl FatBlockDevice {
    pub const BLOCK_COUNT: u32 = (flash::FAT_BYTES / 512) as u32;
}

impl BlockDevice for FatBlockDevice {
    fn block_count(&self) -> u32 {
        Self::BLOCK_COUNT
    }

    async fn read_block(&self, lba: u32, buf: &mut [u8]) -> Result<(), ()> {
        let addr = flash::FAT_OFFSET + lba * 512;
        flash::read(addr, buf).await.map_err(|_| ())
    }

    async fn write_block(&self, lba: u32, buf: &[u8]) -> Result<(), ()> {
        // Signal copy activity to the LED monitor (blue while copying).
        crate::dfu::MSC_WRITE_TICK.fetch_add(1, Ordering::Release);

        let addr = flash::FAT_OFFSET + lba * 512;
        // NOR flash: erase the containing 4 KiB sector, read-modify-write the
        // 512-byte block within it.
        let sector_addr = addr & !(flash::PAGE_SIZE as u32 - 1);
        let offset_in_sector = (addr - sector_addr) as usize;

        let mut sector_buf = [0u8; flash::PAGE_SIZE];
        flash::read(sector_addr, &mut sector_buf)
            .await
            .map_err(|_| ())?;
        sector_buf[offset_in_sector..offset_in_sector + 512].copy_from_slice(buf);
        flash::erase(sector_addr).await.map_err(|_| ())?;
        flash::write(sector_addr, &sector_buf).await.map_err(|_| ())
    }
}
