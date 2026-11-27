//! nRF52840 hardware TRNG, read by direct register access.

/// Generate 32 random bytes from the nRF52840 on-chip hardware TRNG.
///
/// Uses blocking PAC register access, so it is meant for startup. The
/// bias-correction filter (DERCEN) is enabled for better statistical quality
/// at ~85 uA extra current during generation.
///
/// MUST be called before embassy-nrf hands the RNG peripheral to anything
/// else (e.g. the BLE softdevice controller); it bypasses the embassy wrapper.
pub fn seed32() -> [u8; 32] {
    // nRF52840 RNG register offsets (Product Spec section 6.19)
    const RNG_BASE: u32 = 0x4000_D000;
    const TASKS_START: u32 = RNG_BASE;
    const TASKS_STOP: u32 = RNG_BASE + 0x004;
    const EVENTS_VALRDY: u32 = RNG_BASE + 0x100;
    const CONFIG: u32 = RNG_BASE + 0x504; // bit 0: DERCEN
    const VALUE: u32 = RNG_BASE + 0x508;

    // Safety: valid nRF52840 RNG register addresses, only accessed from
    // single-threaded startup before any tasks are spawned.
    unsafe {
        (CONFIG as *mut u32).write_volatile(1); // DERCEN = 1
        (TASKS_START as *mut u32).write_volatile(1);

        let mut seed = [0u8; 32];
        for byte in &mut seed {
            while (EVENTS_VALRDY as *const u32).read_volatile() == 0 {}
            *byte = (VALUE as *const u32).read_volatile() as u8;
            (EVENTS_VALRDY as *mut u32).write_volatile(0);
        }

        (TASKS_STOP as *mut u32).write_volatile(1);
        seed
    }
}
