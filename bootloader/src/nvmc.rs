//! Raw NVMC (internal flash) register programming for nRF52840.
//!
//! Uses direct register writes — no embassy-nrf NVMC wrapper needed in the
//! bootloader context.

const NVMC_BASE: u32 = 0x4001_E000;
const NVMC_READY: *const u32 = (NVMC_BASE + 0x400) as *const u32;
const NVMC_CONFIG: *mut u32 = (NVMC_BASE + 0x504) as *mut u32;
const NVMC_ERASEPAGE: *mut u32 = (NVMC_BASE + 0x508) as *mut u32;

pub const PAGE_SIZE: u32 = 4096;

unsafe fn wait_ready() {
    while unsafe { core::ptr::read_volatile(NVMC_READY) } == 0 {}
}

pub unsafe fn erase_page(addr: u32) {
    unsafe {
        wait_ready();
        core::ptr::write_volatile(NVMC_CONFIG, 2); // ERASEEN
        wait_ready();
        core::ptr::write_volatile(NVMC_ERASEPAGE, addr);
        wait_ready();
        core::ptr::write_volatile(NVMC_CONFIG, 0); // REN (read-only)
    }
}

/// Write a 4-byte-aligned slice to flash.
/// `addr` and `data.len()` must both be multiples of 4.
/// The target page must already be erased.
pub unsafe fn write(addr: u32, data: &[u8]) {
    debug_assert!(addr % 4 == 0);
    debug_assert!(data.len() % 4 == 0);
    unsafe {
        wait_ready();
        core::ptr::write_volatile(NVMC_CONFIG, 1); // WEN
        wait_ready();
        for (i, chunk) in data.chunks_exact(4).enumerate() {
            let word = u32::from_le_bytes(chunk.try_into().unwrap());
            core::ptr::write_volatile((addr + i as u32 * 4) as *mut u32, word);
            wait_ready();
        }
        core::ptr::write_volatile(NVMC_CONFIG, 0); // REN
    }
}
