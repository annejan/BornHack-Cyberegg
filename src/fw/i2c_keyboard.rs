//! Optional I2C keyboard support for text entry.
//!
//! Nicolai Electronics I2C keyboard on the Qwiic bus at address `0x09`:
//! a 10-column × 4-row matrix read as 5 bytes (write register `0`, read 5),
//! plus three RGB LEDs at registers `5..=13`.  Used only while a text-entry
//! screen is open; absent hardware NACKs and everything falls back to the
//! on-screen joystick entry.

use embassy_time::{Duration, with_timeout};

use super::qwiic::QwiicBus;
use crate::text_entry::ExtKey;

/// 7-bit I2C address (firmware `SetupI2CSlave(0x9, ...)`).
pub const ADDR: u8 = 0x09;

/// Short timeout so a wedged / unplugged bus can't stall the display loop.
const IO_TIMEOUT: Duration = Duration::from_millis(20);

/// Matrix key (index = column*4 + row) → text-entry action.  Base layer.
/// Index/letter positions from the keyboard's `test.py`; Space and Alt filled
/// in from the physical silkscreen (the 3-wide spacebar spans cols 2..=4, and
/// the Alt key sits at index 7 — both left unmapped by `test.py`).  `None` =
/// an unpopulated / non-text key (Esc / Menu / F-keys / arrows / PgUp/Dn).
fn key_for(index: usize) -> Option<ExtKey> {
    Some(match index {
        0 => ExtKey::Char(b'q'),
        1 => ExtKey::Char(b'a'),
        2 => ExtKey::Char(b'z'),
        3 => ExtKey::Shift,
        4 => ExtKey::Char(b'w'),
        5 => ExtKey::Char(b's'),
        6 => ExtKey::Char(b'x'),
        7 => ExtKey::Alt, // one-shot toggle applied in text_entry
        8 => ExtKey::Char(b'e'),
        9 => ExtKey::Char(b'd'),
        10 => ExtKey::Char(b'c'),
        11 => ExtKey::Space, // spacebar (left third)
        12 => ExtKey::Char(b'r'),
        13 => ExtKey::Char(b'f'),
        14 => ExtKey::Char(b'v'),
        15 => ExtKey::Space, // spacebar (middle third)
        16 => ExtKey::Char(b't'),
        17 => ExtKey::Char(b'g'),
        18 => ExtKey::Char(b'b'),
        19 => ExtKey::Space, // spacebar (right third)
        20 => ExtKey::Char(b'y'),
        21 => ExtKey::Char(b'h'),
        22 => ExtKey::Char(b'n'),
        24 => ExtKey::Char(b'u'),
        25 => ExtKey::Char(b'j'),
        26 => ExtKey::Char(b'm'),
        28 => ExtKey::Char(b'i'),
        29 => ExtKey::Char(b'k'),
        // 30 => Left (cursor) — ignored (entry has no cursor)
        32 => ExtKey::Char(b'o'),
        33 => ExtKey::Char(b'l'),
        // 34 => Up, 35 => Down — ignored
        36 => ExtKey::Char(b'p'),
        37 => ExtKey::Backspace,
        38 => ExtKey::Enter,
        // 39 => Right — ignored
        _ => return None,
    })
}

/// The 4-bit column read for `column` from the 5 matrix bytes.
fn column_bits(matrix: &[u8; 5], column: usize) -> u8 {
    let shift = if column & 1 == 0 { 4 } else { 0 };
    (matrix[column / 2] >> shift) & 0xF
}

/// Probe presence: a successful matrix read means the keyboard is on the bus.
pub async fn present(bus: &mut QwiicBus<'_>) -> bool {
    read_matrix(bus).await.is_some()
}

/// Read the 5-byte key matrix.  `None` on I2C error / timeout / absent.
pub async fn read_matrix(bus: &mut QwiicBus<'_>) -> Option<[u8; 5]> {
    let mut m = [0u8; 5];
    match with_timeout(IO_TIMEOUT, bus.write_read(ADDR, &[0x00], &mut m)).await {
        Ok(Ok(())) => Some(m),
        _ => None,
    }
}

/// Turn LED A (the first RGB LED) on (dim white) or off.  Registers 5..=7 are
/// LED1 R/G/B; we also clear LED2/LED3 so the keyboard's power-on colours go
/// dark.  Best-effort.
pub async fn set_led_a(bus: &mut QwiicBus<'_>, on: bool) {
    let v = if on { 24 } else { 0 };
    // reg pointer 5, then LED1 rgb, LED2 rgb, LED3 rgb.
    let payload = [5u8, v, v, v, 0, 0, 0, 0, 0, 0];
    let _ = with_timeout(IO_TIMEOUT, bus.write(ADDR, &payload)).await;
}

/// Append the keys that are newly down in `curr` vs `prev` (rising edges) to
/// `out` as base-layer actions.  Shift and Alt are emitted as ExtKey::Shift /
/// ExtKey::Alt; `text_entry` applies them as one-shot toggles (they "stick"
/// for the next key, like the on-screen Shift), so the alt-layer symbol
/// resolution lives there, not here.
pub fn newly_pressed(curr: &[u8; 5], prev: &[u8; 5], out: &mut heapless::Vec<ExtKey, 8>) {
    for column in 0..10usize {
        let rising = column_bits(curr, column) & !column_bits(prev, column);
        if rising == 0 {
            continue;
        }
        for row in 0..4usize {
            if rising & (1 << row) != 0
                && let Some(k) = key_for(column * 4 + row)
            {
                let _ = out.push(k);
            }
        }
    }
}
