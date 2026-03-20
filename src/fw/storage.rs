//! Generic QSPI flash layer backed by TicKV.
//!
//! Provides the low-level `QspiFlashController` that implements TicKV's
//! `FlashController` trait, an `init_qspi()` helper that verifies the chip
//! via JEDEC ID, and the `fnv1a` hash used to derive TicKV keys.
//!
//! All higher-level storage concerns (bonds, settings, …) live in their own
//! modules and build on top of these primitives.

use core::cell::UnsafeCell;

use embassy_nrf::{Peri, bind_interrupts, peripherals, qspi};
use tickv::{ErrorCode, FlashController};

// ---------------------------------------------------------------------------
// Flash geometry
// ---------------------------------------------------------------------------

/// One erase sector on the ZD25WQ16 (4 KiB).  Also the TicKV region size.
pub const REGION_SIZE: usize = 4096;

// ---------------------------------------------------------------------------
// Interrupt binding
// ---------------------------------------------------------------------------

bind_interrupts!(pub struct QspiIrqs {
    QSPI => qspi::InterruptHandler<peripherals::QSPI>;
});

// ---------------------------------------------------------------------------
// Deterministic FNV-1a 64-bit hash (const-capable, no external dep)
// ---------------------------------------------------------------------------

pub const fn fnv1a(data: &[u8]) -> u64 {
    const OFFSET: u64 = 14_695_981_039_346_656_037;
    const PRIME: u64 = 1_099_511_628_211;
    let mut h = OFFSET;
    let mut i = 0;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(PRIME);
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// Buffers
// ---------------------------------------------------------------------------

/// 4-byte-aligned buffer large enough for one flash region.
/// Pass a `StaticCell<AlignedBuf>` to `TicKV::new()`.
#[repr(C, align(4))]
pub struct AlignedBuf(pub [u8; REGION_SIZE]);

/// 256-byte staging area for the write alignment fix (see `write()`).
struct WriteStagingBuf(UnsafeCell<[u32; 64]>); // 64 × 4 = 256 bytes
unsafe impl Sync for WriteStagingBuf {}

static WRITE_STAGING: WriteStagingBuf = WriteStagingBuf(UnsafeCell::new([0u32; 64]));

// ---------------------------------------------------------------------------
// QspiFlashController
// ---------------------------------------------------------------------------

pub struct QspiFlashController {
    /// Wrapped in UnsafeCell because `FlashController` takes `&self`.
    /// Safety: only ever accessed from the single task that owns this struct.
    pub qspi: UnsafeCell<qspi::Qspi<'static>>,
}

// Safety: single-task access guaranteed by ownership.
unsafe impl Send for QspiFlashController {}
unsafe impl Sync for QspiFlashController {}

impl QspiFlashController {
    pub fn new(qspi: qspi::Qspi<'static>) -> Self {
        Self { qspi: UnsafeCell::new(qspi) }
    }

    fn qspi_mut(&self) -> &mut qspi::Qspi<'static> {
        unsafe { &mut *self.qspi.get() }
    }
}

impl<const R: usize> FlashController<R> for QspiFlashController {
    fn read_region(
        &self,
        region_number: usize,
        buf: &mut [u8; R],
    ) -> Result<(), ErrorCode> {
        let addr = (region_number * R) as u32;
        self.qspi_mut()
            .blocking_read(addr, buf)
            .map_err(|_| ErrorCode::ReadFail)
    }

    fn write(&self, address: usize, buf: &[u8]) -> Result<(), ErrorCode> {
        // nRF52840 QSPI DMA requires write address, pointer, and length to be
        // 4-byte aligned. TicKV objects are 11 + value + 4 bytes and are not
        // guaranteed to satisfy this, so we always go through a staging buffer.
        //
        // 1. Round address DOWN to 4-byte boundary; compute prefix length (0–3 B).
        // 2. If prefix > 0, read those bytes from flash (must not alter them —
        //    NOR flash can't flip 0 → 1 without an erase).
        // 3. Copy `buf` after the prefix; pad trailer to next 4-byte boundary
        //    with 0xFF (erased-flash value — TicKV reads padding as "empty").
        // 4. Issue one aligned write of the padded staging slice.
        let addr = address as u32;
        let aligned_addr = addr & !3;
        let prefix_len = (addr - aligned_addr) as usize;
        let total = prefix_len + buf.len();
        let padded_len = (total + 3) & !3;

        // Safety: WRITE_STAGING accessed only from the owning task.
        let staging_u32 = unsafe { &mut *WRITE_STAGING.0.get() };
        let staging = unsafe {
            core::slice::from_raw_parts_mut(staging_u32.as_mut_ptr() as *mut u8, 256)
        };
        assert!(padded_len <= 256, "TicKV write > 256 bytes");

        if prefix_len > 0 {
            self.qspi_mut()
                .blocking_read(aligned_addr, &mut staging[..4])
                .map_err(|_| ErrorCode::WriteFail)?;
        }

        staging[prefix_len..total].copy_from_slice(buf);
        staging[total..padded_len].fill(0xFF);

        self.qspi_mut()
            .blocking_write(aligned_addr, &staging[..padded_len])
            .map_err(|_| ErrorCode::WriteFail)
    }

    fn erase_region(&self, region_number: usize) -> Result<(), ErrorCode> {
        let addr = (region_number * R) as u32;
        self.qspi_mut()
            .blocking_erase(addr)
            .map_err(|_| ErrorCode::EraseFail)
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
    cfg.capacity = 2 * 1024 * 1024;
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
