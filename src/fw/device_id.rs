//! Device identity: a two-byte ID from the nRF52840's factory-programmed FICR
//! device address.
//!
//! Call `init()` once at startup; retrieve the ID from anywhere with `get()`.

use embassy_sync::once_lock::OnceLock;

static DEVICE_ID: OnceLock<[u8; 2]> = OnceLock::new();

/// Read the two-byte ID from FICR DEVICEADDR\[0\] and cache it.  Call once at
/// startup.
pub fn init() {
    let lo = embassy_nrf::pac::FICR.deviceaddr(0).read();
    let b = lo.to_le_bytes();
    let _ = DEVICE_ID.init([b[0], b[1]]);
}

/// Return the cached two-byte device ID.  Panics if `init()` has not been
/// called.
pub fn get() -> [u8; 2] {
    *DEVICE_ID.try_get().expect("device_id::init() not called")
}

/// Return the 6-byte BLE random static address derived from FICR DEVICEADDR.
///
/// FICR DEVICEADDR\[0\] holds the lower 32 bits and DEVICEADDR\[1\] the upper 16
/// bits of the factory-assigned 48-bit address.  The top 2 bits of byte 5 are
/// forced to `0b11` as required for a random static address (BT Core Spec
/// §1.3.2.1).
pub fn get_ble_addr() -> [u8; 6] {
    let lo = embassy_nrf::pac::FICR.deviceaddr(0).read().to_le_bytes();
    let hi = embassy_nrf::pac::FICR.deviceaddr(1).read().to_le_bytes();
    [lo[0], lo[1], lo[2], lo[3], hi[0], hi[1] | 0xC0]
}

/// Return the device ID as four uppercase ASCII hex bytes, e.g. `b"A3F7"`.
pub fn get_bytes() -> [u8; 4] {
    let [id0, id1] = get();
    let h = |n: u8| if n < 10 { b'0' + n } else { b'A' + n - 10 };
    [h(id0 >> 4), h(id0 & 0xF), h(id1 >> 4), h(id1 & 0xF)]
}
