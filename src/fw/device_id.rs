//! Device identity: a two-byte ID from the nRF52840's factory-programmed FICR device address.
//!
//! Call `init()` once at startup; retrieve the ID from anywhere with `get()`.

use embassy_sync::once_lock::OnceLock;

static DEVICE_ID: OnceLock<[u8; 2]> = OnceLock::new();

/// Read the two-byte ID from FICR DEVICEADDR[0] and cache it.  Call once at startup.
pub fn init() {
    let lo = embassy_nrf::pac::FICR.deviceaddr(0).read();
    let b = lo.to_le_bytes();
    let _ = DEVICE_ID.init([b[0], b[1]]);
}

/// Return the cached two-byte device ID.  Panics if `init()` has not been called.
pub fn get() -> [u8; 2] {
    *DEVICE_ID.try_get().expect("device_id::init() not called")
}

/// Return the device ID as four uppercase ASCII hex bytes, e.g. `b"A3F7"`.
pub fn get_bytes() -> [u8; 4] {
    let [id0, id1] = get();
    let h = |n: u8| if n < 10 { b'0' + n } else { b'A' + n - 10 };
    [h(id0 >> 4), h(id0 & 0xF), h(id1 >> 4), h(id1 & 0xF)]
}
