//! Qwiic I2C bus (external connector) — bus scanner.
//!
//! SDA = `P1_10`, SCL = `P1_11`, driven by `TWISPI0` (TWIM0). Qwiic breakout
//! boards carry their own bus pull-ups. Nothing on the badge itself speaks I2C,
//! so the bus stays idle until the user opens the "I2C Scan" screen, which
//! walks the 7-bit address space and lists whatever ACKs.
//!
//! The scan runs in the async display loop (which owns the bus handle); the
//! screen state below is read synchronously by [`draw`] from that loop.

use core::cell::RefCell;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_nrf::gpio::AnyPin;
use embassy_nrf::twim::{self, Twim};
use embassy_nrf::{Peri, bind_interrupts, peripherals};
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;

use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Baseline, Text, TextStyleBuilder};

use crate::ui::{self, TEXT_BLACK, TEXT_BOLD_BLACK};
use crate::{TriColor, WHITE};

bind_interrupts!(struct Irqs {
    TWISPI0 => twim::InterruptHandler<peripherals::TWISPI0>;
});

/// TWIM DMA scratch-buffer length. Scanning only ever does 1-byte reads, but
/// the buffer must live in RAM and outlive the bus handle.
pub const TX_BUF_LEN: usize = 4;

/// Cap on reported devices — a badge will only ever see a handful.
const MAX_FOUND: usize = 16;

/// The concrete Qwiic bus handle (TWIM on `TWISPI0`).
pub type QwiicBus<'d> = Twim<'d>;

#[derive(Clone, Copy)]
struct ScanState {
    /// `false` while a scan is queued/running, `true` once results are valid.
    scanned: bool,
    addrs: [u8; MAX_FOUND],
    count: usize,
    truncated: bool,
}

impl ScanState {
    const EMPTY: Self = Self {
        scanned: false,
        addrs: [0; MAX_FOUND],
        count: 0,
        truncated: false,
    };
}

static ACTIVE: AtomicBool = AtomicBool::new(false);
static SCAN_PENDING: AtomicBool = AtomicBool::new(false);
static STATE: Mutex<CriticalSectionRawMutex, RefCell<ScanState>> =
    Mutex::new(RefCell::new(ScanState::EMPTY));

/// Build the Qwiic TWIM bus on `P1_10` (SDA) / `P1_11` (SCL). `tx` is a DMA
/// scratch buffer that must outlive the returned handle.
pub fn new_bus<'d>(
    twispi0: Peri<'d, peripherals::TWISPI0>,
    sda: Peri<'d, AnyPin>,
    scl: Peri<'d, AnyPin>,
    tx: &'d mut [u8],
) -> QwiicBus<'d> {
    let mut config = twim::Config::default();
    config.frequency = twim::Frequency::K100;
    // Enable the nRF internal pull-ups: the bus pull-ups live on the Qwiic
    // *device*, so with nothing plugged in SDA/SCL float and every read returns
    // garbage (phantom devices). Internal pull-ups (~13 kΩ) idle the bus high so
    // absent addresses cleanly NACK; a plugged-in board's stronger pull-ups just
    // parallel these.
    config.sda_pullup = true;
    config.scl_pullup = true;
    Twim::new(twispi0, Irqs, sda, scl, config, tx)
}

/// `true` while the scanner screen is showing (intercepts input, owns the
/// display).
pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// Open the scanner screen and queue a fresh scan.
pub fn open() {
    STATE.lock(|s| *s.borrow_mut() = ScanState::EMPTY);
    SCAN_PENDING.store(true, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Relaxed);
    crate::FULL_REFRESH_PENDING.store(true, Ordering::Relaxed);
}

/// Close the scanner screen.
pub fn close() {
    ACTIVE.store(false, Ordering::Relaxed);
}

/// Returns `true` exactly once per [`open`] — the display loop uses this to
/// decide whether to run a scan on this iteration.
pub fn take_scan_pending() -> bool {
    SCAN_PENDING.swap(false, Ordering::Relaxed)
}

/// Walk `0x08..=0x77` and record every address that ACKs. Runs in the async
/// display loop (which owns `bus`). ~50 ms at 100 kHz.
pub async fn run_scan(bus: &mut QwiicBus<'_>) {
    let mut result = ScanState {
        scanned: true,
        ..ScanState::EMPTY
    };
    let mut buf = [0u8; 1];
    for addr in 0x08u8..=0x77 {
        // Count a device present ONLY on a successful 1-byte read (address
        // ACKed + data clocked). Treating "anything but AddressNack" as present
        // reports phantoms, because a floating/empty bus returns Receive/
        // Transmit errors rather than clean NACKs.
        let present = bus.read(addr, &mut buf).await.is_ok();
        if present {
            if result.count < MAX_FOUND {
                result.addrs[result.count] = addr;
                result.count += 1;
            } else {
                result.truncated = true;
            }
        }
    }
    STATE.lock(|s| *s.borrow_mut() = result);
    crate::FULL_REFRESH_PENDING.store(true, Ordering::Relaxed);
}

/// Render the scanner screen (152×152).
pub fn draw<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    Rectangle::new(Point::zero(), Size::new(152, 152))
        .into_styled(PrimitiveStyle::with_fill(WHITE))
        .draw(display)?;
    ui::draw_title_bar(display, "I2C Scan", Point::zero(), 152)?;

    let state = STATE.lock(|s| *s.borrow());

    if !state.scanned {
        ui::draw_centered_message(display, "Scanning...", Point::new(76, 85))?;
        return Ok(());
    }
    if state.count == 0 {
        ui::draw_centered_message(display, "No I2C devices", Point::new(76, 85))?;
        return Ok(());
    }

    let left = TextStyleBuilder::new().baseline(Baseline::Top).build();

    let mut header: heapless::String<20> = heapless::String::new();
    let _ = core::fmt::Write::write_fmt(&mut header, format_args!("{} device(s):", state.count));
    Text::with_text_style(header.as_str(), Point::new(4, 22), TEXT_BOLD_BLACK, left).draw(display)?;

    // Up to 12 addresses in two columns of six rows, 7-bit hex.
    for (i, addr) in state.addrs.iter().take(state.count).enumerate() {
        let col = (i % 2) as i32;
        let row = (i / 2) as i32;
        let x = 12 + col * 74;
        let y = 42 + row * 16;
        let mut line: heapless::String<8> = heapless::String::new();
        let _ = core::fmt::Write::write_fmt(&mut line, format_args!("0x{:02X}", addr));
        Text::with_text_style(line.as_str(), Point::new(x, y), TEXT_BLACK, left).draw(display)?;
    }

    if state.truncated {
        ui::draw_centered_message(display, "(more...)", Point::new(76, 146))?;
    }

    Ok(())
}
